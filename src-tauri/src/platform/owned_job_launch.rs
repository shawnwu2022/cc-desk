//! Default-off, one-slot custody for a Job start before NativeRun publication.
#![allow(dead_code)] // No live/IPC entry is enabled by this staging seam.
use super::admitted_child::AdmittedChild;
use super::owned_pty::{IoDropState, IoDropWitness, OwnedPty, PreparedPtyIo};
use crate::cli::{
    launch_service::NativeRun, profiles::error, run_registry::RunKey, snapshot::LaunchSnapshot,
    types::SafeError,
};
use crate::run_supervisor::{drain_unpublished_output, OutputReaderJoin};
use crate::version_history::maintenance::{OwnedChildTicket, StartTicket};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, JobSpawnError, PendingJobChild, PtyPair};
use std::io::Read;
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc, Weak,
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
    Started(Arc<OwnedPty>),
}
pub(crate) struct ManagedStartAttempt {
    identity: Arc<()>,
    snapshot: Arc<LaunchSnapshot>,
}
enum ReaderOwner {
    Claimed,
    Returned,
    Supervised {
        resource: Weak<NativeRun>,
        run: RunKey,
    },
}
#[repr(u8)]
enum JobStopState {
    Idle,
    InFlight,
    Accepted,
    Unknown,
}
#[cfg(test)]
type StopEffect = Box<dyn FnOnce() -> Result<(), SafeError> + Send>;
/// One original Job/PTY identity. This is lifetime custody, not remote authority.
pub(crate) struct ManagedJob {
    original: OwnedHandle,
    pty: Weak<OwnedPty>,
    snapshot: Arc<LaunchSnapshot>,
    reader: Mutex<ReaderOwner>,
    closed: Arc<AtomicBool>,
    stop_state: AtomicU8,
    #[cfg(test)]
    stop_effect: Mutex<Option<StopEffect>>,
}
pub(crate) struct ManagedJobClaim {
    pub(crate) pty: Arc<OwnedPty>,
    pub(crate) job: Arc<ManagedJob>,
}
impl ManagedJobClaim {
    pub(crate) fn snapshot(&self) -> Arc<LaunchSnapshot> {
        self.job.snapshot.clone()
    }
}
impl Drop for ManagedJobClaim {
    fn drop(&mut self) {
        let mut owner = self.job.reader.lock();
        if matches!(*owner, ReaderOwner::Claimed) {
            *owner = ReaderOwner::Returned;
        }
    }
}
impl ManagedJob {
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub(crate) fn active_processes(&self) -> Result<u32, SafeError> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(HANDLE(self.original.as_raw_handle())),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as _,
                std::mem::size_of_val(&info) as u32,
                None,
            )
        }
        .map_err(|_| error("MANAGED_JOB_QUERY_UNKNOWN"))?;
        Ok(info.ActiveProcesses)
    }
    pub(crate) fn request_stop(&self) -> Result<(), SafeError> {
        match self.stop_state.compare_exchange(
            JobStopState::Idle as u8,
            JobStopState::InFlight as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {}
            Err(state) if state == JobStopState::Accepted as u8 => return Ok(()),
            Err(state) if state == JobStopState::InFlight as u8 => {
                return Err(error("MANAGED_JOB_STOP_PENDING"))
            }
            Err(_) => return Err(error("MANAGED_JOB_STOP_UNKNOWN")),
        }
        let result = self.terminate_original();
        self.stop_state.store(
            if result.is_ok() {
                JobStopState::Accepted as u8
            } else {
                JobStopState::Unknown as u8
            },
            Ordering::Release,
        );
        result
    }
    fn terminate_original(&self) -> Result<(), SafeError> {
        #[cfg(test)]
        {
            let effect = self.stop_effect.lock().take();
            if let Some(effect) = effect {
                return effect();
            }
        }
        unsafe { TerminateJobObject(HANDLE(self.original.as_raw_handle()), 1) }
            .map_err(|_| error("MANAGED_JOB_STOP_UNKNOWN"))
    }
    /// Called at actual supervisor insertion, before it may take the sole reader.
    pub(crate) fn register(
        &self,
        run: &RunKey,
        resource: &Arc<NativeRun>,
        insert: impl FnOnce(),
    ) -> Result<(), SafeError> {
        let mut owner = self.reader.lock();
        // Closure revokes publication, not the obligation to register a cleanup
        // owner for this exact already-created resource. The supervisor stops a
        // closed claim and returns no successful launch response.
        if !matches!(*owner, ReaderOwner::Claimed)
            || !self
                .pty
                .upgrade()
                .is_some_and(|pty| Arc::ptr_eq(&pty, &resource.process.pty))
            || !Arc::ptr_eq(&self.snapshot, &resource.process.snapshot)
            || run.run_id != self.snapshot.request().run_id
            || run.generation != self.snapshot.request().generation
        {
            return Err(error("MANAGED_REGISTRATION_REJECTED"));
        }
        insert();
        *owner = ReaderOwner::Supervised {
            resource: Arc::downgrade(resource),
            run: run.clone(),
        };
        Ok(())
    }
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
    job: Option<OwnedHandle>,
    managed: Option<Arc<ManagedJob>>,
    attempt: Option<Arc<()>>,
    snapshot: Option<Arc<LaunchSnapshot>>,
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
    closed: Arc<AtomicBool>,
    retired_io: AtomicU8,
    drained_bytes: AtomicU64,
    #[cfg(test)]
    claim_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl OwnedJobLaunchSlot {
    pub(crate) fn start_native(
        &self,
        snapshot: Arc<LaunchSnapshot>,
        ticket: StartTicket,
    ) -> Result<ManagedStartAttempt, SafeError> {
        let mut preparation = ticket.preparing();
        let request = snapshot.request();
        let spec = super::launch::resolve_process(&crate::cli::invocation::build_invocation(
            request, &snapshot,
        )?)?;
        let command = spec.command()?;
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: request.rows,
                cols: request.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|_| error("HOST_PTY_UNAVAILABLE"))?;
        let handle = unsafe { windows::Win32::System::JobObjects::CreateJobObjectW(None, None) }
            .map_err(|_| error("MANAGED_JOB_UNAVAILABLE"))?;
        let job = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let identity = Arc::new(());
        let result = self.launch_bound(
            pair,
            command,
            preparation.begin_creation(),
            job,
            Some((identity.clone(), snapshot.clone())),
        );
        let slot = self.custody.lock();
        if result != JobLaunchObservation::Pending
            || !slot.as_ref().is_some_and(|owned| {
                owned
                    .attempt
                    .as_ref()
                    .is_some_and(|id| Arc::ptr_eq(id, &identity))
                    && owned.assigned
                    && matches!(owned.root, Some(Root::Started(_)))
            })
        {
            return Err(error("MANAGED_START_NOT_PUBLISHABLE"));
        }
        Ok(ManagedStartAttempt { identity, snapshot })
    }
    pub(crate) fn claim_for_native_run(
        &self,
        attempt: Option<ManagedStartAttempt>,
    ) -> Result<ManagedJobClaim, SafeError> {
        let attempt = attempt.ok_or_else(|| error("MANAGED_START_PROOF_REQUIRED"))?;
        let mut slot = self.custody.lock();
        let owned = slot
            .as_mut()
            .ok_or_else(|| error("MANAGED_START_MISSING"))?;
        if self.closed.load(Ordering::Acquire)
            || owned.failed
            || owned.stop_requested
            || owned.managed.is_some()
            || !owned
                .attempt
                .as_ref()
                .is_some_and(|id| Arc::ptr_eq(id, &attempt.identity))
            || !owned
                .snapshot
                .as_ref()
                .is_some_and(|s| Arc::ptr_eq(s, &attempt.snapshot))
            || !matches!(owned.reader_worker, OutputReaderJoin::Unstarted)
        {
            return Err(error("MANAGED_CLAIM_REJECTED"));
        }
        let Some(Root::Started(pty)) = owned.root.as_ref() else {
            return Err(error("MANAGED_RESUME_UNKNOWN"));
        };
        let pty = pty.clone();
        let job = owned
            .job
            .take()
            .ok_or_else(|| error("MANAGED_JOB_MISSING"))?;
        let job = Arc::new(ManagedJob {
            original: job,
            pty: Arc::downgrade(&pty),
            snapshot: attempt.snapshot,
            reader: Mutex::new(ReaderOwner::Claimed),
            closed: self.closed.clone(),
            stop_state: AtomicU8::new(JobStopState::Idle as u8),
            #[cfg(test)]
            stop_effect: Mutex::new(None),
        });
        owned.managed = Some(job.clone());
        let claim = ManagedJobClaim { pty, job };
        drop(slot);
        #[cfg(test)]
        {
            let hook = self.claim_hook.lock().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        Ok(claim)
    }
    pub(crate) fn complete_native_transfer(
        &self,
        job: &Arc<ManagedJob>,
        resource: &Arc<NativeRun>,
    ) -> Result<(), SafeError> {
        let mut slot = self.custody.lock();
        let owned = slot
            .as_ref()
            .ok_or_else(|| error("MANAGED_START_MISSING"))?;
        if !owned
            .managed
            .as_ref()
            .is_some_and(|original| Arc::ptr_eq(original, job))
        {
            return Err(error("MANAGED_REGISTRATION_REJECTED"));
        }
        let registered = matches!(&*job.reader.lock(),ReaderOwner::Supervised{resource:original,run} if Weak::ptr_eq(original,&Arc::downgrade(resource))&&run.run_id==job.snapshot.request().run_id&&run.generation==job.snapshot.request().generation);
        if !registered {
            return Err(error("MANAGED_REGISTRATION_PENDING"));
        }
        drop(slot.take());
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn set_claim_hook_for_test(&self, hook: Box<dyn FnOnce() + Send>) {
        *self.claim_hook.lock() = Some(hook);
    }
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
        self.launch_bound(pair, command, ticket, job, None)
    }
    fn launch_bound(
        &self,
        pair: PtyPair,
        command: CommandBuilder,
        ticket: StartTicket,
        job: OwnedHandle,
        binding: Option<(Arc<()>, Arc<LaunchSnapshot>)>,
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
            job: Some(job),
            managed: None,
            attempt: binding.as_ref().map(|b| b.0.clone()),
            snapshot: binding.map(|b| b.1),
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
            .spawn_command_in_owned_job(command, owned.job.as_ref().unwrap().as_handle())
        {
            Ok(child) => {
                let child = AdmittedChild::created(child, owned.preparing.take().unwrap());
                owned.root = Some(Root::Started(Arc::new(
                    owned.prepared.take().unwrap().finish(child),
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
                Some(owned.job_handle()),
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
        if let Some(job) = owned.managed.clone() {
            // Claim, not registration, transfers sole-reader custody. A close
            // in that gap may stop the Job but cannot start another reader.
            let returned = matches!(*job.reader.lock(), ReaderOwner::Returned);
            if !returned {
                drop(slot);
                return match job.request_stop() {
                    Ok(()) => JobLaunchObservation::Pending,
                    Err(failure) if failure.code == "MANAGED_JOB_STOP_PENDING" => {
                        JobLaunchObservation::Pending
                    }
                    Err(_) => JobLaunchObservation::Unknown,
                };
            }
        }
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
                        Some(owned.job_handle()),
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
                    unsafe { TerminateJobObject(owned.job_handle(), 1) }.map_err(|_| ())
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
    fn job_handle(&self) -> HANDLE {
        HANDLE(
            self.job
                .as_ref()
                .map(|job| job.as_raw_handle())
                .unwrap_or_else(|| {
                    self.managed
                        .as_ref()
                        .expect("original Job remains owned")
                        .original
                        .as_raw_handle()
                }),
        )
    }
    fn begin_drain(&mut self) -> bool {
        if self
            .managed
            .as_ref()
            .is_some_and(|job| !matches!(*job.reader.lock(), ReaderOwner::Returned))
        {
            return false;
        }
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
    #[test]
    #[allow(non_snake_case)]
    fn ManagedJob_EmptySlotCannotPublishNativeRun_010() {
        let slot = OwnedJobLaunchSlot::default();
        assert!(slot.claim_for_native_run(None).is_err());
        assert_eq!(slot.observe(), JobLaunchObservation::Empty);
    }
    fn no_process_snapshot(cwd: &std::path::Path, program: String) -> Arc<LaunchSnapshot> {
        use crate::cli::{
            environment::EnvMap,
            profiles::{Override, Profile},
            snapshot::{freeze_launch, CallerIdentity, FreezeContext},
            types::{CliKind, LaunchRequest, WireU64},
        };
        let mut profile = Profile::new("TEST_ONLY_MANAGED", CliKind::Codex);
        profile.program_path = Override::Set(program);
        let request:LaunchRequest=serde_json::from_value(serde_json::json!({"requestId":"TEST_ONLY_REQUEST","tabId":"TEST_ONLY_TAB","runId":"TEST_ONLY_RUN","generation":1,"profileId":profile.id,"expectedProfileRevision":"0","cli":"codex","launchCwd":cwd,"action":{"kind":"raw","argv":[]},"extraArgs":[],"cols":80,"rows":24})).unwrap();
        let caller = CallerIdentity {
            instance_id: "TEST_ONLY".into(),
            window_label: "main".into(),
            webview_epoch: WireU64::parse("1").unwrap(),
        };
        let empty = EnvMap::new();
        Arc::new(
            freeze_launch(
                &request,
                &profile,
                &caller,
                &FreezeContext {
                    inherited: &empty,
                    terminal: &empty,
                    legacy: None,
                    observer: None,
                },
            )
            .unwrap(),
        )
    }
    #[test]
    #[allow(non_snake_case)]
    fn ManagedJob_PreCreateRejectionSettlesOriginalAdmission_012() {
        use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("TEST_ONLY_NEVER_EXECUTED.ps1");
        std::fs::write(
            &program,
            b"# no execution; Native launcher rejects this extension",
        )
        .unwrap();
        let snapshot = no_process_snapshot(temp.path(), program.to_string_lossy().into_owned());
        let gate = AdmissionGate::new();
        let slot = OwnedJobLaunchSlot::default();
        let result = slot.start_native(snapshot, gate.begin_start(RuntimeKind::Native).unwrap());
        assert!(result.is_err());
        assert_eq!(slot.observe(), JobLaunchObservation::Empty);
        assert!(
            gate.freeze(&uuid::Uuid::new_v4().to_string()).is_ok(),
            "known pre-Create rejection poisoned admission"
        );
    }
    #[test]
    #[allow(non_snake_case)]
    fn ManagedJob_StopFailureAndInFlightNeverBecomeAccepted_013() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = no_process_snapshot(
            temp.path(),
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let raw =
            unsafe { windows::Win32::System::JobObjects::CreateJobObjectW(None, None) }.unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let job = Arc::new(ManagedJob {
            original: unsafe { OwnedHandle::from_raw_handle(raw.0) },
            pty: Weak::new(),
            snapshot,
            reader: Mutex::new(ReaderOwner::Claimed),
            closed: Arc::new(AtomicBool::new(false)),
            stop_state: AtomicU8::new(JobStopState::Idle as u8),
            stop_effect: Mutex::new(Some(Box::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                entered_tx.send(()).unwrap();
                let _ = release_rx.recv_timeout(std::time::Duration::from_secs(3));
                Err(error("MANAGED_JOB_STOP_UNKNOWN"))
            }))),
        });
        let original = job.clone();
        let worker = std::thread::spawn(move || original.request_stop());
        let entered = entered_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .is_ok();
        let in_flight = job.request_stop();
        let _ = release_tx.send(());
        let failed = worker.join().unwrap();
        let later = job.request_stop();
        // Original thread is joined before any RED assertion; this empty Job
        // and injected no-effect failure create no native process or hanging IO.
        assert!(entered);
        assert!(failed.is_err());
        assert!(
            in_flight.is_err(),
            "an in-flight original stop became Accepted"
        );
        assert!(later.is_err(), "a failed original stop became Accepted");
        assert_eq!(in_flight.unwrap_err().code, "MANAGED_JOB_STOP_PENDING");
        assert_eq!(later.unwrap_err().code, "MANAGED_JOB_STOP_UNKNOWN");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "unknown stop must not be automatically reissued"
        );
        assert_eq!(job.active_processes(), Ok(0));
    }
    #[test]
    #[allow(non_snake_case)]
    fn ManagedJob_ForeignAttemptAndClosedClaimKeepOriginalCustody_011() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = no_process_snapshot(
            temp.path(),
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let identity = Arc::new(());
        let raw =
            unsafe { windows::Win32::System::JobObjects::CreateJobObjectW(None, None) }.unwrap();
        let job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let slot = OwnedJobLaunchSlot::default();
        // No process/PTY exists in this fixture. The binding check must reject
        // before inspecting or extracting a root/reader, even with equal DTOs.
        *slot.custody.lock() = Some(Custody {
            prepared: None,
            job: Some(job),
            managed: None,
            attempt: Some(identity.clone()),
            snapshot: Some(snapshot.clone()),
            preparing: None,
            created: None,
            root: None,
            assigned: false,
            stop_requested: false,
            failed: false,
            process_quiescent: true,
            io_drop: IoDropWitness::default(),
            reader_handoff: Arc::new(Mutex::new(None)),
            drain_facts: Arc::new(DrainFacts::default()),
            reader_worker: OutputReaderJoin::Unstarted,
        });
        let wrong_attempt = slot.claim_for_native_run(Some(ManagedStartAttempt {
            identity: Arc::new(()),
            snapshot: snapshot.clone(),
        }));
        let same_dto_different_snapshot = slot.claim_for_native_run(Some(ManagedStartAttempt {
            identity: identity.clone(),
            snapshot: Arc::new((*snapshot).clone()),
        }));
        slot.close_admission();
        let closed = slot.claim_for_native_run(Some(ManagedStartAttempt { identity, snapshot }));
        let intact = slot.custody.lock().as_ref().is_some_and(|owned| {
            owned.job.is_some()
                && owned.managed.is_none()
                && matches!(owned.reader_worker, OutputReaderJoin::Unstarted)
        });
        // Explicit no-process fixture teardown, never a native Unknown escape.
        drop(slot.custody.lock().take());
        assert!(wrong_attempt.is_err());
        assert!(same_dto_different_snapshot.is_err());
        assert!(closed.is_err());
        assert!(intact);
    }
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
                job: Some(job),
                managed: None,
                attempt: None,
                snapshot: None,
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
