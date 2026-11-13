//! 真实 relay + 原生 Host/Phone 链路；不代表真实手机/WebView 验收。
#![allow(non_snake_case)]
use super::*;
use cc_desk_remote_transport::wss::RelayMode;

async fn owned_start_original_node(phone_first: bool, failure: u8) {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let mut fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let url: tauri::Url = "http://tauri.localhost/".parse().unwrap();
    let document = crate::cli::document::DocumentAuthority::new(
        fixture.service.registry().clone(),
        url.clone(),
    )
    .unwrap();
    let mut resources = tauri::ResourceTable::default();
    let binding = Arc::new(document.attach(&mut resources).unwrap());
    document.started(&url);
    document.finished(&url);
    fixture.caller = document.test_caller();
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let status = fixture.start();
    let mut cleanup = Cleanup {
        supervisor: fixture.supervisor.clone(),
        host: host.clone(),
        run: status.run.clone(),
        pin: Some(fixture.supervisor.pin_shutdown(&status.run)),
        stop: None,
        proofs: Vec::new(),
    };
    let ready = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(Instant::now() < ready);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let relay_point = network_hex("046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5");
    let mut relay = NetworkRelayFixture::start_with_host_point(Some(relay_point)).await;
    let admission = fixture
        .supervisor
        .admit_remote_run(
            fixture
                .service
                .access(&fixture.caller, &status.run)
                .unwrap(),
            &status.run,
        )
        .unwrap();
    let admit = || {
        fixture
            .supervisor
            .admit_remote_run(
                fixture
                    .service
                    .access(&fixture.caller, &status.run)
                    .unwrap(),
                &status.run,
            )
            .unwrap()
    };
    let scope = admission.scope().unwrap();
    let controller = crate::remote_host_controller::RemoteHostController::default();
    let opened = Arc::new([
        std::sync::atomic::AtomicUsize::new(0),
        std::sync::atomic::AtomicUsize::new(0),
    ]);
    let clock_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = clock_calls.clone();
    let mut fields = super::approval_keys::description(
        &relay,
        &admission,
        Arc::new(move || {
            let call = calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            if failure == 4 && call >= 2 {
                panic!("TEST_ONLY_BEGIN_CLOCK_PANIC");
            }
            if failure == 3 && call >= 2 {
                Err(cc_desk_remote_security::GrantError::Revoked)
            } else {
                Ok(1000)
            }
        }),
    );
    fields.relay_mode = RelayMode::RendezvousV1;
    let supplied = crate::native_crypto::test_existing_host_supplier(
        cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin).unwrap(),
        Instant::now() + Duration::from_secs(60),
        opened.clone(),
    );
    let frozen = crate::remote_host_approval::FrozenHostApproval::supplied(
        fields,
        &admission,
        Instant::now() + Duration::from_secs(60),
        supplied,
    )
    .unwrap();
    controller
        .install_frozen_configuration(&binding, fixture.caller.clone(), &admission, frozen)
        .unwrap();
    assert!(controller
        .test_start_owned(
            &binding,
            &fixture.caller,
            admit(),
            runtime.clone(),
            host.clone()
        )
        .is_err());
    assert!(host.test_activation_observation().is_none());
    controller
        .test_request_frozen(&binding, &fixture.caller, &admission, true)
        .unwrap();
    let owner = controller.test_approval_owner();
    let token = owner.test_native_token();
    let mut foreign = fixture.caller.clone();
    foreign.window_label.push_str("-FOREIGN");
    assert!(controller
        .test_start_owned(&binding, &foreign, admit(), runtime.clone(), host.clone())
        .is_err());
    assert!(!token.is_revoked() && host.test_activation_observation().is_none());
    clock_calls.store(0, std::sync::atomic::Ordering::Release);
    let (at_gate, gate_entered) = tokio::sync::oneshot::channel();
    let (release_gate, held_gate) = std::sync::mpsc::channel();
    if failure == 1 || failure == 2 {
        owner.test_before_network(Box::new(move || {
            let _ = at_gate.send(());
            held_gate.recv_timeout(Duration::from_secs(5)).unwrap();
        }));
    } else {
        drop(at_gate);
    }

    let (completed, completion) = tokio::sync::oneshot::channel();
    controller.test_driver_hooks(crate::remote_host::driver::DriverHooks {
        after_work: Some(Box::new(move || {
            let _ = completed.send(());
        })),
        ..Default::default()
    });
    let mut peer = None;
    let mut attempt = None;
    let outcome: Result<(), &'static str> = async {
        if failure != 0 && failure != 5 {
            controller.test_start_owned(&binding, &fixture.caller, admit(), runtime.clone(), host.clone()).map_err(|_| "negative start rejected too early")?;
            if failure == 1 || failure == 2 {
                tokio::time::timeout(Duration::from_secs(5), gate_entered).await.map_err(|_| "gate timeout")?.map_err(|_| "gate lost")?;
                if failure == 1 { controller.cancel(&binding, &fixture.caller, &admission).map_err(|_| "cancel")?; }
                else { host.test_begin_host_shutdown(); }
                release_gate.send(()).map_err(|_| "gate release")?;
            }
            tokio::time::timeout(Duration::from_secs(5), completion).await.map_err(|_| "negative completion timeout")?.map_err(|_| "negative completion lost")?;
            if host.test_network_attempts() != 0 || !token.is_revoked() { return Err("failed begin retained authority or started network"); }
            if controller.test_start_owned(&binding, &fixture.caller, admit(), runtime.clone(), host.clone()).is_ok() { return Err("failed begin retried"); }
            return Ok(());
        }
        let phone = network_client_mode(&relay, scope, RelayMode::RendezvousV1);
        tokio::pin!(phone);
        if phone_first {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(350)) => {},
                result = &mut phone => { peer = Some(result?); return Err("Phone started inner channel without Host"); }
            }
        }
        controller.test_start_owned(&binding, &fixture.caller, admission, runtime.clone(), host.clone()).map_err(|_| "owned start rejected")?;
        tokio::time::timeout(Duration::from_secs(5), completion).await.map_err(|_| "driver completion timeout")?.map_err(|_| "driver completion lost")?;
        if controller.test_start_owned(&binding, &fixture.caller, admit(), runtime.clone(), host.clone()).is_ok() { return Err("duplicate start accepted"); }
        if token.is_revoked() || host.test_network_attempts() != 1 { return Err("duplicate changed original attempt"); }
        attempt = host.test_original_attempt();
        if attempt.is_none() { return Err("owned start did not publish original network attempt"); }
        if failure == 5 {
            let refused = tokio::time::timeout(Duration::from_secs(10), attempt.as_ref().unwrap().wait_admitted()).await.map_err(|_| "original setup did not finish")?;
            if refused.is_ok() || !token.is_revoked() { return Err("async failure retained authority"); }
            if controller.test_start_owned(&binding, &fixture.caller, admit(), runtime.clone(), host.clone()).is_ok() { return Err("async failure retried"); }
            return Ok(());
        }
        let (connected, admitted) = tokio::join!(&mut phone, attempt.as_ref().unwrap().wait_admitted());
        peer = Some(connected?);
        admitted.map_err(|_| "rendezvous handoff")?;
        let snapshot = peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#, Duration::from_secs(1)).await.map_err(|_| "original snapshot")?;
        if snapshot.stamp.target.run != status.run.run_id || host.test_pin_count() != 1 { return Err("wrong native owner"); }
        use cc_desk_remote_record::{control_io as wire, projection};
        let mut grid = String::new();
        for bytes in &snapshot.parts {
            let projection::Content::SnapshotPart(part) = projection::decode(bytes, 1).map_err(|_| "projection codec")?.content else { return Err("snapshot part"); };
            grid.extend(part.cells.iter().map(|cell| cell.scalar));
        }
        if !grid.contains("OWNED_READY") { return Err("original Node output absent"); }
        let ack = projection::encode_ack(&projection::Ack { profile: 1, stamp: snapshot.stamp.clone() }, 1).map_err(|_| "ACK codec")?;
        let reply = peer.as_ref().unwrap().0.exchange(&ack, Duration::from_secs(1)).await.map_err(|_| "ACK")?;
        if !matches!(wire::decode(&reply), Ok(wire::Message::ProjectionAcknowledged { stamp, .. }) if stamp == snapshot.stamp) { return Err("wrong ACK identity"); }
        Ok(())
    }.await;
    if let Some(attempt) = &attempt {
        attempt.request_close();
    }
    let phone_known = if let Some((client, mux, cancel)) = peer {
        cancel.cancel();
        mux.request_close();
        let (closed, mux_closed) = tokio::join!(client.close(), mux.wait_closed());
        closed != cc_desk_remote_security::DataError::Uncertain && mux_closed.is_ok()
    } else {
        true
    };
    if let Some(attempt) = &attempt {
        let _ = attempt.wait_closed().await;
    }
    let known = host.test_join_network().await;
    controller.begin_shutdown();
    let activation_known =
        tokio::time::timeout(Duration::from_secs(5), host.test_retire_activation())
            .await
            .unwrap();
    let physical = cleanup.close();
    let relay_closed = relay.close().await;
    eprintln!("TEST_ONLY_RENDEZVOUS phone_first={phone_first} outcome={outcome:?} original_network_joined={known} physical={physical} relay_closed={relay_closed}");
    assert_eq!(
        known,
        failure != 4,
        "network Unknown remains sticky after synchronous clock panic"
    );
    assert!(physical && relay_closed && phone_known && activation_known);
    assert_eq!(outcome, Ok(()));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn HostStart_PhonePolledFirstOriginalNode_001() {
    owned_start_original_node(true, 0).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn HostStart_HostInitiatedFirstOriginalNode_002() {
    owned_start_original_node(false, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn HostStart_CancelAfterRegistration_003() {
    owned_start_original_node(false, 1).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn HostStart_CloseAfterRegistration_004() {
    owned_start_original_node(false, 2).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn HostStart_NetworkClockRejectsOnce_005() {
    owned_start_original_node(false, 3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn HostStart_BeginClockPanicKeepsUnknown_006() {
    owned_start_original_node(false, 4).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn HostStart_AsyncNoPeerFailureCannotRetry_007() {
    owned_start_original_node(false, 5).await;
}
