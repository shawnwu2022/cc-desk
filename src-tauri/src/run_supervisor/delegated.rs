//! 单 run 的原生委托能力；没有 UI、网络或恢复授权入口。
#![allow(dead_code, reason = "原生 consent 生产入口尚未开放")]
use super::*;
use crate::remote_unattended::capability::{CloseObservation, Delegation, Ticket, VerifiedBinding};
use cc_desk_remote_host::native_host::{NativeFrameResult, NativeSessionScope};

// 只能由原生可信边界构造；当前仅测试能创建，不接受 JS DTO 或存储快照。
pub(crate) struct NativeDelegationConsent {
    policy: Arc<Delegation>,
    origin: NativeRunNetworkOrigin,
    scope: NativeSessionScope,
}
struct Captured {
    pin: NativeRunPin,
    original: Arc<NativeRun>,
    authority: NativeRunAuthority,
    scope: NativeSessionScope,
    policy: Arc<Delegation>,
}
pub(crate) struct DelegatedRunOwner(Arc<Captured>);
pub(crate) struct DelegatedRunAdmission {
    captured: Arc<Captured>,
    ticket: Ticket,
}
impl Captured {
    fn validate(&self) -> Result<(), SafeError> {
        let supervisor = self
            .pin
            .origin
            .upgrade()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        if supervisor.shutting_down.load(Ordering::SeqCst) {
            return Err(error("RUN_NOT_READY"));
        }
        let state = self
            .authority
            .state
            .upgrade()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let resource = self
            .authority
            .resource
            .upgrade()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        if !Arc::ptr_eq(&resource, &self.original)
            || state.stop_requested.load(Ordering::SeqCst)
            || state.reaped.load(Ordering::SeqCst)
            || !state
                .resource
                .try_lock()
                .ok_or_else(|| error("RUN_NOT_READY"))?
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &self.original))
        {
            return Err(error("RUN_NOT_READY"));
        }
        let canonical = state
            .native_canonical
            .try_lock()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        if canonical
            .as_ref()
            .ok_or_else(|| error("RUN_NOT_READY"))?
            .remote_scope()?
            != self.scope
        {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }
}
impl DelegatedRunOwner {
    pub(crate) fn capture(
        admission: NativeRunAdmission,
        consent: NativeDelegationConsent,
    ) -> Result<Self, SafeError> {
        admission.validate()?;
        let scope = admission.scope()?;
        if !consent.origin.matches(&admission) || consent.scope != scope {
            return Err(error("FORBIDDEN"));
        }
        admission.validate()?;
        let NativeRunAdmission {
            pin,
            original,
            authority,
            access: _,
        } = admission;
        let captured = Captured {
            pin,
            original,
            authority,
            scope,
            policy: consent.policy,
        };
        captured.validate()?;
        Ok(Self(Arc::new(captured)))
    }
    pub(crate) fn admit(
        &self,
        current: &VerifiedBinding,
        supervisor: &NativeRunSupervisor,
    ) -> Result<DelegatedRunAdmission, SafeError> {
        if !supervisor.owns_live_pin(&self.0.pin) {
            return Err(error("FORBIDDEN"));
        }
        self.0.validate()?;
        let ticket = self
            .0
            .policy
            .begin(current)
            .map_err(|_| error("FORBIDDEN"))?;
        if let Err(e) = self.0.validate() {
            self.request_revoke();
            return Err(e);
        }
        Ok(DelegatedRunAdmission {
            captured: self.0.clone(),
            ticket,
        })
    }
    pub(crate) fn request_revoke(&self) {
        self.0.policy.request_revoke();
    }
    pub(crate) fn observe_close(&self) -> CloseObservation {
        self.0.policy.observe_close()
    }
}
impl DelegatedRunAdmission {
    pub(crate) fn write_frame(
        &self,
        current: &VerifiedBinding,
        bytes: &[u8],
    ) -> Result<NativeFrameResult, SafeError> {
        self.write_inner(current, bytes, || {})
    }
    fn write_inner(
        &self,
        current: &VerifiedBinding,
        bytes: &[u8],
        before: impl FnOnce() + Send + 'static,
    ) -> Result<NativeFrameResult, SafeError> {
        self.captured.validate()?;
        let input = self
            .captured
            .original
            .remote_input()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let captured = self.captured.clone();
        self.captured
            .policy
            .commit(&self.ticket, current, input, bytes, move || {
                before();
                captured.validate().is_ok()
            })
            .map_err(|_| error("FORBIDDEN"))?
            .map_err(|_| error("PTY_WRITE_FAILED"))
    }
    pub(crate) fn resize(
        &self,
        current: &VerifiedBinding,
        rows: u16,
        columns: u16,
    ) -> Result<(), SafeError> {
        self.captured.validate()?;
        let input = self
            .captured
            .original
            .remote_input()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let captured = self.captured.clone();
        self.captured
            .policy
            .resize(&self.ticket, current, input, (rows, columns), move || {
                captured.validate().is_ok()
            })
            .map_err(|_| error("FORBIDDEN"))?
            .map_err(|_| error("PTY_RESIZE_FAILED"))
    }
    #[cfg(test)]
    pub(crate) fn test_write_paused(
        &self,
        current: &VerifiedBinding,
        bytes: &[u8],
        before: impl FnOnce() + Send + 'static,
    ) -> Result<NativeFrameResult, SafeError> {
        self.write_inner(current, bytes, before)
    }
}
impl Drop for DelegatedRunAdmission {
    fn drop(&mut self) {
        // 关闭请求不等待 actor，且不冒充 producer/socket 的回收证明。
        self.captured.policy.request_revoke();
    }
}
#[cfg(test)]
impl NativeDelegationConsent {
    pub(crate) fn test_new(
        admission: &NativeRunAdmission,
        view: bool,
    ) -> (Self, VerifiedBinding, Arc<Delegation>) {
        use sha2::{Digest, Sha256};
        let scope = admission.scope().unwrap();
        let run = Sha256::digest(
            format!("{}:{}:{}", scope.session, scope.run, scope.generation).as_bytes(),
        )
        .into();
        let (policy, current) = Delegation::test_new(run, view);
        (
            Self {
                policy: policy.clone(),
                origin: admission.network_origin().unwrap(),
                scope,
            },
            current,
            policy,
        )
    }
}

impl Drop for DelegatedRunOwner {
    fn drop(&mut self) {
        self.request_revoke();
    }
}
