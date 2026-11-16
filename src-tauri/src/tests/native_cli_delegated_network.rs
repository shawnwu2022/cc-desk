//! 原 Node/PTY 与真实 TLS/AppBind 通道；不连接 Relay 或真实手机。
use super::*;
use cc_desk_remote_client::NativeClient;
use cc_desk_remote_record::{control_io as wire, input, projection};
fn device() -> [u8; 32] {
    let (_, public) = channels::identities::identity(true, false);
    cc_desk_remote_security::P256Spki::new(&public)
        .unwrap()
        .endpoint_id(cc_desk_remote_security::Role::Client)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedChannel_OriginalProducerInputAndDisconnectStayBound_001() {
    if isolated("DelegatedChannel_OriginalProducerInputAndDisconnectStayBound_001") {
        return;
    }
    original_input_case(false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedChannel_OriginalProducerInputAndRevokeStayBound_007() {
    if isolated("DelegatedChannel_OriginalProducerInputAndRevokeStayBound_007") {
        return;
    }
    original_input_case(true).await;
}
async fn original_input_case(revoke: bool) {
    let (f, mut cleanup) = setup();
    let host = cleanup.host.clone();
    let source = admit(&f);
    let (consent, current, _) = NativeDelegationConsent::test_network(&source, device());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let session = host.reserve_delegation(owner).unwrap();
    assert!(host.reserve_activation().is_err());
    let (prepared, channel, _gate, _cancel) = session
        .test_prepare(&current, &f.supervisor, false)
        .await
        .unwrap();
    cleanup.proofs.push(channel.authority());
    session.adopt(prepared).unwrap();
    let installed = channel.client_installed().await.unwrap();
    let client = NativeClient::from_installed(channel, installed).unwrap();
    assert_eq!(host.test_pin_count(), 1);
    let budget = Duration::from_secs(1);
    let snapshot = client
        .snapshot(
            br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
            budget,
        )
        .await
        .unwrap();
    assert_eq!(snapshot.stamp.target.run, f.request_run().run_id);
    let ack = projection::encode_ack(
        &projection::Ack {
            profile: 1,
            stamp: snapshot.stamp,
        },
        1,
    )
    .unwrap();
    client.exchange(&ack, budget).await.unwrap();
    let window = client
        .exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, budget)
        .await
        .unwrap();
    let wire::Message::WindowGranted {
        window,
        not_after_host_ms,
        ..
    } = wire::decode(&window).unwrap()
    else {
        panic!("window");
    };
    let mut token = [0; 32];
    for (index, pair) in window.as_bytes().chunks_exact(2).enumerate() {
        token[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    let frame = input::encode(&input::Input {
        operation: [0x51; 16],
        sequence: 1,
        window: token,
        not_after_host_ms: not_after_host_ms.parse().unwrap(),
        bytes: b"nat".to_vec(),
    })
    .unwrap();
    let receipt = client.exchange(&frame, budget).await.unwrap();
    assert!(matches!(
        wire::decode(&receipt),
        Ok(wire::Message::InputReceipt {
            outcome: wire::Outcome::HostWritten,
            ..
        })
    ));
    if revoke {
        session.revoke();
    } else {
        session.disconnect();
    }
    assert!(client.exchange(&frame, budget).await.is_err());
    assert!(session.test_join_pump().await);
    assert!(session
        .test_prepare(&current, &f.supervisor, false)
        .await
        .is_err());
    // 未提供完整回收证明，即使原 pump 已 join 也不能重新取票据。
    f.service
        .access(&f.caller, &f.request_run())
        .unwrap()
        .write_frame(b"ive")
        .unwrap();
    let actual = report(&f);
    assert_eq!(actual["input"], json!([110, 97, 116, 105, 118, 101]));
    let _ = client.close().await;
    drop(session);
    assert!(cleanup.close());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedChannel_WrongScopeAndPreAdoptRevokeCannotConsumeProducer_002() {
    if isolated("DelegatedChannel_WrongScopeAndPreAdoptRevokeCannotConsumeProducer_002") {
        return;
    }
    for wrong_scope in [true, false] {
        let (f, mut cleanup) = setup();
        let source = admit(&f);
        let (consent, current, policy) = NativeDelegationConsent::test_network(&source, device());
        let owner = DelegatedRunOwner::capture(source, consent).unwrap();
        let session = cleanup.host.reserve_delegation(owner).unwrap();
        let (prepared, client, _gate, _cancel) = session
            .test_prepare(&current, &f.supervisor, wrong_scope)
            .await
            .unwrap();
        if !wrong_scope {
            policy.request_revoke();
        }
        assert!(session.adopt(prepared).is_err());
        assert_eq!(cleanup.host.test_pin_count(), 0);
        client.authority().request_close();
        let _ = client.wait_closed_checked().await;
        f.service
            .access(&f.caller, &f.request_run())
            .unwrap()
            .write_frame(b"native")
            .unwrap();
        assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
        drop(session);
        assert!(cleanup.close());
    }
}
#[test]
fn DelegatedChannel_OneShotReservationAndWrongDeviceReject_003() {
    if isolated("DelegatedChannel_OneShotReservationAndWrongDeviceReject_003") {
        return;
    }
    let (f, mut cleanup) = setup();
    let source = admit(&f);
    let (consent, current, _) = NativeDelegationConsent::test_network(&source, device());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    assert!(owner
        .admit_network(&current.test_wrong_device(), &f.supervisor)
        .is_err());
    let reservation = cleanup.host.reserve_activation().unwrap();
    assert!(cleanup.host.reserve_delegation(owner).is_err());
    drop(reservation);
    assert!(cleanup.close());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedChannel_RevokeWhileOriginalBindQueuedRejectsLateInstall_004() {
    if isolated("DelegatedChannel_RevokeWhileOriginalBindQueuedRejectsLateInstall_004") {
        return;
    }
    let (f, mut cleanup) = setup();
    let source = admit(&f);
    let (consent, current, _) = NativeDelegationConsent::test_network(&source, device());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let session = cleanup.host.reserve_delegation(owner).unwrap();
    let (mut prepared, client, _gate, _cancel) = session
        .test_prepare(&current, &f.supervisor, false)
        .await
        .unwrap();
    let (arrived, seen) = std::sync::mpsc::sync_channel(0);
    let (release, resume) = std::sync::mpsc::sync_channel(0);
    prepared.test_before_actor(move || {
        arrived.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    session.adopt(prepared).unwrap();
    seen.recv_timeout(Duration::from_secs(10)).unwrap();
    session.revoke();
    release.send(()).unwrap();
    assert!(client.client_installed().await.is_err());
    assert!(session.test_join_pump().await);
    assert!(session
        .test_prepare(&current, &f.supervisor, false)
        .await
        .is_err());
    f.service
        .access(&f.caller, &f.request_run())
        .unwrap()
        .write_frame(b"native")
        .unwrap();
    assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
    client.authority().request_close();
    let _ = client.wait_closed_checked().await;
    drop(session);
    assert!(cleanup.close());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedChannel_DisconnectBeforeAdoptClosesStartingAdmission_005() {
    if isolated("DelegatedChannel_DisconnectBeforeAdoptClosesStartingAdmission_005") {
        return;
    }
    let (f, mut cleanup) = setup();
    let source = admit(&f);
    let (consent, current, _) = NativeDelegationConsent::test_network(&source, device());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let session = cleanup.host.reserve_delegation(owner).unwrap();
    let (prepared, client, _gate, _cancel) = session
        .test_prepare(&current, &f.supervisor, false)
        .await
        .unwrap();
    session.disconnect();
    assert!(session.adopt(prepared).is_err());
    assert!(session
        .test_prepare(&current, &f.supervisor, false)
        .await
        .is_err());
    assert_eq!(cleanup.host.test_pin_count(), 0);
    client.authority().request_close();
    let _ = client.wait_closed_checked().await;
    f.service
        .access(&f.caller, &f.request_run())
        .unwrap()
        .write_frame(b"native")
        .unwrap();
    assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
    drop(session);
    assert!(cleanup.close());
}

#[test]
fn DelegatedChannel_ClockUsesOriginalAtomicLifetime_006() {
    if isolated("DelegatedChannel_ClockUsesOriginalAtomicLifetime_006") {
        return;
    }
    for mode in 0..3 {
        let (f, mut cleanup) = setup();
        let source = admit(&f);
        let (consent, current, _) = NativeDelegationConsent::test_network(&source, device());
        let owner = DelegatedRunOwner::capture(source, consent).unwrap();
        let admission = owner.admit_network(&current, &f.supervisor).unwrap();
        let domain = Arc::new(crate::remote_unattended::capability::network::NetworkClock::new());
        let clock = admission.clock(&domain).unwrap();
        assert_eq!(
            admission.test_clock_with_slots_locked(clock.as_ref()),
            Ok(1000)
        );
        match mode {
            0 => admission.test_stop_original_input(),
            1 => {
                cleanup.stop = Some(
                    f.supervisor
                        .begin_shutdown_owned(vec![cleanup.pin.take().unwrap()])
                        .unwrap(),
                )
            }
            _ => {
                f.service
                    .access(&f.caller, &f.request_run())
                    .unwrap()
                    .write_frame(b"native")
                    .unwrap();
                assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
            }
        }
        assert!(clock.now_ms().is_err());
        assert!(owner.admit_network(&current, &f.supervisor).is_err());
        drop(clock);
        drop(admission);
        drop(owner);
        assert!(cleanup.close());
    }
}

// 原工厂每进程终身只接纳一个 Host，退休后的 Weak 也不能重开。
// 隔离测试执行环境，不修改生产 registry、identity 或 incarnation。
fn isolated(case: &str) -> bool {
    const KEY: &str = "CC_DESK_TEST_ONLY_DELEGATED_CHANNEL_CASE";
    if std::env::var(KEY).ok().as_deref() == Some(case) {
        return false;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("worker.log");
    let log = fs::File::create(&path).unwrap();
    let exact = format!(
        "{}::{case}",
        module_path!()
            .strip_prefix("cc_desk::")
            .unwrap_or(module_path!())
    );
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env(KEY, case)
        .stdout(std::process::Stdio::from(log.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(log))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "isolated original Host test timed out: {}",
                fs::read_to_string(&path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let evidence = fs::read_to_string(&path).unwrap();
    assert!(
        status.success() && evidence.contains("1 passed; 0 failed"),
        "{evidence}"
    );
    eprintln!("TEST_ONLY_DELEGATED_ISOLATED {case}: original child joined, 1 passed");
    true
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedSocket_OriginalRelayMuxAndProducerJoin_001() {
    if isolated("DelegatedSocket_OriginalRelayMuxAndProducerJoin_001") {
        return;
    }
    socket_case(false, false, 0).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedSocket_CancelObserverRetainsLateMaterial_002() {
    if isolated("DelegatedSocket_CancelObserverRetainsLateMaterial_002") {
        return;
    }
    socket_case(true, false, 0).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedSocket_SessionDropRetainsOriginalMaterialAndNetwork_007() {
    if isolated("DelegatedSocket_SessionDropRetainsOriginalMaterialAndNetwork_007") {
        return;
    }
    socket_case(true, true, 0).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedReceipt_OriginalJoinsMintUniqueFacts_001() {
    if isolated("DelegatedReceipt_OriginalJoinsMintUniqueFacts_001") {
        return;
    }
    socket_case(false, false, 1).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedReceipt_CancelObserverRetainsOriginalNetworkJoin_002() {
    if isolated("DelegatedReceipt_CancelObserverRetainsOriginalNetworkJoin_002") {
        return;
    }
    socket_case(false, false, 2).await;
}
async fn socket_case(late: bool, drop_owner: bool, receipt_mode: u8) {
    use crate::remote_host_network::delegated::{TestMaterial, TestMaterialBuilder};
    let (f, mut cleanup) = setup();
    let mut relay = NetworkRelayFixture::start().await;
    let source = admit(&f);
    let scope = source.scope().unwrap();
    let (consent, mut current, policy) = NativeDelegationConsent::test_network(&source, device());
    policy.test_network_origin(&mut current, relay.origin.clone());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let session = Arc::new(cleanup.host.reserve_delegation(owner).unwrap());
    let (pump_entered, pump_seen) = tokio::sync::oneshot::channel();
    let (pump_release, pump_resume) = tokio::sync::oneshot::channel();
    if receipt_mode == 3 {
        session.test_hold_pump(pump_entered, pump_resume);
    }
    let origin = relay.origin.clone();
    let address = relay.address;
    let (accepted, ready) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::sync_channel(0);
    let (started, working) = tokio::sync::oneshot::channel();
    let mut build = TestMaterialBuilder::new(move |scope| {
        let _ = started.send(());
        if late {
            held.recv().unwrap();
        }
        TestMaterial {
            address,
            relay: network_relay_identity(&origin, true),
            tunnel: [3; 16],
            local: network_local(false),
            phone_pin: network_pin(true),
            binding: cc_desk_remote_security::HostSnapshot {
                host_key_epoch: 5,
                client_key_epoch: 7,
                activation: [0x33; 32],
                incarnation: 11,
                policy_epoch: 13,
                issuer_epoch: 17,
                origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&origin).unwrap(),
            },
            browser: network_browser_profile(),
            issuer: network_grants::admin(),
            principal: [4; 32],
            policy: cc_desk_remote_security::NativeGrantPolicy {
                scope,
                role: cc_desk_remote_record::grant::DataRole::Controller,
                duration: Duration::from_secs(10),
            },
            accepted: Some(Box::new((accepted, None))),
        }
    });
    let (retirement_entered, retirement_seen) = tokio::sync::oneshot::channel();
    let (retirement_release, retirement_resume) = tokio::sync::oneshot::channel();
    if receipt_mode == 2 {
        build = build.test_hold_retirement(retirement_entered, retirement_resume);
    }
    let socket = session
        .test_begin_socket(&current, &f.supervisor, build)
        .unwrap();
    working.await.unwrap();
    if drop_owner {
        drop(session);
        // Session 已消失，service 仍必须保管原网络任务和在途材料。
        assert!(
            tokio::time::timeout(Duration::from_millis(25), cleanup.host.test_join_network())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        assert!(socket.wait_closed().await.is_ok());
        assert!(cleanup.host.test_join_network().await);
        f.service
            .access(&f.caller, &f.request_run())
            .unwrap()
            .write_frame(b"native")
            .unwrap();
        assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
        assert!(cleanup.close());
        assert!(relay.close().await);
        return;
    }
    if late {
        session.disconnect();
        assert!(
            tokio::time::timeout(Duration::from_millis(25), socket.wait_closed())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        assert!(socket.wait_closed().await.is_ok());
        assert!(socket.wait_admitted().await.is_err());
    } else {
        ready.await.unwrap();
        let (peer, admitted) = tokio::join!(network_client(&relay, scope), socket.wait_admitted());
        assert!(admitted.is_ok(), "{admitted:?}");
        let (client, mux, cancel) = peer.unwrap();
        let snapshot = client
            .snapshot(
                br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(snapshot.stamp.target.run, f.request_run().run_id);
        let budget = Duration::from_secs(1);
        let ack = projection::encode_ack(
            &projection::Ack {
                profile: 1,
                stamp: snapshot.stamp,
            },
            1,
        )
        .unwrap();
        client.exchange(&ack, budget).await.unwrap();
        let window = client
            .exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, budget)
            .await
            .unwrap();
        let wire::Message::WindowGranted {
            window,
            not_after_host_ms,
            ..
        } = wire::decode(&window).unwrap()
        else {
            panic!("window");
        };
        let mut token = [0; 32];
        for (index, pair) in window.as_bytes().chunks_exact(2).enumerate() {
            token[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let frame = input::encode(&input::Input {
            operation: [0x51; 16],
            sequence: 1,
            window: token,
            not_after_host_ms: not_after_host_ms.parse().unwrap(),
            bytes: b"nat".to_vec(),
        })
        .unwrap();
        let receipt = client.exchange(&frame, budget).await.unwrap();
        assert!(matches!(
            wire::decode(&receipt),
            Ok(wire::Message::InputReceipt {
                outcome: wire::Outcome::HostWritten,
                ..
            })
        ));
        session.disconnect();
        if receipt_mode > 0 {
            use crate::remote_host::delegated::retirement::RetirementObservation;
            if receipt_mode == 2 {
                retirement_seen.await.unwrap();
                assert!(tokio::time::timeout(
                    Duration::from_millis(25),
                    session.observe_retirement()
                )
                .await
                .is_err());
                retirement_release.send(()).unwrap();
            }
            if receipt_mode == 3 {
                pump_seen.await.unwrap();
                assert!(tokio::time::timeout(
                    Duration::from_millis(25),
                    session.observe_retirement()
                )
                .await
                .is_err());
                pump_release.send(()).unwrap();
            }
            let (first, second) =
                tokio::join!(session.observe_retirement(), session.observe_retirement());
            assert_eq!(first, RetirementObservation::Ready);
            assert_eq!(second, RetirementObservation::Ready);
        }
        assert!(session.test_join_pump().await);
        assert!(socket.wait_closed().await.is_ok());
        assert!(client
            .snapshot(
                br#"{"message":"Snapshot","profile":1,"request":"2"}"#,
                Duration::from_secs(1)
            )
            .await
            .is_err());
        let _ = client.close().await;
        cancel.cancel();
        mux.request_close();
        assert!(mux.wait_closed().await.is_ok());
    }
    assert!(session
        .test_prepare(&current, &f.supervisor, false)
        .await
        .is_err());
    f.service
        .access(&f.caller, &f.request_run())
        .unwrap()
        .write_frame(if late { b"native" } else { b"ive" })
        .unwrap();
    assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
    drop(session);
    assert!(cleanup.close());
    assert!(relay.close().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedSocket_ReservedShutdownCannotStartWorker_003() {
    socket_failure_case(0).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedSocket_PublishedBeforeGoShutdownCannotStartWorker_004() {
    socket_failure_case(1).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedSocket_MaterialSpawnFailureStaysUnknown_005() {
    socket_failure_case(2).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedSocket_MaterialPanicStaysUnknown_006() {
    socket_failure_case(3).await;
}
async fn socket_failure_case(mode: u8) {
    use crate::remote_host_network::delegated::TestMaterialBuilder;
    let (f, mut cleanup) = setup();
    let host = cleanup.host.clone();
    let source = admit(&f);
    let (consent, current, _) = NativeDelegationConsent::test_network(&source, device());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let session = Arc::new(host.reserve_delegation(owner).unwrap());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let called = calls.clone();
    let mut build = TestMaterialBuilder::new(move |_| {
        called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        panic!("TEST_ONLY material panic");
    });
    if mode == 1 {
        let original = host.clone();
        build = build.test_before_go(move || {
            original.request_exit(71, false);
        });
    }
    if mode == 2 {
        build = build.test_fail_spawn();
    }
    let socket = if mode == 0 {
        session.test_begin_socket_with_hook(&current, &f.supervisor, build, |_| {
            host.request_exit(70, false);
        })
    } else {
        session.test_begin_socket(&current, &f.supervisor, build)
    };
    if mode == 0 {
        assert!(socket.is_err());
        assert!(host.test_join_network().await);
    } else {
        let launch_failed = socket.is_err();
        if mode != 1 {
            assert!(!launch_failed);
        }
        let socket = session.test_socket().unwrap();
        assert!(socket.wait_admitted().await.is_err());
        let expected_known = mode == 1 && !launch_failed;
        assert_eq!(socket.wait_closed().await.is_ok(), expected_known);
        assert_eq!(socket.wait_closed().await.is_ok(), expected_known);
        assert_eq!(host.test_join_network().await, expected_known);
    }
    if mode >= 2 {
        use crate::remote_host::delegated::retirement::RetirementObservation;
        assert_eq!(
            session.observe_retirement().await,
            RetirementObservation::Unknown
        );
        assert_eq!(
            session.observe_retirement().await,
            RetirementObservation::Unknown
        );
    }
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        usize::from(mode == 3)
    );
    assert!(session
        .test_prepare(&current, &f.supervisor, false)
        .await
        .is_err());
    f.service
        .access(&f.caller, &f.request_run())
        .unwrap()
        .write_frame(b"native")
        .unwrap();
    assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
    drop(session);
    // cleanup 仅收齐本机任务；网络 Unknown 已单独断言，不能由该结果洗掉。
    assert!(cleanup.close());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedSocket_LastSessionDropDuringStartDoesNotDeadlock_008() {
    use crate::remote_host_network::delegated::TestMaterialBuilder;
    let (f, mut cleanup) = setup();
    let host = cleanup.host.clone();
    let source = admit(&f);
    let (consent, current, _) = NativeDelegationConsent::test_network(&source, device());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let session = Arc::new(host.reserve_delegation(owner).unwrap());
    let (socket, start) = session
        .test_reserve_socket_start(&current, &f.supervisor)
        .unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let called = calls.clone();
    let (held, holding) = std::sync::mpsc::sync_channel(0);
    let (release, resume) = std::sync::mpsc::sync_channel(0);
    let build = TestMaterialBuilder::new(move |_| {
        called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        panic!("unexpected material construction");
    })
    .test_after_keepalive(move || {
        held.send(()).unwrap();
        resume.recv().unwrap();
    });
    let original = socket.clone();
    let worker = std::thread::spawn(move || original.start(start, build));
    holding.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(session);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !worker.is_finished() {
        assert!(Instant::now() < deadline, "last-owner Drop deadlocked");
        tokio::task::yield_now().await;
    }
    let started = worker.join().unwrap();
    let expected_known = started.is_ok();
    assert!(socket.wait_admitted().await.is_err());
    assert_eq!(socket.wait_closed().await.is_ok(), expected_known);
    assert_eq!(host.test_join_network().await, expected_known);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    f.service
        .access(&f.caller, &f.request_run())
        .unwrap()
        .write_frame(b"native")
        .unwrap();
    assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
    assert!(cleanup.close());
}

fn renewal_material(
    relay: &NetworkRelayFixture,
    accepted: tokio::sync::oneshot::Sender<()>,
) -> crate::remote_host_network::delegated::TestMaterialBuilder {
    use crate::remote_host_network::delegated::{TestMaterial, TestMaterialBuilder};
    let address = relay.address;
    let origin = relay.origin.clone();
    TestMaterialBuilder::new(move |scope| TestMaterial {
        address,
        relay: network_relay_identity(&origin, true),
        tunnel: [3; 16],
        local: network_local(false),
        phone_pin: network_pin(true),
        binding: cc_desk_remote_security::HostSnapshot {
            host_key_epoch: 5,
            client_key_epoch: 7,
            activation: [0x33; 32],
            incarnation: 11,
            policy_epoch: 13,
            issuer_epoch: 17,
            origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&origin).unwrap(),
        },
        browser: network_browser_profile(),
        issuer: network_grants::admin(),
        principal: [4; 32],
        policy: cc_desk_remote_security::NativeGrantPolicy {
            scope,
            role: cc_desk_remote_record::grant::DataRole::Controller,
            duration: Duration::from_secs(10),
        },
        accepted: Some(Box::new((accepted, None))),
    })
}

async fn renewal_input(
    client: &NativeClient,
    bytes: &[u8],
    sequence: u64,
    expected: wire::Outcome,
    settle: bool,
) -> Vec<u8> {
    let budget = Duration::from_secs(1);
    let snapshot = client
        .snapshot(
            br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
            budget,
        )
        .await
        .unwrap();
    let ack = projection::encode_ack(
        &projection::Ack {
            profile: 1,
            stamp: snapshot.stamp,
        },
        1,
    )
    .unwrap();
    client.exchange(&ack, budget).await.unwrap();
    let window = client
        .exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, budget)
        .await
        .unwrap();
    let wire::Message::WindowGranted {
        window,
        not_after_host_ms,
        ..
    } = wire::decode(&window).unwrap()
    else {
        panic!("window")
    };
    let mut token = [0; 32];
    for (index, pair) in window.as_bytes().chunks_exact(2).enumerate() {
        token[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    let frame = input::encode(&input::Input {
        operation: [0x60 + sequence as u8; 16],
        sequence,
        window: token,
        not_after_host_ms: not_after_host_ms.parse().unwrap(),
        bytes: bytes.to_vec(),
    })
    .unwrap();
    let result = client.exchange(&frame, budget).await.unwrap();
    let outcome = match wire::decode(&result) {
        Ok(wire::Message::InputReceipt { outcome, .. }) => outcome,
        Ok(wire::Message::InputNotAccepted { .. }) => panic!("TEST_ONLY input not accepted"),
        _ => panic!("TEST_ONLY wrong response variant"),
    };
    eprintln!("TEST_ONLY_RENEW_INPUT sequence={sequence} outcome={outcome:?}");
    assert_eq!(outcome, expected);
    if settle {
        let wire::Message::InputReceipt {
            operation,
            sequence,
            outcome: wire::Outcome::HostWritten,
            proof: Some(proof),
            ..
        } = wire::decode(&result).unwrap()
        else {
            panic!("exact written proof")
        };
        let ack = wire::encode(&wire::Message::ReceiptAck {
            profile: 1,
            request: "3".into(),
            operation,
            sequence,
            outcome: wire::Outcome::HostWritten,
            proof,
        })
        .unwrap();
        let settled = client.exchange(&ack, budget).await.unwrap();
        assert!(matches!(
            wire::decode(&settled),
            Ok(wire::Message::ReceiptSettled {
                disposition: wire::Disposition::Advanced,
                ..
            })
        ));
    }
    frame
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_TwoOriginalSocketGenerationsOneProducer_001() {
    if isolated("DelegatedRenewal_TwoOriginalSocketGenerationsOneProducer_001") {
        return;
    }
    renewal_case(0).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_RevokeAfterReceiptCannotConsume_002() {
    if isolated("DelegatedRenewal_RevokeAfterReceiptCannotConsume_002") {
        return;
    }
    renewal_case(1).await;
}
async fn renewal_case(mode: u8) {
    use crate::remote_host::delegated::retirement::RetirementObservation;
    let (f, mut cleanup) = setup();
    let mut relay = NetworkRelayFixture::start().await;
    let source = admit(&f);
    let scope = source.scope().unwrap();
    let (consent, mut current, policy) = NativeDelegationConsent::test_network(&source, device());
    policy.test_network_origin(&mut current, relay.origin.clone());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let session = Arc::new(cleanup.host.reserve_delegation(owner).unwrap());
    let (accepted, ready) = tokio::sync::oneshot::channel();
    let first = session
        .test_begin_socket(&current, &f.supervisor, renewal_material(&relay, accepted))
        .unwrap();
    ready.await.unwrap();
    let (peer, admitted) =
        tokio::join!(network_client(&relay, scope.clone()), first.wait_admitted());
    assert!(admitted.is_ok());
    let (client, mux, cancel) = peer.unwrap();
    let old_frame = renewal_input(&client, b"na", 1, wire::Outcome::HostWritten, mode != 4).await;
    let old_client = client;
    assert!(session.prepare_next(&current, &f.supervisor).is_err());
    session.disconnect();
    assert_eq!(
        session.observe_retirement().await,
        RetirementObservation::Ready
    );
    let _ = old_client.close().await;
    cancel.cancel();
    mux.request_close();
    assert!(mux.wait_closed().await.is_ok());
    if mode == 1 {
        session.revoke();
        assert!(session.prepare_next(&current, &f.supervisor).is_err());
    } else if mode == 8 {
        let (_custody, mut start) = session.prepare_next(&current, &f.supervisor).unwrap();
        let mut wrong_scope = scope.clone();
        wrong_scope.run.push_str("-wrong");
        // 仅失败注入用 duplex；它不是第二代 socket/material join 的替代证据。
        let (client, channel, _gate, _cancel) =
            channels::channels_with_clock(wrong_scope, start.clock()).await;
        let channel = Arc::new(channel);
        assert!(start.handoff(channel.clone()).is_err());
        assert_eq!(
            session.observe_retirement().await,
            RetirementObservation::Unknown
        );
        assert_eq!(
            session.observe_retirement().await,
            RetirementObservation::Unknown
        );
        assert!(session.prepare_next(&current, &f.supervisor).is_err());
        client.authority().request_close();
        channel.authority().request_close();
        let _ = client.wait_closed_checked().await;
        let _ = channel.wait_closed_checked().await;
    } else if mode >= 5 {
        if mode == 7 {
            assert!(session
                .prepare_next(&current.test_wrong_device(), &f.supervisor)
                .is_err());
        } else {
            if mode == 5 {
                policy.test_expire();
            } else {
                policy.test_exhaust_sequence();
            }
            assert!(session.prepare_next(&current, &f.supervisor).is_err());
        }
        assert!(policy.test_owner_gate_closed());
        assert!(policy.begin(&current).is_err());
    } else if (2..=3).contains(&mode) {
        let (entered, seen) = std::sync::mpsc::sync_channel(0);
        let (release, resume) = std::sync::mpsc::sync_channel(0);
        session.test_hold_renew(mode == 3, move || {
            entered.send(()).unwrap();
            resume.recv().unwrap();
        });
        std::thread::scope(|threads| {
            let worker = threads.spawn(|| session.prepare_next(&current, &f.supervisor));
            seen.recv().unwrap();
            policy.request_revoke();
            release.send(()).unwrap();
            assert!(worker.join().unwrap().is_err());
        });
        assert!(session.prepare_next(&current, &f.supervisor).is_err());
        assert!(policy.begin(&current).is_err());
    } else {
        // 两个消费者竞争同一 private receipt，只有一个能取得新 Starting generation。
        let (a, b) = std::thread::scope(|threads| {
            let a = threads.spawn(|| session.prepare_next(&current, &f.supervisor));
            let b = threads.spawn(|| session.prepare_next(&current, &f.supervisor));
            (a.join().unwrap(), b.join().unwrap())
        });
        assert_ne!(a.is_ok(), b.is_ok());
        let (second, start) = a.or(b).unwrap();
        assert_eq!(
            session.observe_retirement().await,
            RetirementObservation::Pending
        );
        assert_eq!(
            session.observe_retirement().await,
            RetirementObservation::Pending
        );
        let (accepted, ready) = tokio::sync::oneshot::channel();
        second
            .start(start, renewal_material(&relay, accepted))
            .unwrap();
        // 旧 custody 的迟到关闭不得转成整个 Session 的新代关闭。
        first.request_close();
        ready.await.unwrap();
        let (peer, admitted) = tokio::join!(network_client(&relay, scope), second.wait_admitted());
        assert!(admitted.is_ok(), "{admitted:?}");
        let (client, mux, cancel) = peer.unwrap();
        assert!(old_client
            .exchange(&old_frame, Duration::from_secs(1))
            .await
            .is_err());
        if mode == 4 {
            // 独立负例：冲突 Unknown 后不再在该 channel 提交任何新输入。
            renewal_input(&client, b"ti", 1, wire::Outcome::Unknown, false).await;
        } else {
            renewal_input(&client, b"ti", 2, wire::Outcome::HostWritten, true).await;
        }
        assert_eq!(cleanup.host.test_pin_count(), 1);
        session.disconnect();
        assert!(session.test_join_pump().await);
        assert!(second.wait_closed().await.is_ok());
        let _ = client.close().await;
        cancel.cancel();
        mux.request_close();
        assert!(mux.wait_closed().await.is_ok());
    }
    f.service
        .access(&f.caller, &f.request_run())
        .unwrap()
        .write_frame(if mode != 0 { b"tive" } else { b"ve" })
        .unwrap();
    assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
    drop(session);
    assert!(cleanup.close());
    assert!(relay.close().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedReceipt_CancelObserverRetainsOriginalPeerPumpJoin_003() {
    if isolated("DelegatedReceipt_CancelObserverRetainsOriginalPeerPumpJoin_003") {
        return;
    }
    socket_case(false, false, 3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedReceipt_LateOriginalBindCannotMintInstalledReceipt_004() {
    if isolated("DelegatedReceipt_LateOriginalBindCannotMintInstalledReceipt_004") {
        return;
    }
    use crate::remote_host::delegated::retirement::RetirementObservation;
    let (f, mut cleanup) = setup();
    let mut relay = NetworkRelayFixture::start().await;
    let source = admit(&f);
    let scope = source.scope().unwrap();
    let (consent, mut current, policy) = NativeDelegationConsent::test_network(&source, device());
    policy.test_network_origin(&mut current, relay.origin.clone());
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let session = Arc::new(cleanup.host.reserve_delegation(owner).unwrap());
    let (entered, seen) = tokio::sync::oneshot::channel();
    let (release, resume) = std::sync::mpsc::sync_channel(0);
    session.test_hold_bind(move || {
        entered.send(()).unwrap();
        resume.recv().unwrap();
    });
    let (accepted, ready) = tokio::sync::oneshot::channel();
    let socket = session
        .test_begin_socket(&current, &f.supervisor, renewal_material(&relay, accepted))
        .unwrap();
    ready.await.unwrap();
    let (peer, retired) = {
        let peer = network_client(&relay, scope);
        tokio::pin!(peer);
        tokio::select! {result=seen=>result.unwrap(),_=&mut peer=>panic!("original actor must remain pending")};
        assert!(
            tokio::time::timeout(Duration::from_millis(25), session.observe_retirement())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        tokio::join!(&mut peer, session.observe_retirement())
    };
    assert!(peer.is_err());
    assert_eq!(retired, RetirementObservation::Unknown);
    assert_eq!(
        session.observe_retirement().await,
        RetirementObservation::Unknown
    );
    assert!(session.prepare_next(&current, &f.supervisor).is_err());
    let _ = socket.wait_closed().await;
    f.service
        .access(&f.caller, &f.request_run())
        .unwrap()
        .write_frame(b"native")
        .unwrap();
    assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
    drop(session);
    assert!(cleanup.close());
    assert!(relay.close().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_RevokeAtLockedPreCommitClosesOwner_003() {
    if isolated("DelegatedRenewal_RevokeAtLockedPreCommitClosesOwner_003") {
        return;
    }
    renewal_case(2).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_RevokeAfterSelectionNeverRestoresOldGeneration_004() {
    if isolated("DelegatedRenewal_RevokeAfterSelectionNeverRestoresOldGeneration_004") {
        return;
    }
    renewal_case(3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_OriginalOperationConflictNeverReplays_005() {
    if isolated("DelegatedRenewal_OriginalOperationConflictNeverReplays_005") {
        return;
    }
    renewal_case(4).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_AbsoluteExpiryRejectsReceipt_006() {
    if isolated("DelegatedRenewal_AbsoluteExpiryRejectsReceipt_006") {
        return;
    }
    renewal_case(5).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_SequenceExhaustionClosesGateBeforeUnlock_007() {
    if isolated("DelegatedRenewal_SequenceExhaustionClosesGateBeforeUnlock_007") {
        return;
    }
    renewal_case(6).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_WrongDeviceCannotConsumeAsNewAuthority_008() {
    if isolated("DelegatedRenewal_WrongDeviceCannotConsumeAsNewAuthority_008") {
        return;
    }
    renewal_case(7).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn DelegatedRenewal_RejectedRebindIsUnknownNotPerpetualStarting_009() {
    if isolated("DelegatedRenewal_RejectedRebindIsUnknownNotPerpetualStarting_009") {
        return;
    }
    renewal_case(8).await;
}
