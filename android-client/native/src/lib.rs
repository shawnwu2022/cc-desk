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
mod jni_bridge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserverHandle(i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    UnapprovedProfile,
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
pub fn start_verified_observer(profile: NativeProfile) -> Result<ObserverHandle, StartError> {
    if !matches!(profile.grant.role, ClientRole::Observer)
        || !matches!(profile.relay_mode, RelayMode::RendezvousV1)
        || !matches!(profile.relay.binding(), (key, RelayRole::Phone) if key != [0; 16])
        || profile.tunnel == [0; 16]
        || profile.address.port() == 0
    {
        return Err(StartError::UnapprovedProfile);
    }
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
        .spawn(move || runtime.block_on(observe(profile, owned, rx)))
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
pub fn take_authenticated_snapshot(token: i64) -> Option<Vec<Vec<u8>>> {
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
