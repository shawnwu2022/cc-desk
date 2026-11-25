//! 首次登记只批准原生两用途签名和精确登记传输，不批准存储或 Host 控制。
use super::*;
use crate::remote_host_controller::startup::configuration::FrozenRegistrationStart;

pub(crate) struct NativeRegistrationKeyApproval {
    machine: Arc<ApprovalMachine<FrozenRegistrationStart>>,
}
pub(crate) struct RegistrationKeyPermit {
    pub(crate) frozen: FrozenRegistrationStart,
    pub(crate) signing: RegistrationSigningScope,
}
/// 只能由本模块消费原生 Yes 后产生，不能由公开 DTO 或 Host 批准复原。
pub(crate) struct RegistrationSigningScope {
    token: Arc<crate::native_crypto::owner::NativeRevocation>,
    cap: Instant,
}
impl RegistrationSigningScope {
    pub(super) fn from_native_enrollment(
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: Instant,
    ) -> Self {
        Self { token, cap }
    }
    #[cfg(test)]
    pub(crate) fn test_new(
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: Instant,
    ) -> Self {
        Self { token, cap }
    }
    pub(crate) fn matches(
        &self,
        token: &Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: Instant,
    ) -> bool {
        Arc::ptr_eq(&self.token, token)
            && self.cap == cap
            && !token.is_revoked()
            && Instant::now() < cap
    }
    pub(crate) fn end(&self) -> Instant {
        self.cap
    }
    pub(crate) fn token(&self) -> Arc<crate::native_crypto::owner::NativeRevocation> {
        self.token.clone()
    }
}
impl NativeRegistrationKeyApproval {
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        frozen: FrozenRegistrationStart,
    ) -> Result<Arc<Self>, HostNetworkError> {
        if !frozen.product_role_enabled() {
            return Err(HostNetworkError::Locked);
        }
        let text = frozen.render();
        let cap = frozen.end;
        if Instant::now() >= cap {
            return Err(HostNetworkError::Deadline);
        }
        if DIALOG_ACTIVE
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            return Err(HostNetworkError::Locked);
        }
        Ok(Arc::new(Self {
            machine: ApprovalMachine::show(
                app,
                window,
                frozen,
                text,
                "Approve registration key use",
                cap,
            ),
        }))
    }
    pub(crate) fn take(&self) -> Option<RegistrationKeyPermit> {
        self.machine
            .take()?
            .consume(|frozen, _| {
                frozen
                    .product_role_enabled()
                    .then(|| RegistrationKeyPermit {
                        signing: RegistrationSigningScope {
                            token: self.machine.lease.native.clone(),
                            cap: frozen.end,
                        },
                        frozen,
                    })
            })
            .flatten()
    }
    pub(crate) fn cancel(&self) {
        self.machine.cancel();
    }
    pub(crate) fn pending(&self) -> bool {
        let state = self.machine.state.lock();
        !state.closed
            && Instant::now() < state.pending_end.min(state.cap)
            && state.callback_outstanding
    }
    #[cfg(test)]
    pub(crate) fn test_pending(frozen: FrozenRegistrationStart) -> Arc<Self> {
        let cap = frozen.end;
        Arc::new(Self {
            machine: ApprovalMachine::pending(frozen, Instant::now(), cap),
        })
    }
    #[cfg(test)]
    pub(crate) fn test_complete(&self, yes: bool) {
        self.machine.complete(yes, Instant::now());
    }
}
