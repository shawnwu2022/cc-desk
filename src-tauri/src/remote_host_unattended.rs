//! 原 App 托管的唯一单 run watcher；没有配置、启用或网络入口。
use crate::remote_unattended_store::{
    service::controller::{CloseHandle, ControllerEnd, SingleRunController},
    Sealer, StoreError,
};
use parking_lot::Mutex;
use std::{sync::Arc, thread::JoinHandle};

#[path = "remote_host_preparing.rs"]
pub(crate) mod preparing;
use preparing::PreparingRequest;

pub(crate) struct StartPermit {
    app: Arc<()>,
    close: CloseHandle,
    runtime: Option<Arc<crate::cli::native_runtime::NativeRuntime>>,
}
impl StartPermit {
    pub(crate) fn prepare_next(
        &self,
        session: &Arc<crate::remote_host::delegated::DelegatedSession>,
        current: &crate::remote_unattended::capability::VerifiedBinding,
    ) -> Result<
        (
            Arc<crate::remote_host_network::delegated::SocketCustody>,
            crate::remote_host::delegated::DelegatedSocketStart,
        ),
        crate::cli::types::SafeError,
    > {
        self.runtime
            .as_ref()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?
            .prepare_unattended_next(session, current)
    }
    pub(crate) fn matches_host(&self, host: &crate::remote_host::RemoteHostService) -> bool {
        Arc::ptr_eq(&self.app, &host.unattended.app)
    }
    pub(crate) fn matches(
        &self,
        control: &Arc<crate::remote_host_approval::unattended::final_approval::FinalControl>,
    ) -> bool {
        self.close.matches_control(control)
    }
}
trait Managed: Send {
    fn epoch_end(&self) -> Option<std::time::Instant> {
        None
    }
    fn advance_epoch(&mut self, _permit: &StartPermit) -> Result<(), StoreError> {
        Ok(())
    }
    fn start(&mut self, _permit: &StartPermit) -> Result<(), StoreError> {
        Ok(())
    }
    #[allow(
        dead_code,
        reason = "default-off: production enable supplier is not connected"
    )]
    fn finish(&mut self) -> ControllerEnd;
}
impl<S: Sealer + Send> Managed for SingleRunController<S> {
    fn finish(&mut self) -> ControllerEnd {
        self.finish_owned()
    }
}
impl<S, B, P> Managed for crate::remote_unattended_store::service::pending::PendingStart<S, B, P>
where
    S: Sealer + Send + 'static,
    B: crate::remote_native_pairing_store::Backend + Send + Sync + 'static,
    P: crate::remote_native_pairing_store::Sealer + Send + Sync + 'static,
{
    fn epoch_end(&self) -> Option<std::time::Instant> {
        self.epoch_end()
    }
    fn advance_epoch(&mut self, permit: &StartPermit) -> Result<(), StoreError> {
        self.advance_epoch(permit)
    }
    fn start(&mut self, _permit: &StartPermit) -> Result<(), StoreError> {
        self.start(_permit)
    }
    fn finish(&mut self) -> ControllerEnd {
        self.finish()
    }
}
type HeldController = Arc<Mutex<Option<Box<dyn Managed>>>>;
enum JoinState {
    #[allow(
        dead_code,
        reason = "default-off: production enable supplier is not connected"
    )]
    Starting,
    #[allow(
        dead_code,
        reason = "default-off: production enable supplier is not connected"
    )]
    Running(JoinHandle<ControllerEnd>),
    Joining,
    Joined(ControllerEnd),
    Failed {
        end: Option<ControllerEnd>,
        watcher_joined: bool,
    },
}
struct Owner {
    close: CloseHandle,
    started: Arc<Mutex<Option<Result<(), StoreError>>>>,
    held: HeldController,
    join: Mutex<JoinState>,
    panic: Mutex<Option<std::mem::ManuallyDrop<Box<dyn std::any::Any + Send>>>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Observation {
    pub(crate) present: bool,
    pub(crate) outstanding: bool,
    pub(crate) watcher_joined: bool,
    pub(crate) controller_drop_returned: bool,
    pub(crate) failed: bool,
    pub(crate) durable: Option<Result<(), StoreError>>,
    pub(crate) retired: bool,
}
impl Observation {
    pub(crate) fn known(self) -> bool {
        !self.failed
            && !self.outstanding
            && (!self.present
                || (!self.outstanding
                    && self.watcher_joined
                    && self.controller_drop_returned
                    && !self.failed
                    && self.durable == Some(Ok(()))
                    && self.retired))
    }
}
impl Owner {
    fn advance(&self) -> Observation {
        let handle = {
            let mut state = self.join.lock();
            if matches!(&*state, JoinState::Running(h) if h.is_finished()) {
                let JoinState::Running(h) = std::mem::replace(&mut *state, JoinState::Joining)
                else {
                    unreachable!()
                };
                Some(h)
            } else {
                None
            }
        };
        if let Some(handle) = handle {
            let terminal = match handle.join() {
                Ok(end) => JoinState::Joined(end),
                Err(payload) => {
                    *self.panic.lock() = Some(std::mem::ManuallyDrop::new(payload));
                    JoinState::Failed {
                        end: None,
                        watcher_joined: true,
                    }
                }
            };
            self.close.request_close();
            // Joining 保留到锁外原 Drop 返回；Unknown 只移交隔离，不升为退休。
            let original = self.held.lock().take();
            drop(original);
            *self.join.lock() = terminal;
        }
        match &*self.join.lock() {
            JoinState::Joined(end) => Observation {
                present: true,
                outstanding: false,
                watcher_joined: true,
                controller_drop_returned: true,
                failed: false,
                durable: Some(end.durable),
                retired: end.retired,
            },
            JoinState::Failed {
                end,
                watcher_joined,
            } => Observation {
                present: true,
                outstanding: false,
                watcher_joined: *watcher_joined,
                controller_drop_returned: true,
                failed: true,
                durable: end.map(|e| e.durable),
                retired: false,
            },
            _ => Observation {
                present: true,
                outstanding: true,
                watcher_joined: false,
                controller_drop_returned: false,
                failed: false,
                durable: None,
                retired: false,
            },
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.close.request_close();
        // 观察取消不取走 handle；最终 owner Drop 也不 detach 原线程。
        if let JoinState::Running(handle) = std::mem::replace(
            self.join.get_mut(),
            JoinState::Failed {
                end: None,
                watcher_joined: false,
            },
        ) {
            if let Err(payload) = handle.join() {
                *self.panic.get_mut() = Some(std::mem::ManuallyDrop::new(payload));
            }
        }
        // 未证 effect 退休时仍沿原 SingleRunService Drop 的进程隔离，非成功退休。
    }
}
#[derive(Default)]
struct State {
    closing: bool,
    responsibility_started: bool,
    preparing: Option<Arc<PreparingRequest>>,
    owner: Option<Arc<Owner>>,
    runtime: Option<Arc<crate::cli::native_runtime::NativeRuntime>>,
}
#[derive(Default)]
pub(crate) struct UnattendedCustody {
    app: Arc<()>,
    state: Mutex<State>,
    rejected_cleanup: std::sync::atomic::AtomicUsize,
    rejected_unknown: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    start_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    fail_spawn: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    panic_worker: std::sync::atomic::AtomicBool,
}
impl UnattendedCustody {
    pub(crate) fn reserve_preparation(
        &self,
        runtime: Arc<crate::cli::native_runtime::NativeRuntime>,
        admission: crate::run_supervisor::NativeRunAdmission,
    ) -> Result<Arc<PreparingRequest>, StoreError> {
        if !admission.is_owned_by(&runtime) {
            return Err(StoreError::Unauthorized);
        }
        let mut state = self.state.lock();
        if state.closing || state.owner.is_some() || state.preparing.is_some() {
            return Err(StoreError::Conflict);
        }
        state.responsibility_started = true;
        let request = PreparingRequest::new(admission);
        state.runtime = Some(runtime);
        state.preparing = Some(request.clone());
        Ok(request)
    }
    #[cfg(test)]
    pub(crate) fn install<S: Sealer + Send + 'static>(
        &self,
        controller: SingleRunController<S>,
    ) -> Result<(), StoreError> {
        let end = controller.fixed_end();
        let close = controller.close_handle();
        self.install_managed(Box::new(controller), close, end, None, true, None)
    }
    #[cfg(test)] // Production must first reserve this custody via reserve_preparation.
    pub(crate) fn reserve_start<S, B, P>(
        &self,
        pending: crate::remote_unattended_store::service::pending::PendingStart<S, B, P>,
        runtime: Arc<crate::cli::native_runtime::NativeRuntime>,
    ) -> Result<(), StoreError>
    where
        S: Sealer + Send + 'static,
        B: crate::remote_native_pairing_store::Backend + Send + Sync + 'static,
        P: crate::remote_native_pairing_store::Sealer + Send + Sync + 'static,
    {
        let valid = pending.matches_runtime(&runtime);
        let end = pending.end();
        let close = CloseHandle::from_control(pending.control());
        self.install_managed(Box::new(pending), close, end, Some(runtime), valid, None)
    }
    pub(crate) fn reserve_prepared<S, B, P>(
        &self,
        request: &Arc<PreparingRequest>,
        pending: crate::remote_unattended_store::service::pending::PendingStart<S, B, P>,
        runtime: Arc<crate::cli::native_runtime::NativeRuntime>,
    ) -> Result<(), StoreError>
    where
        S: Sealer + Send + 'static,
        B: crate::remote_native_pairing_store::Backend + Send + Sync + 'static,
        P: crate::remote_native_pairing_store::Sealer + Send + Sync + 'static,
    {
        let valid =
            pending.matches_runtime(&runtime) && request.matches_control(&pending.control());
        let end = pending.end();
        let close = CloseHandle::from_control(pending.control());
        self.install_managed(
            Box::new(pending),
            close,
            end,
            Some(runtime),
            valid,
            Some(request),
        )
    }
    #[allow(
        dead_code,
        reason = "default-off: production enable supplier is not connected"
    )]
    pub(crate) fn start_result(&self) -> Option<Result<(), StoreError>> {
        self.state
            .lock()
            .owner
            .as_ref()
            .and_then(|owner| *owner.started.lock())
    }
    pub(crate) fn matches_exit_runtime(
        &self,
        runtime: &Arc<crate::cli::native_runtime::NativeRuntime>,
    ) -> bool {
        self.state
            .lock()
            .runtime
            .as_ref()
            .is_none_or(|original| Arc::ptr_eq(original, runtime))
    }
    fn install_managed(
        &self,
        managed: Box<dyn Managed>,
        close: CloseHandle,
        end: std::time::Instant,
        runtime: Option<Arc<crate::cli::native_runtime::NativeRuntime>>,
        valid: bool,
        preparation: Option<&Arc<PreparingRequest>>,
    ) -> Result<(), StoreError> {
        self.state.lock().responsibility_started = true;
        let held: HeldController = Arc::new(Mutex::new(Some(managed)));
        let owner = Arc::new(Owner {
            close: close.clone(),
            started: Arc::new(Mutex::new(None)),
            held: held.clone(),
            join: Mutex::new(JoinState::Starting),
            panic: Mutex::new(None),
        });
        let rejection = {
            let mut state = self.state.lock();
            let prepared_matches = match (&state.preparing, preparation) {
                (None, None) => true,
                (Some(original), Some(request)) => {
                    Arc::ptr_eq(original, request) && !request.closed()
                }
                _ => false,
            };
            let runtime_matches = state.runtime.as_ref().is_none_or(|original| {
                runtime
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(original, current))
            });
            if !valid
                || !prepared_matches
                || !runtime_matches
                || state.closing
                || state.owner.is_some()
            {
                Some(RejectedCleanup::new(self))
            } else {
                state.runtime = runtime;
                state.owner = Some(owner.clone());
                if let Some(request) = preparation {
                    request.transferred();
                }
                None
            }
        };
        if let Some(mut rejection) = rejection {
            owner.close.request_close();
            let mut original = owner.held.lock().take();
            let end = original.as_mut().expect("rejected original").finish();
            drop(original);
            rejection.complete(end);
            return Err(end.durable.err().unwrap_or(if end.retired {
                StoreError::Conflict
            } else {
                StoreError::Unknown
            }));
        }
        // 先由原槽持有 controller；spawn 失败或关闭竞争都不会丢失 custody。
        #[cfg(test)]
        {
            let hook = self.start_hook.lock().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        #[cfg(test)]
        let panic_worker = self.panic_worker.load(std::sync::atomic::Ordering::Acquire);
        let permit = StartPermit {
            app: self.app.clone(),
            close: close.clone(),
            runtime: self.state.lock().runtime.clone(),
        };
        let start_result = owner.started.clone();
        let task = move || {
            let started = held
                .lock()
                .as_mut()
                .expect("reserved original")
                .start(&permit);
            *start_result.lock() = Some(started);
            if started.is_ok() {
                loop {
                    let child_end = held
                        .lock()
                        .as_ref()
                        .expect("original controller")
                        .epoch_end();
                    close.wait_until_closed(child_end.map_or(end, |child| child.min(end)));
                    if close.closed() || std::time::Instant::now() >= end {
                        break;
                    }
                    if held
                        .lock()
                        .as_mut()
                        .expect("original controller")
                        .advance_epoch(&permit)
                        .is_err()
                    {
                        break;
                    }
                }
            }
            close.request_close();
            #[cfg(test)]
            if panic_worker {
                std::panic::panic_any("TEST_ONLY watcher failure");
            }
            held.lock().as_mut().expect("original controller").finish()
        };
        #[cfg(test)]
        let fail_spawn = self.fail_spawn.load(std::sync::atomic::Ordering::Acquire);
        #[cfg(not(test))]
        let fail_spawn = false;
        let worker = if fail_spawn {
            Err(std::io::Error::other("TEST_ONLY spawn failure"))
        } else {
            std::thread::Builder::new()
                .name("unattended-single-run".into())
                .spawn(task)
        };
        match worker {
            Ok(handle) => {
                *owner.join.lock() = JoinState::Running(handle);
                Ok(())
            }
            Err(_) => {
                owner.close.request_close();
                let mut original = owner.held.lock().take();
                let end = original.as_mut().map(|o| o.finish());
                drop(original);
                *owner.join.lock() = JoinState::Failed {
                    end,
                    watcher_joined: false,
                };
                Err(StoreError::Io)
            }
        }
    }
    pub(crate) fn preparation_status(&self) -> Option<(String, bool)> {
        self.state
            .lock()
            .preparing
            .as_ref()
            .map(|p| (p.id().to_owned(), p.closed()))
    }
    pub(crate) fn close_preparation(&self, id: &str) -> Result<(), StoreError> {
        let state = self.state.lock();
        if state
            .preparing
            .as_ref()
            .is_none_or(|request| request.id() != id)
        {
            return Err(StoreError::Unauthorized);
        }
        drop(state);
        self.request_close();
        Ok(())
    }
    pub(crate) fn sealed_never_remote(&self) -> bool {
        let state = self.state.lock();
        state.closing
            && !state.responsibility_started
            && state.owner.is_none()
            && state.preparing.is_none()
            && self
                .rejected_cleanup
                .load(std::sync::atomic::Ordering::Acquire)
                == 0
            && !self
                .rejected_unknown
                .load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn request_close(&self) {
        let mut state = self.state.lock();
        state.closing = true;
        if let Some(owner) = &state.owner {
            owner.close.request_close();
        }
        let preparing = state.preparing.clone();
        drop(state);
        if let Some(preparing) = preparing {
            preparing.request_close();
        }
    }
    pub(crate) fn observe(&self) -> Observation {
        let (owner, preparing) = {
            let state = self.state.lock();
            (state.owner.clone(), state.preparing.clone())
        };
        let mut result = owner.map_or(
            Observation {
                present: false,
                outstanding: false,
                watcher_joined: false,
                controller_drop_returned: false,
                failed: false,
                durable: None,
                retired: true,
            },
            |o| o.advance(),
        );
        result.outstanding |= preparing.as_ref().is_some_and(|p| p.outstanding());
        result.outstanding |= self
            .rejected_cleanup
            .load(std::sync::atomic::Ordering::Acquire)
            != 0;
        result.failed |= self
            .rejected_unknown
            .load(std::sync::atomic::Ordering::Acquire);
        result
    }
    pub(crate) fn advance_quarantine(&self) {
        let adopted = self.state.lock().owner.is_some();
        if adopted
            || self
                .rejected_unknown
                .load(std::sync::atomic::Ordering::Acquire)
        {
            crate::remote_unattended_store::service::advance_quarantine();
        }
    }
    pub(crate) async fn wait(&self) -> bool {
        loop {
            let result = self.observe();
            if !result.outstanding {
                return result.known();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

#[cfg(test)]
impl UnattendedCustody {
    pub(crate) fn test_before_spawn(&self, hook: impl FnOnce() + Send + 'static) {
        *self.start_hook.lock() = Some(Box::new(hook));
    }
    pub(crate) fn test_fail_spawn(&self) {
        self.fail_spawn
            .store(true, std::sync::atomic::Ordering::Release);
    }
    pub(crate) fn test_panic_worker(&self) {
        self.panic_worker
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

// 拒收清理仍由原同步调用持有；计数只阻止 App 把正在 Drop 的资源当成空槽。
struct RejectedCleanup<'a> {
    custody: &'a UnattendedCustody,
    completed: bool,
}
impl<'a> RejectedCleanup<'a> {
    fn new(custody: &'a UnattendedCustody) -> Self {
        custody
            .rejected_cleanup
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self {
            custody,
            completed: false,
        }
    }
    fn complete(&mut self, end: ControllerEnd) {
        if end.durable.is_err() || !end.retired {
            self.custody
                .rejected_unknown
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.completed = true;
    }
}
impl Drop for RejectedCleanup<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.custody
                .rejected_unknown
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.custody
            .rejected_cleanup
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}
