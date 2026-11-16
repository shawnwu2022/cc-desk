//! 真实 relay + 原生 Host/Phone 链路；不代表真实手机/WebView 验收。
#![allow(non_snake_case)]
use super::*;
use crate::remote_host_network::*;
use cc_desk_remote_transport::wss::RelayMode;

async fn rendezvous_original_node(phone_first: bool) {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
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
    let mut relay = NetworkRelayFixture::start().await;
    let (_source_watch, revoked) = tokio::sync::watch::channel(false);
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(|| Ok(1000)),
        revoked,
        expires_host_ms: 11000,
    });
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
    let scope = admission.scope().unwrap();
    let (accepted, mut ready_gate) = tokio::sync::oneshot::channel();
    let mut inputs = network_inputs(&relay, scope.clone(), source);
    inputs.relay_mode = RelayMode::RendezvousV1;
    inputs.outer_accepted = Some(Box::new((accepted, None)));
    let (approval, materials) =
        crate::remote_host_approval::NativeHostApproval::test_from_inputs(inputs, &admission, true);
    assert!(approval
        .register_once(&runtime, &admission, &host, materials)
        .is_ok());
    let mut peer = None;
    let mut attempt = None;
    let outcome: Result<(), &'static str> = async {
        let phone = network_client_mode(&relay, scope, RelayMode::RendezvousV1);
        tokio::pin!(phone);
        if phone_first {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(350)) => {},
                result = &mut phone => { peer = Some(result?); return Err("Phone started inner channel without Host"); }
            }
        }
        attempt = Some(host.test_begin_registered_network(runtime.clone(), admission).map_err(|_| "registered rendezvous begin")?);
        if !phone_first {
            // 此探针位于 connect 返回后、Endpoint arm/mux 前；等待不会消耗 IO 预算。
            if tokio::time::timeout(Duration::from_millis(350), &mut ready_gate).await.is_ok() { return Err("Host released endpoint before matching Phone"); }
        }
        let (connected, admitted) = tokio::join!(&mut phone, attempt.as_ref().unwrap().wait_admitted());
        peer = Some(connected?);
        admitted.map_err(|_| "rendezvous handoff")?;
        ready_gate.await.map_err(|_| "matching Ready gate")?;
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
    let physical = cleanup.close();
    let relay_closed = relay.close().await;
    eprintln!("TEST_ONLY_RENDEZVOUS phone_first={phone_first} outcome={outcome:?} original_network_joined={known} physical={physical} relay_closed={relay_closed}");
    assert!(known && physical && relay_closed && phone_known);
    assert_eq!(outcome, Ok(()));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn RelayRendezvous_PhoneFirstOriginalNode_001() {
    rendezvous_original_node(true).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn RelayRendezvous_HostFirstOriginalNode_002() {
    rendezvous_original_node(false).await;
}

async fn rendezvous_no_peer(cancel: bool) {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
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
    let mut relay = NetworkRelayFixture::start().await;
    let (_watch, revoked) = tokio::sync::watch::channel(false);
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(|| Ok(1000)),
        revoked,
        expires_host_ms: 11000,
    });
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
    let (accepted, ready_gate) = tokio::sync::oneshot::channel();
    let (terminated, ending) = tokio::sync::oneshot::channel();
    let mut inputs = network_inputs(&relay, admission.scope().unwrap(), source);
    inputs.relay_mode = RelayMode::RendezvousV1;
    inputs.setup_budget = if cancel {
        Duration::from_secs(8)
    } else {
        Duration::from_millis(250)
    };
    inputs.outer_accepted = Some(Box::new((accepted, None)));
    inputs.termination = Some(terminated);
    let attempt = host
        .test_begin_admitted_network(runtime, admission, inputs)
        .unwrap();
    if cancel {
        tokio::time::sleep(Duration::from_millis(100)).await;
        attempt.request_close();
    }
    let result = tokio::time::timeout(Duration::from_secs(2), attempt.wait_closed()).await;
    attempt.request_close();
    let _ = attempt.wait_closed().await;
    let admitted = attempt.wait_admitted().await;
    let end = ending.await.unwrap();
    let endpoint_never_released = ready_gate.await.is_err();
    let known = host.test_join_network().await;
    let physical = cleanup.close();
    let relay_closed = relay.close().await;
    assert!(known && physical && relay_closed);
    assert!(matches!(result, Ok(Ok(()))));
    assert!(endpoint_never_released && end.mux.is_none());
    // 当前探针将交接前取消与超时都归为 Setup；结果错误区分实际原因。
    assert_eq!(end.cause, TestAttemptCause::Setup);
    assert_eq!(
        admitted,
        Err(if cancel {
            HostNetworkError::Closed
        } else {
            HostNetworkError::Deadline
        })
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn RelayRendezvous_MissingPeerOriginalDeadline_003() {
    rendezvous_no_peer(false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn RelayRendezvous_CancelBeforeReadyNoMux_004() {
    rendezvous_no_peer(true).await;
}
