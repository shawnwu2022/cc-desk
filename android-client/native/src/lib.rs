//! Observer-only transport for the Android app. This crate deliberately has
//! no JNI constructor from a Java DTO, QR payload, displayed host or relay
//! admission result. Its Rust caller must already own an authenticated profile
//! from the current durable native enrollment owner and approved session scope.
use cc_desk_remote_client::{
    ClientRole, ObserverViewReceipt,
    bootstrap::{BootstrapError, NativeProfile, connect_native},
};
use cc_desk_remote_transport::wss::{RelayMode, RelayRole};
use std::{
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::sync::watch;

#[cfg(target_os = "android")]
mod android_pairing;
#[cfg(target_os = "android")]
mod jni_bridge;
#[cfg(target_os = "android")]
pub mod keystore;

pub mod enrollment;
pub mod phone_enrollment;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserverHandle(i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    UnapprovedProfile,
    NoAuthorizedScope,
    Busy,
    OwnerUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ObserverStatus {
    Closed = 0,
    Connecting = 1,
    Authenticated = 2,
    Unavailable = 3,
}

enum State {
    Connecting,
    Viewing(Option<Box<ObserverViewReceipt>>),
    Closed,
    Failed,
}

impl State {
    fn status(&self) -> ObserverStatus {
        match self {
            Self::Connecting => ObserverStatus::Connecting,
            Self::Viewing(_) => ObserverStatus::Authenticated,
            Self::Closed => ObserverStatus::Closed,
            Self::Failed => ObserverStatus::Unavailable,
        }
    }
}

struct AttemptState {
    phase: Mutex<State>,
    stop: watch::Sender<bool>,
}

struct Attempt {
    id: ObserverHandle,
    state: Arc<AttemptState>,
    worker: JoinHandle<bool>,
}

struct Service {
    next: i64,
    poisoned: bool,
    active: Option<Attempt>,
}

static SERVICE: Mutex<Service> = Mutex::new(Service {
    next: 0,
    poisoned: false,
    active: None,
});

/// Backend handoff only. This must never be invoked on a profile assembled from
/// unchecked app metadata. The signers/host pin must come from the approved
/// native owner and use the Keystore; no signer or TLS fallback is supplied.
#[cfg(not(target_os = "android"))]
pub fn start_verified_observer(
    approval: enrollment::DurablyAcceptedEnrollment,
    address: std::net::SocketAddr,
    keys: enrollment::NativeKeystoreBindings,
    state: cc_desk_remote_security::ClientGrantState,
    now_unix_ms: u64,
) -> Result<ObserverHandle, StartError> {
    if approval.scope().is_none() {
        return Err(StartError::NoAuthorizedScope);
    }
    let profile = approval
        .0
        .into_profile(address, keys, state, now_unix_ms)
        .map_err(|_| StartError::UnapprovedProfile)?;
    if !validate_profile(&profile) {
        return Err(StartError::UnapprovedProfile);
    }
    start_attempt(move |runtime, owner, stop| runtime.block_on(observe(profile, owner, stop)))
}

#[cfg(target_os = "android")]
pub fn start_android_observer(
    env: &jni::JNIEnv<'_>,
    facade: &jni::objects::JObject<'_>,
    approval: enrollment::DurablyAcceptedEnrollment,
    address: std::net::SocketAddr,
    state: cc_desk_remote_security::ClientGrantState,
) -> Result<ObserverHandle, StartError> {
    if approval.scope().is_none() {
        return Err(StartError::NoAuthorizedScope);
    }
    let vm = env
        .get_java_vm()
        .map_err(|_| StartError::OwnerUnavailable)?;
    let facade = env
        .new_global_ref(facade)
        .map_err(|_| StartError::OwnerUnavailable)?;
    start_attempt(move |runtime, owner, stop| {
        let approval = approval.0;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|t| u64::try_from(t.as_millis()).ok());
        let Some(now) = now else {
            finish(&owner, true);
            return true;
        };
        if now >= approval.metadata().expires_unix_ms || *stop.borrow() {
            finish(&owner, true);
            return true;
        }
        let (keys, provider) = match keystore::open_roles(
            vm,
            facade,
            approval.endpoint_spki(),
            approval.origin().clone(),
            approval.metadata().phone_id,
        ) {
            Ok(roles) => roles.into_parts(),
            Err(_) => {
                finish(&owner, true);
                return true;
            }
        };
        let closed = match approval.into_profile(address, keys, state, now) {
            Ok(profile) if validate_profile(&profile) => {
                runtime.block_on(observe(profile, owner.clone(), stop))
            }
            _ => {
                finish(&owner, true);
                true
            }
        };
        // Never detach an in-flight Android Keystore operation. Even if the
        // UI cancelled/returned, this attempt keeps its exclusive service slot.
        let provider_closed = provider.retire();
        closed && provider_closed
    })
}

fn validate_profile(profile: &NativeProfile) -> bool {
    matches!(profile.grant.role, ClientRole::Observer)
        && matches!(profile.relay_mode, RelayMode::RendezvousV1)
        && matches!(profile.relay.binding(), (key, RelayRole::Phone) if key != [0; 16])
        && profile.tunnel != [0; 16]
        && profile.address.port() != 0
}

fn start_attempt(
    work: impl FnOnce(tokio::runtime::Runtime, Arc<AttemptState>, watch::Receiver<bool>) -> bool
    + Send
    + 'static,
) -> Result<ObserverHandle, StartError> {
    let mut service = SERVICE.lock().map_err(|_| StartError::OwnerUnavailable)?;
    if service
        .active
        .as_ref()
        .is_some_and(|a| !a.worker.is_finished())
    {
        return Err(StartError::Busy);
    }
    if let Some(old) = service.active.take() {
        service.poisoned |= !matches!(old.worker.join(), Ok(true));
    }
    if service.poisoned {
        return Err(StartError::OwnerUnavailable);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| StartError::OwnerUnavailable)?;
    let id = ObserverHandle(
        service
            .next
            .checked_add(1)
            .ok_or(StartError::OwnerUnavailable)?,
    );
    let (stop, rx) = watch::channel(false);
    let state = Arc::new(AttemptState {
        phase: Mutex::new(State::Connecting),
        stop,
    });
    let owned = state.clone();
    let worker = thread::Builder::new()
        .name("ccdesk-observer".into())
        .spawn(move || work(runtime, owned, rx))
        .map_err(|_| StartError::OwnerUnavailable)?;
    service.next = id.0;
    service.active = Some(Attempt { id, state, worker });
    Ok(id)
}

impl ObserverHandle {
    pub fn token(self) -> i64 {
        self.0
    }
}

fn lookup(id: i64) -> Option<Arc<AttemptState>> {
    let service = SERVICE.lock().ok()?;
    service
        .active
        .as_ref()
        .filter(|a| a.id.0 == id)
        .map(|a| a.state.clone())
}

pub fn status(token: i64) -> ObserverStatus {
    lookup(token)
        .and_then(|a| {
            let phase = a.phase.lock().ok()?;
            Some(if *a.stop.borrow() {
                ObserverStatus::Closed
            } else {
                phase.status()
            })
        })
        .unwrap_or(ObserverStatus::Closed)
}

/// Consume once and check the original authenticated channel authority again.
/// Nothing here acknowledges, sends, resubscribes, reconnects or grants input.
#[cfg(target_os = "android")]
fn take_authenticated_snapshot(token: i64) -> Option<Vec<Vec<u8>>> {
    let a = lookup(token)?;
    let mut phase = a.phase.lock().ok()?;
    if *a.stop.borrow() {
        return None;
    }
    let State::Viewing(slot) = &mut *phase else {
        return None;
    };
    let receipt = slot.take()?;
    match receipt.into_snapshot() {
        Ok(batch) => Some(batch.parts),
        Err(_) => {
            *phase = State::Failed;
            a.stop.send_replace(true);
            None
        }
    }
}

pub fn cancel(token: i64) {
    if let Some(a) = lookup(token) {
        if let Ok(mut phase) = a.phase.lock() {
            a.stop.send_replace(true);
            *phase = State::Closed;
        } else {
            a.stop.send_replace(true);
        }
    }
}

fn finish(state: &AttemptState, failure: bool) {
    if let Ok(mut phase) = state.phase.lock() {
        *phase = if failure && !*state.stop.borrow() {
            State::Failed
        } else {
            State::Closed
        };
    }
}

/// This original worker owns both startup and retirement. No public caller
/// can drop its future, release the slot while it is closing, or start a retry.
async fn observe(
    profile: NativeProfile,
    state: Arc<AttemptState>,
    mut rx: watch::Receiver<bool>,
) -> bool {
    let client = match connect_native(profile, rx.clone()).await {
        Ok(client) => client,
        Err(error) => {
            finish(&state, !matches!(error, BootstrapError::Cancelled));
            return !matches!(error, BootstrapError::RetirementUnknown);
        }
    };
    let view = tokio::select! { biased;
        _ = rx.changed() => None,
        result = client.receive_observer_view(Duration::from_secs(1)) => result.ok(),
    };
    let presented = if let Some(receipt) = view {
        if let Ok(mut phase) = state.phase.lock() {
            if !*state.stop.borrow()
                && client
                    .descriptor()
                    .is_ok_and(|d| matches!(d.role, ClientRole::Observer))
            {
                *phase = State::Viewing(Some(Box::new(receipt)));
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if presented {
        loop {
            tokio::select! { biased;
                _ = rx.changed() => break,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if client.descriptor().is_err() { break; }
                }
            }
        }
    }
    // Remove Java-visible data before waiting for both native transport owners.
    finish(&state, true);
    client.retire().await.is_ok()
}
// No control sequences or terminal input. This is an intentionally limited
// semantic text representation of exactly one complete authenticated grid.
pub fn observer_text(parts: &[Vec<u8>]) -> Option<String> {
    use cc_desk_remote_record::projection::{self, Content, Role};
    let mut original = None;
    let mut cells = Vec::new();
    let mut columns = 0usize;
    let mut rows = 0usize;
    for raw in parts {
        let Content::SnapshotPart(part) = projection::decode(raw, 1).ok()?.content else {
            return None;
        };
        if cells.len() != usize::from(part.start) {
            return None;
        }
        let metadata = (&part.stamp, &part.state);
        if let Some((stamp, state)) = &original {
            if metadata != (stamp, state) {
                return None;
            }
        } else {
            columns = usize::from(part.state.columns);
            rows = usize::from(part.state.rows);
            if !(2..=240).contains(&columns) || !(1..=100).contains(&rows) {
                return None;
            }
            original = Some((part.stamp, part.state));
        }
        if cells.len() + part.cells.len() > rows * columns {
            return None;
        }
        cells.extend(part.cells);
    }
    if original.is_none() || cells.len() != rows * columns {
        return None;
    }
    let mut output = String::with_capacity(cells.len() + rows);
    for (i, cell) in cells.iter().enumerate() {
        let column = i % columns;
        if (cell.wrap_end && column != columns - 1)
            || (cell.role == Role::WideLead
                && (column == columns - 1
                    || !cells.get(i + 1).is_some_and(|c| c.role == Role::WideSpacer)))
            || (cell.role == Role::WideSpacer
                && (column == 0 || cells[i - 1].role != Role::WideLead))
            || (cell.role == Role::LeadingWideSpacer && column != columns - 1)
        {
            return None;
        }
        let hidden = cell.style.hidden;
        let nonprinting = cell.scalar.is_control()
            || matches!(cell.scalar,
            '\u{061c}'|'\u{200b}'..='\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2060}'..='\u{206f}'|'\u{feff}');
        if cell.role == Role::LeadingWideSpacer {
            output.push(' ');
        } else if cell.role != Role::WideSpacer {
            if hidden || nonprinting {
                output.push(' ');
                if cell.role == Role::WideLead {
                    output.push(' ');
                }
            } else {
                output.push(cell.scalar);
            }
        }
        if column == columns - 1 && i + 1 < cells.len() {
            output.push('\n');
        }
    }
    Some(output)
}
