#![allow(non_snake_case)]
use super::*;
use crate::native_crypto::host::*;
use crate::native_crypto::owner::*;
use crate::remote_host::activation::ActivationOperation;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::{
    future::Future,
    sync::{mpsc, Arc},
    task::{Context, Wake, Waker},
    time::{Duration, Instant},
};

struct Gate(Option<mpsc::SyncSender<()>>);
impl Gate {
    fn open(mut self) {
        if let Some(send) = self.0.take() {
            let _ = send.send(());
        }
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        if let Some(send) = self.0.take() {
            let _ = send.send(());
        }
    }
}
fn gate() -> (Box<dyn FnOnce() + Send>, mpsc::Receiver<()>, Gate) {
    let (entered, observed) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    (
        Box::new(move || {
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
        }),
        observed,
        Gate(Some(release)),
    )
}
struct BlockingWake(Mutex<Option<Box<dyn FnOnce() + Send>>>);
impl Wake for BlockingWake {
    fn wake(self: Arc<Self>) {
        let effect = self.0.lock().take();
        if let Some(effect) = effect {
            effect();
        }
    }
}
fn key(
    token: Arc<NativeRevocation>,
    hooks: WorkerHooks,
) -> (KeyCustodian, KeyRequestHandle, KeyStart) {
    let point = hex(vector()["public_key_sec1"].as_str().unwrap());
    let spki = [SPKI_HEAD, &point[1..]].concat();
    KeyCustodian::prepare_revocable_for_test(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
        token,
        hooks,
    )
    .unwrap()
}

#[test]
fn HostKeys_LastOwnerAdvancesSiblingBeforePrepareAndNotify_001() {
    let token = NativeRevocation::new();
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let publisher = reservation
        .prepare_keys(token.clone(), Instant::now() + Duration::from_secs(30))
        .unwrap();
    let observer = operation.test_key_observer();
    let (claim, at_claim, release_claim) = gate();
    let (first, first_signal, first_start) = key(
        token.clone(),
        WorkerHooks {
            before_claim: Some(claim),
            ..Default::default()
        },
    );
    publisher.test_publish(0, first, first_signal.clone(), first_start);
    let mut ticket = first_signal
        .begin(b"PUBLIC_THREE_GATE".to_vec(), SignatureEncoding::Fixed)
        .unwrap();
    at_claim.recv_timeout(Duration::from_secs(3)).unwrap();
    let (wake, in_wake, release_wake) = gate();
    let waker = Waker::from(Arc::new(BlockingWake(Mutex::new(Some(wake)))));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    {
        let _entered = runtime.enter();
        assert!(std::pin::Pin::new(&mut ticket)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
    }
    let (tail, in_tail, release_tail) = gate();
    let (second, second_signal, second_start) = key(
        token.clone(),
        WorkerHooks {
            after_free: Some(tail),
            ..Default::default()
        },
    );
    publisher.test_publish(1, second, second_signal.clone(), second_start);
    second_signal
        .begin(b"PUBLIC_SECOND_KEY".to_vec(), SignatureEncoding::Fixed)
        .unwrap()
        .wait_blocking()
        .unwrap();
    let (prepare_tail, in_prepare, release_prepare) = gate();
    reservation
        .spawn(move |_| {
            publisher.test_close();
            prepare_tail();
            true
        })
        .unwrap();
    in_prepare.recv_timeout(Duration::from_secs(3)).unwrap();
    token.revoke();
    in_wake.recv_timeout(Duration::from_secs(3)).unwrap();
    release_claim.open();
    in_tail.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(observer.snapshot().publisher_closed && operation.snapshot().outstanding);
    let (dropping, drop_started) = mpsc::sync_channel(1);
    let fallback = std::thread::spawn(move || {
        dropping.send(()).unwrap();
        // 真正最后 operation owner 的 Drop；没有测试包装器替它推进。
        drop(operation);
    });
    drop_started.recv_timeout(Duration::from_secs(3)).unwrap();
    release_tail.open();
    let until = Instant::now() + Duration::from_secs(1);
    let joined_while_other_gates_closed = loop {
        // 只读事实，不由主线程调用 observe/advance 代替 fallback。
        if observer.snapshot().keys[1].is_some_and(|fact| !fact.holds_original_join) {
            break true;
        }
        if Instant::now() >= until {
            break false;
        }
        std::thread::yield_now();
    };
    release_prepare.open();
    release_wake.open();
    fallback.join().unwrap();
    drop(ticket);
    assert!(observer.snapshot().disposal_complete);
    assert!(
        joined_while_other_gates_closed,
        "last owner waited for prepare before joining its finished sibling key"
    );
}

fn drain(operation: &ActivationOperation) -> crate::remote_host::activation::ActivationObservation {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let result = operation.retire();
        if !result.outstanding {
            return result;
        }
        assert!(Instant::now() < end, "original custody did not retire");
        std::thread::yield_now();
    }
}

#[test]
fn HostKeys_RetiredOperationRejectsLateCustody_002() {
    let reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    drop(reservation);
    assert!(!operation.snapshot().outstanding);
    let created = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let called = created.clone();
    let installed = operation.test_prepare_keys(
        NativeRevocation::new(),
        Instant::now() + Duration::from_secs(30),
        NotifyHooks {
            before_publish: Some(Box::new(move || {
                called.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            })),
            ..Default::default()
        },
    );
    let rejected = installed.is_err();
    drop(installed);
    drain(&operation);
    assert!(
        rejected,
        "retired operation accepted a fresh notification owner"
    );
    assert_eq!(created.load(std::sync::atomic::Ordering::Acquire), 0);
}

#[test]
fn HostKeys_SnapshotCannotCombineStaleSuccessWithJoinedNotify_003() {
    let (tail, in_tail, release_tail) = gate();
    let custody = Arc::new(HostKeyCustody::reserve_with(
        NativeRevocation::new(),
        Instant::now() + Duration::from_secs(30),
        NotifyHooks {
            after_dispose: Some(Box::new(move || {
                tail();
                panic!("TEST_ONLY_NOTIFY_TAIL");
            })),
            ..Default::default()
        },
    ));
    custody.publisher().unwrap().test_close();
    custody.revoke();
    in_tail.recv_timeout(Duration::from_secs(3)).unwrap();
    let (split, at_split, release_split) = gate();
    custody.test_between_snapshot(split);
    let stale = custody.clone();
    let reader = std::thread::spawn(move || stale.snapshot());
    at_split.recv_timeout(Duration::from_secs(3)).unwrap();
    release_tail.open();
    let until = Instant::now() + Duration::from_secs(3);
    let joined = loop {
        let observation = custody.advance();
        if !observation.outstanding {
            break observation;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    };
    release_split.open();
    let stale = reader.join().unwrap();
    assert!(joined.unknown);
    assert!(
        stale.outstanding || stale.unknown,
        "snapshot combined old success with a newly joined original notification"
    );
}

#[test]
fn HostKeys_CloseDuringNotifyPublicationRetainsLateOwner_004() {
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let token = NativeRevocation::new();
    let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = opened.clone();
    let (publishing, in_publication, release_publication) = gate();
    let worker = std::thread::spawn(move || {
        let publisher = reservation
            .test_prepare_keys(
                token.clone(),
                Instant::now() + Duration::from_secs(30),
                NotifyHooks {
                    before_publish: Some(publishing),
                    ..Default::default()
                },
            )
            .unwrap();
        let point = hex(vector()["public_key_sec1"].as_str().unwrap());
        let spki = [SPKI_HEAD, &point[1..]].concat();
        let (owner, signal, start) = KeyCustodian::prepare_revocable(
            move || {
                calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                import_public_rfc6979_fixture(&point)
            },
            SigningPurpose::Endpoint,
            spki,
            token,
        )
        .unwrap();
        publisher.test_publish(0, owner, signal, start);
        publisher.test_close();
        reservation.spawn(|_| panic!("cancelled prepare")).unwrap();
    });
    in_publication.recv_timeout(Duration::from_secs(3)).unwrap();
    operation.cancel();
    assert!(operation.snapshot().outstanding && operation.advance_join().outstanding);
    release_publication.open();
    worker.join().unwrap();
    let result = drain(&operation);
    let facts = operation.test_key_observer().snapshot();
    assert!(!result.unknown);
    assert_eq!(opened.load(std::sync::atomic::Ordering::Acquire), 0);
    assert!(facts.keys[0].is_some_and(
        |fact| fact.retirement == KeyRetirement::NoKeyCreated && !fact.holds_original_join
    ));
}

fn frozen_binding(cap: Instant, origin: &str) -> FrozenHostBinding {
    let endpoint = hex(vector()["public_key_sec1"].as_str().unwrap());
    let relay=hex("046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5");
    FrozenHostBinding {
        native_profile: HostKeyProfileIdentity {
            id: [7; 16],
            revision: 9,
        },
        endpoint_spki: [SPKI_HEAD, &endpoint[1..]].concat().try_into().unwrap(),
        relay_spki: [SPKI_HEAD, &relay[1..]].concat().try_into().unwrap(),
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(origin).unwrap(),
        relay_id: [1; 16],
        profile_cap: cap,
    }
}
fn supplier(binding: FrozenHostBinding, calls: Arc<[AtomicUsize; 2]>) -> FrozenHostSupplier {
    let endpoint = binding.endpoint_spki[26..].to_vec();
    let relay = binding.relay_spki[26..].to_vec();
    let first = calls.clone();
    FrozenHostSupplier::test_new(
        binding,
        Box::new(move || {
            first[0].fetch_add(1, Ordering::AcqRel);
            import_public_rfc6979_fixture(&endpoint)
        }),
        Box::new(move || {
            calls[1].fetch_add(1, Ordering::AcqRel);
            let mut scalar = vec![0; 32];
            scalar[31] = 1;
            import_known_public_fixture(&relay, &scalar, SigningPurpose::Relay)
        }),
    )
}
fn counters() -> Arc<[AtomicUsize; 2]> {
    Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)])
}
fn wait_disposed(observer: &HostKeyCore) {
    let end = Instant::now() + Duration::from_secs(5);
    while !observer.snapshot().disposal_complete {
        assert!(
            Instant::now() < end,
            "autonomous key disposal did not finish"
        );
        std::thread::yield_now();
    }
}

#[test]
fn HostKeys_MetadataAndPrecancelOpenNothing_005() {
    for mode in 0..6 {
        let mut reservation = ActivationOperation::test_reservation();
        let operation = reservation.operation();
        let token = NativeRevocation::new();
        if mode == 0 {
            operation.cancel();
        }
        let cap = Instant::now() + Duration::from_secs(30);
        let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
        let mut binding = frozen_binding(cap, "wss://relay.invalid/");
        match mode {
            1 => binding.endpoint_spki[0] = 0,
            2 => binding.relay_spki = binding.endpoint_spki,
            3 => binding.native_profile.id = [0; 16],
            4 => binding.profile_cap = Instant::now(),
            5 => {
                binding.origin = cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&format!(
                    "wss://{}.invalid/",
                    "a".repeat(520)
                ))
                .unwrap()
            }
            _ => {}
        }
        let calls = counters();
        assert!(publisher.prepare(supplier(binding, calls.clone())).is_err());
        drop(reservation);
        drain(&operation);
        assert!(token.is_revoked());
        assert_eq!(
            calls
                .iter()
                .map(|c| c.load(Ordering::Acquire))
                .sum::<usize>(),
            0,
            "mode={mode}"
        );
    }
}

#[tokio::test]
async fn HostKeys_FrozenRolesSignWhileBusinessDoneKeepsResources_006() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!(
        "wss://foobar.com:{}/relay",
        listener.local_addr().unwrap().port()
    );
    let cap = Instant::now() + Duration::from_secs(30);
    let mut source = frozen_binding(cap, &origin);
    let expected = source.clone();
    let calls = counters();
    let frozen = supplier(source.clone(), calls.clone());
    source.native_profile.revision += 1;
    source.endpoint_spki[0] = 0;
    assert_eq!(
        frozen.binding().native_profile.revision,
        expected.native_profile.revision
    );
    assert_eq!(frozen.binding().endpoint_spki, expected.endpoint_spki);
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let token = NativeRevocation::new();
    let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    reservation
        .spawn_prepared(move |_| {
            let set = publisher.prepare(frozen).unwrap();
            crate::remote_host::activation::PreparationResult::Prepared(Box::new(move || {
                send.send(set).is_ok()
            }))
        })
        .unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = operation.result() {
            assert!(result);
            break;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    assert!(
        operation.snapshot().outstanding,
        "business completion must not retire live keys"
    );
    let set = receive.recv_timeout(Duration::from_secs(3)).unwrap();
    let signature = set
        .endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .unwrap()
        .sign(b"PUBLIC_FROZEN_HOST")
        .unwrap();
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_ASN1,
        &expected.endpoint_spki[26..],
    )
    .verify(b"PUBLIC_FROZEN_HOST", &signature)
    .unwrap();
    let result = super::signers_tests::relay_trial(
        listener,
        origin,
        &set.relay,
        expected.relay_spki[26..].to_vec(),
    )
    .await;
    assert_eq!(result, (true, false, true));
    let other = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_origin = format!(
        "wss://foobar.com:{}/relay",
        other.local_addr().unwrap().port()
    );
    assert_eq!(
        super::signers_tests::relay_trial(
            other,
            other_origin,
            &set.relay,
            expected.relay_spki[26..].to_vec()
        )
        .await,
        (false, true, false)
    );
    assert_eq!(calls[0].load(Ordering::Acquire), 1);
    assert_eq!(calls[1].load(Ordering::Acquire), 1);
    // 类型只有 Endpoint/Relay 两个 opener；Issuer 私钥入口不存在。
    let held_signer = set
        .endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .unwrap();
    operation.cancel();
    assert!(!set.attempt.is_current());
    assert!(set
        .endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .is_none());
    assert!(held_signer.sign(b"REVOKED_HOST").is_err());
    drop(set);
    assert!(!drain(&operation).unknown);
}

#[test]
fn HostKeys_SecondSpawnFailureKeepsFirstOriginal_007() {
    let cap = Instant::now() + Duration::from_secs(30);
    let binding = frozen_binding(cap, "wss://relay.invalid/");
    let point = binding.endpoint_spki[26..].to_vec();
    let (created, opened) = mpsc::sync_channel(1);
    let second = Arc::new(AtomicUsize::new(0));
    let calls = second.clone();
    let frozen = FrozenHostSupplier::test_new(
        binding,
        Box::new(move || {
            let key = import_public_rfc6979_fixture(&point)?;
            created.send(()).unwrap();
            Ok(key)
        }),
        Box::new(move || {
            calls.fetch_add(1, Ordering::AcqRel);
            Err(CryptoError::Key)
        }),
    );
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let publisher = reservation
        .prepare_keys(NativeRevocation::new(), cap)
        .unwrap();
    assert!(publisher
        .prepare_with(
            frozen,
            PrepareHooks {
                fail_second_spawn: true,
                first_opened: Some(opened)
            }
        )
        .is_err());
    drop(reservation);
    let observer = operation.test_key_observer();
    wait_disposed(&observer);
    assert_eq!(
        observer.snapshot().keys[0].unwrap().retirement,
        KeyRetirement::Retired
    );
    assert!(!observer.snapshot().keys[0].unwrap().holds_original_join);
    assert!(observer.snapshot().keys[1].is_none());
    assert_eq!(second.load(Ordering::Acquire), 0);
    assert!(!drain(&operation).unknown);
}

#[test]
fn HostKeys_FactoryFailureAndExpiryCloseWithoutExit_008() {
    for failure in [false, true] {
        let cap = Instant::now()
            + if failure {
                Duration::from_secs(30)
            } else {
                Duration::from_secs(2)
            };
        let binding = frozen_binding(cap, "wss://relay.invalid/");
        let mut reservation = ActivationOperation::test_reservation();
        let operation = reservation.operation();
        let token = NativeRevocation::new();
        let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
        let observer = operation.test_key_observer();
        let (fail, at_factory, release_factory) = gate();
        let frozen = if failure {
            let endpoint = binding.endpoint_spki[26..].to_vec();
            FrozenHostSupplier::test_new(
                binding,
                Box::new(move || import_public_rfc6979_fixture(&endpoint)),
                Box::new(move || {
                    fail();
                    Err(CryptoError::Key)
                }),
            )
        } else {
            supplier(binding, counters())
        };
        let set = publisher.prepare(frozen).unwrap();
        reservation.spawn(|_| true).unwrap();
        set.endpoint
            .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
            .unwrap()
            .sign(b"PUBLIC_BEFORE_STOP")
            .unwrap();
        if failure {
            at_factory.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        release_factory.open();
        wait_disposed(&observer);
        assert!(token.is_revoked());
        assert!(observer
            .snapshot()
            .keys
            .iter()
            .flatten()
            .all(|f| !f.holds_original_join));
        drop(set);
        let result = drain(&operation);
        assert_eq!(result.unknown, failure);
    }
}

#[test]
fn HostKeys_LastSlotDropStaysOnNotifierWithConcurrentObservers_009() {
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let token = NativeRevocation::new();
    let (finished, notifier) = mpsc::sync_channel(1);
    let publisher = reservation
        .test_prepare_keys(
            token.clone(),
            Instant::now() + Duration::from_secs(30),
            NotifyHooks {
                after_dispose: Some(Box::new(move || {
                    finished.send(std::thread::current().id()).unwrap();
                })),
                ..Default::default()
            },
        )
        .unwrap();
    let (drops, observed) = mpsc::channel();
    for index in 0..2 {
        let (owner, signal, start) = key(token.clone(), WorkerHooks::default());
        let request = signal.clone();
        publisher.test_publish(index, owner, signal, start);
        request
            .begin(b"PUBLIC_SLOT_ALLOCATION".to_vec(), SignatureEncoding::Fixed)
            .unwrap()
            .wait_blocking()
            .unwrap();
        let drops = drops.clone();
        publisher.test_last_drop(
            index,
            Box::new(move || {
                drops.send(std::thread::current().id()).unwrap();
            }),
        );
    }
    publisher.test_close();
    reservation.spawn(|_| true).unwrap();
    let observer = operation.test_key_observer();
    let stop = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..8)
        .map(|_| {
            let core = observer.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    let _ = core.snapshot();
                    std::thread::yield_now();
                }
                std::thread::current().id()
            })
        })
        .collect();
    let result = drain(&operation);
    let notification_thread = notifier.recv_timeout(Duration::from_secs(3)).unwrap();
    let actual = [
        observed.recv_timeout(Duration::from_secs(3)).unwrap(),
        observed.recv_timeout(Duration::from_secs(3)).unwrap(),
    ];
    stop.store(true, Ordering::Release);
    let reader_ids: Vec<_> = readers.into_iter().map(|r| r.join().unwrap()).collect();
    assert!(!result.unknown && actual.iter().all(|id| *id == notification_thread));
    assert!(!reader_ids.contains(&notification_thread));
    assert_eq!(observer.test_notify_joins(), 1);
}

#[test]
fn HostKeys_ReservationAbandonRevokesBeforeLatePreparation_010() {
    let cap = Instant::now() + Duration::from_secs(30);
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let token = NativeRevocation::new();
    let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
    drop(reservation);
    let immediately_revoked = token.is_revoked();
    let calls = counters();
    let prepared = publisher.prepare(supplier(
        frozen_binding(cap, "wss://relay.invalid/"),
        calls.clone(),
    ));
    if let Ok(set) = &prepared {
        let _ = set
            .endpoint
            .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
            .unwrap()
            .sign(b"PUBLIC_LATE_ABANDON");
    }
    let refused = prepared.is_err();
    drop(prepared);
    drain(&operation);
    assert!(
        immediately_revoked && refused,
        "abandoned reservation retained live signing authority"
    );
    assert_eq!(
        calls
            .iter()
            .map(|c| c.load(Ordering::Acquire))
            .sum::<usize>(),
        0
    );
}

#[test]
fn HostKeys_ReservationSpawnFailureRevokesPublishedKeys_011() {
    let cap = Instant::now() + Duration::from_secs(30);
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let token = NativeRevocation::new();
    let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
    let set = publisher
        .prepare(supplier(
            frozen_binding(cap, "wss://relay.invalid/"),
            counters(),
        ))
        .unwrap();
    set.endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .unwrap()
        .sign(b"PUBLIC_BEFORE_SPAWN_ERROR")
        .unwrap();
    assert!(reservation.test_fail_spawn().is_err());
    let immediately_revoked = token.is_revoked();
    drop(set);
    drain(&operation);
    assert!(
        immediately_revoked,
        "prepare spawn failure retained published signing authority"
    );
}

#[test]
fn HostKeys_FailureOutcomesRevokeBeforeRetirement_012() {
    let mut observations = Vec::new();
    for mode in 0..6 {
        let cap = Instant::now() + Duration::from_secs(30);
        let mut reservation = ActivationOperation::test_reservation();
        let operation = reservation.operation();
        let token = NativeRevocation::new();
        let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
        let set = publisher
            .prepare(supplier(
                frozen_binding(cap, "wss://relay.invalid/"),
                counters(),
            ))
            .unwrap();
        set.endpoint
            .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
            .unwrap()
            .sign(b"PUBLIC_BEFORE_FAILURE")
            .unwrap();
        reservation
            .spawn_prepared(move |_| {
                use crate::remote_host::activation::PreparationResult;
                match mode {
                    0 => PreparationResult::Complete(false),
                    1 => panic!("TEST_ONLY_PREPARE_OUTCOME"),
                    2 => PreparationResult::Prepared(Box::new(|| false)),
                    3 => PreparationResult::Prepared(Box::new(|| {
                        panic!("TEST_ONLY_REGISTRATION_OUTCOME")
                    })),
                    4 => PreparationResult::Complete(true),
                    _ => PreparationResult::Prepared(Box::new(|| true)),
                }
            })
            .unwrap();
        let before_observer = if mode < 2 {
            let end = Instant::now() + Duration::from_secs(1);
            while !token.is_revoked() && Instant::now() < end {
                std::thread::yield_now();
            }
            token.is_revoked()
        } else {
            false
        };
        let end = Instant::now() + Duration::from_secs(3);
        let business = loop {
            if let Some(result) = operation.result() {
                break result;
            }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        };
        let revoked_before_retire = token.is_revoked();
        let healthy_success =
            mode < 4 || (business && set.attempt.is_current() && operation.snapshot().outstanding);
        observations.push((
            mode,
            before_observer,
            revoked_before_retire,
            business,
            healthy_success,
        ));
        drop(set);
        drain(&operation);
    }
    eprintln!("FAILURE_OUTCOMES_BEFORE_RETIRE={observations:?}");
    for (mode, worker_revoked, revoked, business, healthy) in observations {
        assert_eq!(revoked, mode < 4, "mode={mode}");
        assert_eq!(business, mode >= 4, "mode={mode}");
        assert!(healthy);
        if mode < 2 {
            assert!(
                worker_revoked,
                "worker must revoke without an observer: mode={mode}"
            );
        }
    }
}

#[test]
fn HostKeys_NotificationCreationFailureAndPublicationUnwind_013() {
    for panic_after_spawn in [false, true] {
        let token = NativeRevocation::new();
        let mut reservation = ActivationOperation::test_reservation();
        let operation = reservation.operation();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            reservation.test_prepare_keys(
                token.clone(),
                Instant::now() + Duration::from_secs(30),
                NotifyHooks {
                    fail_spawn: !panic_after_spawn,
                    before_publish: panic_after_spawn.then(|| {
                        Box::new(|| panic!("TEST_ONLY_NOTIFY_PUBLICATION"))
                            as Box<dyn FnOnce() + Send>
                    }),
                    ..Default::default()
                },
            )
        }));
        assert!(matches!(result, Err(_) | Ok(Err(_))));
        let observer = operation.test_key_observer();
        assert!(token.is_revoked());
        // 发布能力已封闭，但原 prepare reservation 尚未结束，不能冒充总退休。
        assert!(operation.snapshot().outstanding);
        drop(reservation);
        let retired = drain(&operation);
        assert_eq!(retired.unknown, panic_after_spawn);
        assert!(observer.snapshot().publisher_closed && observer.snapshot().disposal_complete);
        assert!(observer.snapshot().keys.iter().all(Option::is_none));
        assert_eq!(observer.test_notify_joins(), usize::from(panic_after_spawn));
    }
}

#[test]
fn HostKeys_NotificationAndFreeUnknownRemainOwned_014() {
    const FLAG: &str = "CC_DESK_TEST_HOST_CUSTODY_UNKNOWN";
    if let Ok(mode) = std::env::var(FLAG) {
        let token = NativeRevocation::new();
        let mut reservation = ActivationOperation::test_reservation();
        let operation = reservation.operation();
        let cap = Instant::now()
            + if mode == "free" {
                Duration::from_secs(2)
            } else {
                Duration::from_secs(30)
            };
        let publisher = reservation
            .test_prepare_keys(
                token.clone(),
                cap,
                NotifyHooks {
                    before_dispose: (mode == "disposal").then(|| {
                        Box::new(|| panic!("TEST_ONLY_IN_FLIGHT_DISPOSAL"))
                            as Box<dyn FnOnce() + Send>
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        let observer = operation.test_key_observer();
        assert!(observer.snapshot().outstanding && !observer.snapshot().publisher_closed);
        let (claim, at_claim, release_claim) = gate();
        let mut held_ticket = None;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        if mode == "free" {
            let binding = frozen_binding(cap, "wss://relay.invalid/");
            let expected = binding.endpoint_spki;
            let (created, opened) = mpsc::sync_channel(1);
            let (owner, signal, start) = KeyCustodian::prepare_revocable(
                move || {
                    // 合成句柄的 free 全部被拦截；不对它们调用任何真实 native API。
                    let key = NativeOwnedSoftwareKey {
                        provider: NCRYPT_PROV_HANDLE(2),
                        key: NCRYPT_KEY_HANDLE(1),
                        approved_purpose: SigningPurpose::Endpoint,
                        approved_spki: expected,
                        worker: std::thread::current().id(),
                        unknown: false,
                        _not_send_or_sync: PhantomData,
                        free_effect: Some(Box::new(|_| Err(CryptoError::Unknown))),
                    };
                    created.send(()).unwrap();
                    Ok(key)
                },
                SigningPurpose::Endpoint,
                expected.to_vec(),
                token.clone(),
            )
            .unwrap();
            publisher.test_publish(0, owner, signal, start);
            opened.recv_timeout(Duration::from_secs(3)).unwrap();
        } else {
            let (owner, signal, start) = key(
                token.clone(),
                WorkerHooks {
                    before_claim: (mode == "notice").then_some(claim),
                    ..Default::default()
                },
            );
            publisher.test_publish(0, owner, signal.clone(), start);
            if mode == "notice" {
                let mut ticket = signal
                    .begin(b"PUBLIC_PANIC_WAKE".to_vec(), SignatureEncoding::Fixed)
                    .unwrap();
                at_claim.recv_timeout(Duration::from_secs(3)).unwrap();
                let waker = Waker::from(Arc::new(BlockingWake(Mutex::new(Some(Box::new(|| {
                    panic!("TEST_ONLY_NOTIFICATION_WAKE")
                }))))));
                {
                    let _entered = runtime.enter();
                    assert!(std::pin::Pin::new(&mut ticket)
                        .poll(&mut Context::from_waker(&waker))
                        .is_pending());
                }
                held_ticket = Some(ticket);
            } else {
                signal
                    .begin(b"PUBLIC_DISPOSAL".to_vec(), SignatureEncoding::Fixed)
                    .unwrap()
                    .wait_blocking()
                    .unwrap();
            }
        }
        let (owner, signal, start) = key(token.clone(), WorkerHooks::default());
        publisher.test_publish(1, owner, signal.clone(), start);
        signal
            .begin(b"PUBLIC_UNKNOWN_SIBLING".to_vec(), SignatureEncoding::Fixed)
            .unwrap()
            .wait_blocking()
            .unwrap();
        publisher.test_close();
        reservation.spawn(|_| true).unwrap();
        if mode != "free" {
            token.revoke();
        }
        let end = Instant::now() + Duration::from_secs(5);
        // free 用原 cap 自动触发，不调用退出/retire 来掩盖故障撤销。
        while !observer.snapshot().unknown {
            assert!(
                Instant::now() < end,
                "failure was not observed without exit"
            );
            std::thread::yield_now();
        }
        assert!(token.is_revoked());
        release_claim.open();
        let (finished, completion) = mpsc::sync_channel(1);
        let _fallback = std::thread::spawn(move || {
            drop(operation);
            let _ = finished.send(());
        });
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let facts = observer.snapshot();
            let joined = if mode == "free" {
                facts.keys[0].is_some_and(|f| {
                    f.retirement == KeyRetirement::Unknown && f.holds_original_join
                }) && facts.keys[1].is_some_and(|f| !f.holds_original_join)
            } else {
                facts
                    .keys
                    .iter()
                    .all(|f| f.is_some_and(|f| !f.holds_original_join))
                    && observer.test_notify_joins() == 1
            };
            if joined {
                assert!(facts.unknown && facts.outstanding && !facts.disposal_complete);
                assert!(matches!(
                    completion.try_recv(),
                    Err(mpsc::TryRecvError::Empty)
                ));
                println!("TEST_ONLY_HOST_CUSTODY_UNKNOWN_RETAINED_NOT_RETIRED:{mode}");
                // 仅负向隔离 fixture；结束进程绝不当作 free、join 或退休回执。
                let _ = &held_ticket;
                std::process::exit(0);
            }
            assert!(
                Instant::now() < end,
                "fallback did not join the independent original sibling"
            );
            std::thread::yield_now();
        }
    }
    struct Child(Option<std::process::Child>);
    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    for mode in ["notice", "disposal", "free"] {
        use std::os::windows::process::CommandExt;
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "native_crypto::tests::host_tests::HostKeys_NotificationAndFreeUnknownRemainOwned_014", "--nocapture", "--test-threads=1"])
            .env(FLAG, mode).creation_flags(0x08000000)
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        let mut original = Child(Some(child));
        let end = Instant::now() + Duration::from_secs(10);
        while original.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < end,
                "owned negative fixture timed out: {mode}"
            );
            std::thread::yield_now();
        }
        let output = original.0.take().unwrap().wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(&format!(
            "TEST_ONLY_HOST_CUSTODY_UNKNOWN_RETAINED_NOT_RETIRED:{mode}"
        )));
    }
}

#[test]
fn HostKeys_WrongNativePurposeFailsClosedWithoutExit_015() {
    let cap = Instant::now() + Duration::from_secs(30);
    let binding = frozen_binding(cap, "wss://relay.invalid/");
    let endpoint = binding.endpoint_spki[26..].to_vec();
    let relay = binding.relay_spki[26..].to_vec();
    let frozen = FrozenHostSupplier::test_new(
        binding,
        Box::new(move || import_public_rfc6979_fixture_for(&endpoint, SigningPurpose::Relay)),
        Box::new(move || {
            let mut scalar = vec![0; 32];
            scalar[31] = 1;
            import_known_public_fixture(&relay, &scalar, SigningPurpose::Relay)
        }),
    );
    let mut reservation = ActivationOperation::test_reservation();
    let operation = reservation.operation();
    let token = NativeRevocation::new();
    let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
    let observer = operation.test_key_observer();
    let set = publisher.prepare(frozen).unwrap();
    reservation.spawn(|_| true).unwrap();
    let signer = set
        .endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .unwrap();
    assert!(signer.sign(b"PUBLIC_WRONG_NATIVE_PURPOSE").is_err());
    assert!(token.is_revoked());
    wait_disposed(&observer);
    assert!(observer
        .snapshot()
        .keys
        .iter()
        .flatten()
        .all(|f| !f.holds_original_join));
    drop(set);
    // 签名已拒绝且资格失效，但正常 Free + 原 join 是已知资源退休。
    assert!(!drain(&operation).unknown);
}
