use super::super::owner::*;
use super::*;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, RawWaker, RawWakerVTable, Waker};
use std::time::{Duration, Instant};

struct Gate(Arc<(Mutex<(bool, bool)>, Condvar)>);
struct WakeProbe {
    handle: KeyRequestHandle,
    called: AtomicBool,
    unlocked: AtomicBool,
}
impl WakeProbe {
    fn check(&self) {
        self.unlocked
            .fetch_and(self.handle.control_available(), Ordering::AcqRel);
    }
    fn wake(&self) {
        self.check();
        self.called.store(true, Ordering::Release);
    }
}
// Each raw token owns exactly one Arc. These callbacks only probe try_lock;
// detecting re-entry cannot block the worker or test cleanup.
unsafe fn probe_clone(data: *const ()) -> RawWaker {
    let probe = std::mem::ManuallyDrop::new(unsafe { Arc::<WakeProbe>::from_raw(data.cast()) });
    probe.check();
    raw_probe(Arc::clone(&probe))
}
unsafe fn probe_wake(data: *const ()) {
    let probe = unsafe { Arc::<WakeProbe>::from_raw(data.cast()) };
    probe.wake();
}
unsafe fn probe_wake_ref(data: *const ()) {
    let probe = std::mem::ManuallyDrop::new(unsafe { Arc::<WakeProbe>::from_raw(data.cast()) });
    probe.wake();
}
unsafe fn probe_drop(data: *const ()) {
    let probe = unsafe { Arc::<WakeProbe>::from_raw(data.cast()) };
    probe.check();
}
static PROBE_VTABLE: RawWakerVTable =
    RawWakerVTable::new(probe_clone, probe_wake, probe_wake_ref, probe_drop);
fn raw_probe(probe: Arc<WakeProbe>) -> RawWaker {
    RawWaker::new(Arc::into_raw(probe).cast(), &PROBE_VTABLE)
}
impl Gate {
    fn new() -> Self {
        Self(Arc::new((Mutex::new((false, false)), Condvar::new())))
    }
    fn blocker(&self) -> Box<dyn FnOnce() + Send> {
        let gate = self.0.clone();
        Box::new(move || {
            let mut state = gate.0.lock().unwrap();
            state.0 = true;
            gate.1.notify_all();
            while !state.1 {
                state = gate.1.wait(state).unwrap();
            }
        })
    }
    fn entered(&self) -> bool {
        let state = self.0 .0.lock().unwrap();
        self.0
             .1
            .wait_timeout_while(state, Duration::from_secs(3), |s| !s.0)
            .unwrap()
            .0
             .0
    }
    fn release(&self) {
        self.0 .0.lock().unwrap().1 = true;
        self.0 .1.notify_all();
    }
}
#[test]
#[allow(non_snake_case)]
fn CngOwner_QueuedCancelSignsZeroAndJoinsOriginal_001() {
    let v = vector();
    let point = hex(v["public_key_sec1"].as_str().unwrap());
    let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(&point);
    let gate = Gate::new();
    let (mut owner, handle) = KeyCustodian::start_for_test(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
        WorkerHooks {
            before_claim: Some(gate.blocker()),
            after_native: None,
        },
    )
    .unwrap();
    let ticket = handle
        .begin(b"PUBLIC_QUEUED_CANCEL".to_vec(), SignatureEncoding::Fixed)
        .unwrap();
    let entered = gate.entered();
    let cancelled = ticket.cancel();
    drop(ticket);
    let pending = owner.observe();
    let rejected = handle
        .begin(b"NO_REUSE".to_vec(), SignatureEncoding::Fixed)
        .is_err();
    gate.release();
    owner.shutdown();
    let deadline = Instant::now() + Duration::from_secs(5);
    let retired = loop {
        if owner.observe() == KeyRetirement::Retired {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // On a genuine native block, custodian Drop retains the original join in
    // blocking cleanup; this bounded assertion never manufactures retirement.
    assert!(
        retired,
        "original custodian still Pending/Unknown after observation budget"
    );
    assert!(entered);
    assert_eq!(cancelled, RequestFact::CancelledNoEffect);
    assert_eq!(pending, KeyRetirement::Pending);
    assert!(rejected);
    assert_eq!(owner.native_sign_count(), 0);
}

fn retire(owner: &mut KeyCustodian) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match owner.observe() {
            KeyRetirement::Retired => return true,
            KeyRetirement::Unknown => return false,
            KeyRetirement::Pending if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => return false,
        }
    }
}

#[test]
#[allow(non_snake_case)]
fn CngOwner_StartedCancelWithholdsLateResultAndRetiresOriginal_002() {
    let point = hex(vector()["public_key_sec1"].as_str().unwrap());
    let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(&point);
    let gate = Gate::new();
    let (mut owner, handle) = KeyCustodian::start_for_test(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
        WorkerHooks {
            before_claim: None,
            after_native: Some(gate.blocker()),
        },
    )
    .unwrap();
    let ticket = handle
        .begin(b"PUBLIC_STARTED_CANCEL".to_vec(), SignatureEncoding::Fixed)
        .unwrap();
    let entered = gate.entered();
    let phase = owner.request_fact();
    let cancelled = ticket.cancel();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let result = runtime.block_on(ticket);
    drop(handle); // The request facade owns no native handle or original join.
    let retained = owner.holds_original_join();
    let pending = owner.observe();
    gate.release(); // Always release the deterministic gate BEFORE assertions.
    let retired = retire(&mut owner);
    assert!(retired);
    assert!(entered);
    assert_eq!(phase, RequestFact::Started);
    assert_eq!(cancelled, RequestFact::Unknown);
    assert_eq!(result, Err(SignFailure::Unknown));
    assert!(retained);
    assert_eq!(pending, KeyRetirement::Pending);
    assert_eq!(owner.request_fact(), RequestFact::Unknown);
    assert_eq!(owner.native_sign_count(), 1);
    assert!(owner.late_discarded());
    assert!(owner.key_freed());
    assert!(!owner.holds_original_join());
}

#[test]
#[allow(non_snake_case)]
fn CngOwner_AsyncPollIsPendingAndDeliveredTicketCannotCancelNext_003() {
    let point = hex(vector()["public_key_sec1"].as_str().unwrap());
    let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(&point);
    let original_point = point.clone();
    let gate = Gate::new();
    let (mut owner, handle) = KeyCustodian::start_for_test(
        move || import_public_rfc6979_fixture(&original_point),
        SigningPurpose::Endpoint,
        spki.clone(),
        WorkerHooks {
            before_claim: Some(gate.blocker()),
            after_native: None,
        },
    )
    .unwrap();
    // Begin is deliberately outside a Tokio runtime.
    let mut first = handle
        .begin(b"sample".to_vec(), SignatureEncoding::Fixed)
        .unwrap();
    let entered = gate.entered();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let probe = Arc::new(WakeProbe {
        handle: handle.clone(),
        called: AtomicBool::new(false),
        unlocked: AtomicBool::new(true),
    });
    // SAFETY: each vtable token owns one Arc and all callbacks are thread-safe.
    let waker = unsafe { Waker::from_raw(raw_probe(probe.clone())) };
    let was_pending = {
        let _runtime = runtime.enter();
        std::pin::Pin::new(&mut first)
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    };
    let independent = runtime.block_on(async { tokio::spawn(async { 17 }).await.unwrap() });
    gate.release();
    let end = Instant::now() + Duration::from_secs(3);
    while !probe.called.load(Ordering::Acquire) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    let wake_unlocked =
        probe.called.load(Ordering::Acquire) && probe.unlocked.load(Ordering::Acquire);
    let fixed = runtime.block_on(&mut first);
    let mut second = handle
        .begin(b"sample".to_vec(), SignatureEncoding::Der)
        .unwrap();
    let stale_cancel = first.cancel();
    drop(first); // Ready already marked this ticket delivered.
    let der = runtime.block_on(&mut second);
    drop(second);
    owner.shutdown();
    let retired = retire(&mut owner);
    assert!(retired);
    assert!(
        entered && was_pending && wake_unlocked,
        "pending/wake lock facts: entered={entered} pending={was_pending} unlocked={wake_unlocked}"
    );
    assert_eq!(independent, 17);
    assert_eq!(stale_cancel, RequestFact::Delivered);
    let fixed = fixed.unwrap();
    let der = der.unwrap();
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, &point)
        .verify(b"sample", &fixed)
        .unwrap();
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_ASN1, &point)
        .verify(b"sample", &der)
        .unwrap();
    assert_eq!(owner.native_sign_count(), 2);
    assert!(owner.key_freed());
    assert!(!owner.holds_original_join());

    // Exercise the hook-free constructor with the same public fixture only.
    let (mut owner, handle) = KeyCustodian::start(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
    )
    .unwrap();
    let mut ticket = handle
        .begin_with_budget(
            b"sample".to_vec(),
            SignatureEncoding::Fixed,
            Duration::from_millis(500),
        )
        .unwrap();
    ticket.expire_at_delivery();
    let expired = runtime.block_on(ticket);
    drop(handle);
    let retired = retire(&mut owner);
    assert!(retired && owner.key_freed());
    assert_eq!(expired, Err(SignFailure::Unknown));
    assert_eq!(owner.request_fact(), RequestFact::Unknown);
}
