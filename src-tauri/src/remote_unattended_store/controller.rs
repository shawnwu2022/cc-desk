//! 唯一内部服务托管；纯关闭不依赖旧 run 或新的批准，不开放 UI/网络。
use super::*;
use crate::remote_host_approval::unattended::final_approval::FinalControl;

#[derive(Clone)]
pub(crate) struct CloseHandle {
    control: Arc<FinalControl>,
}
impl CloseHandle {
    pub(crate) fn request_close(&self) {
        self.control.close();
    }
    #[cfg(test)]
    pub(crate) fn test_closed(&self) -> bool {
        self.control.closed()
    }
    pub(crate) fn wait_until_closed(&self, end: std::time::Instant) {
        self.control.wait_until_closed(end);
    }
}
pub(crate) struct SingleRunController<S: Sealer> {
    close: CloseHandle,
    target: CloseTarget,
    service: parking_lot::Mutex<SingleRunService<S>>,
}
impl<S: Sealer> SingleRunController<S> {
    pub(crate) fn from_started(service: SingleRunService<S>) -> Result<Self> {
        if !service.is_published() {
            return Err(StoreError::Unauthorized);
        }
        let control = service
            .consent
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .control();
        let candidate = service
            .consent
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .candidate();
        let close = CloseHandle { control };
        let target = CloseTarget {
            brand: Arc::new(()),
            store: candidate.target().clone(),
            store_id: candidate.expected().store_id.clone(),
            device_id: candidate.record().device_id.clone(),
            scope: service.consent.as_ref().unwrap().scope().clone(),
            role: service.consent.as_ref().unwrap().role(),
            cutoff: service.consent.as_ref().unwrap().cutoff(),
            close: close.clone(),
        };
        Ok(Self {
            close,
            target,
            service: parking_lot::Mutex::new(service),
        })
    }
    pub(crate) fn close_handle(&self) -> CloseHandle {
        self.close.clone()
    }
    pub(crate) fn request_close(&self) {
        self.close.request_close();
    }
    pub(crate) fn finish_close(&self) -> Result<()> {
        self.request_close();
        self.service.lock().stop()
    }
    #[cfg(test)]
    pub(crate) fn test_lock_service(&self) -> parking_lot::MutexGuard<'_, SingleRunService<S>> {
        self.service.lock()
    }
    #[cfg(test)]
    pub(crate) fn test_into_service(self) -> SingleRunService<S> {
        self.service.into_inner()
    }
}

use crate::{
    remote_host_approval::unattended_close::CloseAndForgetApproval,
    remote_native_pairing_store::{
        LockedSnapshot as PairingSnapshot, Mutation, PublicDeviceRecord, RecordState,
        StoreError as PairingError, StoreTarget,
    },
};
#[derive(Clone)]
pub(crate) struct CloseTarget {
    brand: Arc<()>,
    store: StoreTarget,
    store_id: String,
    device_id: String,
    scope: cc_desk_remote_host::native_host::NativeSessionScope,
    role: crate::remote_host_approval::unattended::RequestedRole,
    cutoff: u64,
    close: CloseHandle,
}
impl CloseTarget {
    pub(crate) fn request_close(&self) {
        self.close.request_close();
    }
}
pub(crate) struct ForgetRequest {
    target: CloseTarget,
    mutation: Mutation,
    record: PublicDeviceRecord,
}
impl ForgetRequest {
    pub(crate) fn matches_target(&self, target: &CloseTarget) -> bool {
        Arc::ptr_eq(&self.target.brand, &target.brand)
            && self.target.store.same_target(&target.store)
            && self.target.store_id == target.store_id
            && self.target.device_id == target.device_id
    }
    pub(crate) fn matches_store(&self, store: &StoreTarget) -> bool {
        self.target.store.same_target(store)
    }
    pub(crate) fn mutation(&self) -> &Mutation {
        &self.mutation
    }
    pub(crate) fn render(&self) -> String {
        format!("关闭此设备当前的单 run 委托并删除公开配对记录？\n先关闭本机权限，再持久撤销，最后写公开 tombstone。若持久撤销失败，不执行删除；若删除失败，本机权限仍已关闭。此操作不删除手机存储、不撤销已执行输入、不允许重新启用。\n原委托 run：{} / generation：{} / role：{:?} / 截止：{}\n精确设备/路由/公钥：{}\n精确删除版本：{}",
            self.target.scope.run, self.target.scope.generation, self.target.role, self.target.cutoff,
            serde_json::to_string(&self.record).expect("validated public record"),
            serde_json::to_string(&self.mutation).expect("bounded mutation"))
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CloseForgetError {
    Pairing(PairingError),
    DurableRevoke(StoreError),
}
impl From<PairingError> for CloseForgetError {
    fn from(error: PairingError) -> Self {
        Self::Pairing(error)
    }
}
impl<S: Sealer> SingleRunController<S> {
    pub(crate) fn prepare_forget<B: PairingBackend, P: PairingSealer>(
        &self,
        pairing: &PairingStore<B, P>,
    ) -> std::result::Result<ForgetRequest, PairingError> {
        if !self.target.store.same_target(&pairing.target()) {
            return Err(PairingError::Unauthorized);
        }
        let snapshot = pairing.load()?;
        if snapshot.expected.store_id != self.target.store_id {
            return Err(PairingError::Unauthorized);
        }
        let record = snapshot
            .devices
            .iter()
            .find(|entry| {
                entry.device_id == self.target.device_id && entry.state == RecordState::Active
            })
            .and_then(|entry| entry.record.clone())
            .ok_or(PairingError::Unauthorized)?;
        Ok(ForgetRequest {
            target: self.target.clone(),
            mutation: Mutation::Forget {
                expected: snapshot.expected,
                device_id: self.target.device_id.clone(),
            },
            record,
        })
    }
    pub(crate) fn close_and_forget<B: PairingBackend, P: PairingSealer>(
        &self,
        approval: &CloseAndForgetApproval,
        pairing: &PairingStore<B, P>,
    ) -> std::result::Result<Option<PairingSnapshot>, CloseForgetError> {
        approval.commit(&self.target, &pairing.target(), |mutation, auth| {
            // 精确批准已在锁外关闭原目标；任一持久错误都终止后续不同事务。
            self.service
                .lock()
                .stop()
                .map_err(CloseForgetError::DurableRevoke)?;
            pairing
                .apply(mutation.clone(), auth)
                .map_err(CloseForgetError::Pairing)
        })
    }
}

#[cfg(test)]
impl<S: Sealer> SingleRunController<S> {
    pub(crate) fn test_fail_durable_revoke(&self) {
        self.service.lock().store.backend.test_fail_after_publish();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControllerEnd {
    pub(crate) durable: Result<()>,
    pub(crate) retired: bool,
}
impl<S: Sealer> SingleRunController<S> {
    pub(crate) fn fixed_end(&self) -> std::time::Instant {
        self.service
            .lock()
            .consent
            .as_ref()
            .expect("started controller")
            .end()
    }
    // 只由接管整个 controller 的原托管线程调用；不增加 effect 或恢复出口。
    pub(crate) fn finish_owned(&self) -> ControllerEnd {
        self.request_close();
        let mut service = self.service.lock();
        let durable = service.stop();
        loop {
            match service.advance_retirement() {
                Some((keys, effects))
                    if !keys.outstanding && effects != CloseObservation::Retiring =>
                {
                    return ControllerEnd {
                        durable,
                        retired: keys.disposal_complete
                            && !keys.unknown
                            && effects == CloseObservation::EffectsClosed,
                    };
                }
                None => {
                    return ControllerEnd {
                        durable,
                        retired: false,
                    }
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
    }
}
