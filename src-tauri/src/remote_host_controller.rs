//! 应用持有的本机会话批准；默认没有原生配置，完整鉴权后返回 Locked。
use crate::{
    cli::{
        document::DocumentBinding, launch_service::NativeRun, profiles::error,
        snapshot::CallerIdentity, types::SafeError,
    },
    remote_host_approval::{HostApprovalDescription, NativeHostApproval},
    run_supervisor::{NativeRunAdmission, NativeRunNetworkOrigin},
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    sync::{Arc, Weak},
    time::Instant,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state")]
pub(crate) enum HostControlReply {
    Locked,
    Pending,
    ApprovalRecorded,
    Consumed,
    Cancelled,
    Expired,
    Registered,
}
#[derive(Clone)]
struct OwnerIdentity {
    caller: CallerIdentity,
    binding: Weak<DocumentBinding<NativeRun>>,
    origin: NativeRunNetworkOrigin,
}
impl OwnerIdentity {
    fn matches(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> bool {
        &self.caller == caller
            && Weak::ptr_eq(&self.binding, &Arc::downgrade(binding))
            && self.origin.matches(admission)
    }
    fn check(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<(), SafeError> {
        binding.check_admitted(caller)?;
        if &self.caller != caller
            || !self
                .binding
                .upgrade()
                .is_some_and(|original| Arc::ptr_eq(&original, binding))
            || !self.origin.matches(admission)
            || admission.network_origin().is_err()
        {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }
}
struct NativeConfiguration {
    identity: OwnerIdentity,
    description: HostApprovalDescription,
    cap: Instant,
}
struct PendingRequest {
    identity: OwnerIdentity,
    cancelled: std::sync::atomic::AtomicBool,
}
struct ApprovalSlot {
    identity: OwnerIdentity,
    owner: Arc<NativeHostApproval>,
}
#[derive(Default)]
struct ControllerState {
    closing: bool,
    requesting: Option<Arc<PendingRequest>>,
    configuration: Option<NativeConfiguration>,
    slot: Option<Arc<ApprovalSlot>>,
}
#[derive(Default)]
pub(crate) struct RemoteHostController {
    state: Mutex<ControllerState>,
    #[cfg(test)]
    operations: Mutex<Vec<std::thread::JoinHandle<bool>>>,
    #[cfg(test)]
    operation_unknown: std::sync::atomic::AtomicBool,
}
impl RemoteHostController {
    fn authenticate(
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<(), SafeError> {
        binding.check_admitted(caller)?;
        admission.network_origin().map_err(|_| error("FORBIDDEN"))?;
        Ok(())
    }
    /// 仅后端原生供应者可调用；没有 IPC 配置入口或默认批准。
    #[allow(dead_code)]
    pub(crate) fn install_native_configuration(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        admission: &NativeRunAdmission,
        description: HostApprovalDescription,
        cap: Instant,
    ) -> Result<(), SafeError> {
        Self::authenticate(binding, &caller, admission)?;
        let identity = OwnerIdentity {
            caller,
            binding: Arc::downgrade(binding),
            origin: admission.network_origin().map_err(|_| error("FORBIDDEN"))?,
        };
        let mut state = self.state.lock();
        if state.closing
            || state.requesting.is_some()
            || state.configuration.is_some()
            || state.slot.is_some()
        {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        state.configuration = Some(NativeConfiguration {
            identity,
            description,
            cap,
        });
        Ok(())
    }
    pub(crate) fn current(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<HostControlReply, SafeError> {
        Self::authenticate(binding, caller, admission)?;
        let (slot, pending, configured) = {
            let state = self.state.lock();
            (
                state.slot.clone(),
                state.requesting.clone(),
                state.configuration.as_ref().map(|c| c.identity.clone()),
            )
        };
        if let Some(pending) = pending {
            pending.identity.check(binding, caller, admission)?;
            return Ok(
                if pending.cancelled.load(std::sync::atomic::Ordering::Acquire) {
                    HostControlReply::Cancelled
                } else {
                    HostControlReply::Pending
                },
            );
        }
        let Some(slot) = slot else {
            if let Some(identity) = configured {
                identity.check(binding, caller, admission)?;
            }
            return Ok(HostControlReply::Locked);
        };
        slot.identity.check(binding, caller, admission)?;
        let reply = slot.owner.control_reply();
        binding.check_admitted(caller)?;
        Ok(reply)
    }
    pub(crate) fn request<R: tauri::Runtime>(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<HostControlReply, SafeError> {
        self.request_inner(binding, caller, admission, |description, cap| {
            NativeHostApproval::show(app, window, description, admission, cap)
                .map(Arc::new)
                .map_err(|_| error("REMOTE_HOST_APPROVAL_UNAVAILABLE"))
        })
    }
    fn request_inner(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        show: impl FnOnce(
            HostApprovalDescription,
            Instant,
        ) -> Result<Arc<NativeHostApproval>, SafeError>,
    ) -> Result<HostControlReply, SafeError> {
        Self::authenticate(binding, caller, admission)?;
        let (configuration, pending) = {
            let mut state = self.state.lock();
            if state.closing {
                return Err(error("REMOTE_HOST_CLOSED"));
            }
            if let Some(pending) = &state.requesting {
                if !pending.identity.matches(binding, caller, admission) {
                    return Err(error("FORBIDDEN"));
                }
                return Err(error("REMOTE_HOST_BUSY"));
            }
            if state.slot.is_some() {
                drop(state);
                return self.current(binding, caller, admission);
            }
            let Some(configuration) = state.configuration.as_ref() else {
                return Ok(HostControlReply::Locked);
            };
            if !configuration.identity.matches(binding, caller, admission) {
                return Err(error("FORBIDDEN"));
            }
            let pending = Arc::new(PendingRequest {
                identity: configuration.identity.clone(),
                cancelled: std::sync::atomic::AtomicBool::new(false),
            });
            let configuration = state.configuration.take().expect("checked under same lock");
            state.requesting = Some(pending.clone());
            (configuration, pending)
        };
        let result = (|| {
            configuration.identity.check(binding, caller, admission)?;
            let owner = show(configuration.description, configuration.cap)?;
            Ok::<_, SafeError>(Arc::new(ApprovalSlot {
                identity: configuration.identity,
                owner,
            }))
        })();
        let mut state = self.state.lock();
        state.requesting = None;
        let slot = result?;
        if state.closing || pending.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            drop(state);
            slot.owner.cancel();
            return Ok(HostControlReply::Cancelled);
        }
        state.slot = Some(slot.clone());
        drop(state);
        if let Err(error) = slot.identity.check(binding, caller, admission) {
            slot.owner.cancel();
            return Err(error);
        }
        Ok(slot.owner.control_reply())
    }
    #[cfg(test)]
    pub(crate) fn test_request(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        show: impl FnOnce(
            HostApprovalDescription,
            Instant,
        ) -> Result<Arc<NativeHostApproval>, SafeError>,
    ) -> Result<HostControlReply, SafeError> {
        self.request_inner(binding, caller, admission, show)
    }
    pub(crate) fn cancel(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<HostControlReply, SafeError> {
        Self::authenticate(binding, caller, admission)?;
        let (slot, pending, configured) = {
            let state = self.state.lock();
            (
                state.slot.clone(),
                state.requesting.clone(),
                state.configuration.as_ref().map(|c| c.identity.clone()),
            )
        };
        if let Some(pending) = pending {
            pending.identity.check(binding, caller, admission)?;
            pending
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            // publication 可能刚完成，再读取已发布的原 slot。
            let published = { self.state.lock().slot.clone() };
            if let Some(slot) = published {
                slot.owner.cancel();
            }
            return Ok(HostControlReply::Cancelled);
        }
        let Some(slot) = slot else {
            if let Some(identity) = configured {
                identity.check(binding, caller, admission)?;
            }
            return Ok(HostControlReply::Locked);
        };
        slot.identity.check(binding, caller, admission)?;
        slot.owner.cancel();
        Ok(HostControlReply::Cancelled)
    }
    #[cfg(test)]
    pub(crate) fn test_prepare_owned(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: NativeRunAdmission,
        runtime: Arc<crate::cli::native_runtime::NativeRuntime>,
        service: Arc<crate::remote_host::RemoteHostService>,
        prepare: impl FnOnce() -> crate::remote_host_network::HostAttemptMaterials + Send + 'static,
    ) -> Result<(), SafeError> {
        Self::authenticate(binding, caller, &admission)?;
        let slot = self
            .state
            .lock()
            .slot
            .clone()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?;
        slot.identity.check(binding, caller, &admission)?;
        let ticket = slot
            .owner
            .take_activation(&admission)
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        let handle = std::thread::spawn(move || {
            ticket
                .prepare_fixture(&runtime, &admission, &service, prepare)
                .is_ok()
        });
        self.operations.lock().push(handle);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn test_join_finished(&self) -> Option<bool> {
        let handles = {
            let mut operations = self.operations.lock();
            if operations.iter().any(|h| !h.is_finished()) {
                return None;
            }
            std::mem::take(&mut *operations)
        };
        let mut outcomes = true;
        for handle in handles {
            match handle.join() {
                Ok(success) => outcomes &= success,
                Err(_) => {
                    self.operation_unknown
                        .store(true, std::sync::atomic::Ordering::Release);
                    outcomes = false;
                }
            }
        }
        Some(
            outcomes
                && !self
                    .operation_unknown
                    .load(std::sync::atomic::Ordering::Acquire),
        )
    }
    pub(crate) fn begin_shutdown(&self) {
        let (configuration, slot) = {
            let mut state = self.state.lock();
            state.closing = true;
            (state.configuration.take(), state.slot.clone())
        };
        drop(configuration);
        if let Some(slot) = slot {
            slot.owner.cancel();
        }
    }
}
