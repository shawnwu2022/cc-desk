//! TEST_ONLY：真实 BrowserWasm 客户端连接原生原始注册 owner。
use super::*;
use crate::remote_host_controller::NativeRegistrationSession;
use cc_desk_remote_security::registration_channel::registration_accept_until;
use cc_desk_remote_security::{cancellation, Unbound};
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "TEST_ONLY requires actual Edge Worker / WASM client"]
async fn NativeRegistration_RelayOracle_903() {
    let mode = std::env::var("CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_MODE").unwrap();
    assert!([
        "yes",
        "deny",
        "document",
        "cancel",
        "native-cancel",
        "native-deadline"
    ]
    .contains(&mode.as_str()));
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
    let mut relay = registration_relay_fixture().await;
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
    let (cancel, token) = cancellation();
    let original_started = tokio::time::Instant::now();
    let original_setup_end = original_started + Duration::from_secs(10);
    let original_end =
        original_started + Duration::from_secs(if mode == "native-deadline" { 4 } else { 60 });
    let ws = cc_desk_remote_transport::wss::test_only_connect_admitted_loopback_with_mode(
        &relay.origin,
        relay.address,
        Duration::from_secs(5),
        &network_relay_identity(&relay.origin, true),
        cc_desk_remote_transport::wss::RelayMode::Legacy,
        [3; 16],
    )
    .await
    .unwrap();
    let mut relay_token = token.clone();
    let (io, transport) = cc_desk_remote_transport::start_admitted_registration(
        ws,
        [3; 16],
        Duration::from_secs(30),
        original_end,
        async move { relay_token.cancelled().await },
    )
    .unwrap();
    let retirement = transport.test_only_retirement();
    let cancellation = if mode == "native-cancel" {
        let handle = cancel.clone();
        Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            handle.cancel();
        }))
    } else {
        None
    };
    let (_, host_spki) = network_identities::identity(false, false);
    emit(
        &serde_json::to_vec(
            &serde_json::json!({"kind":"ready","origin":relay.origin,"hostSpki":host_spki}),
        )
        .unwrap(),
    );
    let pending = registration_accept_until(
        Unbound::new(io),
        network_local(false),
        PeerPin::new(Role::Client, key(1)),
        original_setup_end,
        original_end,
        token,
    )
    .await;
    let mut installed = false;
    let mut acknowledged = false;
    let mut receipt = None;
    if ["cancel", "native-cancel", "native-deadline"].contains(&mode.as_str()) {
        if let Ok(pending) = pending {
            let channel = pending.into_channel().unwrap();
            assert_eq!(channel.deadline(), original_end);
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
        assert_eq!(channel.deadline(), original_end);
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
    assert_eq!(controller.test_has_configuration(), installed);
    if mode != "document" {
        assert_eq!(
            controller
                .current(&binding, &fixture.caller, &admission)
                .unwrap(),
            HostControlReply::Locked
        );
    }
    // TLS owner 释放双工 IO 后，自然等待原始 driver 完整退出。
    transport.wait_closed().await.unwrap();
    assert!(retirement.known_joined());
    if let Some(task) = cancellation {
        task.await.unwrap();
    }
    let elapsed_ms = original_started.elapsed().as_millis();
    if mode == "native-deadline" {
        assert!((3900..7000).contains(&elapsed_ms));
    }
    drop(controller);
    drop(admission);
    assert!(cleanup.close());
    emit(&serde_json::to_vec(&serde_json::json!({"kind":"finished","mode":mode,"nativeCandidateInstalled":installed,"locked":true,"hostNetworkAttempts":0,"hostKeyOpens":0,"registrationTlsConnections":1,"actualRelay":true,"relayMode":"explicit-legacy","nativeTransportRetired":true,"nativeElapsedMs":elapsed_ms,"acknowledged":acknowledged,"receipt":receipt,"genuineGuiApproval":false,"persistentTrust":false})).unwrap());
    // 浏览器确认严格协议退役之前，保留测试基础设施。
    let mut cleanup_permission = String::new();
    std::io::stdin().read_line(&mut cleanup_permission).unwrap();
    assert_eq!(cleanup_permission.trim(), "TEST_ONLY_BROWSER_RETIRED");
    assert!(relay.close().await);
}

// 复用公开 fixture 密钥与已校验 CA/名称，限定浏览器来源和注册寿命。
async fn registration_relay_fixture() -> NetworkRelayFixture {
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserve.local_addr().unwrap();
    drop(reserve);
    let origin = format!("wss://foobar.com:{}/relay", address.port());
    let root = tempfile::tempdir().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let point = |host| {
        (if host {
            network_relay_material(true).1
        } else {
            key_bytes(3)[26..].to_vec()
        })
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
    };
    fs::write(root.path().join("policy.json"),serde_json::to_vec(&json!({"schema":1,"policy_id":"TEST_ONLY_NATIVE_NETWORK","revision":1,"issued_at_unix_ms":now,"not_before_unix_ms":now-1000,"expires_unix_ms":now+90_000,"listen":address.to_string(),"signed_wss_endpoint":origin,"browser_origins":["https://desk.invalid"],"devices":[{"id":"01".repeat(16),"public_key_sec1":point(true),"role":"Host"},{"id":"02".repeat(16),"public_key_sec1":point(false),"role":"Phone"}],"routes":[{"tunnel":"03".repeat(16),"host":"01".repeat(16),"phone":"02".repeat(16)}]})).unwrap()).unwrap();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let relay = std::env::var_os("CC_DESK_TEST_RELAY_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../cc-desk-relay"));
    let executable = std::env::var_os("CC_DESK_TEST_RELAY_EXE")
        .map(std::path::PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                relay.join(path)
            }
        })
        .unwrap_or_else(|| manifest.join("../target-browser-profile/debug/cc-desk-relay.exe"));
    assert!(
        relay.join("tests/fixtures/chain.pem").is_file(),
        "set CC_DESK_TEST_RELAY_ROOT to the fixture checkout"
    );
    assert!(
        executable.is_file(),
        "set CC_DESK_TEST_RELAY_EXE to the built fixture executable"
    );
    let mut child = std::process::Command::new(executable)
        .arg("--policy")
        .arg(root.path().join("policy.json"))
        .arg("--tls-chain")
        .arg(relay.join("tests/fixtures/chain.pem"))
        .arg("--tls-key")
        .arg(relay.join("tests/fixtures/end.key"))
        .arg("--tls-ca")
        .arg(relay.join("tests/fixtures/root.pem"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let reader = std::thread::spawn(move || {
        use std::io::BufRead;
        let mut line = String::new();
        let mut reader = std::io::BufReader::new(stdout);
        let _ = reader.read_line(&mut line);
        let _ = tx.send(line);
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
    });
    let mut owned = NetworkRelayFixture {
        child,
        reader: Some(reader),
        root,
        origin,
        address,
    };
    let ready = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .unwrap()
        .unwrap();
    if !ready.starts_with("WSS_LISTEN=") {
        use std::io::Read;
        let mut detail = String::new();
        owned
            .child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut detail)
            .unwrap();
        panic!("TEST_ONLY_RELAY_START {ready:?} {detail}");
    }
    owned
}
