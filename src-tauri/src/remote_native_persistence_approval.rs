//! 独立批准保存公开设备记录；重开仍 Locked，不恢复任何旧运行权限。
use super::*;
use crate::{
    cli::{document::DocumentBinding, launch_service::NativeRun, snapshot::CallerIdentity},
    remote_native_pairing_store::{
        BrowserPublicIdentity, ExpectedStore, LockedSnapshot, Mutation, MutationAuthorization,
        NativePublicIdentity, PublicDeviceRecord, PublicRoute, StoreError, StoreTarget,
    },
};
use cc_desk_remote_security::browser_registration::UnapprovedBrowserRegistration;
use std::sync::Weak;

/// 目标由原后台 store 产生；前端不能以显示路径或批准布尔代替它。
pub(crate) struct NativePersistenceTarget {
    pub store: StoreTarget,
    pub store_id: String,
    pub expected: Option<ExpectedStore>,
    pub device_id: String,
}

struct FrozenPersistence {
    host: FrozenHostApproval,
    evidence: UnapprovedBrowserRegistration,
    evidence_binding: [u8; 32],
    document: Weak<DocumentBinding<NativeRun>>,
    caller: CallerIdentity,
    target: NativePersistenceTarget,
    mutation: Mutation,
}

fn material(
    host: &FrozenHostApproval,
    evidence: &UnapprovedBrowserRegistration,
    binding: [u8; 32],
    target: &NativePersistenceTarget,
) -> Result<Mutation, StoreError> {
    let invalid = || StoreError::Unauthorized;
    if Instant::now() >= host.cap() {
        return Err(invalid());
    }
    let f = &host.fields;
    if !evidence
        .matches_host_candidate(binding, &f.phone_pin, &f.issuer, &f.browser)
        .map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    let browser = evidence.verified_binding().map_err(|_| invalid())?;
    let supplier = host.supplier.as_ref().ok_or_else(invalid)?;
    let names = supplier.key_references().ok_or_else(invalid)?;
    let native = supplier.binding();
    native.validate().map_err(|_| invalid())?;
    if native.origin != f.binding.origin
        || native.relay_id != f.relay_binding.0
        || P256Spki::new(&native.endpoint_spki)
            .map_err(|_| invalid())?
            .endpoint_id(cc_desk_remote_security::Role::Host)
            != f.endpoint_id
    {
        return Err(invalid());
    }
    let record = PublicDeviceRecord {
        epochs: crate::remote_native_pairing_store::PublicEpochs {
            host_key: f.binding.host_key_epoch,
            client_key: f.binding.client_key_epoch,
            policy: f.binding.policy_epoch,
            issuer: f.binding.issuer_epoch,
        },
        device_id: target.device_id.clone(),
        native: NativePublicIdentity {
            profile_id: native.native_profile.id,
            profile_revision: native.native_profile.revision,
            endpoint_key_name: names[0].clone(),
            relay_key_name: names[1].clone(),
            endpoint_spki: native.endpoint_spki.to_vec(),
            relay_spki: native.relay_spki.to_vec(),
        },
        browser: BrowserPublicIdentity {
            origin: browser.origin.into(),
            rp_id: browser.rp_id.into(),
            credential_id: browser.credential_id.to_vec(),
            webauthn_public_key: browser.credential_spki.to_vec(),
            browser_public_key: browser.tls_spki.to_vec(),
            grant_public_key: browser.grant_spki.to_vec(),
            relay_public_key: browser.relay_spki.to_vec(),
            requested_user_handle: browser.requested_user_handle.to_vec(),
            backup_eligible: browser.backup_eligible,
            allow_backed_up: browser.allow_backed_up,
        },
        route: PublicRoute {
            address: f.address.to_string(),
            origin: f.binding.origin.as_str().into(),
            relay_mode: match f.relay_mode {
                cc_desk_remote_transport::wss::RelayMode::Legacy => "legacyV1",
                cc_desk_remote_transport::wss::RelayMode::RendezvousV1 => "rendezvousV1",
            }
            .into(),
            tunnel: f.tunnel,
            relay_id: native.relay_id,
        },
        principal: f.principal,
    };
    Ok(match &target.expected {
        None => Mutation::Initialize {
            store_id: target.store_id.clone(),
            record,
        },
        Some(expected) if expected.store_id == target.store_id => Mutation::Remember {
            expected: expected.clone(),
            record,
        },
        Some(_) => return Err(invalid()),
    })
}

impl FrozenPersistence {
    fn check(&self, admission: &NativeRunAdmission) -> Result<(), StoreError> {
        let document = self.document.upgrade().ok_or(StoreError::Unauthorized)?;
        document
            .check_admitted(&self.caller)
            .map_err(|_| StoreError::Unauthorized)?;
        if !self.host.matches(admission)
            || admission.network_origin().is_err()
            || material(
                &self.host,
                &self.evidence,
                self.evidence_binding,
                &self.target,
            )? != self.mutation
        {
            return Err(StoreError::Unauthorized);
        }
        Ok(())
    }
    fn render(&self) -> Result<String, StoreError> {
        let record = serde_json::to_string(&self.mutation).map_err(|_| StoreError::Unauthorized)?;
        Ok(format!("Remember this verified public device binding on this Windows account?\nThis separate approval stores the public key names, public keys, credential binding and public route below. It creates no key and stores no private key. On restart the record is only a Locked proposal: every new run still needs its own authenticated scope and Host approval. This does not authorize a Host connection, browser storage, or account/hardware identity.\nProtection is limited to the current Windows user. It is not app-exclusive and cannot detect replay of a valid old record. Forgetting prevents future selection from the current store, not an independently approved live connection.\nTarget: {}\nExact record mutation:\n{}", public_text(self.target.store.label()), public_text(&record)))
    }
}

pub(crate) struct NativePersistenceApproval {
    machine: Arc<ApprovalMachine<FrozenPersistence>>,
}

pub(crate) struct PersistedRegistration {
    pub host: FrozenHostApproval,
    pub evidence: UnapprovedBrowserRegistration,
    pub snapshot: LockedSnapshot,
}

struct PersistenceAuthorization<'a> {
    frozen: &'a FrozenPersistence,
    admission: &'a NativeRunAdmission,
    lease: Arc<ApprovalLease>,
    end: Instant,
}
impl MutationAuthorization for PersistenceAuthorization<'_> {
    fn check(&mut self, target: &StoreTarget, mutation: &Mutation) -> Result<(), StoreError> {
        if self.lease.is_revoked()
            || Instant::now() >= self.end
            || !self.frozen.target.store.same_target(target)
            || &self.frozen.mutation != mutation
        {
            return Err(StoreError::Unauthorized);
        }
        self.frozen.check(self.admission)?;
        if self.lease.is_revoked() || Instant::now() >= self.end {
            return Err(StoreError::Unauthorized);
        }
        Ok(())
    }
}

impl NativePersistenceApproval {
    #[allow(clippy::too_many_arguments)]
    fn freeze(
        mut host: FrozenHostApproval,
        evidence: UnapprovedBrowserRegistration,
        evidence_binding: [u8; 32],
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        target: NativePersistenceTarget,
        end: Instant,
    ) -> Result<(FrozenPersistence, String, Instant), StoreError> {
        host.constrain_from_clock()
            .map_err(|_| StoreError::Unauthorized)?;
        let cap = end.min(host.cap());
        if Instant::now() >= cap {
            return Err(StoreError::Unauthorized);
        }
        let mutation = material(&host, &evidence, evidence_binding, &target)?;
        let frozen = FrozenPersistence {
            host,
            evidence,
            evidence_binding,
            document: Arc::downgrade(document),
            caller: caller.clone(),
            target,
            mutation,
        };
        frozen.check(admission)?;
        let text = frozen.render()?;
        Ok((frozen, text, cap))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        host: FrozenHostApproval,
        evidence: UnapprovedBrowserRegistration,
        evidence_binding: [u8; 32],
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        target: NativePersistenceTarget,
        end: Instant,
    ) -> Result<Arc<Self>, StoreError> {
        let (frozen, text, cap) = Self::freeze(
            host,
            evidence,
            evidence_binding,
            document,
            caller,
            admission,
            target,
            end,
        )?;
        if DIALOG_ACTIVE
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            return Err(StoreError::Unauthorized);
        }
        Ok(Arc::new(Self {
            machine: ApprovalMachine::show(
                app,
                window,
                frozen,
                text,
                "Remember public device binding",
                cap,
            ),
        }))
    }

    /// 原 Yes 在首次尝试时消费；失败或 Unknown 不返回证据，不能自动重放。
    pub(crate) fn commit(
        &self,
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        write: impl FnOnce(
            &Mutation,
            &mut dyn MutationAuthorization,
        ) -> Result<LockedSnapshot, StoreError>,
    ) -> Result<Option<PersistedRegistration>, StoreError> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        permit
            .consume(|frozen, end| {
                if &frozen.caller != caller
                    || !Weak::ptr_eq(&frozen.document, &Arc::downgrade(document))
                {
                    return Err(StoreError::Unauthorized);
                }
                frozen.check(admission)?;
                let mut authorization = PersistenceAuthorization {
                    frozen: &frozen,
                    admission,
                    lease: self.machine.lease.clone(),
                    end,
                };
                let snapshot = write(&frozen.mutation, &mut authorization)?;
                // 一旦存储明确提交，不用迟到的取消把历史写入误报为未发生。
                Ok(PersistedRegistration {
                    host: frozen.host,
                    evidence: frozen.evidence,
                    snapshot,
                })
            })
            .ok_or(StoreError::Unauthorized)
            .map(Some)?
            .transpose()
    }

    pub(crate) fn pending(&self) -> bool {
        self.machine.waiting()
    }

    pub(crate) fn cancel(&self) {
        self.machine.cancel();
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn test_pending(
        host: FrozenHostApproval,
        evidence: UnapprovedBrowserRegistration,
        evidence_binding: [u8; 32],
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        target: NativePersistenceTarget,
        end: Instant,
    ) -> Result<Arc<Self>, StoreError> {
        let (frozen, text, cap) = Self::freeze(
            host,
            evidence,
            evidence_binding,
            document,
            caller,
            admission,
            target,
            end,
        )?;
        assert!(text.contains("only a Locked proposal"));
        Ok(Arc::new(Self {
            machine: ApprovalMachine::pending(frozen, Instant::now(), cap),
        }))
    }
    #[cfg(test)]
    pub(crate) fn test_complete(&self, yes: bool) {
        self.machine.complete(yes, Instant::now());
    }
}
