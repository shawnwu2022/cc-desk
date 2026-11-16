//! TEST_ONLY: genuine BrowserWasm client connects to original native registration owner.
use super::*;
use crate::remote_host_controller::NativeRegistrationSession;
use cc_desk_remote_security::registration_channel::registration_accept;
use cc_desk_remote_security::{cancellation, Unbound};
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "TEST_ONLY requires actual Edge Worker / WASM client"]
async fn NativeRegistration_WasmOracle_902() {
    let mode = std::env::var("CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_MODE").unwrap();
    assert!(["yes", "deny", "document", "cancel"].contains(&mode.as_str()));
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
    let mut relay =
        NetworkRelayFixture::start_with_host_point(Some(key_bytes(1)[26..].to_vec())).await;
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
    let controller = RemoteHostController::default();
    let opened = Arc::new([
        std::sync::atomic::AtomicUsize::new(0),
        std::sync::atomic::AtomicUsize::new(0),
    ]);
    let context = BrowserRegistrationContext {
        origin: "https://desk.invalid".into(),
        rp_id: "desk.invalid".into(),
        user_handle: vec![7; 32],
        owner_binding: [0; 32],
        tls_key: key(1),
        grant_key: key(2),
        relay_key: key(3),
        allow_backup_eligible: false,
        allow_backed_up: false,
    };
    let owner =
        NativeRegistrationOwner::begin(&binding, &fixture.caller, &admission, context).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (_cancel, token) = cancellation();

    let (_, host_spki) = network_identities::identity(false, false);
    emit(
        &serde_json::to_vec(
            &serde_json::json!({"kind":"ready","port":address.port(),"hostSpki":host_spki}),
        )
        .unwrap(),
    );
    let (io, _) = listener.accept().await.unwrap();
    drop(listener);
    let pending = registration_accept(
        Unbound::new(io),
        network_local(false),
        PeerPin::new(Role::Client, key(1)),
        Duration::from_secs(10),
        Duration::from_secs(60),
        token,
    )
    .await;
    let mut installed = false;
    let mut acknowledged = false;
    let mut receipt = None;
    if mode == "cancel" {
        if let Ok(pending) = pending {
            let channel = pending.into_channel().unwrap();
            let mut session = NativeRegistrationSession::new(
                owner,
                channel,
                &binding,
                &fixture.caller,
                &admission,
            )
            .unwrap();
            assert!(session
                .verify(&binding, &fixture.caller, &admission)
                .await
                .is_err());
        }
    } else {
        let channel = pending.unwrap().into_channel().unwrap();
        let mut session =
            NativeRegistrationSession::new(owner, channel, &binding, &fixture.caller, &admission)
                .unwrap();
        session
            .verify(&binding, &fixture.caller, &admission)
            .await
            .unwrap();
        assert!(session
            .install(&binding, &fixture.caller, &admission, &controller)
            .is_err());
        let mut fields =
            super::super::approval_keys::description(&relay, &admission, Arc::new(|| Ok(1000)));
        fields.phone_pin = PeerPin::new(Role::Client, key(1));
        fields.issuer = key(2);
        fields.browser = BrowserProfile::enrolled(
            "https://desk.invalid",
            "desk.invalid",
            vec![7; 32],
            key(4),
            false,
            false,
        )
        .unwrap();
        let cap = Instant::now() + Duration::from_secs(60);
        let supplier = crate::native_crypto::test_existing_host_supplier(
            cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin).unwrap(),
            cap,
            opened.clone(),
        );
        let frozen = FrozenHostApproval::supplied(fields, &admission, cap, supplier).unwrap();
        let approval = session
            .test_request_approval(&binding, &fixture.caller, &admission, frozen)
            .unwrap();
        approval.test_complete(mode != "deny");
        if mode == "document" {
            document.started(&url);
            document.finished(&url);
        }
        match session.install(&binding, &fixture.caller, &admission, &controller) {
            Ok(mut delivery) => {
                installed = true;
                receipt = Some(delivery.outcome().clone());
                acknowledged = delivery.deliver().await.is_ok();
                assert!(delivery.deliver().await.is_err());
            }
            Err(_) => {
                acknowledged = session.reject().await.is_ok();
            }
        }
        assert_eq!(installed, mode == "yes");
        assert_eq!(controller.test_has_configuration(), installed);
        if mode != "document" {
            assert_eq!(
                controller
                    .current(&binding, &fixture.caller, &admission)
                    .unwrap(),
                HostControlReply::Locked
            );
        }
    }
    assert_eq!(host.test_network_attempts(), 0);
    assert_eq!(opened[0].load(std::sync::atomic::Ordering::Acquire), 0);
    assert_eq!(opened[1].load(std::sync::atomic::Ordering::Acquire), 0);
    drop(controller);
    drop(admission);
    assert!(cleanup.close());
    assert!(relay.close().await);
    emit(&serde_json::to_vec(&serde_json::json!({"kind":"finished","mode":mode,"nativeCandidateInstalled":installed,"locked":true,"hostNetworkAttempts":0,"hostKeyOpens":0,"registrationTlsConnections":1,"acknowledged":acknowledged,"receipt":receipt,"genuineGuiApproval":false,"persistentTrust":false})).unwrap());
}
