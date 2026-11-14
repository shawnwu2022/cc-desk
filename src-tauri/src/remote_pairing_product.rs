//! 公开记录只是建议；只有显式选择才冻结到当前已鉴权 run。
use crate::{
    cli::{profiles::error, types::SafeError},
    remote_host_approval::{FrozenHostApproval, HostApprovalDescription, NativePersistenceTarget},
    remote_native_pairing_store::*,
    run_supervisor::NativeRunAdmission,
    version_history::windows::pairing_store::WindowsPairingBackend,
};
use serde::{Deserialize, Serialize};
use std::result::Result;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) fn store_error(e: StoreError) -> SafeError {
    error(match e {
        StoreError::Unknown | StoreError::Poisoned => "REMOTE_NATIVE_STORE_COMMIT_UNKNOWN",
        StoreError::Conflict => "REMOTE_NATIVE_STORE_CONFLICT",
        StoreError::Missing => "REMOTE_NATIVE_STORE_MISSING",
        _ => "REMOTE_NATIVE_STORE_FAILED",
    })
}
fn parent() -> Result<PathBuf, StoreError> {
    // 复用已有应用目录，不因列表读取创建父目录。
    dirs::home_dir()
        .map(|p| p.join(".cc-box"))
        .ok_or(StoreError::Missing)
}
pub(crate) fn open() -> Result<WindowsPairingStore, StoreError> {
    PairingStore::new(
        WindowsPairingBackend::open_existing(&parent()?)?,
        CurrentUserDpapi,
    )
}
pub(crate) fn remember_target() -> Result<(WindowsPairingStore, NativePersistenceTarget), StoreError>
{
    let store = match open() {
        Ok(store) => store,
        Err(StoreError::Missing) => PairingStore::new(
            WindowsPairingBackend::create_new(&parent()?)?,
            CurrentUserDpapi,
        )?,
        Err(e) => return Err(e),
    };
    let target = persistence_target(&store)?;
    Ok((store, target))
}
pub(crate) fn persistence_target<B: Backend, S: Sealer>(
    store: &PairingStore<B, S>,
) -> Result<NativePersistenceTarget, StoreError> {
    let expected = match store.load() {
        Ok(snapshot) => Some(snapshot.expected),
        Err(StoreError::Missing) => None,
        Err(e) => return Err(e),
    };
    let target = NativePersistenceTarget {
        store: store.target(),
        store_id: expected
            .as_ref()
            .map(|e| e.store_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        expected,
        device_id: uuid::Uuid::new_v4().to_string(),
    };
    Ok(target)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceSummary {
    device_id: String,
    browser_origin: String,
    relay_origin: String,
    profile_id: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PairingList {
    state: &'static str,
    store_id: Option<String>,
    revision: Option<String>,
    devices: Vec<DeviceSummary>,
}
pub(crate) fn project(snapshot: Option<LockedSnapshot>) -> PairingList {
    let mut reply = PairingList {
        state: "Locked",
        store_id: None,
        revision: None,
        devices: vec![],
    };
    if let Some(snapshot) = snapshot {
        reply.store_id = Some(snapshot.expected.store_id);
        reply.revision = Some(snapshot.expected.revision.to_string());
        reply.devices = snapshot
            .devices
            .into_iter()
            .filter_map(|e| {
                if e.state != RecordState::Active {
                    return None;
                }
                e.record.map(|r| DeviceSummary {
                    device_id: r.device_id,
                    browser_origin: r.browser.origin,
                    relay_origin: r.route.origin,
                    profile_id: hex(&r.native.profile_id),
                })
            })
            .collect();
    }
    reply
}
pub(crate) fn list() -> Result<PairingList, SafeError> {
    match open().and_then(|s| s.load()) {
        Ok(snapshot) => Ok(project(Some(snapshot))),
        Err(StoreError::Missing) => Ok(project(None)),
        Err(e) => Err(store_error(e)),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeviceRequest {
    store_id: String,
    revision: String,
    device_id: String,
}
pub(crate) fn selected(
    snapshot: &LockedSnapshot,
    text: &str,
) -> Result<PublicDeviceRecord, SafeError> {
    if text.len() > 1024 {
        return Err(error("REMOTE_NATIVE_STORE_CONFLICT"));
    }
    let request: DeviceRequest =
        serde_json::from_str(text).map_err(|_| error("REMOTE_NATIVE_STORE_CONFLICT"))?;
    if snapshot.expected.store_id != request.store_id
        || snapshot.expected.revision.to_string() != request.revision
    {
        return Err(error("REMOTE_NATIVE_STORE_CONFLICT"));
    }
    snapshot
        .devices
        .iter()
        .find(|e| e.device_id == request.device_id && e.state == RecordState::Active)
        .and_then(|e| e.record.clone())
        .ok_or_else(|| error("REMOTE_NATIVE_STORE_CONFLICT"))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}

pub(crate) fn freeze(
    record: &PublicDeviceRecord,
    admission: &NativeRunAdmission,
) -> Result<FrozenHostApproval, SafeError> {
    use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
    use cc_desk_remote_security::{
        BrowserProfile, HostSnapshot, NativeGrantPolicy, P256Spki, PeerPin, Role,
    };
    use cc_desk_remote_transport::wss::{RelayMode, RelayRole};
    record.validate().map_err(store_error)?;
    let invalid = || error("REMOTE_HOST_CONFIGURATION");
    let anchor = Instant::now();
    let cap = anchor + Duration::from_secs(60);
    let n = &record.native;
    let b = &record.browser;
    let r = &record.route;
    let origin = CanonicalOrigin::trusted(&r.origin).map_err(|_| invalid())?;
    let endpoint = P256Spki::new(&n.endpoint_spki).map_err(|_| invalid())?;
    let supplier = crate::native_crypto::host_profile::NativeHostProfile::new(
        crate::native_crypto::host::FrozenHostBinding {
            native_profile: crate::native_crypto::host::HostKeyProfileIdentity {
                id: n.profile_id,
                revision: n.profile_revision,
            },
            endpoint_spki: n.endpoint_spki.clone().try_into().map_err(|_| invalid())?,
            relay_spki: n.relay_spki.clone().try_into().map_err(|_| invalid())?,
            origin: origin.clone(),
            relay_id: r.relay_id,
            profile_cap: cap,
        },
        &n.endpoint_key_name,
        &n.relay_key_name,
    )
    .map_err(|_| invalid())?
    .freeze();
    let mut activation = [0; 32];
    activation[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    activation[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    let incarnation = u64::from_le_bytes(
        uuid::Uuid::new_v4().as_bytes()[..8]
            .try_into()
            .map_err(|_| invalid())?,
    )
    .max(1);
    let fields = HostApprovalDescription {
        endpoint_id: endpoint.endpoint_id(Role::Host),
        endpoint_role: Role::Host,
        relay_binding: (r.relay_id, RelayRole::Host),
        address: r.address.parse().map_err(|_| invalid())?,
        tunnel: r.tunnel,
        relay_mode: match r.relay_mode.as_str() {
            "legacyV1" => RelayMode::Legacy,
            "rendezvousV1" => RelayMode::RendezvousV1,
            _ => return Err(invalid()),
        },
        phone_pin: PeerPin::new(
            Role::Client,
            P256Spki::new(&b.browser_public_key).map_err(|_| invalid())?,
        ),
        binding: HostSnapshot {
            host_key_epoch: record.epochs.host_key,
            client_key_epoch: record.epochs.client_key,
            policy_epoch: record.epochs.policy,
            issuer_epoch: record.epochs.issuer,
            activation,
            incarnation,
            origin,
        },
        browser: BrowserProfile::enrolled(
            &b.origin,
            &b.rp_id,
            b.credential_id.clone(),
            P256Spki::new(&b.webauthn_public_key).map_err(|_| invalid())?,
            b.backup_eligible,
            b.allow_backed_up,
        )
        .map_err(|_| invalid())?,
        issuer: P256Spki::new(&b.grant_public_key).map_err(|_| invalid())?,
        principal: record.principal,
        policy: NativeGrantPolicy {
            scope: admission.scope()?,
            role: DataRole::Controller,
            duration: Duration::from_secs(60),
        },
        clock: Arc::new(move || {
            u64::try_from(anchor.elapsed().as_millis())
                .map_err(|_| cc_desk_remote_security::GrantError::Revoked)
        }),
        expires_host_ms: 60000,
        setup_budget: Duration::from_secs(8),
        io_budget: Duration::from_secs(2),
    };
    FrozenHostApproval::supplied(fields, admission, cap, supplier).map_err(|_| invalid())
}
#[derive(Serialize)]
#[serde(tag = "state")]
pub(crate) enum ForgetReply {
    Forgotten,
    Declined,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ForgetRequest {
    store_id: String,
    revision: String,
    device_id: String,
    request_id: String,
    request_sequence: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelForgetRequest {
    request_id: String,
    request_sequence: String,
}
pub(crate) fn forget_request(text: &str) -> Result<(String, u64, String), SafeError> {
    if text.len() > 1024 {
        return Err(error("REMOTE_NATIVE_STORE_CONFLICT"));
    }
    let r: ForgetRequest =
        serde_json::from_str(text).map_err(|_| error("REMOTE_NATIVE_STORE_CONFLICT"))?;
    check_request_id(&r.request_id)?;
    Ok((
        r.request_id,
        crate::remote_host_configuration::decimal(&r.request_sequence)?,
        serde_json::json!({"storeId":r.store_id,"revision":r.revision,"deviceId":r.device_id})
            .to_string(),
    ))
}
pub(crate) fn cancel_request(text: &str) -> Result<(String, u64), SafeError> {
    if text.len() > 128 {
        return Err(error("REMOTE_NATIVE_STORE_CONFLICT"));
    }
    let r: CancelForgetRequest =
        serde_json::from_str(text).map_err(|_| error("REMOTE_NATIVE_STORE_CONFLICT"))?;
    check_request_id(&r.request_id)?;
    Ok((
        r.request_id,
        crate::remote_host_configuration::decimal(&r.request_sequence)?,
    ))
}
fn check_request_id(text: &str) -> Result<(), SafeError> {
    let id = uuid::Uuid::parse_str(text).map_err(|_| error("REMOTE_NATIVE_STORE_CONFLICT"))?;
    if id.is_nil() || id.to_string() != text {
        return Err(error("REMOTE_NATIVE_STORE_CONFLICT"));
    }
    Ok(())
}
use crate::cli::{document::DocumentBinding, launch_service::NativeRun, snapshot::CallerIdentity};
use parking_lot::Mutex;
use std::sync::{LazyLock, Weak};
struct ForgetLane {
    sequence: u64,
    id: String,
    operation: Arc<ForgetOperation>,
}
static FORGET_OPERATIONS: LazyLock<Mutex<Vec<ForgetLane>>> = LazyLock::new(Mutex::default);
pub(crate) struct ForgetOperation {
    document: Weak<DocumentBinding<NativeRun>>,
    caller: CallerIdentity,
    origin: crate::run_supervisor::NativeRunNetworkOrigin,
    state: Mutex<ForgetState>,
}
#[derive(Default)]
struct ForgetState {
    cancelled: bool,
    started: bool,
    finished: bool,
    approval: Option<Arc<crate::remote_host_approval::NativeForgetApproval>>,
}
impl ForgetOperation {
    fn check(
        &self,
        document: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
    ) -> Result<(), SafeError> {
        document.check_admitted(caller)?;
        if self.caller != *caller
            || !Weak::ptr_eq(&self.document, &Arc::downgrade(document))
            || !self.origin.matches(admission)
            || admission.network_origin().is_err()
        {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }
    pub(crate) fn show(
        &self,
        work: impl FnOnce() -> Result<crate::remote_host_approval::NativeForgetApproval, StoreError>,
    ) -> Result<(), SafeError> {
        let mut s = self.state.lock();
        if s.cancelled || s.started || s.finished {
            return Err(error("REMOTE_NATIVE_STORE_CANCELLED"));
        }
        s.started = true;
        s.approval = Some(Arc::new(work().map_err(store_error)?));
        Ok(())
    }
    pub(crate) fn commit<B: Backend, S: Sealer>(
        &self,
        admission: &NativeRunAdmission,
        store: &PairingStore<B, S>,
    ) -> Result<Option<ForgetReply>, SafeError> {
        let mut s = self.state.lock();
        if s.cancelled || s.finished {
            return Ok(Some(ForgetReply::Declined));
        }
        let a = s
            .approval
            .as_ref()
            .ok_or_else(|| error("REMOTE_NATIVE_STORE_FAILED"))?;
        let result = a.commit(admission, |m, a| store.apply(m.clone(), a));
        match result {
            Ok(Some(_)) => {
                s.finished = true;
                s.approval = None;
                Ok(Some(ForgetReply::Forgotten))
            }
            Ok(None) if a.pending() => Ok(None),
            Ok(None) => {
                s.finished = true;
                s.approval = None;
                Ok(Some(ForgetReply::Declined))
            }
            Err(e) => {
                s.finished = true;
                s.approval = None;
                Err(store_error(e))
            }
        }
    }
    pub(crate) fn cancel(&self) {
        let mut s = self.state.lock();
        s.cancelled = true;
        if let Some(a) = s.approval.take() {
            a.cancel();
        }
    }
}
pub(crate) fn forget_operation(
    id: String,
    sequence: u64,
    document: &Arc<DocumentBinding<NativeRun>>,
    caller: &CallerIdentity,
    admission: &NativeRunAdmission,
) -> Result<Arc<ForgetOperation>, SafeError> {
    document.check_admitted(caller)?;
    admission.scope()?;
    if sequence == 0 {
        return Err(error("REMOTE_NATIVE_STORE_CONFLICT"));
    }
    let mut operations = FORGET_OPERATIONS.lock();
    operations.retain(|lane| {
        lane.operation.origin.is_live()
            && lane
                .operation
                .document
                .upgrade()
                .is_some_and(|d| d.check_admitted(&lane.operation.caller).is_ok())
    });
    let lane = operations
        .iter_mut()
        .find(|lane| lane.operation.check(document, caller, admission).is_ok());
    if let Some(lane) = lane {
        if sequence < lane.sequence {
            return Err(error("REMOTE_NATIVE_STORE_CANCELLED"));
        }
        if sequence == lane.sequence {
            if id != lane.id {
                return Err(error("REMOTE_NATIVE_STORE_CONFLICT"));
            }
            return Ok(lane.operation.clone());
        }
        // 高水位只前进。旧取消和迟到请求永远不能重建已退役的 intent。
        lane.operation.cancel();
        let operation = Arc::new(ForgetOperation {
            document: Arc::downgrade(document),
            caller: caller.clone(),
            origin: admission.network_origin()?,
            state: Mutex::default(),
        });
        *lane = ForgetLane {
            sequence,
            id,
            operation: operation.clone(),
        };
        return Ok(operation);
    }
    if operations.len() >= 64 {
        return Err(error("REMOTE_NATIVE_STORE_BUSY"));
    }
    let operation = Arc::new(ForgetOperation {
        document: Arc::downgrade(document),
        caller: caller.clone(),
        origin: admission.network_origin()?,
        state: Mutex::default(),
    });
    operations.push(ForgetLane {
        sequence,
        id,
        operation: operation.clone(),
    });
    Ok(operation)
}
