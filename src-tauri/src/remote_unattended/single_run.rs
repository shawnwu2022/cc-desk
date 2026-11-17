//! 从独立本机最终确认派生精确单 run 事实；不接受调用者提供的摘要。
use super::*;
use crate::remote_host_approval::unattended::{final_approval::FinalConsent, RequestedRole};
use sha2::{Digest, Sha256};

fn digest(domain: &[u8], values: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update((domain.len() as u64).to_le_bytes());
    hash.update(domain);
    for value in values {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
    }
    hash.finalize().into()
}
pub(crate) struct SingleRunTargetDigest {
    pub(crate) installation: [u8; 32],
    pub(crate) device: [u8; 32],
    pub(crate) run: [u8; 32],
    pub(crate) binding: [u8; 32],
}
impl VerifiedBinding {
    pub(crate) fn single_run(consent: &FinalConsent) -> Result<Self, Denied> {
        let candidate = consent.candidate();
        let record = candidate.record();
        let scope = consent.scope();
        let full_record_digest = crate::remote_native_pairing_store::record_digest(record)
            .map_err(|_| Denied::Locked)?;
        let binding = Binding {
            installation: digest(
                b"cc-desk/single-run/pairing-namespace/v1",
                &[candidate.expected().store_id.as_bytes()],
            ),
            device: digest(
                b"cc-desk/single-run/device/v1",
                &[record.device_id.as_bytes()],
            ),
            key_fingerprints: [
                digest(b"native-endpoint", &[&record.native.endpoint_spki]),
                digest(b"native-relay", &[&record.native.relay_spki]),
                digest(b"browser-tls", &[&record.browser.browser_public_key]),
                digest(b"browser-webauthn", &[&record.browser.webauthn_public_key]),
            ],
            full_record_digest,
            enrollment_revision: candidate.confirmed_revision(),
            issuer: digest(b"browser-grant", &[&record.browser.grant_public_key]),
            principal: record.principal,
            host_epoch: record.epochs.host_key,
            client_epoch: record.epochs.client_key,
            policy_epoch: record.epochs.policy,
            issuer_epoch: record.epochs.issuer,
            run: digest(
                b"cc-desk/single-run/scope/v1",
                &[
                    scope.session.as_bytes(),
                    scope.run.as_bytes(),
                    scope.generation.as_bytes(),
                ],
            ),
            origin: record.browser.origin.clone(),
            rp: record.browser.rp_id.clone(),
            relay_endpoint: record.route.origin.clone(),
            route: record.route.tunnel,
            role: match consent.role() {
                RequestedRole::View => Role::View,
                RequestedRole::Control => Role::Control,
            },
        };
        if !binding.bounded() {
            return Err(Denied::Bounds);
        }
        Ok(Self(binding))
    }
    pub(crate) fn single_run_target(&self) -> Result<SingleRunTargetDigest, Denied> {
        let encoded = serde_json::to_vec(&self.0).map_err(|_| Denied::Bounds)?;
        Ok(SingleRunTargetDigest {
            installation: self.0.installation,
            device: self.0.device,
            run: self.0.run,
            binding: digest(b"cc-desk/single-run/full-binding/v1", &[&encoded]),
        })
    }
}
impl Delegation {
    pub(crate) fn single_run(
        consent: &FinalConsent,
        current: &VerifiedBinding,
        revision: u64,
    ) -> Result<Arc<Self>, Denied> {
        if current.0 != VerifiedBinding::single_run(consent)?.0 || consent.control().closed() {
            return Err(Denied::Locked);
        }
        let clock = Clock::System;
        let now = clock.sample()?;
        let owner = Owner::disabled();
        owner.enable(
            NativeConsent {
                binding: current.0.clone(),
                revision,
                cutoff_ms: consent.cutoff(),
            },
            now,
        )?;
        // 墙钟转换不能把原固定 monotonic cap 延后。
        {
            let mut state = owner.lock()?;
            state.end = state.end.map(|end| end.min(consent.end()));
        }
        let record = consent.candidate().record();
        let client_endpoint =
            cc_desk_remote_security::P256Spki::new(&record.browser.browser_public_key)
                .map_err(|_| Denied::Locked)?
                .endpoint_id(cc_desk_remote_security::Role::Client);
        let relay_origin =
            cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&record.route.origin)
                .map_err(|_| Denied::Locked)?;
        Ok(Arc::new(Self {
            channel_identity: Some(ChannelIdentity {
                client_endpoint,
                relay_origin,
            }),
            final_control: Some(consent.control()),
            owner,
            closed: Arc::new(AtomicBool::new(false)),
            uncertain: AtomicBool::new(false),
            clock,
            #[cfg(test)]
            progress_clock: AtomicBool::new(false),
        }))
    }
    pub(crate) fn matches_single_run(&self, consent: &FinalConsent) -> bool {
        self.final_control
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, &consent.control()))
            && VerifiedBinding::single_run(consent).is_ok_and(|b| {
                self.owner
                    .lock()
                    .is_ok_and(|s| s.binding.as_ref() == Some(&b.0))
            })
    }
}
