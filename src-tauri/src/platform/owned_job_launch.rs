//! Default-off, one-slot custody for a Job start before NativeRun publication.
#![allow(dead_code)] // No live/IPC entry is enabled by this staging seam.
use super::admitted_child::AdmittedChild;
use super::owned_pty::{IoDropState, IoDropWitness, OwnedPty, PreparedPtyIo};
use crate::run_supervisor::{drain_unpublished_output, OutputReaderJoin};
use crate::version_history::maintenance::{OwnedChildTicket, StartTicket};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, JobSpawnError, PendingJobChild, PtyPair};
use std::io::Read;
use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc,
};
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
    Started(OwnedPty),
}
#[derive(Default)]
struct DrainFacts {
    end: AtomicU8,
    bytes: AtomicU64,
}
// These resources are installed BEFORE CreateProcess; there is no late Err
// owner for a caller to accidentally drop or erase with map_err.
struct Custody {
    prepared: Option<PreparedPtyIo>,
    job: OwnedHandle,
    preparing: Option<StartTicket>,
    created: Option<OwnedChildTicket>,
    root: Option<Root>,
    assigned: bool,
    stop_requested: bool,
    failed: bool,
    process_quiescent: bool,
    io_drop: IoDropWitness,
    // Reader only: this handoff must never pin its own master/OwnedPty.
    reader_handoff: Arc<Mutex<Option<Box<dyn Read + Send>>>>,
    drain_facts: Arc<DrainFacts>,
    reader_worker: OutputReaderJoin,
}
#[derive(Default)]
pub(crate) struct OwnedJobLaunchSlot {
    custody: Mutex<Option<Custody>>,
    closed: AtomicBool,
    retired_io: AtomicU8,
    drained_bytes: AtomicU64,
}
impl OwnedJobLaunchSlot {
    #[cfg(test)]
    pub(crate) fn io_retirement_for_test(&self) -> (bool, bool, bool) {
        let facts = self.retired_io.load(Ordering::Acquire);
        (facts & 1 != 0, facts & 2 != 0, facts & 4 != 0)
    }
    #[cfg(test)]
    pub(crate) fn drained_bytes_for_test(&self) -> u64 {
        self.drained_bytes.load(Ordering::Acquire)
    }
    #[cfg(test)]
    pub(crate) fn begin_drain_for_test(&self) -> bool {
        self.close_admission();
        self.custody.lock().as_mut().unwrap().begin_drain()
    }
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
        let prepared = match PreparedPtyIo::new(pair) {
            Ok(prepared) => prepared,
            Err(_) => return JobLaunchObservation::Rejected,
        };
        let io_drop = prepared.io_drop_witness();
        *slot = Some(Custody {
            prepared: Some(prepared),
            job,
            preparing: Some(preparation.begin_creation()),
            created: None,
            root: None,
            assigned: false,
            stop_requested: false,
            failed: false,
            process_quiescent: false,
            io_drop,
            reader_handoff: Arc::new(Mutex::new(None)),
            drain_facts: Arc::new(DrainFacts::default()),
            reader_worker: OutputReaderJoin::Unstarted,
        });
        let owned = slot.as_mut().unwrap();
        match owned
            .prepared
            .as_ref()
            .unwrap()
            .slave()
            .spawn_command_in_owned_job(command, owned.job.as_handle())
        {
            Ok(child) => {
                let child = AdmittedChild::created(child, owned.preparing.take().unwrap());
                owned.root = Some(Root::Started(owned.prepared.take().unwrap().finish(child)));
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
        // Publish the consumer before requesting stop: root exit itself may
        // depend on outstanding native output IO making progress.
        let drain_ready = owned.begin_drain();
        // Observe already-finished failures BEFORE any native IO is closed.
        // Otherwise a failed consumer could make ClosePseudoConsole block
        // instead of reporting the known Unknown. Recheck after Drop too.
        owned.reader_worker.observe();
        let known_reader_failure = !drain_ready
            || matches!(owned.reader_worker, OutputReaderJoin::Unknown)
            || owned.drain_facts.end.load(Ordering::Acquire) == 2
            || owned.io_drop.observe() == IoDropState::Unknown;
        if !owned.process_quiescent {
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
                owned.process_quiescent = true;
            } else if !owned.stop_requested {
                owned.stop_requested = true;
                let result = if owned.assigned {
                    unsafe { TerminateJobObject(HANDLE(owned.job.as_raw_handle()), 1) }
                        .map_err(|_| ())
                } else {
                    match owned.root.as_ref() {
                        Some(Root::Pending(root)) => {
                            root.request_root_termination().map_err(|_| ())
                        }
                        _ => Err(()),
                    }
                };
                if result.is_err() {
                    owned.failed = true;
                    return JobLaunchObservation::Unknown;
                }
            }
        }
        if known_reader_failure {
            owned.failed = true;
            return JobLaunchObservation::Unknown;
        }
        if !owned.process_quiescent {
            return JobLaunchObservation::Pending;
        }
        // Consumer is active, original slave is gone. Close original master and
        // writer on this blocking owner, BEFORE waiting for EOF/reader join.
        // While native destruction blocks, try-lock observers remain Pending.
        drop(owned.root.take());
        drop(owned.prepared.take());
        let joined = owned.reader_worker.observe();
        let end = owned.drain_facts.end.load(Ordering::Acquire);
        if matches!(owned.reader_worker, OutputReaderJoin::Unknown)
            || end == 2
            || owned.io_drop.observe() == IoDropState::Unknown
        {
            owned.failed = true;
            return JobLaunchObservation::Unknown;
        }
        if !joined || end != 1 || owned.io_drop.observe() != IoDropState::Returned {
            return JobLaunchObservation::Pending;
        }
        self.drained_bytes.store(
            owned.drain_facts.bytes.load(Ordering::Acquire),
            Ordering::Release,
        );
        self.retired_io.store(7, Ordering::Release);
        drop(slot.take()); // original Job released last, after independent IO facts
        JobLaunchObservation::Empty
    }
}
impl Custody {
    fn begin_drain(&mut self) -> bool {
        if matches!(self.reader_worker, OutputReaderJoin::Unstarted) {
            let original = match self.root.as_ref() {
                Some(Root::Started(pty)) => pty.take_reader(),
                _ => match self.prepared.as_mut() {
                    Some(prepared) => {
                        prepared.close_slave();
                        prepared.take_reader()
                    }
                    None => {
                        self.failed = true;
                        return false;
                    }
                },
            };
            let Ok(reader) = original else {
                self.failed = true;
                return false;
            };
            *self.reader_handoff.lock() = Some(reader);
            self.reader_worker.reserve();
            let handoff = self.reader_handoff.clone();
            let facts = self.drain_facts.clone();
            let worker = std::thread::Builder::new()
                .name("native-unpublished-drain".into())
                .spawn(move || {
                    // Take once, and never hold this mutex across blocking Read.
                    let reader = handoff.lock().take();
                    let Some(mut reader) = reader else {
                        facts.end.store(2, Ordering::Release);
                        return;
                    };
                    let result = drain_unpublished_output(&mut *reader);
                    drop(reader); // original TrackedDrop must return before EOF publication
                    match result {
                        Ok(bytes) => {
                            facts.bytes.store(bytes, Ordering::Release);
                            facts.end.store(1, Ordering::Release);
                        }
                        Err(()) => facts.end.store(2, Ordering::Release),
                    }
                });
            self.reader_worker = match worker {
                Ok(worker) => OutputReaderJoin::Running(worker),
                Err(_) => {
                    self.failed = true;
                    OutputReaderJoin::Unknown
                }
            };
        }
        !matches!(self.reader_worker, OutputReaderJoin::Unknown)
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

#[cfg(test)]
mod cleanup_error_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    struct Master(Arc<AtomicUsize>);
    impl Drop for Master {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl portable_pty::MasterPty for Master {
        fn resize(&self, _: portable_pty::PtySize) -> anyhow::Result<()> {
            Ok(())
        }
        fn get_size(&self) -> anyhow::Result<portable_pty::PtySize> {
            Ok(Default::default())
        }
        fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
            Ok(Box::new(std::io::empty()))
        }
        fn take_writer(&self) -> anyhow::Result<Box<dyn std::io::Write + Send>> {
            Ok(Box::new(std::io::sink()))
        }
    }
    struct Slave;
    impl portable_pty::SlavePty for Slave {
        fn spawn_command(
            &self,
            _: CommandBuilder,
        ) -> anyhow::Result<Box<dyn portable_pty::Child + Send + Sync>> {
            panic!("TEST_ONLY: this pure cleanup fault never creates a child");
        }
    }
    #[test]
    #[allow(non_snake_case)]
    fn OwnedJob_KnownDrainFailureKeepsOriginalIo_009() {
        for read_failure in [true, false] {
            let drops = Arc::new(AtomicUsize::new(0));
            let prepared = PreparedPtyIo::new(PtyPair {
                slave: Box::new(Slave),
                master: Box::new(Master(drops.clone())),
            })
            .unwrap();
            let io_drop = prepared.io_drop_witness();
            let raw = unsafe {
                windows::Win32::System::Threading::CreateEventW(None, false, false, None)
            }
            .unwrap();
            let job = unsafe {
                <OwnedHandle as std::os::windows::io::FromRawHandle>::from_raw_handle(raw.0)
            };
            let facts = Arc::new(DrainFacts::default());
            let worker = if read_failure {
                facts.end.store(2, Ordering::Release);
                OutputReaderJoin::Joined
            } else {
                let worker = std::thread::spawn(|| panic!("TEST_ONLY_DRAIN_PANIC"));
                let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
                while !worker.is_finished() {
                    assert!(std::time::Instant::now() < end);
                    std::thread::yield_now();
                }
                OutputReaderJoin::Running(worker)
            };
            let slot = OwnedJobLaunchSlot::default();
            *slot.custody.lock() = Some(Custody {
                prepared: Some(prepared),
                job,
                preparing: None,
                created: None,
                root: None,
                assigned: false,
                stop_requested: true,
                failed: false,
                process_quiescent: true,
                io_drop,
                reader_handoff: Arc::new(Mutex::new(None)),
                drain_facts: facts,
                reader_worker: worker,
            });
            let result = slot.cleanup_step();
            let prematurely_closed = drops.load(Ordering::SeqCst);
            // Explicit teardown is safe only because every resource here is a
            // pure no-process fixture, not an actual unknown native lifetime.
            drop(slot.custody.lock().take());
            assert_eq!(result, JobLaunchObservation::Unknown);
            assert_eq!(
                prematurely_closed, 0,
                "known failed consumer must not close original IO"
            );
        }
    }
}
