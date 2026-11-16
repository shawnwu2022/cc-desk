//! TEST_ONLY：真实 BrowserWasm 客户端连接原生原始注册 owner。
use super::*;
use crate::remote_host_controller::NativeRegistrationSession;
use cc_desk_remote_security::registration_channel::registration_accept_until;
use cc_desk_remote_security::{cancellation, Unbound};
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "TEST_ONLY requires actual Edge Worker / WASM client"]
async fn NativeRegistration_RelayOracle_903() {
    let mode = std::env::var("CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_MODE").unwrap();
    let (relay_mode, browser_relay_mode) = registration_relay_mode();
    assert!([
        "yes",
        "deny",
        "document",
        "cancel",
        "native-cancel",
        "native-deadline",
        "install",
        "install-deny",
        "install-document",
        "install-expired",
        "remember",
        "remember-deny",
        "remember-document",
        "remember-expired",
        "remember-target"
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
    let (_, host_spki) = network_identities::identity(false, false);
    let mut fields =
        super::super::approval_keys::description(&relay, &admission, Arc::new(|| Ok(1000)));
    fields.phone_pin = PeerPin::new(Role::Client, key(1));
    fields.issuer = key(2);
    fields.relay_mode = relay_mode;
    // TEST_ONLY 独立配置在收到任何新 credential 之前冻结，不能从登记 outcome 反推。
    let plan = {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
        serde_json::json!({"databaseName":"TEST_ONLY.registration-chain","expectedRevision":null,"relaySpki":B64.encode(key_bytes(3)),"requestedUserHandle":B64.encode([7;32]),"configuration":{
            "version":1,"profile":{"origin":"https://desk.invalid","rpId":"desk.invalid"},"localSpki":B64.encode(key_bytes(1)),"hostSpki":B64.encode(&host_spki),
            "trust":{"hostKeyEpoch":fields.binding.host_key_epoch.to_string(),"clientKeyEpoch":fields.binding.client_key_epoch.to_string(),"origin":fields.binding.origin.as_str()},
            "grant":{"issuerSpki":B64.encode(key_bytes(2)),"principal":fields.principal.iter().map(|v|format!("{v:02x}")).collect::<String>(),"scope":fields.policy.scope,"role":fields.policy.role,"budgetMs":fields.policy.duration.as_millis() as u64,"peerInstall":{"budgetMs":5000},"profile":{"origin":"https://desk.invalid","rpId":"desk.invalid","allowBackedUp":false}},
            "relay":{"mode":browser_relay_mode,"endpoint":fields.binding.origin.as_str(),"tunnel":B64.encode(fields.tunnel),"keyId":B64.encode([2;16])},"artifacts":{"wasm":"https://desk.invalid/fixture_bg.wasm","glue":"https://desk.invalid/fixture_bg.js"}
        }})
    };
    let ready_message = serde_json::to_vec(
        &serde_json::json!({"kind":"ready","origin":relay.origin,"hostSpki":host_spki,"plan":plan}),
    )
    .unwrap();
    // 会合等待双方 Join；旧协议必须保持 Host 先准入，避免 Browser 首帧遇到 Offline。
    if relay_mode == cc_desk_remote_transport::wss::RelayMode::RendezvousV1 {
        emit(&ready_message);
    }
    let ws = cc_desk_remote_transport::wss::test_only_connect_admitted_loopback_with_mode(
        &relay.origin,
        relay.address,
        Duration::from_secs(5),
        &network_relay_identity(&relay.origin, true),
        relay_mode,
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
    if relay_mode == cc_desk_remote_transport::wss::RelayMode::Legacy {
        emit(&ready_message);
    }
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
    let mut native_record_written = false;
    let mut native_reload_locked = false;
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
        let verified = session
            .verified_binding(&binding, &fixture.caller, &admission)
            .unwrap();
        fields.browser = BrowserProfile::enrolled(
            verified.origin,
            verified.rp_id,
            verified.credential_id.to_vec(),
            verified.credential_key.clone(),
            verified.backup_eligible,
            verified.allow_backed_up,
        )
        .unwrap();
        let cap = Instant::now()
            + if mode == "install-expired" || mode == "remember-expired" {
                Duration::from_millis(200)
            } else {
                Duration::from_secs(60)
            };
        let supplier = crate::native_crypto::test_existing_host_supplier(
            cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin).unwrap(),
            cap,
            opened.clone(),
        );
        let frozen = FrozenHostApproval::supplied(fields, &admission, cap, supplier).unwrap();
        let frozen = if mode.starts_with("remember") {
            use crate::remote_native_pairing_store::{
                tests::{reopen_synthetic_file_store, synthetic_file_store},
                StoreError,
            };
            let task_temp = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".audit")
                .join("runtime-temp");
            let root = tempfile::Builder::new()
                .prefix("native-record-")
                .tempdir_in(&task_temp)
                .unwrap();
            let store = synthetic_file_store(root.path()).unwrap();
            let approval = session
                .test_request_native_persistence(
                    &binding,
                    &fixture.caller,
                    &admission,
                    frozen,
                    crate::remote_host_approval::NativePersistenceTarget {
                        store: store.target(),
                        store_id: uuid::Uuid::new_v4().to_string(),
                        expected: None,
                        device_id: uuid::Uuid::new_v4().to_string(),
                    },
                )
                .unwrap();
            assert!(session
                .install_browser(&binding, &fixture.caller, &admission, &controller)
                .is_err());
            assert_eq!(store.load(), Err(StoreError::Missing));
            approval.test_complete(mode != "remember-deny");
            if mode == "remember-document" {
                document.started(&url);
                document.finished(&url);
            }
            if mode == "remember-expired" {
                tokio::time::sleep(Duration::from_millis(220)).await;
            }
            let wrong_root = tempfile::Builder::new()
                .prefix("native-record-wrong-")
                .tempdir_in(&task_temp)
                .unwrap();
            let wrong_store = synthetic_file_store(wrong_root.path()).unwrap();
            let decision = session.persist_native(
                &binding,
                &fixture.caller,
                &admission,
                |mutation, authorization| {
                    if mode == "remember-target" {
                        wrong_store.apply(mutation.clone(), authorization)
                    } else {
                        store.apply(mutation.clone(), authorization)
                    }
                },
            );
            assert_eq!(wrong_store.load(), Err(StoreError::Missing));
            if mode == "remember" {
                let (host, snapshot) = decision.unwrap().unwrap();
                assert_eq!(snapshot.expected.revision, 1);
                assert_eq!(snapshot.devices.len(), 1);
                assert!(snapshot.devices[0].record.is_some());
                assert!(session
                    .persist_native(&binding, &fixture.caller, &admission, |_, _| panic!(
                        "consumed native persistence permit"
                    ))
                    .is_err());
                drop(store);
                let reopened = reopen_synthetic_file_store(root.path()).unwrap();
                assert_eq!(reopened.load().unwrap(), snapshot);
                assert_eq!(
                    controller
                        .current(&binding, &fixture.caller, &admission)
                        .unwrap(),
                    HostControlReply::Locked
                );
                native_record_written = true;
                native_reload_locked = true;
                Some(host)
            } else {
                assert!(decision.is_err() || matches!(decision, Ok(None)));
                assert_eq!(store.load(), Err(StoreError::Missing));
                None
            }
        } else {
            Some(frozen)
        };
        if let Some(frozen) = frozen {
            let browser_install = mode.starts_with("install") || mode == "remember";
            if browser_install {
                let approval = session
                    .test_request_browser_installation(
                        &binding,
                        &fixture.caller,
                        &admission,
                        frozen,
                        crate::remote_host_approval::BrowserInstallationTarget {
                            database_name: "TEST_ONLY.registration-chain".into(),
                            expected_revision: None,
                            host_spki,
                            relay_id: [2; 16],
                            wasm_url: "https://desk.invalid/fixture_bg.wasm".into(),
                            glue_url: "https://desk.invalid/fixture_bg.js".into(),
                            peer_budget_ms: 5000,
                        },
                    )
                    .unwrap();
                assert!(session
                    .install(&binding, &fixture.caller, &admission, &controller)
                    .is_err());
                assert!(session
                    .install_browser(&binding, &fixture.caller, &admission, &controller)
                    .is_err());
                approval.test_complete(mode != "install-deny");
            } else {
                let approval = session
                    .test_request_approval(&binding, &fixture.caller, &admission, frozen)
                    .unwrap();
                approval.test_complete(mode != "deny");
            }
            if mode == "document" || mode == "install-document" {
                document.started(&url);
                document.finished(&url);
            }
            if mode == "install-expired" {
                tokio::time::sleep(Duration::from_millis(220)).await;
            }
            let installation = if browser_install {
                session.install_browser(&binding, &fixture.caller, &admission, &controller)
            } else {
                session.install(&binding, &fixture.caller, &admission, &controller)
            };
            match installation {
                Ok(mut delivery) => {
                    assert!(session
                        .install_browser(&binding, &fixture.caller, &admission, &controller)
                        .is_err());
                    installed = true;
                    receipt = Some(delivery.outcome().clone());
                    acknowledged = delivery.deliver().await.is_ok();
                    assert!(delivery.deliver().await.is_err());
                }
                Err(_) => {
                    acknowledged = session.reject().await.is_ok();
                }
            }
        } else {
            acknowledged = session.reject().await.is_ok();
        }
        assert_eq!(
            installed,
            mode == "yes" || mode == "install" || mode == "remember"
        );
        assert_eq!(controller.test_has_configuration(), installed);
        if mode != "document" && mode != "install-document" && mode != "remember-document" {
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
    if mode != "document" && mode != "install-document" && mode != "remember-document" {
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
    emit(&serde_json::to_vec(&serde_json::json!({"kind":"finished","mode":mode,"nativeCandidateInstalled":installed,"locked":true,"hostNetworkAttempts":0,"hostKeyOpens":0,"registrationTlsConnections":1,"actualRelay":true,"relayMode":if browser_relay_mode == "legacy" { "explicit-legacy" } else { browser_relay_mode },"nativeTransportRetired":true,"nativeElapsedMs":elapsed_ms,"acknowledged":acknowledged,"receipt":receipt,"genuineGuiApproval":false,"persistentTrust":false,"nativeRecordWritten":native_record_written,"nativeReloadLocked":native_reload_locked})).unwrap());
    // 浏览器确认严格协议退役之前，保留测试基础设施。
    let mut cleanup_permission = String::new();
    std::io::stdin().read_line(&mut cleanup_permission).unwrap();
    assert_eq!(cleanup_permission.trim(), "TEST_ONLY_BROWSER_RETIRED");
    assert!(relay.close().await);
}

// 复用公开 fixture 密钥与已校验 CA/名称，限定浏览器来源和注册寿命。
pub(super) async fn registration_relay_fixture() -> NetworkRelayFixture {
    registration_relay_fixture_with_phone(&key_bytes(3)).await
}

pub(super) async fn registration_relay_fixture_with_phone(phone_spki: &[u8]) -> NetworkRelayFixture {
    assert_eq!(phone_spki.len(), 91);
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
            phone_spki[26..].to_vec()
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

// TEST_ONLY：两端独立读取显式选择，未知模式不得默默退回旧协议。
pub(super) fn registration_relay_mode() -> (cc_desk_remote_transport::wss::RelayMode, &'static str)
{
    use cc_desk_remote_transport::wss::RelayMode;
    match std::env::var("CC_DESK_TEST_ONLY_REGISTRATION_RELAY_MODE").as_deref() {
        Ok("legacy") | Err(std::env::VarError::NotPresent) => (RelayMode::Legacy, "legacy"),
        Ok("rendezvous-v1") => (RelayMode::RendezvousV1, "rendezvous-v1"),
        _ => panic!("unsupported TEST_ONLY registration relay mode"),
    }
}
