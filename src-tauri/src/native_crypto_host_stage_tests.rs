#![allow(non_snake_case)]
use super::*;
use std::task::{RawWaker, RawWakerVTable};

#[tokio::test]
async fn HostStages_RegistrationHeldEndpointHonorsSetupEnd_010() {
    let mut f = Fixture::new();
    let scope =
        crate::remote_host_approval::RegistrationSigningScope::test_new(f.token.clone(), f.cap);
    let end = Instant::now() + Duration::from_secs(1);
    f.set().stages.begin_registration(scope, end).unwrap();
    let relay = f
        .set()
        .stages
        .test_relay_future(b"PUBLIC_REGISTRATION_RELAY".to_vec())
        .unwrap();
    assert!(relay.await.is_ok());
    f.set().stages.arm_endpoint().unwrap();
    let (claim, in_claim, release_claim) = gate();
    f.set()
        .stages
        .test_on_mint(Box::new(move |attempt| attempt.hook_claim(claim)));
    let signer = f
        .set()
        .endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .unwrap();
    let signing = std::thread::spawn(move || signer.sign(b"PUBLIC_REGISTRATION_ENDPOINT"));
    in_claim.recv_timeout(Duration::from_secs(3)).unwrap();
    let actual = f
        .set()
        .stages
        .test_attempt(SigningPurpose::Endpoint)
        .unwrap()
        .end();
    assert!(actual <= end);
    tokio::time::sleep_until(end.into()).await;
    release_claim.open();
    assert!(signing.join().unwrap().is_err());
    f.finish();
}

struct Fixture {
    set: Option<HostSignerSet>,
    operation: Arc<ActivationOperation>,
    token: Arc<NativeRevocation>,
    cap: Instant,
    calls: Arc<[AtomicUsize; 2]>,
}
impl Fixture {
    fn new() -> Self {
        let cap = Instant::now() + Duration::from_secs(30);
        let token = NativeRevocation::new();
        let mut reservation = ActivationOperation::test_reservation();
        let operation = reservation.operation();
        let calls = counters();
        let set = reservation
            .prepare_keys(token.clone(), cap)
            .unwrap()
            .prepare(supplier(
                frozen_binding(cap, "wss://relay.invalid/"),
                calls.clone(),
            ))
            .unwrap();
        reservation.spawn(|_| true).unwrap();
        Self {
            set: Some(set),
            operation,
            token,
            cap,
            calls,
        }
    }
    fn set(&mut self) -> &mut HostSignerSet {
        self.set.as_mut().unwrap()
    }
    fn finish(mut self) {
        drop(self.set.take());
        assert!(!drain(&self.operation).unknown);
        assert!(self.calls.iter().all(|c| c.load(Ordering::Acquire) <= 1));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.token.revoke();
        drop(self.set.take());
        let _ = drain(&self.operation);
    }
}

#[test]
fn HostStages_ProbesAndConcurrentClonesConsumeOnce_001() {
    let mut f = Fixture::new();
    let mut signers = Vec::new();
    for _ in 0..8 {
        signers.push(
            f.set()
                .endpoint
                .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
                .unwrap(),
        );
    }
    assert_eq!(f.set().stages.test_mints(), [0, 0]);
    f.set()
        .stages
        .test_endpoint_only(Instant::now() + Duration::from_secs(5))
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(signers.len()));
    let threads: Vec<_> = signers
        .into_iter()
        .map(|signer| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                signer.sign(b"PUBLIC_CONCURRENT_STAGE").is_ok()
            })
        })
        .collect();
    let succeeded = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(succeeded, 1);
    assert_eq!(f.set().stages.test_mints(), [1, 0]);
    assert!(!f.token.is_revoked());
    assert!(f
        .set()
        .endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .is_none());
    assert!(
        f.set()
            .stages
            .test_attempt(SigningPurpose::Endpoint)
            .unwrap()
            .end()
            <= f.cap
    );
    f.finish();
}

#[test]
fn HostStages_UnpolledRelayDropRevokesWithoutMint_002() {
    let mut f = Fixture::new();
    f.set()
        .stages
        .test_begin_relay(Instant::now() + Duration::from_secs(8))
        .unwrap();
    let future = f
        .set()
        .stages
        .test_relay_future(b"PUBLIC_UNPOLLED".to_vec())
        .unwrap();
    assert!(f
        .set()
        .stages
        .test_relay_future(b"SECOND".to_vec())
        .is_err());
    assert_eq!(f.set().stages.test_mints(), [0, 0]);
    drop(future);
    assert!(f.token.is_revoked());
    assert!(f.set().stages.test_relay_future(b"RETRY".to_vec()).is_err());
    f.finish();
}

fn clone_panic_waker() -> Waker {
    unsafe fn clone(_: *const ()) -> RawWaker {
        panic!("TEST_ONLY_RAW_WAKER_CLONE");
    }
    unsafe fn noop(_: *const ()) {}
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // 不携带指针或引用；唯一效果是显式 clone panic，没有资源被遗失。
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}
#[test]
fn HostStages_QueuedDropAndPollPanicRevokeBeforeBlockingWake_003() {
    for panic_poll in [false, true] {
        let mut f = Fixture::new();
        f.set()
            .stages
            .test_begin_relay(Instant::now() + Duration::from_secs(8))
            .unwrap();
        let (claim, in_claim, release_claim) = gate();
        f.set()
            .stages
            .test_on_mint(Box::new(move |attempt| attempt.hook_claim(claim)));
        let mut future = f
            .set()
            .stages
            .test_relay_future(b"PUBLIC_QUEUED_STAGE".to_vec())
            .unwrap();
        let (wake, in_wake, release_wake) = gate();
        let revoked_at_wake = Arc::new(AtomicBool::new(false));
        let observed = revoked_at_wake.clone();
        let token = f.token.clone();
        let waker = Waker::from(Arc::new(BlockingWake(Mutex::new(Some(Box::new(
            move || {
                observed.store(token.is_revoked(), Ordering::Release);
                wake();
            },
        ))))));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        {
            let _entered = runtime.enter();
            assert!(future
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
        }
        in_claim.recv_timeout(Duration::from_secs(3)).unwrap();
        let panic_revoke = if panic_poll {
            let panic_waker = clone_panic_waker();
            let _entered = runtime.enter();
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = future.as_mut().poll(&mut Context::from_waker(&panic_waker));
            }))
            .is_err());
            f.token.is_revoked()
        } else {
            true
        };
        let dropping = std::thread::spawn(move || drop(future));
        in_wake.recv_timeout(Duration::from_secs(3)).unwrap();
        let before_cleanup = revoked_at_wake.load(Ordering::Acquire) && f.token.is_revoked();
        release_claim.open();
        let end = Instant::now() + Duration::from_secs(2);
        let sibling_joined = loop {
            f.operation.advance_join();
            let fact = f.operation.test_key_observer().snapshot();
            if fact.keys[0].is_some_and(|k| !k.holds_original_join) {
                break true;
            }
            if Instant::now() >= end {
                break false;
            }
            std::thread::yield_now();
        };
        release_wake.open();
        dropping.join().unwrap();
        eprintln!("STAGE_QUEUED panic={panic_poll} revoke_before_drop={panic_revoke} revoke_before_wake={before_cleanup} sibling_joined={sibling_joined}");
        assert!(panic_revoke && before_cleanup && sibling_joined);
        assert_eq!(f.set().stages.test_mints(), [0, 1]);
        assert!(f.set().stages.test_relay_future(b"RETRY".to_vec()).is_err());
        f.finish();
    }
}

#[tokio::test]
async fn HostStages_BoundsAndOriginalSetupExpiryNeverRefresh_004() {
    for expired in [false, true] {
        let mut f = Fixture::new();
        let end = Instant::now()
            + if expired {
                Duration::from_millis(20)
            } else {
                Duration::from_secs(8)
            };
        f.set().stages.test_begin_relay(end).unwrap();
        if expired {
            tokio::time::sleep_until(end.into()).await;
        }
        let future = f.set().stages.test_relay_future(vec![1; 100_000]);
        if let Ok(future) = future {
            assert!(future.await.is_err());
        }
        assert!(f.token.is_revoked());
        assert!(f.set().stages.test_relay_future(b"RETRY".to_vec()).is_err());
        assert_eq!(f.set().stages.test_mints(), [0, usize::from(!expired)]);
        f.finish();
    }
}

#[test]
fn HostStages_ScopeIdentityAndCapCannotBeSubstituted_005() {
    for foreign in [false, true] {
        let mut f = Fixture::new();
        let other = NativeRevocation::new();
        let scope = crate::remote_host_network::NetworkSigningScope::test_new(
            if foreign {
                other.clone()
            } else {
                f.token.clone()
            },
            if foreign {
                f.cap
            } else {
                f.cap + Duration::from_secs(1)
            },
            Instant::now() + Duration::from_secs(8),
        );
        assert!(f.set().stages.begin_relay(scope).is_err());
        assert!(f.token.is_revoked() && !other.is_revoked());
        assert_eq!(f.set().stages.test_mints(), [0, 0]);
        f.finish();
    }
}

#[tokio::test]
async fn HostStages_BusyAndQueuedTimeoutConsumeWithoutRetry_006() {
    for timeout in [false, true] {
        let mut f = Fixture::new();
        f.set()
            .stages
            .test_begin_relay(Instant::now() + Duration::from_secs(8))
            .unwrap();
        let mut occupied = None;
        let (claim, in_claim, release_claim) = gate();
        if timeout {
            f.set()
                .stages
                .test_on_mint(Box::new(move |attempt| attempt.hook_claim(claim)));
        } else {
            occupied = Some(f.set().stages.test_queue_relay().unwrap());
        }
        let mut future = f
            .set()
            .stages
            .test_relay_future(b"PUBLIC_FAILURE_STAGE".to_vec())
            .unwrap();
        if timeout {
            let waker = Waker::from(Arc::new(BlockingWake(Mutex::new(None))));
            assert!(future
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
            in_claim.recv_timeout(Duration::from_secs(3)).unwrap();
            let end = f
                .set()
                .stages
                .test_attempt(SigningPurpose::Relay)
                .unwrap()
                .end();
            assert!(end <= f.cap);
            tokio::time::sleep_until(end.into()).await;
        }
        assert!(future.await.is_err());
        let revoked_before_cleanup = f.token.is_revoked();
        assert!(f.set().stages.test_relay_future(b"RETRY".to_vec()).is_err());
        assert_eq!(f.set().stages.test_mints(), [0, usize::from(timeout)]);
        release_claim.open();
        drop(occupied);
        assert!(revoked_before_cleanup);
        f.finish();
    }
}

struct PollThreadWake {
    token: Arc<NativeRevocation>,
    target: Mutex<Option<std::thread::ThreadId>>,
    effect: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl Wake for PollThreadWake {
    fn wake(self: Arc<Self>) {
        // timer 的普通唤醒不阻塞；检测原 poll/worker 的通知，或先撤销后由 custody 接管的通知。
        let target = *self.target.lock();
        if target == Some(std::thread::current().id())
            || (target.is_some() && self.token.is_revoked())
        {
            let effect = self.effect.lock().take();
            if let Some(effect) = effect {
                effect();
            }
        }
    }
}
#[tokio::test]
async fn HostStages_QueuedExpiryRevokesBeforeSynchronousWake_007() {
    let mut f = Fixture::new();
    f.set()
        .stages
        .test_begin_relay(Instant::now() + Duration::from_secs(8))
        .unwrap();
    let (claim, in_claim, release_claim) = gate();
    f.set()
        .stages
        .test_on_mint(Box::new(move |attempt| attempt.hook_claim(claim)));
    let mut future = f
        .set()
        .stages
        .test_relay_future(b"PUBLIC_QUEUED_EXPIRY".to_vec())
        .unwrap();
    let (wake, in_wake, release_wake) = gate();
    let revoked_at_wake = Arc::new(AtomicBool::new(false));
    let observed = revoked_at_wake.clone();
    let token = f.token.clone();
    let notification = Arc::new(PollThreadWake {
        token: f.token.clone(),
        target: Mutex::new(None),
        effect: Mutex::new(Some(Box::new(move || {
            observed.store(token.is_revoked(), Ordering::Release);
            wake();
        }))),
    });
    let waker = Waker::from(notification.clone());
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    in_claim.recv_timeout(Duration::from_secs(3)).unwrap();
    let end = f
        .set()
        .stages
        .test_attempt(SigningPurpose::Relay)
        .unwrap()
        .end();
    tokio::time::sleep_until(end.into()).await;
    let runtime = tokio::runtime::Handle::current();
    let poller = std::thread::spawn(move || {
        let _entered = runtime.enter();
        *notification.target.lock() = Some(std::thread::current().id());
        let result = future.as_mut().poll(&mut Context::from_waker(&waker));
        drop(future);
        matches!(result, std::task::Poll::Ready(Err(_)))
    });
    in_wake.recv_timeout(Duration::from_secs(3)).unwrap();
    let before_cleanup = revoked_at_wake.load(Ordering::Acquire) && f.token.is_revoked();
    release_claim.open();
    release_wake.open();
    assert!(poller.join().unwrap());
    eprintln!("STAGE_QUEUED_EXPIRY revoke_before_blocking_cancel_wake={before_cleanup}");
    f.finish();
    assert!(
        before_cleanup,
        "queued expiry notified before revoking original Host token"
    );
}

#[tokio::test]
async fn HostStages_WorkerExpiryRevokesBeforeObserverNotification_008() {
    let mut f = Fixture::new();
    f.set()
        .stages
        .test_begin_relay(Instant::now() + Duration::from_secs(8))
        .unwrap();
    let (claim, in_claim, release_claim) = gate();
    let (wake, in_wake, release_wake) = gate();
    let revoked_at_wake = Arc::new(AtomicBool::new(false));
    let observed = revoked_at_wake.clone();
    let token = f.token.clone();
    let notification = Arc::new(PollThreadWake {
        token: f.token.clone(),
        target: Mutex::new(None),
        effect: Mutex::new(Some(Box::new(move || {
            observed.store(token.is_revoked(), Ordering::Release);
            wake();
        }))),
    });
    let worker = notification.clone();
    f.set().stages.test_on_mint(Box::new(move |attempt| {
        attempt.hook_claim(Box::new(move || {
            *worker.target.lock() = Some(std::thread::current().id());
            claim();
        }))
    }));
    let mut future = f
        .set()
        .stages
        .test_relay_future(b"PUBLIC_WORKER_EXPIRY".to_vec())
        .unwrap();
    let waker = Waker::from(notification);
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    in_claim.recv_timeout(Duration::from_secs(3)).unwrap();
    let end = f
        .set()
        .stages
        .test_attempt(SigningPurpose::Relay)
        .unwrap()
        .end();
    tokio::time::sleep_until(end.into()).await;
    release_claim.open();
    in_wake.recv_timeout(Duration::from_secs(3)).unwrap();
    let before_observer = revoked_at_wake.load(Ordering::Acquire) && f.token.is_revoked();
    release_wake.open();
    assert!(future.await.is_err());
    eprintln!("STAGE_WORKER_EXPIRY revoke_before_observer={before_observer}");
    f.finish();
    assert!(
        before_observer,
        "worker expiry notified before revoking original Host token"
    );
}

struct DropProbe {
    effect: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    armed: AtomicBool,
}
fn drop_probe_waker(probe: Arc<DropProbe>) -> Waker {
    unsafe fn clone(p: *const ()) -> RawWaker {
        Arc::increment_strong_count(p as *const DropProbe);
        RawWaker::new(p, &VTABLE)
    }
    unsafe fn wake(p: *const ()) {
        drop(Arc::from_raw(p as *const DropProbe));
    }
    unsafe fn wake_ref(_: *const ()) {}
    unsafe fn dispose(p: *const ()) {
        let probe = Arc::from_raw(p as *const DropProbe);
        if probe.armed.load(Ordering::Acquire) {
            let effect = probe.effect.lock().take();
            if let Some(effect) = effect {
                effect();
            }
        }
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_ref, dispose);
    // 各原始指针精确对应一个 Arc 引用；clone 增加，wake/drop 各消费一次。
    unsafe { Waker::from_raw(RawWaker::new(Arc::into_raw(probe) as *const (), &VTABLE)) }
}
#[tokio::test]
async fn HostStages_DeliveryExpiryRevokesBeforeWakerDrop_009() {
    let mut f = Fixture::new();
    f.set()
        .stages
        .test_begin_relay(Instant::now() + Duration::from_secs(8))
        .unwrap();
    let (blocking_drop, in_drop, release_drop) = gate();
    let revoked_at_drop = Arc::new(AtomicBool::new(false));
    let observed = revoked_at_drop.clone();
    let token = f.token.clone();
    let probe = Arc::new(DropProbe {
        armed: AtomicBool::new(false),
        effect: Mutex::new(Some(Box::new(move || {
            observed.store(token.is_revoked(), Ordering::Release);
            blocking_drop();
        }))),
    });
    let armed = probe.clone();
    let end = Arc::new(Mutex::new(None));
    let recorded = end.clone();
    f.set().stages.test_on_mint(Box::new(move |attempt| {
        *recorded.lock() = Some(attempt.end())
    }));
    f.set().stages.test_on_ticket(Box::new(move |ticket| {
        ticket.hook_delivery(Box::new(move || {
            let end = end.lock().unwrap();
            if let Some(left) = end.checked_duration_since(Instant::now()) {
                std::thread::sleep(left);
            }
            armed.armed.store(true, Ordering::Release);
        }))
    }));
    let mut future = f
        .set()
        .stages
        .test_relay_future(b"PUBLIC_DELIVERY_EXPIRY".to_vec())
        .unwrap();
    let waker = drop_probe_waker(probe);
    let runtime = tokio::runtime::Handle::current();
    let poller = std::thread::spawn(move || {
        let _entered = runtime.enter();
        loop {
            match future.as_mut().poll(&mut Context::from_waker(&waker)) {
                std::task::Poll::Ready(result) => break result.is_err(),
                std::task::Poll::Pending => std::thread::yield_now(),
            }
        }
    });
    in_drop.recv_timeout(Duration::from_secs(7)).unwrap();
    let before_cleanup = revoked_at_drop.load(Ordering::Acquire) && f.token.is_revoked();
    release_drop.open();
    assert!(poller.join().unwrap());
    eprintln!("STAGE_DELIVERY_EXPIRY revoke_before_waker_drop={before_cleanup}");
    f.finish();
    assert!(
        before_cleanup,
        "delivery failure dropped notification before Host revoke"
    );
}

#[tokio::test]
async fn HostStages_ExpiredPollRevokesBeforeReplacingOldWaker_010() {
    let mut f = Fixture::new();
    f.set()
        .stages
        .test_begin_relay(Instant::now() + Duration::from_secs(8))
        .unwrap();
    let (claim, in_claim, release_claim) = gate();
    f.set()
        .stages
        .test_on_mint(Box::new(move |attempt| attempt.hook_claim(claim)));
    let (blocking_drop, in_drop, release_drop) = gate();
    let revoked_at_drop = Arc::new(AtomicBool::new(false));
    let observed = revoked_at_drop.clone();
    let token = f.token.clone();
    let probe = Arc::new(DropProbe {
        armed: AtomicBool::new(false),
        effect: Mutex::new(Some(Box::new(move || {
            observed.store(token.is_revoked(), Ordering::Release);
            blocking_drop();
        }))),
    });
    let waker = drop_probe_waker(probe.clone());
    let mut future = f
        .set()
        .stages
        .test_relay_future(b"PUBLIC_POLL_ENTRY_EXPIRY".to_vec())
        .unwrap();
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    in_claim.recv_timeout(Duration::from_secs(3)).unwrap();
    let end = f
        .set()
        .stages
        .test_attempt(SigningPurpose::Relay)
        .unwrap()
        .end();
    tokio::time::sleep_until(end.into()).await;
    probe.armed.store(true, Ordering::Release);
    let runtime = tokio::runtime::Handle::current();
    let poller = std::thread::spawn(move || {
        let _entered = runtime.enter();
        matches!(
            future.as_mut().poll(&mut Context::from_waker(&waker)),
            std::task::Poll::Ready(Err(_))
        )
    });
    in_drop.recv_timeout(Duration::from_secs(3)).unwrap();
    let before_cleanup = revoked_at_drop.load(Ordering::Acquire) && f.token.is_revoked();
    release_claim.open();
    release_drop.open();
    assert!(poller.join().unwrap());
    eprintln!("STAGE_POLL_ENTRY_EXPIRY revoke_before_old_waker_drop={before_cleanup}");
    f.finish();
    assert!(before_cleanup);
}
#[tokio::test]
async fn HostStages_StaleIdDoesNotRevokeNewOriginalRequest_011() {
    let mut f = Fixture::new();
    let mut first = f.set().stages.test_raw_relay_attempt().unwrap();
    (&mut first).await.unwrap();
    let second = f.set().stages.test_raw_relay_attempt().unwrap();
    assert_eq!(first.cancel(), RequestFact::Delivered);
    assert!(!f.token.is_revoked());
    second.await.unwrap();
    assert!(!f.token.is_revoked());
    f.finish();
}
