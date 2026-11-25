//! Original Android enrollment owner; JNI calls only enqueue, read or cancel.
//! The worker owns DNS, authenticated WSS, hardware worker and the durable write.
use crate::{
    ObserverHandle,
    enrollment::{DurablyAcceptedEnrollment, HostCommitment},
    keystore::{AndroidDurableStore, open_roles},
    phone_enrollment::{ExplicitPairingRequest, HardwarePairingIdentity, redeem},
    start_android_observer,
};
use cc_desk_remote_record::appbind::CanonicalOrigin;
use cc_desk_remote_security::{ClientGrantState, GrantError, P256Spki, Role};
use jni::{
    JNIEnv, JavaVM,
    objects::GlobalRef,
};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

pub(crate) struct Begin {
    pub origin: CanonicalOrigin,
    pub host: HostCommitment,
    pub code: zeroize::Zeroizing<String>,
    pub local: [u8; 16],
    pub endpoint: [u8; 91],
    pub relay: [u8; 91],
    pub approval: [u8; 91],
}

enum Phase {
    Pending,
    Paired {
        host_id: String,
        exact: Option<Box<DurablyAcceptedEnrollment>>,
        addr: SocketAddr,
        facade: GlobalRef,
    },
    Failed,
    Cancelled,
}

struct Attempt {
    id: i64,
    stop: watch::Sender<bool>,
    phase: Arc<Mutex<Phase>>,
    worker: JoinHandle<bool>,
}

struct Registry {
    sequence: i64,
    faulty: bool,
    current: Option<Attempt>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    sequence: 0,
    faulty: false,
    current: None,
});

pub(crate) fn begin(vm: JavaVM, facade: GlobalRef, input: Begin) -> Option<i64> {
    if input.local == [0; 16]
        || P256Spki::new(&input.endpoint).is_err()
        || P256Spki::new(&input.relay).is_err()
        || P256Spki::new(&input.approval).is_err()
        || input.endpoint == input.relay
        || input.endpoint == input.approval
        || input.approval == input.relay
    {
        return None;
    }
    let mut r = REGISTRY.lock().ok()?;
    if r.faulty || r.current.as_ref().is_some_and(|x| !x.worker.is_finished()) {
        return None;
    }
    if let Some(old) = r.current.take() {
        r.faulty |= !matches!(old.worker.join(), Ok(true));
    }
    if r.faulty {
        return None;
    }
    let id = r.sequence.checked_add(1)?;
    let phase = Arc::new(Mutex::new(Phase::Pending));
    let owned = phase.clone();
    let (stop, rx) = watch::channel(false);
    let worker = thread::Builder::new()
        .name("ccdesk-enrollment".into())
        .spawn(move || run(vm, facade, input, owned, rx))
        .ok()?;
    r.sequence = id;
    r.current = Some(Attempt {
        id,
        stop,
        phase,
        worker,
    });
    Some(id)
}

fn original(id: i64) -> Option<(Arc<Mutex<Phase>>, watch::Sender<bool>)> {
    let r = REGISTRY.lock().ok()?;
    let a = r.current.as_ref().filter(|a| a.id == id)?;
    Some((a.phase.clone(), a.stop.clone()))
}

pub(crate) fn state(id: i64) -> i32 {
    let Some((p, _)) = original(id) else { return 0 };
    p.lock()
        .map(|phase| match &*phase {
            Phase::Pending => 1,
            Phase::Paired { .. } => 2,
            Phase::Failed => 3,
            Phase::Cancelled => 0,
        })
        .unwrap_or(0)
}
pub(crate) fn host_id(id: i64) -> Option<String> {
    let (p, _) = original(id)?;
    let phase = p.lock().ok()?;
    match &*phase {
        Phase::Paired { host_id, .. } => Some(host_id.clone()),
        _ => None,
    }
}
pub(crate) fn cancel(id: i64) {
    if let Some((p, stop)) = original(id) {
        stop.send_replace(true);
        if let Ok(mut phase) = p.lock() {
            *phase = Phase::Cancelled;
        }
    }
}
pub(crate) fn start_view(env: &JNIEnv<'_>, id: i64) -> Option<ObserverHandle> {
    let (p, stop) = original(id)?;
    let mut phase = p.lock().ok()?;
    if *stop.borrow() {
        return None;
    }
    let Phase::Paired {
        exact,
        addr,
        facade,
        ..
    } = &mut *phase
    else {
        return None;
    };
    let approval = *exact.take()?;
    // Existing native grant clock receives a local monotonic value; no wall
    // clock is inferred from Relay metadata.
    let started = Instant::now();
    let clock = Arc::new(move || {
        u64::try_from(started.elapsed().as_millis()).map_err(|_| GrantError::Clock)
    });
    start_android_observer(
        env,
        facade.as_obj(),
        approval,
        *addr,
        ClientGrantState::new(clock),
    )
    .ok()
}

fn wall_ms() -> Option<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}

fn attempt(
    vm: &JavaVM,
    facade: &GlobalRef,
    mut input: Begin,
    stop: watch::Receiver<bool>,
) -> (Option<Phase>, bool) {
    let result = || -> Option<(SocketAddr, ExplicitPairingRequest, [u8; 16])> {
        let url = url::Url::parse(input.origin.as_str()).ok()?;
        let host = url.host_str()?;
        let address = (host, url.port().unwrap_or(443))
            .to_socket_addrs()
            .ok()?
            .next()?;
        if *stop.borrow() {
            return None;
        }
        let mut relay_id = [0; 16];
        SystemRandom::new().fill(&mut relay_id).ok()?;
        if relay_id == [0; 16] {
            return None;
        }
        let code = std::mem::take(&mut *input.code);
        let request =
            ExplicitPairingRequest::new(input.origin.clone(), address, input.host, code).ok()?;
        Some((address, request, relay_id))
    };
    let Some((addr, request, relay_id)) = result() else {
        return (None, true);
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return (None, true);
    };
    let Ok(env) = vm.attach_current_thread() else {
        return (None, true);
    };
    let hardware_vm = match env.get_java_vm() {
        Ok(x) => x,
        Err(_) => return (None, true),
    };
    drop(env);
    let roles = match open_roles(
        hardware_vm,
        facade.clone(),
        input.endpoint,
        input.origin,
        relay_id,
    ) {
        Ok(r) => r,
        Err(_) => return (None, true),
    };
    let identity = HardwarePairingIdentity {
        relay: roles.relay_for_enrollment(relay_id),
        relay_sec1: input.relay[26..].try_into().expect("fixed SPKI size"),
        endpoint_spki: input.endpoint,
        approval_spki: input.approval,
    };
    let received = runtime.block_on(redeem(request, identity, stop.clone(), |bytes| {
        roles.sign_enrollment(bytes)
    }));
    let (keys, hardware) = roles.into_parts();
    drop(keys);
    if !hardware.retire() {
        return (None, false);
    }
    let Ok(receipt) = received else {
        return (None, true);
    };
    if *stop.borrow() {
        return (None, true);
    }
    let durable = (|| {
        let mut env = vm.attach_current_thread().ok()?;
        let id = receipt.host_endpoint().endpoint_id(Role::Host);
        let host_id = id.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let exact = receipt
            .persist_once(
                input.local,
                &mut AndroidDurableStore::new(&mut env, facade.as_obj()),
                wall_ms()?,
            )
            .ok()?;
        Some(Phase::Paired {
            host_id,
            exact: Some(Box::new(exact)),
            addr,
            facade: facade.clone(),
        })
    })();
    (durable, true)
}

fn run(
    vm: JavaVM,
    facade: GlobalRef,
    input: Begin,
    state: Arc<Mutex<Phase>>,
    stop: watch::Receiver<bool>,
) -> bool {
    let (result, retired) = attempt(&vm, &facade, input, stop.clone());
    if let Ok(mut p) = state.lock() {
        *p = if *stop.borrow() {
            Phase::Cancelled
        } else {
            result.unwrap_or(Phase::Failed)
        };
    }
    retired
}
