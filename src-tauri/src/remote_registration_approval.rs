//! 原生登记批准仅安装配置，不批准远程会话或提供账户身份权威。
use super::*;
use cc_desk_remote_security::browser_registration::UnapprovedBrowserRegistration;

struct FrozenRegistration {
    host: FrozenHostApproval,
    evidence: UnapprovedBrowserRegistration,
    binding: [u8; 32],
}
impl FrozenRegistration {
    fn check(&self) -> Result<(), HostNetworkError> {
        if Instant::now() >= self.host.cap() {
            return Err(HostNetworkError::Deadline);
        }
        let f = &self.host.fields;
        if !self
            .evidence
            .matches_host_candidate(self.binding, &f.phone_pin, &f.issuer, &f.browser)
            .map_err(|_| HostNetworkError::Authority)?
        {
            return Err(HostNetworkError::Authority);
        }
        Ok(())
    }
    fn render(&self) -> Result<String, HostNetworkError> {
        self.check()?;
        let bytes = self
            .evidence
            .description()
            .map_err(|_| HostNetworkError::Authority)?;
        let evidence = std::str::from_utf8(&bytes).map_err(|_| HostNetworkError::Authority)?;
        Ok(format!("Install this browser registration for this native run only?\nThis approves local registration, not account identity, hardware provenance or persistent trust.\nNo connection will start. The separate Host session approval is still required.\nThe following session description is frozen for later review, NOT authorized by this Yes.\n\nVerified public registration evidence:\n{}\n\n{}", public_text(evidence), NativeHostApproval::render_frozen(&self.host)))
    }
}

pub(crate) struct NativeRegistrationApproval {
    machine: Arc<ApprovalMachine<FrozenRegistration>>,
}
impl NativeRegistrationApproval {
    fn freeze(
        mut host: FrozenHostApproval,
        evidence: UnapprovedBrowserRegistration,
        binding: [u8; 32],
        end: Instant,
    ) -> Result<(FrozenRegistration, String, Instant), HostNetworkError> {
        host.constrain_from_clock()?;
        let cap = end.min(host.cap());
        if Instant::now() >= cap {
            return Err(HostNetworkError::Deadline);
        }
        let frozen = FrozenRegistration {
            host,
            evidence,
            binding,
        };
        let text = frozen.render()?;
        Ok((frozen, text, cap))
    }
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        host: FrozenHostApproval,
        evidence: UnapprovedBrowserRegistration,
        binding: [u8; 32],
        end: Instant,
    ) -> Result<Arc<Self>, HostNetworkError> {
        let (frozen, text, cap) = Self::freeze(host, evidence, binding, end)?;
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
                "Approve this browser registration",
                cap,
            ),
        }))
    }
    pub(crate) fn take(&self) -> Result<Option<FrozenHostApproval>, HostNetworkError> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        permit
            .consume(|frozen, _| {
                frozen.check()?;
                Ok(frozen.host)
            })
            .ok_or(HostNetworkError::Authority)
            .map(Some)?
            .transpose()
    }
    pub(crate) fn cancel(&self) {
        self.machine.cancel();
    }
    #[cfg(test)]
    pub(crate) fn test_pending(
        host: FrozenHostApproval,
        evidence: UnapprovedBrowserRegistration,
        binding: [u8; 32],
        end: Instant,
    ) -> Result<Arc<Self>, HostNetworkError> {
        let (frozen, text, cap) = Self::freeze(host, evidence, binding, end)?;
        assert!(text.contains("separate Host session approval"));
        Ok(Arc::new(Self {
            machine: ApprovalMachine::pending(frozen, Instant::now(), cap),
        }))
    }
    #[cfg(test)]
    pub(crate) fn test_complete(&self, yes: bool) {
        self.machine.complete(yes, Instant::now());
    }
}
