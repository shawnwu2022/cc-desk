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
