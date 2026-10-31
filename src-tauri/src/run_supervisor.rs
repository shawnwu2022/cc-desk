//! Production owner for native PTY wait, output drain, and parsed completion.
//! Process exit, PTY EOF, and renderer parsing are deliberately independent.

use crate::cli::launch_service::{NativeRun, RunSupervisor};
use crate::cli::profiles::error;
use crate::cli::run_registry::{LaunchStatus, RunKey, RunRegistry};
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{SafeError, WireU64};
#[cfg(test)]
use crate::run_lifecycle::ProcessLifecycle;
use crate::run_lifecycle::{LifecycleRecord, OutputLifecycle};
use crate::terminal_transport::{OutputProgress, TerminalStream, TerminalTransports};
use parking_lot::{Condvar, Mutex};
use std::collections::{HashMap, VecDeque};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

type RunIdentity = (String, u32);

// 诊断快照保留上限：超过后按完成顺序淘汰最老记录，避免 completed 无界增长。
const SUPERVISOR_COMPLETED_MAX: usize = 256;
const SUPERVISOR_RETRY_DELAY: Duration = Duration::from_millis(100);

#[cfg(all(windows, any(test, feature = "remote-host")))]
pub(crate) fn drain_unpublished_output(reader: &mut dyn std::io::Read) -> Result<u64, ()> {
    // Shutdown-only discard, never an authorized TerminalStream or replay body.
    let mut scratch = [0u8; 8192];
    let mut count = 0u64;
    loop {
        match reader.read(&mut scratch) {
            Ok(0) => return Ok(count),
            Ok(bytes) => count = count.checked_add(bytes as u64).ok_or(())?,
            Err(failure) if crate::pty::is_pty_stream_end(&failure) => return Ok(count),
            Err(_) => return Err(()),
        }
    }
}

#[cfg(all(windows, test))]
mod unpublished_drain_tests {
    use super::drain_unpublished_output;
    use std::io::{self, Cursor, Read};
    #[test]
    #[allow(non_snake_case)]
    fn UnpublishedDrain_EofUsesBoundedScratch_001() {
        struct Bounded(Cursor<Vec<u8>>);
        impl Read for Bounded {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                assert!(
                    bytes.len() <= 8192,
                    "unpublished output must not allocate an unbounded frame"
                );
                self.0.read(bytes)
            }
        }
        let mut reader = Bounded(Cursor::new(vec![b'x'; 1024 * 1024]));
        assert_eq!(drain_unpublished_output(&mut reader), Ok(1024 * 1024));
        assert_eq!(reader.0.position(), 1024 * 1024);
    }
    #[test]
    #[allow(non_snake_case)]
    fn UnpublishedDrain_ReadFailureIsNotEof_002() {
        struct Failure;
        impl Read for Failure {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("TEST_ONLY_READ_FAILURE"))
            }
        }
        assert_eq!(drain_unpublished_output(&mut Failure), Err(()));
    }
}

#[derive(Default)]
struct CompletedRuns {
    order: VecDeque<RunIdentity>,
    records: HashMap<RunIdentity, LifecycleRecord>,
}

fn record_completed(completed: &mut CompletedRuns, key: RunIdentity, snapshot: LifecycleRecord) {
    completed.records.insert(key.clone(), snapshot);
    if !completed.order.contains(&key) {
        completed.order.push_back(key.clone());
    }
    while completed.order.len() > SUPERVISOR_COMPLETED_MAX {
        let Some(oldest) = completed.order.pop_front() else {
            break;
        };
        completed.records.remove(&oldest);
    }
}

struct SupervisorCore {
    transports: Arc<TerminalTransports>,
    active: Mutex<HashMap<RunIdentity, Arc<SupervisedRun>>>,
    completed: Mutex<CompletedRuns>,
    shutting_down: AtomicBool,
    #[cfg(any(test, feature = "remote-host"))]
    owned_shutdown_started: AtomicBool,
    settled: Condvar,
    #[cfg(test)]
    fail_next_wait: AtomicBool,
    #[cfg(test)]
    fail_wait_count: AtomicUsize,
    #[cfg(test)]
    fail_wait_after_stop: AtomicBool,
    #[cfg(test)]
    fail_next_mark_exited: AtomicBool,
    #[cfg(test)]
    fail_next_retire: AtomicBool,
    #[cfg(test)]
    registry_step_calls: AtomicUsize,
    #[cfg(test)]
    automatic_retry_calls: AtomicUsize,
    #[cfg(test)]
    automatic_retry_paused: Mutex<bool>,
    #[cfg(test)]
    automatic_retry_waiting: AtomicBool,
    #[cfg(test)]
    automatic_retry_changed: Condvar,
    #[cfg(test)]
    registry_steps_paused: Mutex<bool>,
    #[cfg(test)]
    registry_steps_changed: Condvar,
    #[cfg(test)]
    retirement_publish_paused: Mutex<bool>,
    #[cfg(test)]
    retirement_publish_changed: Condvar,
    #[cfg(test)]
    adoption_setup_paused: Mutex<bool>,
    #[cfg(test)]
    adoption_setup_waiting: AtomicBool,
    #[cfg(test)]
    adoption_setup_changed: Condvar,
    #[cfg(test)]
    reader_exit_paused: Mutex<bool>,
    #[cfg(test)]
    reader_exit_waiting: AtomicBool,
    #[cfg(test)]
    reader_exit_changed: Condvar,
    #[cfg(test)]
    wait_exit_paused: Mutex<bool>,
    #[cfg(test)]
    wait_exit_waiting: AtomicBool,
    #[cfg(test)]
    wait_exit_changed: Condvar,
}

pub(crate) struct NativeRunSupervisor {
    core: Arc<SupervisorCore>,
}

impl NativeRunSupervisor {
    pub(crate) fn new(transports: Arc<TerminalTransports>) -> Self {
        Self {
            core: Arc::new(SupervisorCore {
                transports,
                active: Mutex::new(HashMap::new()),
                completed: Mutex::new(CompletedRuns::default()),
                shutting_down: AtomicBool::new(false),
                #[cfg(any(test, feature = "remote-host"))]
                owned_shutdown_started: AtomicBool::new(false),
                settled: Condvar::new(),
                #[cfg(test)]
                fail_next_wait: AtomicBool::new(false),
                #[cfg(test)]
                fail_wait_count: AtomicUsize::new(0),
                #[cfg(test)]
                fail_wait_after_stop: AtomicBool::new(false),
                #[cfg(test)]
                fail_next_mark_exited: AtomicBool::new(false),
                #[cfg(test)]
                fail_next_retire: AtomicBool::new(false),
                #[cfg(test)]
                registry_step_calls: AtomicUsize::new(0),
                #[cfg(test)]
                automatic_retry_calls: AtomicUsize::new(0),
                #[cfg(test)]
                automatic_retry_paused: Mutex::new(false),
                #[cfg(test)]
                automatic_retry_waiting: AtomicBool::new(false),
                #[cfg(test)]
                automatic_retry_changed: Condvar::new(),
                #[cfg(test)]
                registry_steps_paused: Mutex::new(false),
                #[cfg(test)]
                registry_steps_changed: Condvar::new(),
                #[cfg(test)]
                retirement_publish_paused: Mutex::new(false),
                #[cfg(test)]
                retirement_publish_changed: Condvar::new(),
                #[cfg(test)]
                adoption_setup_paused: Mutex::new(false),
                #[cfg(test)]
                adoption_setup_waiting: AtomicBool::new(false),
                #[cfg(test)]
                adoption_setup_changed: Condvar::new(),
                #[cfg(test)]
                reader_exit_paused: Mutex::new(false),
                #[cfg(test)]
                reader_exit_waiting: AtomicBool::new(false),
                #[cfg(test)]
                reader_exit_changed: Condvar::new(),
                #[cfg(test)]
                wait_exit_paused: Mutex::new(false),
                #[cfg(test)]
                wait_exit_waiting: AtomicBool::new(false),
                #[cfg(test)]
                wait_exit_changed: Condvar::new(),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_next_wait_for_test(&self) {
        self.core.fail_next_wait.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn fail_wait_count_for_test(&self, count: usize) {
        self.core.fail_wait_count.store(count, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn fail_wait_after_stop_for_test(&self) {
        self.core.fail_wait_after_stop.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn fail_next_registry_steps_for_test(&self) {
        self.core
            .fail_next_mark_exited
            .store(true, Ordering::SeqCst);
        self.core.fail_next_retire.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn fail_next_mark_exited_for_test(&self) {
        self.core
            .fail_next_mark_exited
            .store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn registry_step_calls_for_test(&self) -> usize {
        self.core.registry_step_calls.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn automatic_retry_calls_for_test(&self) -> usize {
        self.core.automatic_retry_calls.load(Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn retry_handle_waiting_for_test(&self, run: &RunKey) -> bool {
        self.core.automatic_retry_waiting.load(Ordering::SeqCst)
            && self
                .core
                .active
                .lock()
                .get(&identity(run))
                .is_some_and(|state| {
                    state.wait_workers.lock().handles.iter().any(|worker| {
                        worker.thread().name() == Some("native-run-wait-retry")
                            && !worker.is_finished()
                    })
                })
    }

    #[cfg(test)]
    pub(crate) fn pause_automatic_retry_for_test(&self) {
        *self.core.automatic_retry_paused.lock() = true;
    }

    #[cfg(test)]
    pub(crate) fn release_automatic_retry_for_test(&self) {
        *self.core.automatic_retry_paused.lock() = false;
        self.core.automatic_retry_changed.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn pause_registry_steps_for_test(&self) {
        *self.core.registry_steps_paused.lock() = true;
    }

    #[cfg(test)]
    pub(crate) fn release_registry_steps_for_test(&self) {
        *self.core.registry_steps_paused.lock() = false;
        self.core.registry_steps_changed.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn pause_reader_exit_for_test(&self) {
        *self.core.reader_exit_paused.lock() = true;
    }
    #[cfg(test)]
    pub(crate) fn pause_wait_exit_for_test(&self) {
        *self.core.wait_exit_paused.lock() = true;
    }
    #[cfg(test)]
    pub(crate) fn release_wait_exit_for_test(&self) {
        *self.core.wait_exit_paused.lock() = false;
        self.core.wait_exit_changed.notify_all();
    }
    #[cfg(test)]
    pub(crate) fn wait_exit_waiting_for_test(&self) -> bool {
        self.core.wait_exit_waiting.load(Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn release_reader_exit_for_test(&self) {
        *self.core.reader_exit_paused.lock() = false;
        self.core.reader_exit_changed.notify_all();
    }
    #[cfg(test)]
    pub(crate) fn reader_exit_waiting_for_test(&self) -> bool {
        self.core.reader_exit_waiting.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn pause_retirement_publish_for_test(&self) {
        *self.core.retirement_publish_paused.lock() = true;
    }

    #[cfg(test)]
    pub(crate) fn release_retirement_publish_for_test(&self) {
        *self.core.retirement_publish_paused.lock() = false;
        self.core.retirement_publish_changed.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn pause_adoption_setup_for_test(&self) {
        *self.core.adoption_setup_paused.lock() = true;
    }

    #[cfg(test)]
    pub(crate) fn adoption_setup_waiting_for_test(&self) -> bool {
        self.core.adoption_setup_waiting.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn release_adoption_setup_for_test(&self) {
        *self.core.adoption_setup_paused.lock() = false;
        self.core.adoption_setup_changed.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn retry_ready_for_test(&self, run: &RunKey) -> bool {
        self.core
            .active
            .lock()
            .get(&identity(run))
            .is_some_and(|state| !state.waiter_running.load(Ordering::SeqCst))
    }

    #[cfg(test)]
    pub(crate) fn active_for_test(&self, run: &RunKey) -> bool {
        self.core.active.lock().contains_key(&identity(run))
    }

    #[cfg(test)]
    pub(crate) fn stream_for_test(&self, run: &RunKey) -> Option<Arc<TerminalStream>> {
        let state = self.core.active.lock().get(&identity(run)).cloned()?;
        let stream = state.stream.lock().clone();
        stream
    }

    #[allow(dead_code)] // D15 test/diagnostic observation; not a wire command.
    pub(crate) fn snapshot(&self, run: &RunKey) -> Option<LifecycleRecord> {
        let key = identity(run);
        if let Some(state) = self.core.active.lock().get(&key).cloned() {
            return Some(state.lifecycle.lock().clone());
        }
        self.core.completed.lock().records.get(&key).cloned()
    }

    pub(crate) fn stop(
        &self,
        caller: &CallerIdentity,
        receipt: &LaunchStatus,
    ) -> Result<(), SafeError> {
        let state = self
            .core
            .active
            .lock()
            .get(&identity(&receipt.run))
            .cloned()
            .ok_or_else(|| error("RUN_NOT_FOUND"))?;
        state.authorize_stop(caller, receipt)?;
        state.request_stop()
    }

    pub(crate) fn shutdown(&self) {
        const SHUTDOWN_REAP_TIMEOUT: Duration = Duration::from_secs(5);

        self.core.shutting_down.store(true, Ordering::SeqCst);
        let started = Instant::now();
        let mut next_control_at = started;
        loop {
            if Instant::now() >= next_control_at {
                let states: Vec<_> = self.core.active.lock().values().cloned().collect();
                if states.is_empty() {
                    return;
                }
                for state in states {
                    if let Err(failure) = state.request_stop() {
                        log::warn!(
                            "native run shutdown control failed for {}:{}: {}",
                            state.run.run_id,
                            state.run.generation,
                            failure.code
                        );
                    }
                }
                next_control_at = Instant::now() + SUPERVISOR_RETRY_DELAY;
            }

            let remaining = SHUTDOWN_REAP_TIMEOUT.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                log::warn!(
                    "native run shutdown timed out with {} supervised run(s) still active",
                    self.core.active.lock().len()
                );
                return;
            }
            let mut active = self.core.active.lock();
            if active.is_empty() {
                return;
            }
            // 早到的故障通知只用于重查 active；下一次控制仍受单调时钟节流。
            let until_control = next_control_at.saturating_duration_since(Instant::now());
            self.core
                .settled
                .wait_for(&mut active, remaining.min(until_control));
        }
    }
}

impl RunSupervisor for NativeRunSupervisor {
    fn adopt(
        &self,
        registry: Arc<RunRegistry<NativeRun>>,
        run: &RunKey,
        resource: Arc<NativeRun>,
    ) -> Result<(), SafeError> {
        let state = SupervisedRun::new(
            Arc::downgrade(&self.core),
            registry,
            run.clone(),
            resource.clone(),
        )?;
        {
            let mut active = self.core.active.lock();
            if active.contains_key(&identity(run)) {
                return Err(error("RUN_SUPERVISOR_BUSY"));
            }
            active.insert(identity(run), state.clone());
        }
        let mut adoption = AdoptionGuard::new(state.clone());

        // Install the reap owner before any transport setup that can fail. Once
        // spawn has published a NativeRun, an attach/setup failure must still
        // terminate and reap that exact creation-owned child.
        if state.start_waiter().is_err() {
            return Err(error("RUN_SUPERVISOR_FAILED"));
        }

        #[cfg(test)]
        {
            let mut paused = self.core.adoption_setup_paused.lock();
            self.core
                .adoption_setup_waiting
                .store(true, Ordering::SeqCst);
            while *paused {
                self.core.adoption_setup_changed.wait(&mut paused);
            }
            self.core
                .adoption_setup_waiting
                .store(false, Ordering::SeqCst);
        }

        let mut reader = resource.process.pty.take_reader()?;

        let progress: Arc<dyn OutputProgress> = state.clone();
        let stream = self.core.transports.attach_observed(
            resource.process.snapshot.owner().clone(),
            run.clone(),
            resource.route(),
            progress,
        )?;
        state.set_stream(stream.clone())?;

        #[cfg(any(test, feature = "remote-host"))]
        {
            if !state.output_reader.lock().reserve() {
                return Err(error("RUN_SUPERVISOR_FAILED"));
            }
        }
        let reader_state = state.clone();
        let reader_worker = std::thread::Builder::new()
            .name("native-output-reader".into())
            .spawn(move || {
                loop {
                    match stream.pump_once_with_end(&mut *reader, crate::pty::is_pty_stream_end) {
                        Ok(0) => {
                            reader_state.output_end(stream.stream_epoch(), stream.sent_offset());
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => {
                            reader_state.reader_failed();
                            break;
                        }
                    }
                }
                #[cfg(test)]
                if let Some(core) = reader_state.core.upgrade() {
                    let mut paused = core.reader_exit_paused.lock();
                    core.reader_exit_waiting.store(true, Ordering::SeqCst);
                    while *paused {
                        core.reader_exit_changed.wait(&mut paused);
                    }
                    core.reader_exit_waiting.store(false, Ordering::SeqCst);
                }
            });
        match reader_worker {
            Ok(worker) => {
                #[cfg(any(test, feature = "remote-host"))]
                {
                    *state.output_reader.lock() = OutputReaderJoin::Running(worker);
                }
                #[cfg(not(any(test, feature = "remote-host")))]
                drop(worker);
            }
            Err(_) => {
                #[cfg(any(test, feature = "remote-host"))]
                {
                    *state.output_reader.lock() = OutputReaderJoin::Unknown;
                }
                return Err(error("RUN_SUPERVISOR_FAILED"));
            }
        }

        // Close the shutdown-vs-adopt race. shutdown() may have taken its active
        // snapshot before this state was inserted; a post-install check makes the
        // newly adopted run incomplete and stops it without a second spawn.
        if self.core.shutting_down.load(Ordering::SeqCst) {
            state.request_stop()?;
        }

        adoption.complete();
        Ok(())
    }
}

struct AdoptionGuard {
    state: Arc<SupervisedRun>,
    complete: bool,
}

impl AdoptionGuard {
    fn new(state: Arc<SupervisedRun>) -> Self {
        Self {
            state,
            complete: false,
        }
    }

    fn complete(&mut self) {
        self.state.complete_adoption();
        self.complete = true;
    }
}

impl Drop for AdoptionGuard {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        self.state.reader_failed();
        let _ = self.state.request_stop();
        self.state.complete_adoption();
    }
}

/// Exactly one existing output reader; never a claim about waiter/master/tree.
#[cfg(any(test, feature = "remote-host"))]
pub(crate) enum OutputReaderJoin {
    Unstarted,
    Publishing,
    Running(std::thread::JoinHandle<()>),
    Joined,
    Unknown,
}
#[cfg(any(test, feature = "remote-host"))]
impl OutputReaderJoin {
    pub(crate) fn reserve(&mut self) -> bool {
        if !matches!(self, Self::Unstarted) {
            return false;
        }
        *self = Self::Publishing;
        true
    }
    pub(crate) fn observe(&mut self) -> bool {
        if matches!(self,Self::Running(worker) if worker.is_finished()) {
            let Self::Running(worker) = std::mem::replace(self, Self::Unknown) else {
                unreachable!()
            };
            *self = if worker.join().is_ok() {
                Self::Joined
            } else {
                Self::Unknown
            };
        }
        matches!(self, Self::Joined)
    }
}

/// Only this run's original waiter/retry threads, never global process evidence.
#[cfg(any(test, feature = "remote-host"))]
#[derive(Default)]
struct WaitWorkers {
    handles: Vec<std::thread::JoinHandle<()>>,
    started: bool,
    panicked: bool,
}
#[cfg(any(test, feature = "remote-host"))]
#[derive(PartialEq, Eq)]
enum WaitWorkersState {
    Pending,
    Joined,
    Unknown,
}
#[cfg(any(test, feature = "remote-host"))]
impl WaitWorkers {
    const MAX: usize = 64;
    fn observe(&mut self) -> WaitWorkersState {
        let mut index = 0;
        while index < self.handles.len() {
            if self.handles[index].is_finished() {
                self.panicked |= self.handles.swap_remove(index).join().is_err();
            } else {
                index += 1;
            }
        }
        if !self.handles.is_empty() {
            WaitWorkersState::Pending
        } else if self.started && !self.panicked {
            WaitWorkersState::Joined
        } else {
            WaitWorkersState::Unknown
        }
    }
}

struct SupervisedRun {
    core: Weak<SupervisorCore>,
    registry: Arc<RunRegistry<NativeRun>>,
    run: RunKey,
    owner: CallerIdentity,
    request_id: String,
    lifecycle: Mutex<LifecycleRecord>,
    stream: Mutex<Option<Arc<TerminalStream>>>,
    resource: Mutex<Option<Arc<NativeRun>>>,
    stop_control: Mutex<()>,
    stop_requested: AtomicBool,
    waiter_running: AtomicBool,
    reaped: AtomicBool,
    registry_retired: AtomicBool,
    adoption_complete: AtomicBool,
    automatic_retry_used: AtomicBool,
    #[cfg(any(test, feature = "remote-host"))]
    output_reader: Mutex<OutputReaderJoin>,
    #[cfg(any(test, feature = "remote-host"))]
    wait_workers: Mutex<WaitWorkers>,
    #[cfg(any(test, feature = "remote-host"))]
    io_drop: crate::platform::owned_pty::IoDropWitness,
    changed: Condvar,
}

impl SupervisedRun {
    fn new(
        core: Weak<SupervisorCore>,
        registry: Arc<RunRegistry<NativeRun>>,
        run: RunKey,
        resource: Arc<NativeRun>,
    ) -> Result<Arc<Self>, SafeError> {
        let mut lifecycle = LifecycleRecord::new(run.clone());
        lifecycle.process_running()?;
        let owner = resource.process.snapshot.owner().clone();
        let request_id = resource.process.snapshot.request().request_id.clone();
        #[cfg(any(test, feature = "remote-host"))]
        let io_drop = resource.process.pty.io_drop_witness();
        Ok(Arc::new(Self {
            core,
            registry,
            run,
            owner,
            request_id,
            lifecycle: Mutex::new(lifecycle),
            stream: Mutex::new(None),
            resource: Mutex::new(Some(resource)),
            stop_control: Mutex::new(()),
            stop_requested: AtomicBool::new(false),
            waiter_running: AtomicBool::new(false),
            reaped: AtomicBool::new(false),
            registry_retired: AtomicBool::new(false),
            adoption_complete: AtomicBool::new(false),
            automatic_retry_used: AtomicBool::new(false),
            #[cfg(any(test, feature = "remote-host"))]
            output_reader: Mutex::new(OutputReaderJoin::Unstarted),
            #[cfg(any(test, feature = "remote-host"))]
            wait_workers: Mutex::new(WaitWorkers::default()),
            #[cfg(any(test, feature = "remote-host"))]
            io_drop,
            changed: Condvar::new(),
        }))
    }

    fn spawn_wait_worker(
        &self,
        name: &str,
        work: impl FnOnce() + Send + 'static,
    ) -> Result<bool, SafeError> {
        #[cfg(any(test, feature = "remote-host"))]
        {
            let mut workers = self.wait_workers.lock();
            workers.observe();
            // Recheck under the same lock used for publication and observation.
            // Once registry retirement plus actual joins is observed, a starter
            // delayed after its earlier check cannot create another thread.
            if self.registry_retired.load(Ordering::SeqCst) {
                drop(workers);
                drop(work);
                return Ok(false);
            }
            if workers.handles.len() >= WaitWorkers::MAX {
                drop(workers);
                drop(work);
                return Err(error("RUN_SUPERVISOR_CAPACITY"));
            }
            let handle = std::thread::Builder::new()
                .name(name.into())
                .spawn(work)
                .map_err(|_| error("RUN_SUPERVISOR_FAILED"))?;
            workers.handles.push(handle);
            workers.started = true;
            Ok(true)
        }
        #[cfg(not(any(test, feature = "remote-host")))]
        {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(work)
                .map_err(|_| error("RUN_SUPERVISOR_FAILED"))?;
            Ok(true)
        }
    }

    fn start_waiter(self: &Arc<Self>) -> Result<(), SafeError> {
        if self.registry_retired.load(Ordering::SeqCst) {
            self.release_resource_if_transport_terminal();
            self.finish_if_terminal();
            return Ok(());
        }
        if self
            .waiter_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(());
        }
        let resource = if self.reaped.load(Ordering::SeqCst) {
            None
        } else {
            match self.resource.lock().clone() {
                Some(resource) => Some(resource),
                None => {
                    self.waiter_running.store(false, Ordering::SeqCst);
                    return Err(error("RUN_NOT_READY"));
                }
            }
        };
        let state = self.clone();
        let spawned = self.spawn_wait_worker("native-run-waiter", move || {
            #[cfg(test)]
            let finished = state.clone();
            state.wait_and_converge(resource);
            #[cfg(test)]
            if let Some(core) = finished.core.upgrade() {
                let mut paused = core.wait_exit_paused.lock();
                core.wait_exit_waiting.store(true, Ordering::SeqCst);
                while *paused {
                    core.wait_exit_changed.wait(&mut paused);
                }
                core.wait_exit_waiting.store(false, Ordering::SeqCst);
            }
        });
        if !matches!(spawned, Ok(true)) {
            self.waiter_running.store(false, Ordering::SeqCst);
            self.notify_retryable();
        }
        spawned?;
        Ok(())
    }

    fn wait_and_converge(self: Arc<Self>, resource: Option<Arc<NativeRun>>) {
        if !self.reaped.load(Ordering::SeqCst) {
            let Some(resource) = resource else {
                self.waiter_running.store(false, Ordering::SeqCst);
                self.notify_retryable();
                return;
            };
            #[cfg(test)]
            let waited = {
                let core = self.core.upgrade();
                if core
                    .as_ref()
                    .is_some_and(|core| core.fail_wait_after_stop.load(Ordering::SeqCst))
                {
                    let mut lifecycle = self.lifecycle.lock();
                    while !self.stop_requested.load(Ordering::SeqCst) {
                        self.changed.wait(&mut lifecycle);
                    }
                    drop(lifecycle);
                    core.expect("test supervisor core must remain alive")
                        .fail_wait_after_stop
                        .store(false, Ordering::SeqCst);
                    Err(error("PROCESS_WAIT_FAILED"))
                } else if core.is_some_and(|core| {
                    core.fail_next_wait.swap(false, Ordering::SeqCst)
                        || core
                            .fail_wait_count
                            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                                remaining.checked_sub(1)
                            })
                            .is_ok()
                }) {
                    Err(error("PROCESS_WAIT_FAILED"))
                } else {
                    resource.process.pty.wait()
                }
            };
            #[cfg(not(test))]
            let waited = resource.process.pty.wait();
            if waited.is_err() {
                log::error!("native run waiter failed");
                self.process_failed();
                self.retry_after_failure();
                return;
            }
            self.reaped.store(true, Ordering::SeqCst);
        }

        if let Err(failure) = self.lifecycle.lock().process_exited() {
            log::error!("native lifecycle exit conflict: {}", failure.code);
            self.retry_after_failure();
            return;
        }
        if let Err(failure) = self.mark_registry_exited() {
            log::error!(
                "native run registry exit transition failed: {}",
                failure.code
            );
            self.retry_after_failure();
            return;
        }
        if let Err(failure) = self.retire_registry_resource() {
            log::error!("native run resource retirement failed: {}", failure.code);
            self.retry_after_failure();
            return;
        }
        #[cfg(test)]
        if let Some(core) = self.core.upgrade() {
            let mut paused = core.retirement_publish_paused.lock();
            while *paused {
                core.retirement_publish_changed.wait(&mut paused);
            }
        }
        self.registry_retired.store(true, Ordering::SeqCst);
        self.changed.notify_all();
        self.release_resource_if_transport_terminal();
        self.finish_if_terminal();
        self.await_real_output_end();
        self.waiter_running.store(false, Ordering::SeqCst);
        self.notify_retryable();
    }

    fn mark_registry_exited(&self) -> Result<(), SafeError> {
        #[cfg(test)]
        if let Some(core) = self.core.upgrade() {
            let mut paused = core.registry_steps_paused.lock();
            while *paused {
                core.registry_steps_changed.wait(&mut paused);
            }
            drop(paused);
            core.registry_step_calls.fetch_add(1, Ordering::SeqCst);
        }
        #[cfg(test)]
        if self
            .core
            .upgrade()
            .is_some_and(|core| core.fail_next_mark_exited.swap(false, Ordering::SeqCst))
        {
            return Err(error("RUN_STATE_CONFLICT"));
        }
        self.registry.mark_exited(&self.run)
    }

    fn retire_registry_resource(&self) -> Result<(), SafeError> {
        #[cfg(test)]
        if let Some(core) = self.core.upgrade() {
            core.registry_step_calls.fetch_add(1, Ordering::SeqCst);
        }
        #[cfg(test)]
        if self
            .core
            .upgrade()
            .is_some_and(|core| core.fail_next_retire.swap(false, Ordering::SeqCst))
        {
            return Err(error("RUN_NOT_READY"));
        }
        self.registry.retire(&self.run)
    }

    fn set_stream(&self, stream: Arc<TerminalStream>) -> Result<(), SafeError> {
        self.lifecycle
            .lock()
            .output_started(&stream.stream_epoch().to_string())?;
        *self.stream.lock() = Some(stream);
        Ok(())
    }

    fn authorize_stop(
        &self,
        caller: &CallerIdentity,
        receipt: &LaunchStatus,
    ) -> Result<(), SafeError> {
        if self.owner != *caller || receipt.run != self.run || receipt.request_id != self.request_id
        {
            return Err(error("FORBIDDEN"));
        }
        self.registry.check_run(caller, &self.run)?;
        match self.registry.resource(caller, &self.run) {
            Ok(current) => {
                let owned = self
                    .resource
                    .lock()
                    .clone()
                    .ok_or_else(|| error("RUN_NOT_READY"))?;
                if !Arc::ptr_eq(&current, &owned) {
                    return Err(error("FORBIDDEN"));
                }
                Ok(())
            }
            Err(failure)
                if failure.code == "RUN_NOT_READY" && self.reaped.load(Ordering::SeqCst) =>
            {
                // Registry 退役可早于输出排空；固定 state Arc、冻结 owner 与精确 run
                // 继续构成此阶段的授权绑定。
                Ok(())
            }
            Err(failure) => Err(failure),
        }
    }

    fn process_failed(&self) {
        self.lifecycle.lock().process_failed();
        self.changed.notify_all();
    }

    fn output_end(&self, epoch: WireU64, final_offset: WireU64) {
        {
            let mut lifecycle = self.lifecycle.lock();
            let result = lifecycle.output_started(&epoch.to_string()).and_then(|()| {
                lifecycle.output_end_for(&epoch.to_string(), &final_offset.to_string())
            });
            if let Err(failure) = result {
                log::error!("native lifecycle output-end conflict: {}", failure.code);
                let _ = lifecycle.mark_degraded();
            }
        }
        self.changed.notify_all();
        self.release_resource_if_transport_terminal();
        self.finish_if_terminal();
    }

    fn reader_failed(&self) {
        {
            let mut lifecycle = self.lifecycle.lock();
            if lifecycle.output() != OutputLifecycle::Degraded {
                if let Err(failure) = lifecycle.mark_incomplete() {
                    log::error!("native lifecycle reader failure conflict: {}", failure.code);
                }
            }
        }
        self.changed.notify_all();
        self.release_resource_if_transport_terminal();
        self.finish_if_terminal();
    }

    fn update_stream(
        &self,
        epoch: WireU64,
        update: impl FnOnce(&mut LifecycleRecord) -> Result<(), SafeError>,
    ) {
        {
            let mut lifecycle = self.lifecycle.lock();
            let result = lifecycle
                .output_started(&epoch.to_string())
                .and_then(|()| update(&mut lifecycle));
            if let Err(failure) = result {
                log::error!("native lifecycle transport conflict: {}", failure.code);
                let _ = lifecycle.mark_degraded();
            }
        }
        self.changed.notify_all();
        self.release_resource_if_transport_terminal();
        self.finish_if_terminal();
    }

    fn request_stop(self: &Arc<Self>) -> Result<(), SafeError> {
        // 每次显式控制允许一次延迟自动重试；连续底层错误仍保持未知并等待下一次控制。
        self.automatic_retry_used.store(false, Ordering::SeqCst);
        {
            let mut lifecycle = self.lifecycle.lock();
            lifecycle.mark_incomplete()?;
        }
        self.changed.notify_all();
        {
            let _control = self.stop_control.lock();
            if !self.stop_requested.load(Ordering::SeqCst) {
                if !self.reaped.load(Ordering::SeqCst) {
                    let resource = self
                        .resource
                        .lock()
                        .clone()
                        .ok_or_else(|| error("RUN_NOT_READY"))?;
                    // 只在系统接受 terminate 后发布 stop_requested，避免并发 stop 误报成功。
                    resource.process.pty.terminate_root()?;
                }
                // 真实 reap 已知时无需 terminate；两种成功控制路径都登记后续有界重试。
                self.stop_requested.store(true, Ordering::SeqCst);
                self.changed.notify_all();
            }
        }
        self.start_waiter()
    }

    fn await_real_output_end(&self) {
        let started = Instant::now();
        let mut lifecycle = self.lifecycle.lock();
        while lifecycle.process() == crate::run_lifecycle::ProcessLifecycle::Exited
            && lifecycle.output() == OutputLifecycle::Draining
            && lifecycle.final_offset().is_none()
        {
            let remaining = crate::pty::PTY_OUTPUT_DRAIN_TIMEOUT.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break;
            }
            self.changed.wait_for(&mut lifecycle, remaining);
        }

        let timed_out = lifecycle.process() == crate::run_lifecycle::ProcessLifecycle::Exited
            && lifecycle.output() == OutputLifecycle::Draining
            && lifecycle.final_offset().is_none();
        if timed_out {
            if let Err(failure) = lifecycle.mark_incomplete() {
                log::error!("native lifecycle drain timeout conflict: {}", failure.code);
            }
        }
        drop(lifecycle);

        if timed_out {
            // ConPTY can keep its output pipe open after the root process exits.
            // Closing the owned PTY is the bounded convergence action. We do not
            // manufacture output-end/finalOffset, so this run remains incomplete.
            self.resource.lock().take();
            self.changed.notify_all();
            self.finish_if_terminal();
        }
    }

    fn release_resource_if_transport_terminal(&self) {
        let release = {
            let lifecycle = self.lifecycle.lock();
            lifecycle.process() == crate::run_lifecycle::ProcessLifecycle::Exited
                && (lifecycle.final_offset().is_some()
                    || matches!(
                        lifecycle.output(),
                        OutputLifecycle::Degraded | OutputLifecycle::Incomplete
                    ))
        };
        if release {
            self.resource.lock().take();
        }
    }

    fn complete_adoption(&self) {
        self.adoption_complete.store(true, Ordering::SeqCst);
        self.changed.notify_all();
        self.release_resource_if_transport_terminal();
        self.finish_if_terminal();
    }

    fn finish_if_terminal(&self) {
        if !self.adoption_complete.load(Ordering::SeqCst) {
            return;
        }
        if !self.registry_retired.load(Ordering::SeqCst) {
            return;
        }
        let snapshot = {
            let lifecycle = self.lifecycle.lock();
            if !lifecycle.can_retire() {
                return;
            }
            lifecycle.clone()
        };
        let Some(core) = self.core.upgrade() else {
            return;
        };
        let key = identity(&self.run);
        let mut active = core.active.lock();
        if !active
            .get(&key)
            .is_some_and(|current| std::ptr::eq(current.as_ref(), self))
        {
            return;
        }
        if let Some(stream) = self.stream.lock().clone() {
            if let Err(failure) = core.transports.detach(&stream) {
                log::error!("native output stream detach failed: {}", failure.code);
                return;
            }
        }
        if let Err(failure) = self
            .registry
            .complete_retirement(&self.run, &self.request_id)
        {
            log::error!(
                "native run registry retirement completion failed: {}",
                failure.code
            );
            return;
        }
        record_completed(&mut core.completed.lock(), key.clone(), snapshot);
        active.remove(&key);
        drop(active);
        core.settled.notify_all();
        // TerminalTransports stores only a Weak. Keep this strong reference
        // through EOF and the final parsed ACK, then release exact credit/route.
        self.stream.lock().take();
        self.resource.lock().take();
    }

    fn notify_retryable(&self) {
        self.changed.notify_all();
        if let Some(core) = self.core.upgrade() {
            core.settled.notify_all();
        }
    }

    fn retry_after_failure(self: &Arc<Self>) {
        self.waiter_running.store(false, Ordering::SeqCst);
        self.notify_retryable();
        if !self.stop_requested.load(Ordering::SeqCst)
            || self.automatic_retry_used.swap(true, Ordering::SeqCst)
            || self
                .core
                .upgrade()
                .is_some_and(|core| core.shutting_down.load(Ordering::SeqCst))
        {
            return;
        }
        let state = self.clone();
        if self
            .spawn_wait_worker("native-run-wait-retry", move || {
                std::thread::sleep(SUPERVISOR_RETRY_DELAY);
                if state
                    .core
                    .upgrade()
                    .is_some_and(|core| core.shutting_down.load(Ordering::SeqCst))
                {
                    return;
                }
                #[cfg(test)]
                if let Some(core) = state.core.upgrade() {
                    let mut paused = core.automatic_retry_paused.lock();
                    core.automatic_retry_waiting.store(true, Ordering::SeqCst);
                    while *paused {
                        core.automatic_retry_changed.wait(&mut paused);
                    }
                    core.automatic_retry_waiting.store(false, Ordering::SeqCst);
                    drop(paused);
                    core.automatic_retry_calls.fetch_add(1, Ordering::SeqCst);
                }
                if let Err(failure) = state.start_waiter() {
                    log::warn!("native run waiter retry failed: {}", failure.code);
                }
            })
            .is_err()
        {
            log::warn!("native run waiter retry thread failed");
        }
    }
}

impl OutputProgress for SupervisedRun {
    fn sent_through(&self, stream_epoch: WireU64, through: WireU64) {
        self.update_stream(stream_epoch, |lifecycle| {
            lifecycle.sent_through(&through.to_string())
        });
    }

    fn parsed_through(&self, stream_epoch: WireU64, through: WireU64) {
        self.update_stream(stream_epoch, |lifecycle| {
            lifecycle.parsed_through_for(&stream_epoch.to_string(), &through.to_string())
        });
    }

    fn degraded(&self, stream_epoch: WireU64) {
        self.update_stream(stream_epoch, |lifecycle| lifecycle.mark_degraded());
    }
}

fn identity(run: &RunKey) -> RunIdentity {
    (run.run_id.clone(), run.generation)
}

#[cfg(test)]
#[allow(non_snake_case)]
mod convergence_tests {
    use super::*;

    fn lifecycle() -> LifecycleRecord {
        LifecycleRecord::new(RunKey {
            run_id: "run-a".into(),
            generation: 1,
        })
    }

    // wait 失败只表示进程状态未知，不能据此释放监督权。
    #[test]
    fn Supervisor_ProcessFailure_FailedIncompleteStaysActive_001() {
        let mut record = lifecycle();
        record.process_running().unwrap();
        record.process_failed();
        assert_eq!(record.process(), ProcessLifecycle::Failed);
        assert_eq!(record.output(), OutputLifecycle::Incomplete);
        assert!(!record.can_retire());
    }

    // 已降级输出也不能把 wait 错误伪装成 OS 退出。
    #[test]
    fn Supervisor_ProcessFailure_DegradedFailureStaysActive_002() {
        let mut record = lifecycle();
        record.process_running().unwrap();
        record.output_started("1").unwrap();
        record.mark_degraded().unwrap();
        record.process_failed();
        assert_eq!(record.output(), OutputLifecycle::Degraded);
        assert!(!record.can_retire());
    }

    // 后续真实 wait/reap 证据可以将 Failed 恢复为 Exited。
    #[test]
    fn Supervisor_ProcessFailure_RealReapRecoversExited_003() {
        let mut record = lifecycle();
        record.process_running().unwrap();
        record.output_started("1").unwrap();
        record.process_failed();
        record.process_exited().unwrap();
        assert_eq!(record.process(), ProcessLifecycle::Exited);
        assert!(record.can_retire());
    }

    // completed 诊断表有界：超出上限按完成顺序淘汰最老记录。
    #[test]
    fn Supervisor_CompletedRetention_BoundedAndEvictsOldest_004() {
        let mut completed = CompletedRuns::default();
        for index in 0..(SUPERVISOR_COMPLETED_MAX + 5) {
            let run_id = format!("run-{index}");
            let mut record = LifecycleRecord::new(RunKey {
                run_id: run_id.clone(),
                generation: 1,
            });
            record.process_running().unwrap();
            record.output_started("1").unwrap();
            record.process_exited().unwrap();
            record.output_end("0").unwrap();
            record.parsed_through("0").unwrap();
            record_completed(&mut completed, (run_id, 1), record);
        }
        assert_eq!(completed.records.len(), SUPERVISOR_COMPLETED_MAX);
        assert!(!completed.records.contains_key(&(String::from("run-0"), 1)));
        assert!(!completed.records.contains_key(&(String::from("run-4"), 1)));
        assert!(completed.records.contains_key(&(String::from("run-5"), 1)));
        assert!(completed
            .records
            .contains_key(&(format!("run-{}", SUPERVISOR_COMPLETED_MAX + 4), 1)));
        assert_eq!(
            completed.order.front().map(|(run_id, _)| run_id.clone()),
            Some(String::from("run-5"))
        );
    }

    // 同一 run 的重复完成（迟到 reader 终结）只保留最新快照且不重复排队。
    #[test]
    fn Supervisor_CompletedRetention_RepublishKeepsSingleOrderEntry_005() {
        let mut completed = CompletedRuns::default();
        let key = (String::from("run-a"), 1);
        record_completed(&mut completed, key.clone(), lifecycle());
        record_completed(&mut completed, key.clone(), lifecycle());
        assert_eq!(completed.records.len(), 1);
        assert_eq!(completed.order.len(), 1);
        assert_eq!(completed.order.front(), Some(&key));
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod remote_shutdown_observation_tests {
    use super::*;
    #[test]
    fn RemoteShutdown_EvictedLegacyRecordIsUnknown_001() {
        let supervisor = NativeRunSupervisor::new(Arc::new(TerminalTransports::new()));
        for index in 0..=SUPERVISOR_COMPLETED_MAX {
            let key = RunKey {
                run_id: format!("TEST_ONLY_EVICTED_{index}"),
                generation: 1,
            };
            let mut record = LifecycleRecord::new(key.clone());
            record.process_running().unwrap();
            record.output_started("1").unwrap();
            record.process_exited().unwrap();
            record.output_end("0").unwrap();
            record.parsed_through("0").unwrap();
            record_completed(
                &mut supervisor.core.completed.lock(),
                identity(&key),
                record,
            );
        }
        let absent = RunKey {
            run_id: "TEST_ONLY_ABSENT".into(),
            generation: 1,
        };
        let evicted = RunKey {
            run_id: "TEST_ONLY_EVICTED_0".into(),
            generation: 1,
        };
        let mut shutdown = supervisor
            .begin_shutdown_owned(vec![
                supervisor.pin_shutdown(&absent),
                supervisor.pin_shutdown(&evicted),
            ])
            .unwrap();
        assert_eq!(
            shutdown.observe_run(&RunKey {
                run_id: "TEST_ONLY_EVICTED_0".into(),
                generation: 1
            }),
            NativeShutdownObservation::Unknown {
                direct_child_reaped: false,
                output_reader_joined: false,
                wait_workers_joined: false,
                original_io_drop_returned: false,
                io_threads_joined: false,
                stop_job_joined: false
            }
        );
        assert_eq!(
            shutdown.observe_run(&RunKey {
                run_id: "TEST_ONLY_ABSENT".into(),
                generation: 1
            }),
            NativeShutdownObservation::Unknown {
                direct_child_reaped: false,
                output_reader_joined: false,
                wait_workers_joined: false,
                original_io_drop_returned: false,
                io_threads_joined: false,
                stop_job_joined: false
            }
        );
    }

    #[test]
    fn RemoteShutdown_DuplicateAndSecondInvocationAreRejected_003() {
        let supervisor = NativeRunSupervisor::new(Arc::new(TerminalTransports::new()));
        let run = RunKey {
            run_id: "TEST_ONLY_DUPLICATE".into(),
            generation: 1,
        };
        assert!(supervisor
            .begin_shutdown_owned(vec![
                supervisor.pin_shutdown(&run),
                supervisor.pin_shutdown(&run)
            ])
            .is_err());
        assert!(!supervisor.core.shutting_down.load(Ordering::SeqCst));
        let _held = supervisor
            .begin_shutdown_owned(vec![supervisor.pin_shutdown(&run)])
            .unwrap();
        assert!(supervisor.begin_shutdown_owned(Vec::new()).is_err());
    }

    #[test]
    fn RemoteShutdown_ForeignSupervisorPinRejectedBeforeStop_004() {
        let original = NativeRunSupervisor::new(Arc::new(TerminalTransports::new()));
        let foreign = NativeRunSupervisor::new(Arc::new(TerminalTransports::new()));
        let pin = original.pin_shutdown(&RunKey {
            run_id: "TEST_ONLY_PROVENANCE".into(),
            generation: 1,
        });
        assert!(foreign.begin_shutdown_owned(vec![pin]).is_err());
        assert!(!foreign.core.shutting_down.load(Ordering::SeqCst));
    }
}

/// Narrow observations only: OwnedPty currently proves a direct child, not its tree.
#[cfg(any(test, feature = "remote-host"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeShutdownObservation {
    Pending,
    Unknown {
        direct_child_reaped: bool,
        output_reader_joined: bool,
        wait_workers_joined: bool,
        original_io_drop_returned: bool,
        io_threads_joined: bool,
        stop_job_joined: bool,
    },
}
/// Native-only pin. A missing/evicted run never becomes evidence of retirement.
#[cfg(any(test, feature = "remote-host"))]
pub(crate) struct NativeRunPin {
    run: RunKey,
    origin: Weak<SupervisorCore>,
    state: Option<Arc<SupervisedRun>>,
}
#[cfg(any(test, feature = "remote-host"))]
struct NativeStopEntry {
    pin: NativeRunPin,
    job: Option<std::thread::JoinHandle<Result<(), SafeError>>>,
    joined: bool,
    failed: bool,
}
#[must_use = "Retain the real stop jobs until observed; dropping is not retirement"]
#[cfg(any(test, feature = "remote-host"))]
pub(crate) struct NativeShutdown {
    entries: Vec<NativeStopEntry>,
}
#[cfg(any(test, feature = "remote-host"))]
impl NativeRunSupervisor {
    pub(crate) fn pin_shutdown(&self, run: &RunKey) -> NativeRunPin {
        NativeRunPin {
            run: run.clone(),
            origin: Arc::downgrade(&self.core),
            state: self.core.active.lock().get(&identity(run)).cloned(),
        }
    }
    /// Explicit application shutdown only. Never use for phone disconnection.
    #[cfg(feature = "remote-host")]
    pub(crate) fn owns_live_pin(&self, pin: &NativeRunPin) -> bool {
        pin.state.is_some()
            && pin
                .origin
                .upgrade()
                .is_some_and(|origin| Arc::ptr_eq(&origin, &self.core))
    }

    /// Explicit application shutdown only. Never use for phone disconnection.
    pub(crate) fn begin_shutdown_owned(
        &self,
        pins: Vec<NativeRunPin>,
    ) -> Result<NativeShutdown, SafeError> {
        if pins.len() > 16 {
            return Err(error("REMOTE_RESOURCE_CAPACITY"));
        }
        for (index, pin) in pins.iter().enumerate() {
            if !pin
                .origin
                .upgrade()
                .is_some_and(|origin| Arc::ptr_eq(&origin, &self.core))
            {
                return Err(error("REMOTE_RESOURCE_ORIGIN"));
            }
            if pins[..index]
                .iter()
                .any(|other| identity(&other.run) == identity(&pin.run))
            {
                return Err(error("REMOTE_DUPLICATE_RESOURCE"));
            }
        }
        if self
            .core
            .owned_shutdown_started
            .swap(true, Ordering::SeqCst)
        {
            return Err(error("REMOTE_SHUTDOWN_ALREADY_STARTED"));
        }
        self.core.shutting_down.store(true, Ordering::SeqCst);
        let mut entries = Vec::with_capacity(pins.len());
        for pin in pins {
            let job = pin.state.as_ref().map(|state| {
                let original = state.clone();
                std::thread::Builder::new()
                    .name("remote-native-stop".into())
                    .spawn(move || original.request_stop())
            });
            let (job, failed) = match job {
                Some(Ok(job)) => (Some(job), false),
                Some(Err(_)) => (None, true),
                None => (None, false),
            };
            entries.push(NativeStopEntry {
                pin,
                job,
                joined: false,
                failed,
            });
        }
        Ok(NativeShutdown { entries })
    }
}
#[cfg(any(test, feature = "remote-host"))]
impl NativeShutdown {
    pub(crate) fn observe_run(&mut self, run: &RunKey) -> NativeShutdownObservation {
        let unknown =
            |reaped, joined, reader, workers, io_drop| NativeShutdownObservation::Unknown {
                direct_child_reaped: reaped,
                output_reader_joined: reader,
                wait_workers_joined: workers,
                original_io_drop_returned: io_drop,
                io_threads_joined: false,
                stop_job_joined: joined,
            };
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| identity(&entry.pin.run) == identity(run))
        else {
            return unknown(false, false, false, false, false);
        };
        let Some(state) = entry.pin.state.as_ref() else {
            return unknown(false, false, false, false, false);
        };
        // Observe the original reader even while stop/root publication is pending.
        let reader_joined = state.output_reader.lock().observe();
        // Never block the observer behind thread creation/publication.
        let workers = state
            .wait_workers
            .try_lock()
            .map(|mut workers| workers.observe())
            .unwrap_or(WaitWorkersState::Pending);
        let workers_joined =
            workers == WaitWorkersState::Joined && state.registry_retired.load(Ordering::SeqCst);
        if let Some(job) = entry.job.as_ref() {
            if !job.is_finished() {
                return NativeShutdownObservation::Pending;
            }
            let result = entry.job.take().expect("observed owned stop job").join();
            entry.joined = true;
            entry.failed = !matches!(result, Ok(Ok(())));
        }
        let reaped = state.reaped.load(Ordering::SeqCst);
        if entry.failed {
            return unknown(
                reaped,
                entry.joined,
                reader_joined,
                workers_joined,
                state.io_drop.all_returned(),
            );
        }
        if !entry.joined
            || !reaped
            || !state.adoption_complete.load(Ordering::SeqCst)
            || !state.registry_retired.load(Ordering::SeqCst)
            || workers == WaitWorkersState::Pending
        {
            return NativeShutdownObservation::Pending;
        }
        // These original thread joins do not attest remaining I/O, master or
        // PTY descendants. Keep the whole-resource result Unknown.
        unknown(
            reaped,
            entry.joined,
            reader_joined,
            workers_joined,
            state.io_drop.all_returned(),
        )
    }

    #[cfg(feature = "remote-host")]
    pub(crate) fn observe_all(&mut self) -> Option<NativeShutdownObservation> {
        let keys: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.pin.run.clone())
            .collect();
        let mut result = None;
        for key in keys {
            let observed = self.observe_run(&key);
            let reader_pending = self
                .entries
                .iter()
                .find(|entry| identity(&entry.pin.run) == identity(&key))
                .and_then(|entry| entry.pin.state.as_ref())
                .is_some_and(|state| {
                    matches!(
                        *state.output_reader.lock(),
                        OutputReaderJoin::Publishing | OutputReaderJoin::Running(_)
                    )
                });
            let workers_pending = self
                .entries
                .iter()
                .find(|entry| identity(&entry.pin.run) == identity(&key))
                .and_then(|entry| entry.pin.state.as_ref())
                .is_some_and(|state| {
                    state
                        .wait_workers
                        .try_lock()
                        .map(|mut workers| workers.observe() == WaitWorkersState::Pending)
                        .unwrap_or(true)
                });
            let io_pending = self
                .entries
                .iter()
                .find(|entry| identity(&entry.pin.run) == identity(&key))
                .and_then(|entry| entry.pin.state.as_ref())
                .is_some_and(|state| {
                    state.io_drop.observe() == crate::platform::owned_pty::IoDropState::Pending
                });
            if reader_pending
                || workers_pending
                || io_pending
                || observed == NativeShutdownObservation::Pending
            {
                result = Some(NativeShutdownObservation::Pending);
            } else if result.is_none() {
                result = Some(observed);
            }
        }
        result
    }
}

#[cfg(feature = "remote-host")]
impl NativeRunPin {
    pub(crate) fn key(&self) -> &RunKey {
        &self.run
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod output_reader_join_tests {
    use super::*;
    #[test]
    fn ReaderJoin_UnstartedPublishingAndUnknownNeverAttestJoin_006() {
        let mut slot = OutputReaderJoin::Unstarted;
        assert!(!slot.observe());
        assert!(slot.reserve());
        assert!(!slot.observe());
        assert!(!slot.reserve());
        slot = OutputReaderJoin::Unknown;
        assert!(!slot.observe());
        assert!(!slot.reserve());
    }
    #[test]
    fn ReaderJoin_OriginalHandleCannotBeReplacedAndFinishedJoinIsCached_007() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let mut slot = OutputReaderJoin::Running(std::thread::spawn(move || {
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }));
        assert!(!slot.reserve());
        assert!(!slot.observe());
        tx.send(()).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while !slot.observe() {
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        assert!(slot.observe());
        assert!(!slot.reserve());
    }
    #[test]
    fn ReaderJoin_PanicConsumesOriginalHandleButRemainsUnknown_008() {
        let mut slot =
            OutputReaderJoin::Running(std::thread::spawn(|| panic!("TEST_ONLY_READER_PANIC")));
        let end = Instant::now() + Duration::from_secs(2);
        while matches!(slot, OutputReaderJoin::Running(_)) {
            assert!(!slot.observe());
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        assert!(matches!(slot, OutputReaderJoin::Unknown));
        assert!(!slot.observe());
        assert!(!slot.reserve());
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod wait_worker_join_tests {
    use super::*;
    // Thread-custody fixture only: no NativeRun, PTY, credentials or provider.
    fn owner() -> Arc<SupervisedRun> {
        let run = RunKey {
            run_id: "TEST_ONLY_THREADS".into(),
            generation: 1,
        };
        Arc::new(SupervisedRun {
            core: Weak::new(),
            registry: Arc::new(RunRegistry::new(1)),
            lifecycle: Mutex::new(LifecycleRecord::new(run.clone())),
            run,
            owner: CallerIdentity {
                instance_id: "TEST_ONLY".into(),
                window_label: "main".into(),
                webview_epoch: WireU64::parse("1").unwrap(),
            },
            request_id: "TEST_ONLY".into(),
            stream: Mutex::new(None),
            resource: Mutex::new(None),
            stop_control: Mutex::new(()),
            stop_requested: AtomicBool::new(false),
            waiter_running: AtomicBool::new(false),
            reaped: AtomicBool::new(false),
            registry_retired: AtomicBool::new(false),
            adoption_complete: AtomicBool::new(false),
            automatic_retry_used: AtomicBool::new(false),
            output_reader: Mutex::new(OutputReaderJoin::Unstarted),
            wait_workers: Mutex::new(WaitWorkers::default()),
            io_drop: crate::platform::owned_pty::IoDropWitness::default(),
            changed: Condvar::new(),
        })
    }
    struct Gate(Arc<(Mutex<bool>, Condvar)>);
    impl Gate {
        fn new() -> Self {
            Self(Arc::new((Mutex::new(false), Condvar::new())))
        }
        fn work(&self) -> impl FnOnce() + Send + 'static {
            let gate = self.0.clone();
            move || {
                let mut released = gate.0.lock();
                let end = Instant::now() + Duration::from_secs(10);
                while !*released && Instant::now() < end {
                    gate.1.wait_for(&mut released, Duration::from_millis(50));
                }
            }
        }
    }
    impl Drop for Gate {
        fn drop(&mut self) {
            *self.0 .0.lock() = true;
            self.0 .1.notify_all();
        }
    }
    fn finish(owner: &SupervisedRun) -> WaitWorkersState {
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let observed = owner.wait_workers.lock().observe();
            if observed != WaitWorkersState::Pending {
                return observed;
            }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
    }
    #[test]
    fn WaitWorkers_CapacityRejectsWithoutLosingOriginals_011() {
        let state = owner();
        let gate = Gate::new();
        for _ in 0..WaitWorkers::MAX {
            assert!(state
                .spawn_wait_worker("TEST_ONLY_HELD", gate.work())
                .unwrap());
        }
        let rejected =
            state.spawn_wait_worker("TEST_ONLY_REJECTED", || panic!("must never execute"));
        let retained = state.wait_workers.lock().handles.len();
        drop(gate);
        assert_eq!(rejected.unwrap_err().code, "RUN_SUPERVISOR_CAPACITY");
        assert_eq!(retained, WaitWorkers::MAX);
        assert!(finish(&state) == WaitWorkersState::Joined);
        assert!(state
            .spawn_wait_worker("TEST_ONLY_AFTER_PRUNE", || {})
            .unwrap());
        assert!(finish(&state) == WaitWorkersState::Joined);
    }
    #[test]
    fn WaitWorkers_LateStarterCannotCreateAfterRetirement_012() {
        let state = owner();
        let locked = state.wait_workers.lock();
        let (tx, rx) = std::sync::mpsc::channel();
        let late = state.clone();
        let worker = std::thread::spawn(move || {
            tx.send(()).unwrap();
            late.spawn_wait_worker("TEST_ONLY_LATE", || panic!("late work"))
        });
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        state.registry_retired.store(true, Ordering::SeqCst);
        drop(locked);
        assert!(!worker.join().unwrap().unwrap());
        assert!(state.wait_workers.lock().handles.is_empty());
        assert!(state.wait_workers.lock().observe() == WaitWorkersState::Unknown);
    }
    #[test]
    fn WaitWorkers_PanicStaysUnknownAfterOtherOriginalReturns_013() {
        let state = owner();
        let gate = Gate::new();
        state
            .spawn_wait_worker("TEST_ONLY_PANIC", || panic!("TEST_ONLY_WAIT_PANIC"))
            .unwrap();
        state
            .spawn_wait_worker("TEST_ONLY_HELD", gate.work())
            .unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let mut workers = state.wait_workers.lock();
            assert!(workers.observe() == WaitWorkersState::Pending);
            if workers.panicked {
                break;
            }
            drop(workers);
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        drop(gate);
        assert!(finish(&state) == WaitWorkersState::Unknown);
        assert!(state.wait_workers.lock().observe() == WaitWorkersState::Unknown);
    }
}
