//! Process-local desktop enrollment owner. Paired membership is separate from
//! live sharing; this owner never starts a data route or selects an active tab.
use super::{
    control, decode, hex, invitation_codes, ApprovalMachine,
    setup::TrustedHostProfile,
    store::{RelayPair, RevokeApproval, State as DeviceState, Store},
    ObserverApproval, RelayCandidate, RelayInvite, VerifiedCandidate,
};
use crate::{
    cli::{
        document::DocumentBinding, launch_service::NativeRun, profiles::error,
        snapshot::CallerIdentity, types::SafeError,
    },
    native_crypto::owner::NativeRevocation,
    remote_native_pairing_store::StoreError,
};
use cc_desk_remote_transport::wss::{
    EnrollmentInvite, EnrollmentPair, HostEnrollmentCommand as Command,
    HostEnrollmentResponse as Response,
};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Window};
use tokio::sync::Mutex;

#[path = "remote_native_observer_enrollment_sharing_owner.rs"]
mod shares_owner;
#[path = "remote_native_observer_catalogue_owner.rs"]
mod catalogue_owner;

#[derive(Default)]
pub(crate) struct OwnerService {
    active: parking_lot::Mutex<Option<Arc<Owner>>>,
    install_lock: Mutex<()>,
}
pub(crate) struct Owner {
    ticket: String,
    caller: CallerIdentity,
    document: std::sync::Weak<DocumentBinding<NativeRun>>,
    profile: Arc<TrustedHostProfile>,
    address: SocketAddr,
    lifetime: Arc<NativeRevocation>,
    deadline: Instant,
    state: Mutex<Cache>,
    store: Arc<Store>,
    sharing: Mutex<Option<shares_owner::Task>>,
    runtime: Arc<crate::cli::native_runtime::NativeRuntime>,
    host_service: Arc<crate::remote_host::RemoteHostService>,
}
#[derive(Default)]
struct Cache {
    invite: Option<Created>,
}
struct Created {
    id: [u8; 16],
    tunnel: [u8; 16],
    code: String,
    expiry: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Invitation {
    invite_id: String,
    expires_unix_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    manual_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    qr_rows: Option<Vec<String>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    invite: Option<Invitation>,
    pending: Vec<Pending>,
    devices: Vec<Device>,
    scope_choices: Vec<()>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Pending {
    invite_id: String,
    request_id: String,
    phone_id: String,
    phone_relay_public_key_sec1: String,
    phone_endpoint_spki: String,
    endpoint_binding: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeviceTarget {
    tunnel: String,
    phone_id: String,
    phone_relay_public_key_sec1: String,
    phone_endpoint_spki: String,
    endpoint_binding: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Device {
    #[serde(flatten)]
    identity: DeviceTarget,
    sharing: &'static str,
    revocation_pending: bool,
}
pub(crate) enum Operation {
    Current,
    Create(u16),
    Cancel(String),
    Reject(Pending),
    Approve(Pending),
    Revoke(DeviceTarget),
}

impl OwnerService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn install(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        profile: Arc<TrustedHostProfile>,
        address: SocketAddr,
        parent: PathBuf,
        runtime: Arc<crate::cli::native_runtime::NativeRuntime>,
        host_service: Arc<crate::remote_host::RemoteHostService>,
    ) -> Result<(String, String), SafeError> {
        binding.check_admitted(&caller)?;
        let configured = profile.clone();
        let store = tokio::task::spawn_blocking(move || {
            let open = |create| {
                Store::new(
                    &parent,
                    create,
                    configured.endpoint_spki(),
                    configured.endpoint().clone(),
                )
            };
            match open(false) {
                Err(StoreError::Missing) => open(true),
                result => result,
            }
        })
        .await
        .map_err(|_| locked())?
        .map_err(|_| locked())?;
        binding.check_admitted(&caller)?;
        let _install = self.install_lock.lock().await;
        let previous = self.active.lock().clone();
        if let Some(previous) = previous {
            previous.stop_share(None).await?;
        }
        binding.check_admitted(&caller)?;
        let ticket = uuid::Uuid::new_v4().to_string();
        let endpoint = profile.endpoint().as_str().to_owned();
        let owner = Arc::new(Owner {
            ticket: ticket.clone(),
            caller,
            document: Arc::downgrade(binding),
            profile,
            address,
            lifetime: NativeRevocation::new(),
            deadline: Instant::now() + Duration::from_secs(720),
            state: Mutex::new(Cache::default()),
            store: Arc::new(store),
            sharing: Mutex::new(None),
            runtime,
            host_service,
        });
        if let Some(old) = self.active.lock().replace(owner) {
            old.lifetime.revoke();
        }
        Ok((ticket, endpoint))
    }
    pub(crate) fn get(
        &self,
        ticket: &str,
        caller: &CallerIdentity,
        binding: &Arc<DocumentBinding<NativeRun>>,
    ) -> Result<Arc<Owner>, SafeError> {
        let owner = self.active.lock().clone().ok_or_else(locked)?;
        if owner.ticket != ticket
            || &owner.caller != caller
            || !std::sync::Weak::ptr_eq(&owner.document, &Arc::downgrade(binding))
        {
            return Err(locked());
        }
        owner.check()?;
        Ok(owner)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.lifetime.revoke();
    }
}
fn locked() -> SafeError {
    error("REMOTE_PAIRING_LOCKED")
}
fn uncertain() -> SafeError {
    error("REMOTE_PAIRING_OUTCOME_UNKNOWN")
}
impl Owner {
    fn check(&self) -> Result<(), SafeError> {
        if Instant::now() >= self.deadline || self.lifetime.is_revoked() {
            return Err(locked());
        }
        self.document
            .upgrade()
            .ok_or_else(locked)?
            .check_admitted(&self.caller)
    }
    fn control_setup(&self) -> Result<super::setup::ApprovedSetup, SafeError> {
        self.check()?;
        self.profile
            .operation(
                &self.lifetime,
                self.deadline.min(Instant::now() + Duration::from_secs(8)),
            )
            .map_err(|_| locked())
    }
    async fn wire(&self, command: Command) -> Result<Response, SafeError> {
        let request = control::request(self.control_setup()?, self.address, command);
        tokio::pin!(request);
        let result = loop {
            tokio::select! {
                result = &mut request => break result,
                _ = tokio::time::sleep(Duration::from_millis(25)) => {
                    if self.check().is_err() {
                        self.lifetime.revoke();
                        let _ = request.await;
                        return Err(uncertain());
                    }
                }
            }
        };
        self.check()?;
        match result {
            Ok(response) => Ok(response),
            Err(control::ControlFailure::RetirementUnknown) => {
                self.lifetime.revoke();
                Err(uncertain())
            }
            Err(_) => Err(uncertain()),
        }
    }
    async fn listing(&self) -> Result<(Option<EnrollmentInvite>, Vec<EnrollmentPair>), SafeError> {
        match self.wire(Command::List).await? {
            Response::List { mut invites, pairs } if invites.len() <= 1 => {
                Ok((invites.pop(), pairs))
            }
            _ => Err(uncertain()),
        }
    }
    fn candidate(
        &self,
        invite: &EnrollmentInvite,
        cache: &Cache,
    ) -> Result<Option<(Pending, VerifiedCandidate)>, SafeError> {
        let (Some(created), Some(candidate)) = (&cache.invite, &invite.candidate) else {
            return Ok(None);
        };
        let id = decode(&invite.invite_id).map_err(|_| locked())?;
        let tunnel = decode(&invite.tunnel).map_err(|_| locked())?;
        if id != created.id || tunnel != created.tunnel || invite.expires_unix_ms != created.expiry
        {
            return Ok(None);
        }
        let relay = RelayCandidate {
            request_id: decode(&candidate.request_id).map_err(|_| locked())?,
            phone_id: decode(&candidate.phone_id).map_err(|_| locked())?,
            phone_relay_public_key_sec1: decode(&candidate.phone_public_key_sec1)
                .map_err(|_| locked())?,
            endpoint_binding: decode(&candidate.endpoint_binding).map_err(|_| locked())?,
        };
        let Ok(verified) = VerifiedCandidate::verify(
            self.profile.endpoint().clone(),
            &created.code,
            RelayInvite {
                id,
                tunnel,
                expires_unix_ms: created.expiry,
            },
            relay,
            &candidate.candidate_attestation,
        ) else {
            return Ok(None);
        };
        let pending = Pending {
            invite_id: invite.invite_id.clone(),
            request_id: candidate.request_id.clone(),
            phone_id: candidate.phone_id.clone(),
            phone_relay_public_key_sec1: candidate.phone_public_key_sec1.clone(),
            phone_endpoint_spki: hex(&verified.phone_endpoint),
            endpoint_binding: candidate.endpoint_binding.clone(),
        };
        Ok(Some((pending, verified)))
    }
    fn eq_pending(target: &Pending, actual: &Pending) -> bool {
        target.invite_id == actual.invite_id
            && target.request_id == actual.request_id
            && target.phone_id == actual.phone_id
            && target.phone_endpoint_spki == actual.phone_endpoint_spki
            && target.phone_relay_public_key_sec1 == actual.phone_relay_public_key_sec1
            && target.endpoint_binding == actual.endpoint_binding
    }
    async fn snapshot(&self, cache: &mut Cache) -> Result<Snapshot, SafeError> {
        let (invite, pairs) = self.listing().await?;
        let pending = if let Some(invite) = &invite {
            self.candidate(invite, cache)?
                .map(|(view, _)| view)
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        let invite = invite.map(|invite| {
            let codes = cache
                .invite
                .as_ref()
                .filter(|c| {
                    hex(&c.id) == invite.invite_id
                        && hex(&c.tunnel) == invite.tunnel
                        && c.expiry == invite.expires_unix_ms
                })
                .and_then(|c| {
                    invitation_codes(
                        self.profile.endpoint(),
                        &c.code,
                        &self.profile.endpoint_spki(),
                    )
                    .ok()
                });
            let (manual_code, qr_rows) =
                codes.map_or((None, None), |(qr, manual)| (Some(manual), qr_rows(&qr)));
            Invitation {
                invite_id: invite.invite_id,
                expires_unix_ms: invite.expires_unix_ms,
                manual_code,
                qr_rows,
            }
        });
        if invite.is_none() {
            cache.invite = None;
        }
        let sharing = self
            .sharing
            .lock()
            .await
            .as_ref()
            .map(|t| (t.tunnel, t.phase()));
        let store = self.store.clone();
        let devices = tokio::task::spawn_blocking(move || -> Result<Vec<Device>, SafeError> {
            let devices = store.list_locked().map_err(|_| locked())?;
            let mut result = Vec::new();
            for d in devices {
                let candidate = RelayPair {
                    request_id: d.request_id,
                    tunnel: d.tunnel,
                    phone_id: d.phone_id,
                    phone_relay_public_key_sec1: d.phone_relay_public_key_sec1,
                    endpoint_binding: d.endpoint_binding,
                };
                if !pairs.iter().any(|p| party_matches(p, &candidate)) {
                    continue;
                }
                if d.state == DeviceState::Pending {
                    store
                        .reconcile_present(&candidate)
                        .map_err(|_| uncertain())?;
                }
                result.push(Device {
                    sharing: match sharing
                        .filter(|(id, _)| *id == d.tunnel)
                        .map(|(_, phase)| phase)
                    {
                        Some(1) => "offered",
                        Some(2) => "active",
                        Some(3) => "unknown",
                        Some(4) => "controlling",
                        _ => "none",
                    },
                    revocation_pending: d.state == DeviceState::Revoked,
                    identity: DeviceTarget {
                        tunnel: hex(&d.tunnel),
                        phone_id: hex(&d.phone_id),
                        phone_relay_public_key_sec1: hex(&d.phone_relay_public_key_sec1),
                        phone_endpoint_spki: hex(&d.phone_endpoint_spki),
                        endpoint_binding: hex(&d.endpoint_binding),
                    },
                });
            }
            Ok(result)
        })
        .await
        .map_err(|_| uncertain())??;
        self.check()?;
        Ok(Snapshot {
            invite,
            pending,
            devices,
            scope_choices: Vec::new(),
        })
    }
    pub(crate) async fn execute(
        &self,
        op: Operation,
        app: &AppHandle,
        window: &Window,
    ) -> Result<Snapshot, SafeError> {
        let mut cache = self
            .state
            .try_lock()
            .map_err(|_| error("REMOTE_PAIRING_BUSY"))?;
        self.check()?;
        match op {
            Operation::Current => (),
            Operation::Create(ttl) => {
                if !matches!(ttl, 60 | 300 | 600) {
                    return Err(locked());
                }
                if let Response::Created {
                    invite_id,
                    tunnel,
                    code,
                    expires_unix_ms,
                } = self.wire(Command::Create { ttl_seconds: ttl }).await?
                {
                    cache.invite = Some(Created {
                        id: decode(&invite_id).map_err(|_| uncertain())?,
                        tunnel: decode(&tunnel).map_err(|_| uncertain())?,
                        code,
                        expiry: expires_unix_ms,
                    });
                } else {
                    return Err(uncertain());
                }
            }
            Operation::Cancel(id) => {
                let (current, _) = self.listing().await?;
                if current.as_ref().is_none_or(|i| i.invite_id != id) {
                    return Err(locked());
                }
                let invite_id = decode(&id).map_err(|_| locked())?;
                if !matches!(
                    self.wire(Command::Cancel { invite_id }).await?,
                    Response::Closed { .. }
                ) {
                    return Err(uncertain());
                }
                cache.invite = None;
            }
            Operation::Reject(pending) => {
                let (current, _) = self.listing().await?;
                let Some((view, verified)) = current
                    .as_ref()
                    .map(|i| self.candidate(i, &cache))
                    .transpose()?
                    .flatten()
                else {
                    return Err(locked());
                };
                if !Self::eq_pending(&pending, &view) {
                    return Err(locked());
                }
                if !matches!(
                    self.wire(Command::Decide {
                        invite_id: verified.invite.id,
                        request_id: verified.relay.request_id,
                        approve: false,
                        native_approval: None
                    })
                    .await?,
                    Response::Closed { .. }
                ) {
                    return Err(uncertain());
                }
                cache.invite = None;
            }
            Operation::Approve(pending) => {
                let (current, _) = self.listing().await?;
                let Some((view, verified)) = current
                    .as_ref()
                    .map(|i| self.candidate(i, &cache))
                    .transpose()?
                    .flatten()
                else {
                    return Err(locked());
                };
                if !Self::eq_pending(&pending, &view) {
                    return Err(locked());
                }
                let consent = Arc::new(
                    ObserverApproval::show(
                        app,
                        window,
                        verified,
                        None,
                        self.profile.endpoint_spki(),
                        self.profile.epochs(),
                    )
                    .map_err(|_| locked())?,
                );
                // Match the native dialog clock; no polling is sent to the Relay.
                while consent.pending() && !consent.approved() {
                    self.check()?;
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                if !consent.approved() {
                    return self.snapshot(&mut cache).await;
                }
                let issued = control::sign_membership(self.control_setup()?, consent.clone())
                    .await
                    .map_err(|_| uncertain())?
                    .ok_or_else(locked)?;
                self.check()?;
                let s = self.store.clone();
                let issued = tokio::task::spawn_blocking(move || {
                    s.remember_pending(&issued, None).map(|()| issued)
                })
                .await
                .map_err(|_| uncertain())?
                .map_err(|_| uncertain())?;
                if !consent.current(&issued) {
                    return Err(locked());
                }
                let outcome = self
                    .wire(Command::Decide {
                        invite_id: issued.invite_id,
                        request_id: issued.request_id,
                        approve: true,
                        native_approval: Some(issued.wire),
                    })
                    .await?;
                if !matches!(outcome, Response::Approved { tunnel } if tunnel == hex(&issued.tunnel))
                {
                    return Err(uncertain());
                }
                cache.invite = None;
            }
            Operation::Revoke(target) => {
                let (_, pairs) = self.listing().await?;
                let store = self.store.clone();
                let durable = tokio::task::spawn_blocking(move || store.list_locked())
                    .await
                    .map_err(|_| uncertain())?
                    .map_err(|_| locked())?;
                let row = durable
                    .into_iter()
                    .find(|d| {
                        hex(&d.tunnel) == target.tunnel
                            && hex(&d.phone_id) == target.phone_id
                            && hex(&d.phone_endpoint_spki) == target.phone_endpoint_spki
                            && hex(&d.phone_relay_public_key_sec1)
                                == target.phone_relay_public_key_sec1
                            && hex(&d.endpoint_binding) == target.endpoint_binding
                    })
                    .ok_or_else(locked)?;
                let pair = RelayPair {
                    request_id: row.request_id,
                    tunnel: row.tunnel,
                    phone_id: row.phone_id,
                    phone_relay_public_key_sec1: row.phone_relay_public_key_sec1,
                    endpoint_binding: row.endpoint_binding,
                };
                if !pairs.iter().any(|p| party_matches(p, &pair)) {
                    return Err(locked());
                }
                let consent =
                    Arc::new(RevokeApproval::show(app, window, pair).map_err(|_| locked())?);
                while consent.pending() {
                    self.check()?;
                    let s = self.store.clone();
                    let decision = consent.clone();
                    let retired = tokio::task::spawn_blocking(move || s.revoke_local(&decision))
                        .await
                        .map_err(|_| uncertain())?
                        .map_err(|_| uncertain())?;
                    if let Some(tunnel) = retired {
                        self.stop_share(Some(tunnel)).await?;
                        if !matches!(self.wire(Command::Revoke { tunnel }).await?, Response::Revoked { tunnel:id } if id == hex(&tunnel))
                        {
                            return Err(uncertain());
                        }
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
        self.snapshot(&mut cache).await
    }
}
fn qr_rows(payload: &str) -> Option<Vec<String>> {
    let qr = qrcode::QrCode::new(payload.as_bytes()).ok()?;
    let width = qr.width();
    if !(21..=177).contains(&width) {
        return None;
    }
    Some(
        (0..width)
            .map(|y| {
                (0..width)
                    .map(|x| {
                        if qr[(x, y)] == qrcode::Color::Dark {
                            '1'
                        } else {
                            '0'
                        }
                    })
                    .collect()
            })
            .collect(),
    )
}
fn party_matches(p: &EnrollmentPair, exact: &RelayPair) -> bool {
    p.request_id == hex(&exact.request_id)
        && p.tunnel == hex(&exact.tunnel)
        && p.phone_id == hex(&exact.phone_id)
        && p.phone_public_key_sec1 == hex(&exact.phone_relay_public_key_sec1)
        && p.endpoint_binding == hex(&exact.endpoint_binding)
}
