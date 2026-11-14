//! 单独批准受控浏览器存储，不把 run-local Yes、公开候选或回执升级为持久权威。
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use cc_desk_remote_security::{
    browser_registration::UnapprovedBrowserRegistration, registration_channel::BrowserInstallation,
};

/// 由独立可信后端选择；没有 IPC 反序列化或默认来源。
pub(crate) struct BrowserInstallationTarget {
    pub database_name: String,
    pub expected_revision: Option<String>,
    pub host_spki: Vec<u8>,
    pub relay_id: [u8; 16],
    pub wasm_url: String,
    pub glue_url: String,
    pub peer_budget_ms: u64,
}
struct FrozenBrowserInstallation {
    host: FrozenHostApproval,
    evidence: UnapprovedBrowserRegistration,
    binding: [u8; 32],
    target: BrowserInstallationTarget,
    installation: BrowserInstallation,
}
fn material(
    host: &FrozenHostApproval,
    evidence: &UnapprovedBrowserRegistration,
    binding: [u8; 32],
    target: &BrowserInstallationTarget,
) -> Result<BrowserInstallation, HostNetworkError> {
    let invalid = || HostNetworkError::Authority;
    let f = &host.fields;
    let b = evidence.verified_binding().map_err(|_| invalid())?;
    if Instant::now() >= host.cap()
        || !evidence
            .matches_host_candidate(binding, &f.phone_pin, &f.issuer, &f.browser)
            .map_err(|_| invalid())?
        || P256Spki::new(&target.host_spki)
            .map_err(|_| invalid())?
            .endpoint_id(cc_desk_remote_security::Role::Host)
            != f.endpoint_id
        || target.database_name.is_empty()
        || target.database_name.len() > 64
        || !target
            .database_name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        || target.expected_revision.as_ref().is_some_and(|r| {
            !r.parse::<u64>()
                .ok()
                .is_some_and(|n| n > 0 && n.to_string() == *r)
        })
        || target.relay_id == [0; 16]
        || target.peer_budget_ms == 0
        || target.peer_budget_ms > 10000
        || f.policy.duration.is_zero()
        || f.policy.duration > Duration::from_secs(30)
    {
        return Err(invalid());
    }
    for artifact in [&target.wasm_url, &target.glue_url] {
        let url = tauri::Url::parse(artifact).map_err(|_| invalid())?;
        if artifact.len() > 2048
            || !artifact.is_ascii()
            || artifact.contains(['%', '\\'])
            || url.as_str() != artifact
            || url.origin().ascii_serialization() != b.origin
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
    }
    let credential = B64.encode(b.credential_id);
    let configuration = serde_json::json!({
        "version":1,
        "profile":{"origin":b.origin,"rpId":b.rp_id,"credentialId":credential},
        "localSpki":B64.encode(b.tls_spki),"hostSpki":B64.encode(&target.host_spki),
        "trust":{"hostKeyEpoch":f.binding.host_key_epoch.to_string(),"clientKeyEpoch":f.binding.client_key_epoch.to_string(),"origin":f.binding.origin.as_str()},
        "grant":{"issuerSpki":B64.encode(b.grant_spki),"principal":f.principal.iter().map(|v|format!("{v:02x}")).collect::<String>(),"scope":f.policy.scope,"role":f.policy.role,
            "budgetMs":f.policy.duration.as_millis() as u64,"peerInstall":{"budgetMs":target.peer_budget_ms},
            "profile":{"origin":b.origin,"rpId":b.rp_id,"credentialId":credential,"credentialSpki":B64.encode(b.credential_spki),"backupEligible":b.backup_eligible,"allowBackedUp":b.allow_backed_up}},
        "relay":{"mode":match f.relay_mode {cc_desk_remote_transport::wss::RelayMode::Legacy=>"legacy",cc_desk_remote_transport::wss::RelayMode::RendezvousV1=>"rendezvous-v1"},"endpoint":f.binding.origin.as_str(),"tunnel":B64.encode(f.tunnel),"keyId":B64.encode(target.relay_id)},
        "artifacts":{"wasm":target.wasm_url,"glue":target.glue_url}
    }).to_string();
    if configuration.len() > 10000 {
        return Err(HostNetworkError::Bounds);
    }
    Ok(BrowserInstallation {
        database_name: target.database_name.clone(),
        expected_revision: target.expected_revision.clone(),
        configuration,
        relay_spki: B64.encode(b.relay_spki),
        requested_user_handle: B64.encode(b.requested_user_handle),
    })
}
impl FrozenBrowserInstallation {
    fn check(&self) -> Result<(), HostNetworkError> {
        if material(&self.host, &self.evidence, self.binding, &self.target)? != self.installation {
            return Err(HostNetworkError::Authority);
        }
        Ok(())
    }
    fn render(&self) -> Result<String, HostNetworkError> {
        self.check()?;
        Ok(format!("Allow this browser to persist this device pairing?\nThis separately authorizes the exact origin and database below to store the original non-exportable TLS, grant and relay keys and this credential binding. It does not authenticate an account or hardware, create native restart trust, or authorize any Host connection. Native configuration is installed Locked for this run only. Browser storage and native installation are separate transactions; delivery failure does not undo native history. Revoke through the browser store before replacement; no implicit recovery.\nDatabase: {}\nExpected revoked revision: {:?}\nRequested user association (not authenticated identity): {}\nBrowser relay SPKI: {}\nExact browser configuration:\n{}\n\n{}",public_text(&self.installation.database_name),self.installation.expected_revision,self.installation.requested_user_handle,self.installation.relay_spki,public_text(&self.installation.configuration),NativeHostApproval::render_frozen(&self.host)))
    }
}
pub(crate) struct NativeBrowserInstallationApproval {
    machine: Arc<ApprovalMachine<FrozenBrowserInstallation>>,
}
impl NativeBrowserInstallationApproval {
    fn freeze(
        mut host: FrozenHostApproval,
        evidence: UnapprovedBrowserRegistration,
        binding: [u8; 32],
        target: BrowserInstallationTarget,
        end: Instant,
    ) -> Result<(FrozenBrowserInstallation, String, Instant), HostNetworkError> {
        host.constrain_from_clock()?;
        let cap = end.min(host.cap());
        if Instant::now() >= cap {
            return Err(HostNetworkError::Deadline);
        }
        let installation = material(&host, &evidence, binding, &target)?;
        let frozen = FrozenBrowserInstallation {
            host,
            evidence,
            binding,
            target,
            installation,
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
        target: BrowserInstallationTarget,
        end: Instant,
    ) -> Result<Arc<Self>, HostNetworkError> {
        let (frozen, text, cap) = Self::freeze(host, evidence, binding, target, end)?;
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
                "Approve browser device pairing storage",
                cap,
            ),
        }))
    }
    pub(crate) fn take(
        &self,
    ) -> Result<Option<(FrozenHostApproval, BrowserInstallation)>, HostNetworkError> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        permit
            .consume(|frozen, _| {
                frozen.check()?;
                Ok((frozen.host, frozen.installation))
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
        target: BrowserInstallationTarget,
        end: Instant,
    ) -> Result<Arc<Self>, HostNetworkError> {
        let (frozen, text, cap) = Self::freeze(host, evidence, binding, target, end)?;
        assert!(text.contains("persist this device pairing"));
        Ok(Arc::new(Self {
            machine: ApprovalMachine::pending(frozen, Instant::now(), cap),
        }))
    }
    #[cfg(test)]
    pub(crate) fn test_complete(&self, yes: bool) {
        self.machine.complete(yes, Instant::now());
    }
}
