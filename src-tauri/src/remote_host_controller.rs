//! 应用持有的本机会话批准；默认没有原生配置，完整鉴权后返回 Locked。
use crate::{
    cli::{
        document::DocumentBinding, launch_service::NativeRun, profiles::error,
        snapshot::CallerIdentity, types::SafeError,
    },
    remote_host_approval::{FrozenHostApproval, HostApprovalDescription, NativeHostApproval},
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
    frozen: FrozenHostApproval,
    #[cfg(test)]
    prepare: Option<FixturePreparation>,
}
#[cfg(test)]
type FixturePreparation =
    Box<dyn FnOnce() -> crate::remote_host_network::HostAttemptMaterials + Send>;
struct PendingRequest {
    identity: OwnerIdentity,
    cancelled: std::sync::atomic::AtomicBool,
}
struct ApprovalSlot {
    identity: OwnerIdentity,
    owner: Arc<NativeHostApproval>,
    #[cfg(test)]
    prepare: Mutex<Option<FixturePreparation>>,
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
    operation: Mutex<Option<Weak<crate::remote_host::activation::ActivationOperation>>>,
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
    #[cfg(test)]
    pub(crate) fn test_install_preparation(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        admission: &NativeRunAdmission,
        description: HostApprovalDescription,
        cap: Instant,
        prepare: FixturePreparation,
    ) -> Result<(), SafeError> {
        self.install_configuration(binding, caller, admission, description, cap, Some(prepare))
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
        self.install_configuration(
            binding,
            caller,
            admission,
            description,
            cap,
            #[cfg(test)]
            None,
        )
    }
    fn install_configuration(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        admission: &NativeRunAdmission,
        description: HostApprovalDescription,
        cap: Instant,
        #[cfg(test)] prepare: Option<FixturePreparation>,
    ) -> Result<(), SafeError> {
        Self::authenticate(binding, &caller, admission)?;
        let frozen = FrozenHostApproval::description(description, admission, cap)
            .map_err(|_| error("REMOTE_HOST_CONFIGURATION"))?;
        self.install_frozen(
            binding,
            caller,
            admission,
            frozen,
            #[cfg(test)]
            prepare,
        )
    }
    #[allow(dead_code)]
    pub(crate) fn install_frozen_configuration(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        admission: &NativeRunAdmission,
        frozen: FrozenHostApproval,
    ) -> Result<(), SafeError> {
        self.install_frozen(
            binding,
            caller,
            admission,
            frozen,
            #[cfg(test)]
            None,
        )
    }
    fn install_frozen(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        admission: &NativeRunAdmission,
        frozen: FrozenHostApproval,
        #[cfg(test)] prepare: Option<FixturePreparation>,
    ) -> Result<(), SafeError> {
        Self::authenticate(binding, &caller, admission)?;
        if !frozen.matches(admission) {
            return Err(error("FORBIDDEN"));
        }
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
            frozen,
            #[cfg(test)]
            prepare,
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
        self.request_inner(binding, caller, admission, |frozen| {
            NativeHostApproval::show(app, window, frozen, admission)
                .map(Arc::new)
                .map_err(|_| error("REMOTE_HOST_APPROVAL_UNAVAILABLE"))
        })
    }
    fn request_inner(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        show: impl FnOnce(FrozenHostApproval) -> Result<Arc<NativeHostApproval>, SafeError>,
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
            let owner = show(configuration.frozen)?;
            Ok::<_, SafeError>(Arc::new(ApprovalSlot {
                identity: configuration.identity,
                owner,
                #[cfg(test)]
                prepare: Mutex::new(configuration.prepare),
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
        self.request_inner(binding, caller, admission, |frozen| {
            let (description, cap) = frozen.into_fixture_parts();
            show(description, cap)
        })
    }
    #[cfg(test)]
    pub(crate) fn test_request_frozen(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        yes: bool,
    ) -> Result<HostControlReply, SafeError> {
        self.request_inner(binding, caller, admission, |frozen| {
            NativeHostApproval::test_record_frozen(frozen, yes)
                .map(Arc::new)
                .map_err(|_| error("REMOTE_HOST_APPROVAL_UNAVAILABLE"))
        })
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
    ) -> Result<(), SafeError> {
        Self::authenticate(binding, caller, &admission)?;
        let slot = self
            .state
            .lock()
            .slot
            .clone()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?;
        slot.identity.check(binding, caller, &admission)?;
        if slot.owner.control_reply() != HostControlReply::ApprovalRecorded
            || (slot.prepare.lock().is_none() && !slot.owner.has_supplier())
        {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        let mut reservation = service.reserve_activation()?;
        *self.operation.lock() = Some(Arc::downgrade(&reservation.operation()));
        let claim = slot
            .owner
            .claim_activation(&admission)
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        #[cfg(windows)]
        let publisher = if claim.has_supplier() {
            Some(
                reservation
                    .prepare_keys(claim.native_token(), claim.end())
                    .map_err(|_| error("REMOTE_HOST_PREPARE_FAILED"))?,
            )
        } else {
            None
        };
        let prepare = slot.prepare.lock().take();
        // 原 clock 可能阻塞或重入。尚无准备线程，通知线程已由同一 service 预留保管。
        let ticket = match claim.validate(&admission) {
            Ok(ticket) => ticket,
            Err(_) => {
                drop(prepare);
                drop(admission);
                drop(runtime);
                return Err(error("REMOTE_HOST_LOCKED"));
            }
        };
        let weak_service = Arc::downgrade(&service);
        reservation
            .spawn_prepared(move |signal| {
                #[cfg(windows)]
                if let Some(publisher) = publisher {
                    return ticket.prepare_frozen(
                        runtime,
                        admission,
                        weak_service,
                        signal,
                        publisher,
                    );
                }
                let Some(prepare) = prepare else {
                    return crate::remote_host::activation::PreparationResult::Complete(false);
                };
                ticket.prepare_fixture(runtime, admission, weak_service, signal, prepare)
            })
            .map_err(|_| error("REMOTE_HOST_PREPARE_FAILED"))
    }
    #[cfg(test)]
    pub(crate) fn test_finish_preparation(&self) -> Option<bool> {
        // 显式推进的测试入口，不是 current 查询；不持 controller 锁调用登记。
        let operation = self.operation.lock().as_ref()?.upgrade()?;
        operation.result()
    }
    #[cfg(all(test, windows))]
    pub(crate) fn test_approval_owner(&self) -> Arc<NativeHostApproval> {
        self.state.lock().slot.as_ref().unwrap().owner.clone()
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
