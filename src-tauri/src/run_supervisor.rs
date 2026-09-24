//! Production owner for native PTY wait, output drain, and parsed completion.
//! Process exit, PTY EOF, and renderer parsing are deliberately independent.

use crate::cli::launch_service::{NativeRun, RunSupervisor};
use crate::cli::profiles::error;
use crate::cli::run_registry::{RunKey, RunRegistry};
use crate::cli::types::{SafeError, WireU64};
use crate::run_lifecycle::{LifecycleRecord, OutputLifecycle};
use crate::terminal_transport::{OutputProgress, TerminalStream, TerminalTransports};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, Weak};

type RunIdentity = (String, u32);

struct SupervisorCore {
    transports: Arc<TerminalTransports>,
    active: Mutex<HashMap<RunIdentity, Arc<SupervisedRun>>>,
    completed: Mutex<HashMap<RunIdentity, LifecycleRecord>>,
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
                completed: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub(crate) fn snapshot(&self, run: &RunKey) -> Option<LifecycleRecord> {
        let key = identity(run);
        if let Some(state) = self.core.active.lock().get(&key).cloned() {
            return Some(state.lifecycle.lock().clone());
        }
        self.core.completed.lock().get(&key).cloned()
    }

    pub(crate) fn stop(&self, run: &RunKey) -> Result<(), SafeError> {
        let state = self
            .core
            .active
            .lock()
            .get(&identity(run))
            .cloned()
            .ok_or_else(|| error("RUN_NOT_FOUND"))?;
        state.request_stop()
    }

    pub(crate) fn shutdown(&self) {
        let active: Vec<_> = self.core.active.lock().values().cloned().collect();
        for state in active {
            if let Err(failure) = state.request_stop() {
                log::warn!(
                    "native run shutdown control failed for {}:{}: {}",
                    state.run.run_id,
                    state.run.generation,
                    failure.code
                );
            }
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
        let mut reader = resource.process.pty.take_reader()?;
        let state = SupervisedRun::new(
            Arc::downgrade(&self.core),
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

        let progress: Arc<dyn OutputProgress> = state.clone();
        let stream = match self.core.transports.attach_observed(
            resource.process.snapshot.owner().clone(),
            run.clone(),
            resource.route(),
            progress,
        ) {
            Ok(stream) => stream,
            Err(failure) => {
                self.core.active.lock().remove(&identity(run));
                return Err(failure);
            }
        };
        state.set_stream(stream.clone())?;

        // Reap ownership is installed before the reader thread. If the second
        // thread cannot be created, the run still has a waiter and a truthful
        // incomplete output state instead of an orphaned child.
        let waiter_state = state.clone();
        let waiter_run = run.clone();
        let waiter_resource = resource.clone();
        std::thread::Builder::new()
            .name("native-run-waiter".into())
            .spawn(move || match waiter_resource.process.pty.wait() {
                Ok(_) => {
                    if let Err(failure) = registry.mark_exited(&waiter_run) {
                        log::error!(
                            "native run registry exit transition failed: {}",
                            failure.code
                        );
                        waiter_state.process_failed();
                        return;
                    }
                    waiter_state.process_exited();
                    if let Err(failure) = registry.retire(&waiter_run) {
                        log::error!("native run resource retirement failed: {}", failure.code);
                        waiter_state.reader_failed();
                        return;
                    }
                    // Registry retirement removes caller control immediately after
                    // wait/reap. SupervisedRun retains its own Arc until PTY EOF (or
                    // an incomplete/degraded terminal state), so descendants that
                    // still hold the slave can finish their tail output.
                    drop(waiter_resource);
                }
                Err(_) => {
                    log::error!("native run waiter failed");
                    waiter_state.process_failed();
                }
            })
            .map_err(|_| error("RUN_SUPERVISOR_FAILED"))?;

        let reader_state = state.clone();
        if std::thread::Builder::new()
            .name("native-output-reader".into())
            .spawn(move || loop {
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
            })
            .is_err()
        {
            state.reader_failed();
        }

        Ok(())
    }
}

struct SupervisedRun {
    core: Weak<SupervisorCore>,
    run: RunKey,
    lifecycle: Mutex<LifecycleRecord>,
    stream: Mutex<Option<Arc<TerminalStream>>>,
    resource: Mutex<Option<Arc<NativeRun>>>,
}

impl SupervisedRun {
    fn new(
        core: Weak<SupervisorCore>,
        run: RunKey,
        resource: Arc<NativeRun>,
    ) -> Result<Arc<Self>, SafeError> {
        let mut lifecycle = LifecycleRecord::new(run.clone());
        lifecycle.process_running()?;
        Ok(Arc::new(Self {
            core,
            run,
            lifecycle: Mutex::new(lifecycle),
            stream: Mutex::new(None),
            resource: Mutex::new(Some(resource)),
        }))
    }

    fn set_stream(&self, stream: Arc<TerminalStream>) -> Result<(), SafeError> {
        self.lifecycle
            .lock()
            .output_started(&stream.stream_epoch().to_string())?;
        *self.stream.lock() = Some(stream);
        Ok(())
    }

    fn process_exited(&self) {
        let result = self.lifecycle.lock().process_exited();
        if let Err(failure) = result {
            log::error!("native lifecycle exit conflict: {}", failure.code);
        }
        self.release_resource_if_transport_terminal();
        self.finish_if_terminal();
    }

    fn process_failed(&self) {
        self.lifecycle.lock().process_failed();
    }

    fn output_end(&self, epoch: WireU64, final_offset: WireU64) {
        {
            let mut lifecycle = self.lifecycle.lock();
            let result = lifecycle
                .output_started(&epoch.to_string())
                .and_then(|()| {
                    lifecycle.output_end_for(&epoch.to_string(), &final_offset.to_string())
                });
            if let Err(failure) = result {
                log::error!("native lifecycle output-end conflict: {}", failure.code);
                let _ = lifecycle.mark_degraded();
            }
        }
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
        self.release_resource_if_transport_terminal();
        self.finish_if_terminal();
    }

    fn request_stop(&self) -> Result<(), SafeError> {
        {
            let mut lifecycle = self.lifecycle.lock();
            if lifecycle.process() == crate::run_lifecycle::ProcessLifecycle::Exited {
                return Err(error("RUN_NOT_READY"));
            }
            lifecycle.mark_incomplete()?;
        }
        let resource = self
            .resource
            .lock()
            .clone()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        resource.process.pty.terminate_root()
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

    fn finish_if_terminal(&self) {
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
        core.completed.lock().insert(key.clone(), snapshot);
        core.active.lock().remove(&key);
        // TerminalTransports stores only a Weak. Keep this strong reference
        // through EOF and the final parsed ACK, then release exact credit/route.
        self.stream.lock().take();
        self.resource.lock().take();
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
