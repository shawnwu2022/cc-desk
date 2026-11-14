//! 后端持有原登记尝试；没有 IPC 入口、公开批准字段或持久账户身份根。
#![allow(
    dead_code,
    reason = "registration transport and persistent trust are separate gates"
)]
use super::*;
use crate::remote_host_approval::NativeRegistrationApproval;
use crate::remote_host_approval::{BrowserInstallationTarget, NativeBrowserInstallationApproval};
use cc_desk_remote_security::{
    browser_registration::{
        BrowserRegistrationAttempt, BrowserRegistrationContext, BrowserRegistrationPossession,
        UnapprovedBrowserRegistration,
    },
    cancellation, CancelHandle,
};
use std::time::Duration;

enum Stage {
    Create(BrowserRegistrationAttempt),
    Possession(BrowserRegistrationPossession),
    Evidence(UnapprovedBrowserRegistration),
    Awaiting(Arc<NativeRegistrationApproval>),
    AwaitingBrowserInstallation(Arc<NativeBrowserInstallationApproval>),
    Closed,
}
pub(crate) struct NativeRegistrationOwner {
    identity: OwnerIdentity,
    context_binding: [u8; 32],
    client_endpoint: [u8; 32],
    end: Instant,
    cancel: CancelHandle,
    stage: Stage,
}
impl NativeRegistrationOwner {
    /// 原持有证据的只读数据，不批准连接或持久安装。
    pub(crate) fn verified_binding<'a>(
        &'a self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<
        cc_desk_remote_security::browser_registration::VerifiedBrowserRegistrationBinding<'a>,
        SafeError,
    > {
        self.check(binding, caller, admission)?;
        let Stage::Evidence(e) = &self.stage else {
            return Err(error("REMOTE_HOST_LOCKED"));
        };
        e.verified_binding()
            .map_err(|_| error("REMOTE_HOST_LOCKED"))
    }
    pub(crate) fn begin(
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        mut context: BrowserRegistrationContext,
    ) -> Result<Self, SafeError> {
        RemoteHostController::authenticate(binding, caller, admission)?;
        let identity = OwnerIdentity {
            caller: caller.clone(),
            binding: Arc::downgrade(binding),
            origin: admission.network_origin().map_err(|_| error("FORBIDDEN"))?,
        };
        // 后端随机标签仅关联原 owner；不是从公开请求恢复的信任根。
        let mut context_binding = [0; 32];
        context_binding[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        context_binding[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        context.owner_binding = context_binding;
        let client_endpoint = context
            .tls_key
            .endpoint_id(cc_desk_remote_security::Role::Client);
        let budget = Duration::from_secs(120);
        let end = Instant::now() + budget;
        let (cancel, token) = cancellation();
        let attempt = BrowserRegistrationAttempt::new(context, token, budget)
            .map_err(|_| error("REMOTE_HOST_CONFIGURATION"))?;
        Ok(Self {
            identity,
            context_binding,
            client_endpoint,
            end,
            cancel,
            stage: Stage::Create(attempt),
        })
    }
    fn check(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<(), SafeError> {
        self.identity.check(binding, caller, admission)?;
        if Instant::now() >= self.end || matches!(self.stage, Stage::Closed) {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        Ok(())
    }
    pub(crate) fn request(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<Vec<u8>, SafeError> {
        self.check(binding, caller, admission)?;
        match &self.stage {
            Stage::Create(a) => a.request(),
            Stage::Possession(a) => a.request(),
            _ => return Err(error("REMOTE_HOST_LOCKED")),
        }
        .map_err(|_| error("REMOTE_HOST_LOCKED"))
    }
    pub(crate) fn receive_create(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        bytes: &[u8],
    ) -> Result<(), SafeError> {
        self.check(binding, caller, admission)?;
        let stage = std::mem::replace(&mut self.stage, Stage::Closed);
        match stage {
            Stage::Create(a) => match a.verify_create(bytes) {
                Ok(p) => {
                    self.stage = Stage::Possession(p);
                    Ok(())
                }
                Err(_) => {
                    self.cancel();
                    Err(error("REMOTE_HOST_LOCKED"))
                }
            },
            _ => {
                self.cancel();
                Err(error("REMOTE_HOST_LOCKED"))
            }
        }
    }
    pub(crate) fn receive_assertion(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        bytes: &[u8],
    ) -> Result<(), SafeError> {
        self.check(binding, caller, admission)?;
        let stage = std::mem::replace(&mut self.stage, Stage::Closed);
        match stage {
            Stage::Possession(a) => match a.verify_assertion(bytes) {
                Ok(e) => {
                    self.stage = Stage::Evidence(e);
                    Ok(())
                }
                Err(_) => {
                    self.cancel();
                    Err(error("REMOTE_HOST_LOCKED"))
                }
            },
            _ => {
                self.cancel();
                Err(error("REMOTE_HOST_LOCKED"))
            }
        }
    }
    fn evidence(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: &FrozenHostApproval,
    ) -> Result<UnapprovedBrowserRegistration, SafeError> {
        self.check(binding, caller, admission)?;
        if !host.matches(admission) {
            return Err(error("FORBIDDEN"));
        }
        match std::mem::replace(&mut self.stage, Stage::Closed) {
            Stage::Evidence(e) => Ok(e),
            _ => {
                self.cancel();
                Err(error("REMOTE_HOST_LOCKED"))
            }
        }
    }
    pub(crate) fn request_approval<R: tauri::Runtime>(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: FrozenHostApproval,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<(), SafeError> {
        let evidence = self.evidence(binding, caller, admission, &host)?;
        let approved = NativeRegistrationApproval::show(
            app,
            window,
            host,
            evidence,
            self.context_binding,
            self.end,
        )
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        self.stage = Stage::Awaiting(approved);
        Ok(())
    }
    pub(crate) fn install(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        controller: &RemoteHostController,
    ) -> Result<u64, SafeError> {
        self.check(binding, caller, admission)?;
        let Stage::Awaiting(approval) = &self.stage else {
            return Err(error("REMOTE_HOST_LOCKED"));
        };
        let host = approval
            .take()
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?;
        // 原许可只消费一次；交付失败不能重新解释此前的 Yes。
        self.identity.check(binding, caller, admission)?;
        self.stage = Stage::Closed;
        if Instant::now() >= self.end {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        controller.install_frozen(
            binding,
            caller.clone(),
            admission,
            host,
            #[cfg(test)]
            None,
        )
    }
    pub(crate) fn request_browser_installation<R: tauri::Runtime>(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        request: (FrozenHostApproval, BrowserInstallationTarget),
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<(), SafeError> {
        let (host, target) = request;
        let evidence = self.evidence(binding, caller, admission, &host)?;
        let approved = NativeBrowserInstallationApproval::show(
            app,
            window,
            host,
            evidence,
            self.context_binding,
            target,
            self.end,
        )
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        self.stage = Stage::AwaitingBrowserInstallation(approved);
        Ok(())
    }
    pub(crate) fn install_browser(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        controller: &RemoteHostController,
    ) -> Result<
        (
            u64,
            cc_desk_remote_security::registration_channel::BrowserInstallation,
        ),
        SafeError,
    > {
        self.check(binding, caller, admission)?;
        let Stage::AwaitingBrowserInstallation(approval) = &self.stage else {
            return Err(error("REMOTE_HOST_LOCKED"));
        };
        let (host, installation) = approval
            .take()
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?;
        self.identity.check(binding, caller, admission)?;
        self.stage = Stage::Closed;
        if Instant::now() >= self.end {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        let receipt = controller.install_frozen(
            binding,
            caller.clone(),
            admission,
            host,
            #[cfg(test)]
            None,
        )?;
        Ok((receipt, installation))
    }
    pub(crate) fn cancel(&mut self) {
        self.cancel.cancel();
        if let Stage::Awaiting(a) = &self.stage {
            a.cancel();
        }
        if let Stage::AwaitingBrowserInstallation(a) = &self.stage {
            a.cancel();
        }
        self.stage = Stage::Closed;
    }
    #[cfg(test)]
    pub(crate) fn test_request_approval(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: FrozenHostApproval,
    ) -> Result<Arc<NativeRegistrationApproval>, SafeError> {
        let evidence = self.evidence(binding, caller, admission, &host)?;
        let approval = NativeRegistrationApproval::test_pending(
            host,
            evidence,
            self.context_binding,
            self.end,
        )
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        self.stage = Stage::Awaiting(approval.clone());
        Ok(approval)
    }
    #[cfg(test)]
    pub(crate) fn test_expire(&mut self) {
        self.end = Instant::now();
    }
    #[cfg(test)]
    pub(crate) fn test_request_browser_installation(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: FrozenHostApproval,
        target: BrowserInstallationTarget,
    ) -> Result<Arc<NativeBrowserInstallationApproval>, SafeError> {
        let evidence = self.evidence(binding, caller, admission, &host)?;
        let approved = NativeBrowserInstallationApproval::test_pending(
            host,
            evidence,
            self.context_binding,
            target,
            self.end,
        )
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        self.stage = Stage::AwaitingBrowserInstallation(approved.clone());
        Ok(approved)
    }
}
impl Drop for NativeRegistrationOwner {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[path = "remote_registration_session.rs"]
mod session;
pub(crate) use session::NativeRegistrationSession;
