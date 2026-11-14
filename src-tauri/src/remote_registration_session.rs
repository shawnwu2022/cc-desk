//! Own the original native attempt and independent authenticated registration IO together.
use super::*;
use cc_desk_remote_security::{
    registration_channel::{RegistrationChannel, RegistrationError, RegistrationOutcome},
    Role,
};
use tokio::io::{AsyncRead, AsyncWrite};

pub(crate) struct NativeRegistrationSession<S> {
    owner: Option<NativeRegistrationOwner>,
    channel: Option<RegistrationChannel<S>>,
}
/// Historical native result retained before any delivery await. Never a trust capability.
pub(crate) struct InstalledRegistration<S> {
    outcome: RegistrationOutcome,
    channel: Option<RegistrationChannel<S>>,
}
impl<S> InstalledRegistration<S> {
    pub(crate) fn outcome(&self) -> &RegistrationOutcome {
        &self.outcome
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> InstalledRegistration<S> {
    /// Failure or cancellation leaves the already-installed local candidate unchanged.
    pub(crate) async fn deliver(&mut self) -> Result<(), RegistrationError> {
        let mut channel = self.channel.take().ok_or(RegistrationError::Closed)?;
        channel.host_finish(self.outcome.clone()).await
    }
}
impl<S> NativeRegistrationSession<S> {
    pub(crate) fn verified_binding<'a>(
        &'a self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<
        cc_desk_remote_security::browser_registration::VerifiedBrowserRegistrationBinding<'a>,
        SafeError,
    > {
        self.channel
            .as_ref()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .check_host_decision()
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        self.owner
            .as_ref()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .verified_binding(binding, caller, admission)
    }
    pub(crate) fn new(
        mut owner: NativeRegistrationOwner,
        channel: RegistrationChannel<S>,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<Self, SafeError> {
        owner.check(binding, caller, admission)?;
        if !matches!(owner.stage, Stage::Create(_))
            || channel.check_host_start().is_err()
            || channel.local_role() != Role::Host
            || channel.client_endpoint_id() != owner.client_endpoint
        {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        owner.end = owner.end.min(channel.deadline().into_std());
        owner.check(binding, caller, admission)?;
        Ok(Self {
            owner: Some(owner),
            channel: Some(channel),
        })
    }
    fn check_live_decision(&mut self) -> Result<(), SafeError> {
        if self
            .channel
            .as_ref()
            .is_none_or(|channel| channel.check_host_decision().is_err())
        {
            self.cancel();
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        Ok(())
    }
    fn check_host(&mut self, host: &FrozenHostApproval) -> Result<(), SafeError> {
        self.check_live_decision()?;
        let channel = self
            .channel
            .as_ref()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?;
        if host.endpoint_id() != channel.host_endpoint_id() {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        Ok(())
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
        self.check_host(&host)?;
        self.owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .request_approval(binding, caller, admission, host, app, window)
    }
    /// Native commit happens synchronously, once. Preserve its real receipt before delivery.
    pub(crate) fn install(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        controller: &RemoteHostController,
    ) -> Result<InstalledRegistration<S>, SafeError> {
        let scope = admission.scope().map_err(|_| error("FORBIDDEN"))?;
        self.check_live_decision()?;
        let receipt = self
            .owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .install(binding, caller, admission, controller)?;
        self.owner.take();
        Ok(InstalledRegistration {
            outcome: RegistrationOutcome::RunLocalCandidateInstalled {
                receipt: receipt.to_string(),
                scope,
            },
            channel: self.channel.take(),
        })
    }
    pub(crate) fn cancel(&mut self) {
        self.owner.take();
        self.channel.take();
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
        self.check_host(&request.0)?;
        self.owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .request_browser_installation(binding, caller, admission, request, app, window)
    }
    pub(crate) fn install_browser(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        controller: &RemoteHostController,
    ) -> Result<InstalledRegistration<S>, SafeError> {
        let scope = admission.scope().map_err(|_| error("FORBIDDEN"))?;
        self.check_live_decision()?;
        let (receipt, installation) = self
            .owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .install_browser(binding, caller, admission, controller)?;
        self.owner.take();
        Ok(InstalledRegistration {
            outcome: RegistrationOutcome::BrowserInstallationApproved {
                receipt: receipt.to_string(),
                scope,
                installation,
            },
            channel: self.channel.take(),
        })
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
        self.check_host(&host)?;
        self.owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .test_request_browser_installation(binding, caller, admission, host, target)
    }
    #[cfg(test)]
    pub(crate) fn test_rebind(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<Self, SafeError> {
        Self::new(
            self.owner.take().unwrap(),
            self.channel.take().unwrap(),
            binding,
            caller,
            admission,
        )
    }
    #[cfg(test)]
    pub(crate) fn test_request_approval(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        host: FrozenHostApproval,
    ) -> Result<Arc<NativeRegistrationApproval>, SafeError> {
        self.check_host(&host)?;
        self.owner
            .as_mut()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?
            .test_request_approval(binding, caller, admission, host)
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> NativeRegistrationSession<S> {
    pub(crate) async fn verify(
        &mut self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<(), SafeError> {
        // Move both original owners out before await: a dropped future closes both, never resumes.
        let mut owner = self
            .owner
            .take()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?;
        let mut channel = self
            .channel
            .take()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?;
        let request = owner.request(binding, caller, admission)?;
        let created = channel
            .host_create(&request)
            .await
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        owner.receive_create(binding, caller, admission, &created)?;
        let request = owner.request(binding, caller, admission)?;
        let asserted = channel
            .host_confirm(&request)
            .await
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        owner.receive_assertion(binding, caller, admission, &asserted)?;
        self.owner = Some(owner);
        self.channel = Some(channel);
        Ok(())
    }
    pub(crate) async fn reject(mut self) -> Result<(), RegistrationError> {
        self.owner.take();
        let mut channel = self.channel.take().ok_or(RegistrationError::Closed)?;
        channel.host_finish(RegistrationOutcome::Rejected).await
    }
}
