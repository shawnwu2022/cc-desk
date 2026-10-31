//! Default-off, one-slot custody for a Job start before NativeRun publication.
#![allow(dead_code)] // No live/IPC entry is enabled by this staging seam.
use super::admitted_child::AdmittedChild;
use crate::version_history::maintenance::{OwnedChildTicket, StartTicket};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, JobSpawnError, PendingJobChild, PtyPair};
use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::{
    Foundation::HANDLE,
    System::JobObjects::{
        JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    },
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum JobLaunchObservation {
    Empty,
    Pending,
    Unknown,
    Rejected,
}

enum Root {
    Pending(PendingJobChild),
    Started(AdmittedChild),
}
// These resources are installed BEFORE CreateProcess; there is no late Err
// owner for a caller to accidentally drop or erase with map_err.
struct Custody {
    pair: PtyPair,
    job: OwnedHandle,
    preparing: Option<StartTicket>,
    created: Option<OwnedChildTicket>,
    root: Option<Root>,
    assigned: bool,
    stop_requested: bool,
    failed: bool,
}
#[derive(Default)]
pub(crate) struct OwnedJobLaunchSlot {
    custody: Mutex<Option<Custody>>,
    closed: AtomicBool,
}
impl OwnedJobLaunchSlot {
    pub(crate) fn launch(
        &self,
        pair: PtyPair,
        command: CommandBuilder,
        ticket: StartTicket,
        job: OwnedHandle,
    ) -> JobLaunchObservation {
        let mut preparation = ticket.preparing();
        let Some(mut slot) = self.custody.try_lock() else {
            return JobLaunchObservation::Rejected;
        };
        if self.closed.load(Ordering::Acquire) || slot.is_some() {
            return JobLaunchObservation::Rejected;
        }
        *slot = Some(Custody {
            pair,
            job,
            preparing: Some(preparation.begin_creation()),
            created: None,
            root: None,
            assigned: false,
            stop_requested: false,
            failed: false,
        });
        let owned = slot.as_mut().unwrap();
        match owned
            .pair
            .slave
            .spawn_command_in_owned_job(command, owned.job.as_handle())
        {
            Ok(child) => {
                owned.root = Some(Root::Started(AdmittedChild::created(
                    child,
                    owned.preparing.take().unwrap(),
                )));
                owned.assigned = true;
                JobLaunchObservation::Pending
            }
            Err(failure) => {
                let (classification, pending) = failure.into_parts();
                if let Some(pending) = pending {
                    owned.assigned = pending.assigned_to_job();
                    owned.root = Some(Root::Pending(pending));
                    owned.created = Some(owned.preparing.take().unwrap().child_created());
                    if classification == JobSpawnError::OutcomeUnknown {
                        JobLaunchObservation::Unknown // execution fact, NOT cleanup completion
                    } else {
                        JobLaunchObservation::Pending
                    }
                } else {
                    owned.preparing.take().unwrap().no_child_created();
                    // Only the typed before-Create path has no original child.
                    drop(slot.take());
                    JobLaunchObservation::Rejected
                }
            }
        }
    }
    pub(crate) fn observe(&self) -> JobLaunchObservation {
        let Some(slot) = self.custody.try_lock() else {
            return JobLaunchObservation::Pending;
        };
        match slot.as_ref() {
            None => JobLaunchObservation::Empty,
            Some(owned) if owned.failed => JobLaunchObservation::Unknown,
            Some(_) => JobLaunchObservation::Pending,
        }
    }
    pub(crate) fn close_admission(&self) {
        self.closed.store(true, Ordering::Release);
    }
    #[cfg(test)]
    pub(crate) fn mark_previous_query_unknown_for_test(&self) {
        self.custody.lock().as_mut().unwrap().failed = true;
    }
    #[cfg(test)]
    pub(crate) fn probe_originals_for_test(&self) -> (bool, u32) {
        let mut slot = self.custody.lock();
        let owned = slot.as_mut().unwrap();
        let signalled = match owned.root.as_mut().unwrap() {
            Root::Pending(root) => root.try_wait_root().unwrap(),
            Root::Started(root) => root.observe_exact_root().unwrap(),
        };
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(HANDLE(owned.job.as_raw_handle())),
                JobObjectBasicAccountingInformation,
                &mut information as *mut _ as _,
                std::mem::size_of_val(&information) as u32,
                None,
            )
        }
        .unwrap();
        (signalled, information.ActiveProcesses)
    }
    /// Call on the original blocking desktop-stop worker, never the UI/async
    /// observer. CreateProcess/ClosePseudoConsole are NOT preemptible deadlines.
    pub(crate) fn cleanup_step(&self) -> JobLaunchObservation {
        self.close_admission();
        let Some(mut slot) = self.custody.try_lock() else {
            return JobLaunchObservation::Pending;
        };
        let Some(owned) = slot.as_mut() else {
            return JobLaunchObservation::Empty;
        };
        // A previous failed observation is not proof of either exit or life.
        // Explicit reconciliation may still establish positive original-handle
        // facts. stop_requested below prevents blind termination retries.
        let root_reaped = match owned.root.as_mut() {
            Some(Root::Pending(root)) => root.try_wait_root().map_err(|_| ()),
            Some(Root::Started(root)) => root.observe_exact_root().map_err(|_| ()),
            None => Err(()), // unwind/unsupported incomplete creation cannot become no-child
        };
        let Ok(root_reaped) = root_reaped else {
            owned.failed = true;
            return JobLaunchObservation::Unknown;
        };
        if root_reaped {
            if let Some(ticket) = owned.created.take() {
                ticket.reaped();
            }
        }
        let active = if owned.assigned {
            let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            if unsafe {
                QueryInformationJobObject(
                    Some(HANDLE(owned.job.as_raw_handle())),
                    JobObjectBasicAccountingInformation,
                    &mut information as *mut _ as _,
                    std::mem::size_of_val(&information) as u32,
                    None,
                )
            }
            .is_err()
            {
                owned.failed = true;
                return JobLaunchObservation::Unknown;
            }
            information.ActiveProcesses
        } else {
            0
        };
        if root_reaped && active == 0 {
            // Holding the slot during original-resource Drop makes a concurrent
            // exit observation Pending until that blocking destructor returns.
            drop(slot.take());
            return JobLaunchObservation::Empty;
        }
        if !owned.stop_requested {
            owned.stop_requested = true;
            let result = if owned.assigned {
                unsafe { TerminateJobObject(HANDLE(owned.job.as_raw_handle()), 1) }.map_err(|_| ())
            } else {
                match owned.root.as_ref() {
                    Some(Root::Pending(root)) => root.request_root_termination().map_err(|_| ()),
                    _ => Err(()),
                }
            };
            if result.is_err() {
                owned.failed = true;
                return JobLaunchObservation::Unknown;
            }
        }
        JobLaunchObservation::Pending
    }
}
impl Drop for OwnedJobLaunchSlot {
    fn drop(&mut self) {
        if let Some(unresolved) = self.custody.get_mut().take() {
            // Abandonment is NOT retirement. Keep this one bounded custody
            // (including original OS handles and admission ticket) for process
            // lifetime instead of silently closing an unassigned Unknown root.
            // Normal app exit is gated by observe(), and must never reach here.
            std::mem::forget(unresolved);
        }
    }
}
