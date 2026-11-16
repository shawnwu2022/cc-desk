#![allow(non_snake_case)]
use super::super::activation::PreparationResult;
use super::*;
use crate::remote_host::{ExitDecision, RemoteHostService};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

fn wait(mut condition: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if condition() {
            return true;
        }
        std::thread::yield_now();
    }
    false
}
fn empty_native() -> Arc<crate::cli::native_runtime::NativeRuntime> {
    use crate::cli::{launch_service::LaunchService, storage::WorkspaceRepository};
    let dir = tempfile::tempdir().unwrap();
    Arc::new(crate::cli::native_runtime::NativeRuntime::new(Arc::new(
        LaunchService::new(
            WorkspaceRepository::open(dir.path().join("workspace.json")).unwrap(),
            Some(Default::default()),
            None,
        ),
    )))
}
#[test]
fn HostDriver_AutonomousRegistrationWithoutObserver_001() {
    let service = RemoteHostService::default();
    let (reservation, owner, start) = service.reserve_driven_activation().unwrap();
    let operation = reservation.operation();
    let link = owner.link();
    let (completed, completion) = mpsc::sync_channel(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let registered = calls.clone();
    let gate = start.spawn().unwrap();
    reservation
        .spawn_prepared(move |_| {
            PreparationResult::Prepared(Box::new(move || {
                registered.fetch_add(1, Ordering::AcqRel);
                completed.send(()).unwrap();
                true
            }))
        })
        .unwrap();
    gate.open();
    completion.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(wait(|| operation.preparation_result() == Some(true)));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(
        link.snapshot().outstanding,
        "business result is not driver join"
    );
    for _ in 0..16 {
        assert_eq!(operation.preparation_result(), Some(true));
    }
    assert!(wait(|| !link.advance().outstanding));
    assert!(!link.snapshot().unknown);
    drop(owner);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn HostDriver_BlockedPreparationAndRegistrationKeepExitPending_002() {
    for registration in [false, true] {
        let service = Arc::new(RemoteHostService::default());
        let (reservation, owner, start) = service.reserve_driven_activation().unwrap();
        let operation = reservation.operation();
        let link = owner.link();
        let (entered, at_gate) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let registered = calls.clone();
        let gate = start.spawn().unwrap();
        reservation
            .spawn_prepared(move |signal| {
                if !registration {
                    entered.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(5)).unwrap();
                    return PreparationResult::Complete(false);
                }
                PreparationResult::Prepared(Box::new(move || {
                    entered.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(5)).unwrap();
                    if signal.cancelled() {
                        false
                    } else {
                        registered.fetch_add(1, Ordering::AcqRel);
                        true
                    }
                }))
            })
            .unwrap();
        gate.open();
        at_gate.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(service.request_exit(37, false), ExitDecision::Wait(37));
        let (exited, mut exit) = tokio::sync::oneshot::channel();
        service.start_exit(
            37,
            empty_native(),
            Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
            move |code| {
                let _ = exited.send(code);
            },
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            while !service.test_activation_exit_observed() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // activation 首次被 poll 不保证 native 原 owner 已到终态；保持 driver 门关闭等真实事实。
        let other_observed = tokio::time::timeout(Duration::from_secs(2), async {
            while !service.state.lock().native_observed_terminal {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_ok();
        let pending = matches!(
            exit.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        );
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), exit)
                .await
                .unwrap()
                .unwrap(),
            37
        );
        let waiter = service.state.lock().waiter.take().unwrap();
        waiter.await.unwrap();
        assert!(
            pending && other_observed,
            "driver wait must not prevent original native branch observation"
        );
        assert_eq!(operation.preparation_result(), Some(false));
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert!(link.snapshot().retired());
        drop(owner);
    }
}
#[test]
fn HostDriver_LastServiceAndOwnerReleaseWithoutSelfJoin_003() {
    let service = Arc::new(RemoteHostService::default());
    let weak = Arc::downgrade(&service);
    let (reservation, owner, start) = service.reserve_driven_activation().unwrap();
    let link = owner.link();
    let target = weak.clone();
    let (entered, at_gate) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let gate = start.spawn().unwrap();
    reservation
        .spawn_prepared(move |signal| {
            PreparationResult::Prepared(Box::new(move || {
                let original = target.upgrade().unwrap();
                entered.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(5)).unwrap();
                drop(original);
                !signal.cancelled()
            }))
        })
        .unwrap();
    gate.open();
    at_gate.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(service);
    let (dropping, at_drop) = mpsc::sync_channel(1);
    let (done, finished) = mpsc::sync_channel(1);
    let original = std::thread::spawn(move || {
        dropping.send(()).unwrap();
        drop(owner);
        done.send(()).unwrap();
    });
    at_drop.recv_timeout(Duration::from_secs(3)).unwrap();
    let pending = link.snapshot().outstanding;
    release.send(()).unwrap();
    finished.recv_timeout(Duration::from_secs(5)).unwrap();
    original.join().unwrap();
    assert!(pending && weak.upgrade().is_none());
    assert!(link.snapshot().retired());
}
#[test]
fn HostDriver_SpawnPublicationAndGateFailuresKeepOriginal_004() {
    for mode in 0..3 {
        let service = RemoteHostService::default();
        let (reservation, owner, start) = service.reserve_driven_activation().unwrap();
        let operation = reservation.operation();
        let link = owner.link();
        let calls = Arc::new(AtomicUsize::new(0));
        let work = calls.clone();
        if mode < 2 {
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                start
                    .test_hooks(DriverHooks {
                        fail_spawn: mode == 0,
                        before_publish: (mode == 1).then(|| {
                            Box::new(|| panic!("TEST_ONLY_DRIVER_PUBLICATION"))
                                as Box<dyn FnOnce() + Send>
                        }),
                        ..Default::default()
                    })
                    .spawn()
            }));
            assert!(if mode == 0 {
                matches!(attempt, Ok(Err(_)))
            } else {
                attempt.is_err()
            });
            drop(reservation);
        } else {
            let gate = start.spawn().unwrap();
            operation.cancel();
            reservation
                .spawn(move |_| {
                    work.fetch_add(1, Ordering::AcqRel);
                    true
                })
                .unwrap();
            drop(gate);
        }
        assert!(wait(|| !link.advance().outstanding));
        assert_eq!(link.snapshot().unknown, mode == 1);
        assert!(operation.cancelled());
        assert_eq!(calls.load(Ordering::Acquire), 0);
        drop(owner);
    }
}
#[test]
fn HostDriver_BusinessCompleteStillWaitsOriginalTail_005() {
    let service = RemoteHostService::default();
    let (reservation, owner, start) = service.reserve_driven_activation().unwrap();
    let operation = reservation.operation();
    let link = owner.link();
    let (entered, at_tail) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let gate = start
        .test_hooks(DriverHooks {
            after_work: Some(Box::new(move || {
                entered.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(5)).unwrap();
            })),
            ..Default::default()
        })
        .spawn()
        .unwrap();
    reservation.spawn(|_| true).unwrap();
    gate.open();
    at_tail.recv_timeout(Duration::from_secs(5)).unwrap();
    let pending = link.advance().outstanding;
    assert_eq!(operation.preparation_result(), Some(true));
    let readers: Vec<_> = (0..8)
        .map(|_| {
            let observed = link.clone();
            std::thread::spawn(move || observed.advance().outstanding)
        })
        .collect();
    let outcomes: Vec<_> = readers.into_iter().map(|reader| reader.join()).collect();
    let all_pending = outcomes
        .into_iter()
        .all(|outcome| matches!(outcome, Ok(true)));
    release.send(()).unwrap();
    assert!(wait(|| !link.advance().outstanding));
    assert!(pending && all_pending && link.snapshot().retired());
    drop(owner);
}
#[test]
fn HostDriver_PreparationFailureClassificationRemainsIndependent_006() {
    for mode in 0..4 {
        let service = RemoteHostService::default();
        let (reservation, owner, start) = service.reserve_driven_activation().unwrap();
        let operation = reservation.operation();
        let link = owner.link();
        let gate = start.spawn().unwrap();
        reservation
            .spawn_prepared(move |_| match mode {
                0 => PreparationResult::Complete(false),
                1 => PreparationResult::Prepared(Box::new(|| false)),
                2 => panic!("TEST_ONLY_PREPARE_PANIC"),
                _ => {
                    PreparationResult::Prepared(Box::new(|| panic!("TEST_ONLY_REGISTRATION_PANIC")))
                }
            })
            .unwrap();
        gate.open();
        assert!(wait(|| !link.advance().outstanding));
        assert!(operation.cancelled());
        assert_eq!(operation.preparation_result(), Some(false));
        assert_eq!(operation.snapshot().unknown, mode >= 2);
        assert!(
            !link.snapshot().unknown,
            "handled preparation failure is not a driver panic"
        );
        drop(owner);
    }
}
#[test]
fn HostDriver_BlockedPanicPayloadKeepsHandleAndSiblingJoins_007() {
    struct Payload {
        entered: mpsc::SyncSender<bool>,
        held: mpsc::Receiver<()>,
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        _material: crate::native_crypto::host::HostSignerSet,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            self.entered.send(self.token.is_revoked()).unwrap();
            self.held.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
    let service = RemoteHostService::default();
    let (mut reservation, owner, start) = service.reserve_driven_activation().unwrap();
    let operation = reservation.operation();
    let token = crate::native_crypto::owner::NativeRevocation::new();
    let cap = Instant::now() + Duration::from_secs(30);
    let publisher = reservation.prepare_keys(token.clone(), cap).unwrap();
    let observer = operation.test_key_observer();
    let opens = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let supplier = crate::native_crypto::test_host_supplier(
        cc_desk_remote_record::appbind::CanonicalOrigin::trusted("wss://test.invalid/relay")
            .unwrap(),
        cap,
        opens.clone(),
    );
    let material = publisher.prepare(supplier).unwrap();
    assert!(wait(|| opens
        .iter()
        .all(|n| n.load(Ordering::Acquire) == 1)));
    let (entered, at_drop) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let pure = token.clone();
    let gate = start
        .test_hooks(DriverHooks {
            after_work: Some(Box::new(move || {
                std::panic::panic_any(Payload {
                    entered,
                    held,
                    token: pure,
                    _material: material,
                })
            })),
            ..Default::default()
        })
        .spawn()
        .unwrap();
    reservation.spawn(|_| true).unwrap();
    gate.open();
    let revoked_before_drop = at_drop.recv_timeout(Duration::from_secs(5)).unwrap();
    let link = owner.link();
    let pending = link.advance().outstanding;
    let (done, finished) = mpsc::sync_channel(1);
    let fallback = std::thread::spawn(move || {
        drop(owner);
        done.send(()).unwrap();
    });
    let joined_before_release = wait(|| {
        observer
            .snapshot()
            .keys
            .iter()
            .all(|f| f.is_some_and(|f| !f.holds_original_join))
    });
    release.send(()).unwrap();
    finished.recv_timeout(Duration::from_secs(5)).unwrap();
    fallback.join().unwrap();
    assert!(revoked_before_drop && pending && joined_before_release);
    assert!(!link.snapshot().outstanding && link.snapshot().unknown);
}
#[test]
fn HostDriver_ResidualPanicPayloadStaysQuarantined_008() {
    const CHILD: &str = "CC_DESK_DRIVER_PAYLOAD_CHILD";
    if std::env::var_os(CHILD).is_some() {
        struct Residual(Arc<AtomicUsize>);
        impl Drop for Residual {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::AcqRel);
            }
        }
        struct First(Arc<AtomicUsize>);
        impl Drop for First {
            fn drop(&mut self) {
                std::panic::panic_any(Residual(self.0.clone()));
            }
        }
        let service = RemoteHostService::default();
        let (reservation, owner, start) = service.reserve_driven_activation().unwrap();
        let link = owner.link();
        let dropped = Arc::new(AtomicUsize::new(0));
        let probe = dropped.clone();
        let gate = start
            .test_hooks(DriverHooks {
                after_work: Some(Box::new(move || std::panic::panic_any(First(probe)))),
                ..Default::default()
            })
            .spawn()
            .unwrap();
        reservation.spawn(|_| true).unwrap();
        gate.open();
        let quarantined = wait(|| {
            let f = link.advance();
            f.unknown && f.outstanding
        });
        let kept = quarantined && dropped.load(Ordering::Acquire) == 0;
        eprintln!("TEST_ONLY_DRIVER_QUARANTINE retained={kept} retired=false");
        // 负向进程仅证明原责任仍保管；退出不是正常退休。
        std::process::exit(if kept { 0 } else { 91 });
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "remote_host::driver::tests::HostDriver_ResidualPanicPayloadStaysQuarantined_008",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let ended = wait(|| child.try_wait().unwrap().is_some());
    if !ended {
        child.kill().unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        ended && output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("retained=true retired=false"));
}
