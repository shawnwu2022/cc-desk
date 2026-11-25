//! Setup for an operator-provisioned Host identity. This never imports, creates,
//! replaces or exports a private CNG key and never treats a URL as authorization.
use super::*;
use crate::native_crypto::{
    host::{FrozenHostBinding, FrozenHostSupplier, HostKeyProfileIdentity},
    host_profile::NativeHostProfile,
};

pub(crate) struct TrustedHostProfile {
    profile_id: [u8; 16],
    revision: u64,
    endpoint_name: String,
    relay_name: String,
    endpoint_spki: [u8; 91],
    relay_spki: [u8; 91],
    relay_id: [u8; 16],
    origin: CanonicalOrigin,
    epochs: HostEpochs,
}
/// Inputs come from an explicit trusted operator import, not a relay response
/// or a renderer typing an address. Verification separately opens both exact
/// CNG keys and proves the Relay Host role against this exact WSS origin.
pub(crate) struct OperatorHostProfileInput {
    pub(crate) profile_id: [u8; 16],
    pub(crate) revision: u64,
    pub(crate) endpoint_name: String,
    pub(crate) relay_name: String,
    pub(crate) endpoint_spki: [u8; 91],
    pub(crate) relay_spki: [u8; 91],
    pub(crate) relay_id: [u8; 16],
    pub(crate) origin: CanonicalOrigin,
    pub(crate) epochs: HostEpochs,
}
impl TrustedHostProfile {
    pub(crate) fn from_operator(
        input: OperatorHostProfileInput,
        typed_origin: &str,
    ) -> Result<Self> {
        let selected = CanonicalOrigin::trusted(typed_origin).map_err(|_| Error::Invalid)?;
        if !url_shape(&selected) || selected != input.origin {
            return Err(Error::Authority);
        }
        let profile = Self {
            profile_id: input.profile_id,
            revision: input.revision,
            endpoint_name: input.endpoint_name,
            relay_name: input.relay_name,
            endpoint_spki: input.endpoint_spki,
            relay_spki: input.relay_spki,
            relay_id: input.relay_id,
            origin: selected,
            epochs: input.epochs,
        };
        // Structural validation only; no I/O, no proof and no authorization.
        if [
            profile.epochs.host_key,
            profile.epochs.client_key,
            profile.epochs.policy,
            profile.epochs.issuer,
        ]
        .contains(&0)
        {
            return Err(Error::Authority);
        }
        let end = Instant::now() + Duration::from_secs(30);
        drop(profile.prepare_candidate(end)?);
        Ok(profile)
    }

    pub(crate) fn operation(
        &self,
        owner: &Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: Instant,
    ) -> Result<ApprovedSetup> {
        use crate::native_crypto::owner::NativeRevocation;
        if Instant::now() >= cap || owner.is_revoked() {
            return Err(Error::Authority);
        }
        let local = NativeRevocation::new();
        let roots = [
            owner.root_witness().ok_or(Error::Authority)?,
            local.root_witness().ok_or(Error::Authority)?,
        ];
        let signing = crate::remote_host_approval::RegistrationSigningScope::from_native_enrollment(
            NativeRevocation::child(roots),
            cap,
        );
        Ok(ApprovedSetup {
            keys: self.prepare_candidate(cap)?,
            signing,
        })
    }
    pub(crate) fn endpoint(&self) -> &CanonicalOrigin {
        &self.origin
    }
    pub(crate) fn epochs(&self) -> HostEpochs {
        self.epochs
    }
    pub(crate) fn endpoint_spki(&self) -> [u8; 91] {
        self.endpoint_spki
    }
    fn prepare_candidate(&self, end: Instant) -> Result<FrozenHostSupplier> {
        let binding = FrozenHostBinding {
            native_profile: HostKeyProfileIdentity {
                id: self.profile_id,
                revision: self.revision,
            },
            endpoint_spki: self.endpoint_spki,
            relay_spki: self.relay_spki,
            origin: self.origin.clone(),
            relay_id: self.relay_id,
            profile_cap: end,
        };
        NativeHostProfile::new(binding, &self.endpoint_name, &self.relay_name)
            .map(NativeHostProfile::freeze)
            .map_err(|_| Error::Authority)
    }

    pub(crate) fn show_setup<R: tauri::Runtime>(
        self: Arc<Self>,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<SetupApproval> {
        let fingerprint = P256Spki::new(&self.endpoint_spki)
            .map_err(|_| Error::Authority)?
            .endpoint_id(Role::Host);
        let text = format!(
            "Allow this desktop to check its configured relay?\n\nRelay: {}\nDesktop identity: {}",
            self.origin.as_str(),
            hex(&fingerprint),
        );
        reserve_dialog()?;
        Ok(SetupApproval {
            machine: ApprovalMachine::show(
                app,
                window,
                self,
                text,
                "Verify configured relay",
                Instant::now() + Duration::from_secs(30),
            ),
        })
    }

    /// Sharing public registration material is separate from setup approval.
    /// Registration itself remains the operator's explicit policy action.
    pub(crate) fn show_public_metadata<R: tauri::Runtime>(
        self: Arc<Self>,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<MetadataApproval> {
        let text = format!(
            "Show this desktop's public relay registration details?\n\nOnly public metadata is shown. The relay operator still has to authorize this desktop.\nRelay: {}",
            self.origin.as_str(),
        );
        reserve_dialog()?;
        Ok(MetadataApproval {
            machine: ApprovalMachine::show(
                app,
                window,
                self,
                text,
                "Show public registration details",
                Instant::now() + Duration::from_secs(30),
            ),
        })
    }
}
fn reserve_dialog() -> Result<()> {
    DIALOG_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| Error::Busy)
}

pub(crate) struct SetupApproval {
    machine: Arc<ApprovalMachine<Arc<TrustedHostProfile>>>,
}
pub(crate) struct ApprovedSetup {
    pub(crate) keys: FrozenHostSupplier,
    pub(crate) signing: crate::remote_host_approval::RegistrationSigningScope,
}
impl SetupApproval {
    pub(crate) fn pending(&self) -> bool {
        self.machine.waiting()
    }
    pub(crate) fn cancel(&self) {
        self.machine.cancel();
    }
    // Consumed once on the owner. Caller retains this original approval and
    // native custody until exact Relay LIST proof completes or is retired.
    pub(crate) fn take(&self) -> Result<Option<ApprovedSetup>> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        let result = permit
            .consume(|profile, cap| {
                let keys = profile.prepare_candidate(cap)?;
                let signing =
                    crate::remote_host_approval::RegistrationSigningScope::from_native_enrollment(
                        self.machine.lease.native.clone(),
                        cap,
                    );
                Ok(ApprovedSetup { keys, signing })
            })
            .ok_or(Error::Authority)?;
        result.map(Some)
    }
}

pub(crate) struct MetadataApproval {
    machine: Arc<ApprovalMachine<Arc<TrustedHostProfile>>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicRegistrationMetadata {
    version: u8,
    role: &'static str,
    relay_id: String,
    relay_public_key_sec1: String,
}
impl MetadataApproval {
    pub(crate) fn pending(&self) -> bool {
        self.machine.waiting()
    }
    pub(crate) fn cancel(&self) {
        self.machine.cancel();
    }
    pub(crate) fn take(&self) -> Option<PublicRegistrationMetadata> {
        self.machine
            .take()?
            .consume(|profile, _| PublicRegistrationMetadata {
                version: 1,
                role: "Host",
                relay_id: hex(&profile.relay_id),
                relay_public_key_sec1: hex(&profile.relay_spki[26..]),
            })
    }
}
