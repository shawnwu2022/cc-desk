//! Durable public pairing suggestions, never restored authority. No key/network I/O.
#![allow(
    dead_code,
    reason = "native persistence requires its separate approval gate"
)]
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};
#[path = "remote_native_pairing_store/dpapi.rs"]
mod dpapi;
pub(crate) use dpapi::CurrentUserDpapi;
pub(crate) type WindowsPairingStore = PairingStore<
    crate::version_history::windows::pairing_store::WindowsPairingBackend,
    CurrentUserDpapi,
>;
#[cfg(test)]
#[path = "remote_native_pairing_store/tests.rs"]
pub(crate) mod tests;
pub(crate) const MAX_PLAINTEXT: usize = 256 * 1024;
pub(crate) const MAX_SEALED: usize = MAX_PLAINTEXT + 65536;
const DOMAIN: &str = "cc-desk/native-public-pairing/v1";
const MAX_DEVICES: usize = 32;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreError {
    Missing,
    Invalid,
    Conflict,
    Unauthorized,
    Busy,
    Io,
    Unknown,
    Poisoned,
}
pub(crate) type Result<T> = std::result::Result<T, StoreError>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativePublicIdentity {
    pub(crate) profile_id: [u8; 16],
    pub(crate) profile_revision: u64,
    pub(crate) endpoint_key_name: String,
    pub(crate) relay_key_name: String,
    pub(crate) endpoint_spki: Vec<u8>,
    pub(crate) relay_spki: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserPublicIdentity {
    pub(crate) origin: String,
    pub(crate) rp_id: String,
    pub(crate) credential_id: Vec<u8>,
    pub(crate) webauthn_public_key: Vec<u8>,
    /// TLS SPKI, not a private key or key handle.
    pub(crate) browser_public_key: Vec<u8>,
    pub(crate) grant_public_key: Vec<u8>,
    pub(crate) relay_public_key: Vec<u8>,
    pub(crate) requested_user_handle: Vec<u8>,
    pub(crate) backup_eligible: bool,
    pub(crate) allow_backed_up: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicRoute {
    pub(crate) address: String,
    pub(crate) origin: String,
    pub(crate) relay_mode: String,
    pub(crate) tunnel: [u8; 16],
    pub(crate) relay_id: [u8; 16],
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicDeviceRecord {
    pub(crate) epochs: PublicEpochs,
    pub(crate) device_id: String,
    pub(crate) native: NativePublicIdentity,
    pub(crate) browser: BrowserPublicIdentity,
    pub(crate) route: PublicRoute,
    pub(crate) principal: [u8; 32],
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicEpochs {
    pub(crate) host_key: u64,
    pub(crate) client_key: u64,
    pub(crate) policy: u64,
    pub(crate) issuer: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RecordState {
    Active,
    Revoked,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeviceEntry {
    pub(crate) device_id: String,
    pub(crate) state: RecordState,
    /// Tombstones retain identity only.
    pub(crate) record: Option<PublicDeviceRecord>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema: u32,
    domain: String,
    store_id: String,
    revision: u64,
    devices: Vec<DeviceEntry>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    domain: String,
    document: Document,
    digest: [u8; 32],
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ExpectedStore {
    pub(crate) store_id: String,
    pub(crate) revision: u64,
}
/// Deliberately has no conversion to approval, supplier or run owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockedSnapshot {
    pub(crate) expected: ExpectedStore,
    pub(crate) devices: Vec<DeviceEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum Mutation {
    Initialize {
        store_id: String,
        record: PublicDeviceRecord,
    },
    Remember {
        expected: ExpectedStore,
        record: PublicDeviceRecord,
    },
    Forget {
        expected: ExpectedStore,
        device_id: String,
    },
}
/// A capability for this exact store object, not a caller-provided pathname.
#[derive(Debug, Clone)]
pub(crate) struct StoreTarget {
    identity: Arc<()>,
    label: String,
}
impl StoreTarget {
    pub(crate) fn same_target(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }
    pub(crate) fn label(&self) -> &str {
        &self.label
    }
}
/// Production implementations belong to the native approval owner. It consumes
/// its one-shot permission before apply, even on error, and checks exact data,
/// target, original owner, live lease and deadline on every check.
pub(crate) trait MutationAuthorization {
    fn check(&mut self, target: &StoreTarget, mutation: &Mutation) -> Result<()>;
}
pub(crate) trait Sealer {
    fn seal(&self, bytes: &[u8]) -> Result<Vec<u8>>;
    fn unseal(&self, bytes: &[u8]) -> Result<Vec<u8>>;
}
pub(crate) trait Transaction {
    fn read(&mut self) -> Result<Option<Vec<u8>>>;
    /// Lock-held publication. Any error after publication was attempted is Unknown.
    fn commit(
        &mut self,
        bytes: &[u8],
        replacing: bool,
        authorize: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()>;
}
pub(crate) trait Backend {
    type Guard: Transaction;
    fn begin(&self) -> Result<Self::Guard>;
    /// Obtained from the pinned private directory, never a display-only caller path.
    fn target_label(&self) -> Result<String>;
}
pub(crate) struct PairingStore<B, S> {
    backend: B,
    sealer: S,
    target: StoreTarget,
    poisoned: parking_lot::Mutex<bool>,
}
impl<B: Backend, S: Sealer> PairingStore<B, S> {
    pub(crate) fn new(backend: B, sealer: S) -> Result<Self> {
        let target = StoreTarget {
            identity: Arc::new(()),
            label: backend.target_label()?,
        };
        Ok(Self {
            backend,
            sealer,
            target,
            poisoned: parking_lot::Mutex::new(false),
        })
    }
    pub(crate) fn target(&self) -> StoreTarget {
        self.target.clone()
    }
    pub(crate) fn load(&self) -> Result<LockedSnapshot> {
        self.with_locked_snapshot(Ok)
    }
    pub(crate) fn with_locked_snapshot<T>(
        &self,
        work: impl FnOnce(LockedSnapshot) -> Result<T>,
    ) -> Result<T> {
        let poison = self.poisoned.lock();
        if *poison {
            return Err(StoreError::Poisoned);
        }
        let mut tx = self.backend.begin()?;
        let bytes = tx.read()?.ok_or(StoreError::Missing)?;
        work(snapshot(self.decode(&bytes)?))
    }
    pub(crate) fn apply<A: MutationAuthorization + ?Sized>(
        &self,
        mutation: Mutation,
        authorization: &mut A,
    ) -> Result<LockedSnapshot> {
        let mut poison = self.poisoned.lock();
        if *poison {
            return Err(StoreError::Poisoned);
        }
        authorization.check(&self.target, &mutation)?;
        let mut tx = self.backend.begin()?;
        let previous = tx.read()?;
        let replacing = previous.is_some();
        let mut doc = match &mutation {
            Mutation::Initialize { store_id, record } => {
                if previous.is_some() {
                    return Err(StoreError::Conflict);
                }
                uuid_text(store_id)?;
                record.validate()?;
                Document {
                    schema: 2,
                    domain: DOMAIN.into(),
                    store_id: store_id.clone(),
                    revision: 1,
                    devices: vec![entry(record.clone())],
                }
            }
            Mutation::Remember { expected, record } => {
                let mut doc = self.decode(previous.as_deref().ok_or(StoreError::Missing)?)?;
                cas(&doc, expected)?;
                record.validate()?;
                match doc
                    .devices
                    .iter_mut()
                    .find(|e| e.device_id == record.device_id)
                {
                    Some(e) if e.state == RecordState::Revoked => *e = entry(record.clone()),
                    Some(_) => return Err(StoreError::Conflict),
                    None => doc.devices.push(entry(record.clone())),
                }
                doc.revision = doc.revision.checked_add(1).ok_or(StoreError::Invalid)?;
                doc
            }
            Mutation::Forget {
                expected,
                device_id,
            } => {
                let mut doc = self.decode(previous.as_deref().ok_or(StoreError::Missing)?)?;
                cas(&doc, expected)?;
                let e = doc
                    .devices
                    .iter_mut()
                    .find(|e| &e.device_id == device_id)
                    .ok_or(StoreError::Conflict)?;
                if e.state != RecordState::Active {
                    return Err(StoreError::Conflict);
                }
                e.state = RecordState::Revoked;
                e.record = None;
                doc.revision = doc.revision.checked_add(1).ok_or(StoreError::Invalid)?;
                doc
            }
        };
        doc.devices.sort_by(|a, b| a.device_id.cmp(&b.device_id));
        validate_document(&doc)?;
        let bytes = self.encode(&doc)?;
        if let Err(error) = tx.commit(&bytes, replacing, &mut || {
            authorization.check(&self.target, &mutation)
        }) {
            if error == StoreError::Unknown {
                *poison = true;
            }
            return Err(error);
        }
        Ok(snapshot(doc))
    }
    fn encode(&self, doc: &Document) -> Result<Vec<u8>> {
        let canonical = serde_json::to_vec(doc).map_err(|_| StoreError::Invalid)?;
        let envelope = Envelope {
            domain: DOMAIN.into(),
            document: doc.clone(),
            digest: Sha256::digest(&canonical).into(),
        };
        let plaintext = serde_json::to_vec(&envelope).map_err(|_| StoreError::Invalid)?;
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(StoreError::Invalid);
        }
        let sealed = self.sealer.seal(&plaintext)?;
        if sealed.is_empty() || sealed.len() > MAX_SEALED {
            return Err(StoreError::Invalid);
        }
        Ok(sealed)
    }
    fn decode(&self, bytes: &[u8]) -> Result<Document> {
        if bytes.is_empty() || bytes.len() > MAX_SEALED {
            return Err(StoreError::Invalid);
        }
        let plaintext = self.sealer.unseal(bytes)?;
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(StoreError::Invalid);
        }
        // Direct struct decode rejects duplicate as well as unknown fields.
        let e: Envelope = serde_json::from_slice(&plaintext).map_err(|_| StoreError::Invalid)?;
        if e.domain != DOMAIN {
            return Err(StoreError::Invalid);
        }
        validate_document(&e.document)?;
        let canonical = serde_json::to_vec(&e.document).map_err(|_| StoreError::Invalid)?;
        let digest: [u8; 32] = Sha256::digest(canonical).into();
        if digest != e.digest {
            return Err(StoreError::Invalid);
        }
        Ok(e.document)
    }
}
fn snapshot(doc: Document) -> LockedSnapshot {
    LockedSnapshot {
        expected: ExpectedStore {
            store_id: doc.store_id,
            revision: doc.revision,
        },
        devices: doc.devices,
    }
}
fn entry(record: PublicDeviceRecord) -> DeviceEntry {
    DeviceEntry {
        device_id: record.device_id.clone(),
        state: RecordState::Active,
        record: Some(record),
    }
}
fn cas(doc: &Document, expected: &ExpectedStore) -> Result<()> {
    if doc.store_id != expected.store_id || doc.revision != expected.revision {
        Err(StoreError::Conflict)
    } else {
        Ok(())
    }
}
fn uuid_text(value: &str) -> Result<()> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| StoreError::Invalid)?;
    if parsed.is_nil() || parsed.to_string() != value {
        return Err(StoreError::Invalid);
    }
    Ok(())
}
fn key_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" _-.".contains(&b))
    {
        return Err(StoreError::Invalid);
    }
    Ok(())
}
fn validate_document(doc: &Document) -> Result<()> {
    if doc.schema != 2
        || doc.domain != DOMAIN
        || doc.revision == 0
        || doc.devices.is_empty()
        || doc.devices.len() > MAX_DEVICES
    {
        return Err(StoreError::Invalid);
    }
    uuid_text(&doc.store_id)?;
    let mut seen = BTreeSet::new();
    for e in &doc.devices {
        uuid_text(&e.device_id)?;
        if !seen.insert(&e.device_id) {
            return Err(StoreError::Invalid);
        }
        match (&e.state, &e.record) {
            (RecordState::Active, Some(r)) if r.device_id == e.device_id => r.validate()?,
            (RecordState::Revoked, None) => (),
            _ => return Err(StoreError::Invalid),
        }
    }
    Ok(())
}
impl PublicDeviceRecord {
    pub(crate) fn validate(&self) -> Result<()> {
        use cc_desk_remote_record::appbind::CanonicalOrigin;
        use cc_desk_remote_security::{BrowserProfile, P256Spki};
        uuid_text(&self.device_id)?;
        let n = &self.native;
        let b = &self.browser;
        let r = &self.route;
        key_name(&n.endpoint_key_name)?;
        key_name(&n.relay_key_name)?;
        if [
            self.epochs.host_key,
            self.epochs.client_key,
            self.epochs.policy,
            self.epochs.issuer,
        ]
        .contains(&0)
            || n.profile_id == [0; 16]
            || n.profile_revision == 0
            || n.endpoint_key_name == n.relay_key_name
            || n.endpoint_spki == n.relay_spki
            || self.principal == [0; 32]
            || r.tunnel == [0; 16]
            || r.relay_id == [0; 16]
        {
            return Err(StoreError::Invalid);
        }
        for key in [
            &n.endpoint_spki,
            &n.relay_spki,
            &b.webauthn_public_key,
            &b.browser_public_key,
            &b.grant_public_key,
            &b.relay_public_key,
        ] {
            P256Spki::new(key).map_err(|_| StoreError::Invalid)?;
        }
        if b.requested_user_handle.is_empty()
            || b.requested_user_handle.len() > 64
            || b.origin.len() > 512
            || b.rp_id.len() > 253
            || r.origin.len() > 512
            || r.address.len() > 128
        {
            return Err(StoreError::Invalid);
        }
        BrowserProfile::enrolled(
            &b.origin,
            &b.rp_id,
            b.credential_id.clone(),
            P256Spki::new(&b.webauthn_public_key).map_err(|_| StoreError::Invalid)?,
            b.backup_eligible,
            b.allow_backed_up,
        )
        .map_err(|_| StoreError::Invalid)?;
        let origin = CanonicalOrigin::trusted(&r.origin).map_err(|_| StoreError::Invalid)?;
        let address: std::net::SocketAddr = r.address.parse().map_err(|_| StoreError::Invalid)?;
        if origin.as_str() != r.origin
            || address.to_string() != r.address
            || address.port() == 0
            || address.ip().is_unspecified()
            || address.ip().is_multicast()
            || !matches!(r.relay_mode.as_str(), "legacyV1" | "rendezvousV1")
        {
            return Err(StoreError::Invalid);
        }
        Ok(())
    }
}
