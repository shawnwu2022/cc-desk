//! Retains public signed pairings under the existing private NTFS/DPAPI rules;
//! reopening storage, including a rolled-back blob, never grants a live scope.
use super::*;
use crate::{
    remote_native_pairing_store::{
        Backend, CurrentUserDpapi, Result, Sealer, StoreError, Transaction,
    },
    version_history::windows::pairing_store::WindowsPairingBackend,
};

const MAX_RECORDS: usize = 32;
const DOMAIN: &str = "cc-desk/native-observer-pairings/v1";

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum State {
    Pending,
    Confirmed,
    Revoked,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Item {
    wire: String,
    state: State,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    domain: String,
    records: Vec<Item>,
}
impl Document {
    fn empty() -> Self {
        Self {
            domain: DOMAIN.into(),
            records: Vec::new(),
        }
    }
}
#[derive(Clone)]
pub(crate) struct DeviceSummary {
    pub(crate) request_id: [u8; 16],
    pub(crate) phone_relay_public_key_sec1: [u8; 65],
    pub(crate) phone_endpoint_spki: [u8; 91],
    pub(crate) phone_grant_spki: [u8; 91],
    pub(crate) tunnel: [u8; 16],
    pub(crate) phone_id: [u8; 16],
    pub(crate) endpoint_binding: [u8; 32],
    pub(crate) state: State,
}

pub(crate) struct RelayPair {
    pub(crate) request_id: [u8; 16],
    pub(crate) tunnel: [u8; 16],
    pub(crate) phone_id: [u8; 16],
    pub(crate) phone_relay_public_key_sec1: [u8; 65],
    pub(crate) endpoint_binding: [u8; 32],
}
impl RelayPair {
    fn matches(&self, r: &Receipt) -> bool {
        r.request_id == hex(&self.request_id)
            && r.tunnel == hex(&self.tunnel)
            && r.phone_id == hex(&self.phone_id)
            && r.phone_relay_public_key_sec1 == hex(&self.phone_relay_public_key_sec1)
            && r.endpoint_binding == hex(&self.endpoint_binding)
    }
}

pub(crate) struct RevokeApproval {
    machine: Arc<ApprovalMachine<RelayPair>>,
}
impl RevokeApproval {
    pub(crate) fn pending(&self) -> bool {
        self.machine.waiting()
    }
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        pair: RelayPair,
    ) -> Result<Self> {
        let text = format!(
            "Revoke this Android device?\n\nDevice: {}\nBinding: {}",
            hex(&pair.phone_id),
            hex(&pair.endpoint_binding)
        );
        DIALOG_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| StoreError::Unauthorized)?;
        Ok(Self {
            machine: ApprovalMachine::show(
                app,
                window,
                pair,
                text,
                "Revoke view-only device",
                Instant::now() + Duration::from_secs(30),
            ),
        })
    }
    pub(crate) fn cancel(&self) {
        self.machine.cancel();
    }
}

pub(crate) struct Store {
    backend: WindowsPairingBackend,
    sealer: CurrentUserDpapi,
    host: [u8; 91],
    origin: CanonicalOrigin,
}
impl Store {
    /// The original app owner decides explicit creation; failed opens never
    /// replace, reset or auto-create the protected directory.
    pub(crate) fn new(
        parent: &std::path::Path,
        create: bool,
        host: [u8; 91],
        origin: CanonicalOrigin,
    ) -> Result<Self> {
        P256Spki::new(&host).map_err(|_| StoreError::Invalid)?;
        if !url_shape(&origin) {
            return Err(StoreError::Invalid);
        }
        let backend = if create {
            WindowsPairingBackend::create_observer(parent)?
        } else {
            WindowsPairingBackend::open_observer(parent)?
        };
        Ok(Self {
            backend,
            sealer: CurrentUserDpapi,
            host,
            origin,
        })
    }

    fn parse(&self, wire: &str) -> Result<Receipt> {
        if wire.is_empty() || wire.len() > MAX_ENVELOPE {
            return Err(StoreError::Invalid);
        }
        let env: Envelope =
            serde_json::from_slice(&B64.decode(wire).map_err(|_| StoreError::Invalid)?)
                .map_err(|_| StoreError::Invalid)?;
        let payload = B64
            .decode(env.payload_b64)
            .map_err(|_| StoreError::Invalid)?;
        let sig = B64
            .decode(env.signature_b64)
            .map_err(|_| StoreError::Invalid)?;
        verify_p256(&self.host, &transcript(HOST_DOMAIN, &payload), &sig)
            .map_err(|_| StoreError::Invalid)?;
        let receipt: Receipt = serde_json::from_slice(&payload).map_err(|_| StoreError::Invalid)?;
        if receipt.version != 1
            || receipt.role != cc_desk_remote_record::grant::DataRole::Observer
            || receipt.relay_origin != self.origin
            || receipt.host_endpoint_spki != hex(&self.host)
        {
            return Err(StoreError::Invalid);
        }
        match (&receipt.session, &receipt.run, &receipt.generation) {
            (None, None, None) => {}
            (Some(session), Some(run), Some(generation)) => {
                encode_scope(&SessionScope {
                    session: session.clone(),
                    run: run.clone(),
                    generation: generation.clone(),
                })
                .map_err(|_| StoreError::Invalid)?;
            }
            _ => return Err(StoreError::Invalid),
        }
        Ok(receipt)
    }
    fn decode_document(&self, bytes: Option<&[u8]>) -> Result<(Document, Vec<DeviceSummary>)> {
        let Some(bytes) = bytes else {
            return Ok((Document::empty(), Vec::new()));
        };
        let doc: Document =
            serde_json::from_slice(&self.sealer.unseal(bytes)?).map_err(|_| StoreError::Invalid)?;
        if doc.domain != DOMAIN || doc.records.len() > MAX_RECORDS {
            return Err(StoreError::Invalid);
        }
        let mut tunnels = std::collections::BTreeSet::new();
        let mut devices = Vec::with_capacity(doc.records.len());
        for row in &doc.records {
            let receipt = self.parse(&row.wire)?;
            let tunnel = decode(&receipt.tunnel).map_err(|_| StoreError::Invalid)?;
            if !tunnels.insert(tunnel) {
                return Err(StoreError::Invalid);
            }
            devices.push(DeviceSummary {
                request_id: decode(&receipt.request_id).map_err(|_| StoreError::Invalid)?,
                phone_relay_public_key_sec1: decode(&receipt.phone_relay_public_key_sec1)
                    .map_err(|_| StoreError::Invalid)?,
                phone_endpoint_spki: decode(&receipt.phone_endpoint_spki)
                    .map_err(|_| StoreError::Invalid)?,
                phone_grant_spki: decode(&receipt.phone_grant_spki)
                    .map_err(|_| StoreError::Invalid)?,
                tunnel,
                phone_id: decode(&receipt.phone_id).map_err(|_| StoreError::Invalid)?,
                endpoint_binding: decode(&receipt.endpoint_binding)
                    .map_err(|_| StoreError::Invalid)?,
                state: row.state,
            });
        }
        Ok((doc, devices))
    }
    pub(crate) fn list_locked(&self) -> Result<Vec<DeviceSummary>> {
        let bytes = self.backend.begin()?.read()?;
        self.decode_document(bytes.as_deref())
            .map(|(_, devices)| devices)
    }
    pub(crate) fn remember_pending(
        &self,
        issued: &SignedObserverReceipt,
        admission: Option<&NativeRunAdmission>,
    ) -> Result<()> {
        let check = || {
            if Instant::now() >= issued.end
                || issued.owner.is_revoked()
                || exact_scope(admission).map_err(|_| StoreError::Unauthorized)?
                    != issued.initial_scope
            {
                return Err(StoreError::Unauthorized);
            }
            Ok(())
        };
        check()?;
        let parsed = self.parse(&issued.wire)?;
        if decode(&parsed.tunnel).map_err(|_| StoreError::Invalid)? != issued.tunnel
            || decode(&parsed.request_id).map_err(|_| StoreError::Invalid)? != issued.request_id
            || decode(&parsed.invite_id).map_err(|_| StoreError::Invalid)? != issued.invite_id
            || decode(&parsed.endpoint_binding).map_err(|_| StoreError::Invalid)?
                != issued.endpoint_binding
        {
            return Err(StoreError::Unauthorized);
        }
        let mut txn = self.backend.begin()?;
        let old = txn.read()?;
        let (mut doc, devices) = self.decode_document(old.as_deref())?;
        if doc.records.len() >= MAX_RECORDS || devices.iter().any(|d| d.tunnel == issued.tunnel) {
            return Err(StoreError::Invalid);
        }
        doc.records.push(Item {
            wire: issued.wire.clone(),
            state: State::Pending,
        });
        let json = serde_json::to_vec(&doc).map_err(|_| StoreError::Invalid)?;
        let sealed = self.sealer.seal(&json)?;
        txn.commit(&sealed, old.is_some(), &mut || check())
    }

    // Only pass a freshly authenticated Host-control LIST record. This only
    // updates a display hint; creates no receive mode or session authority.
    pub(crate) fn reconcile_present(&self, relay: &RelayPair) -> Result<()> {
        self.transition(relay, State::Confirmed, &mut || Ok(()))
    }

    // Persist local denial first; if Relay later fails, never restore this row.
    // Caller must separately stop an active stream and retire the exact Relay
    // membership, reporting Unknown until confirmed.
    pub(crate) fn revoke_local(&self, approval: &RevokeApproval) -> Result<Option<[u8; 16]>> {
        let Some(permit) = approval.machine.take() else {
            return Ok(None);
        };
        let result = permit
            .consume(|pair, end| {
                self.transition(&pair, State::Revoked, &mut || {
                    if Instant::now() >= end || approval.machine.lease.is_revoked() {
                        return Err(StoreError::Unauthorized);
                    }
                    Ok(())
                })?;
                Ok(pair.tunnel)
            })
            .ok_or(StoreError::Unauthorized)?;
        result.map(Some)
    }

    fn transition(
        &self,
        target: &RelayPair,
        state: State,
        check: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        check()?;
        let mut txn = self.backend.begin()?;
        let old = txn.read()?;
        let (mut doc, devices) = self.decode_document(old.as_deref())?;
        let position = devices
            .iter()
            .position(|r| r.tunnel == target.tunnel)
            .ok_or(StoreError::Unauthorized)?;
        let entry = &mut doc.records[position];
        if !target.matches(&self.parse(&entry.wire)?) {
            return Err(StoreError::Unauthorized);
        }
        if entry.state == state {
            return check();
        }
        if entry.state == State::Revoked || state == State::Pending {
            return Err(StoreError::Unauthorized);
        }
        entry.state = state;
        let payload = serde_json::to_vec(&doc).map_err(|_| StoreError::Invalid)?;
        let sealed = self.sealer.seal(&payload)?;
        txn.commit(&sealed, old.is_some(), check)
    }
}
