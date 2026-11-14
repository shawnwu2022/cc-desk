//! TEST_ONLY：实际 startup owner 负责首次 Yes、原 key custody、WSS/TLS 和登记会话。
use super::*;
use crate::remote_host_controller::startup::{
    configuration::FrozenRegistrationStart, RegistrationReply,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "TEST_ONLY requires actual Edge Worker / WASM client"]
async fn NativeRegistration_StartOracle_904() {
    let mode = std::env::var("CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_MODE").unwrap();
    assert!([
        "start-yes",
        "start-deny",
        "start-cancel",
        "start-remember",
        "start-remember-deny",
        "start-remember-cancel"
    ]
    .contains(&mode.as_str()));
    let remember = mode.starts_with("start-remember");
    let (_, browser_relay_mode) = super::registration_relay::registration_relay_mode();
    let native_relay_mode = if browser_relay_mode == "rendezvous-v1" {
        "rendezvousV1"
    } else {
        "legacyV1"
    };
    let native_store_dir = tempfile::Builder::new()
        .prefix("native-remember-")
        .tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".audit"),
        )
        .unwrap();
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
    let mut relay = super::registration_relay::registration_relay_fixture().await;
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
    let controller = Arc::new(RemoteHostController::default());
    let opened = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let host_spki = key_bytes(4);
    let mut relay_spki = key_bytes(1)[..26].to_vec();
    relay_spki.extend(network_relay_material(true).1);
    let candidate = serde_json::json!({"remember":remember,"version":1,"address":relay.address.to_string(),"origin":relay.origin,"relayMode":native_relay_mode,"tunnel":"03".repeat(16),"relayId":"01".repeat(16),"profileId":"07".repeat(16),"profileRevision":"9","endpoint":{"keyName":"TEST_ONLY_ENDPOINT","spki":hex(&host_spki)},"relay":{"keyName":"TEST_ONLY_RELAY","spki":hex(&relay_spki)},"principal":"09".repeat(32),"activation":"02".repeat(32),"hostKeyEpoch":"1","clientKeyEpoch":"1","policyEpoch":"1","issuerEpoch":"1","incarnation":"1","browser":{"origin":"https://desk.invalid","rpId":"desk.invalid","userHandle":"07".repeat(32),"tlsSpki":hex(&key_bytes(1)),"grantSpki":hex(&key_bytes(2)),"relaySpki":hex(&key_bytes(3)),"relayId":"02".repeat(16),"databaseName":"TEST_ONLY.registration-chain","expectedRevision":null,"wasmUrl":"https://desk.invalid/fixture_bg.wasm","glueUrl":"https://desk.invalid/fixture_bg.js"}});
    let mut frozen = FrozenRegistrationStart::parse(&candidate.to_string()).unwrap();
    let mut endpoint_scalar = [0; 32];
    endpoint_scalar[31] = 4;
    frozen.test_keys(
        [
            endpoint_scalar,
            network_relay_material(true).0.try_into().unwrap(),
        ],
        opened.clone(),
    );
    let plan = serde_json::json!({"databaseName":"TEST_ONLY.registration-chain","expectedRevision":null,"relaySpki":B64.encode(key_bytes(3)),"requestedUserHandle":B64.encode([7;32]),"configuration":{
        "version":1,"profile":{"origin":"https://desk.invalid","rpId":"desk.invalid"},"localSpki":B64.encode(key_bytes(1)),"hostSpki":B64.encode(&host_spki),
        "trust":{"hostKeyEpoch":"1","clientKeyEpoch":"1","origin":relay.origin},
        "grant":{"issuerSpki":B64.encode(key_bytes(2)),"principal":"09".repeat(32),"scope":scope,"role":cc_desk_remote_record::grant::DataRole::Controller,"budgetMs":30000,"peerInstall":{"budgetMs":10000},"profile":{"origin":"https://desk.invalid","rpId":"desk.invalid","allowBackedUp":false}},
        "relay":{"mode":browser_relay_mode,"endpoint":relay.origin,"tunnel":B64.encode([3;16]),"keyId":B64.encode([2;16])},"artifacts":{"wasm":"https://desk.invalid/fixture_bg.wasm","glue":"https://desk.invalid/fixture_bg.js"}
    }});
    emit(&serde_json::to_vec(&serde_json::json!({"kind":"ready","origin":relay.origin,"hostSpki":host_spki,"plan":plan})).unwrap());
    let start = Instant::now();
    let handle = host
        .registration
        .test_request(
            binding.clone(),
            fixture.caller.clone(),
            admission,
            frozen,
            controller.clone(),
            Some(mode != "start-deny"),
            true,
            if remember {
                Some(crate::remote_host_controller::startup::TestPersistence {
                    store: crate::remote_native_pairing_store::tests::synthetic_file_store(
                        native_store_dir.path(),
                    )
                    .unwrap(),
                    yes: if mode == "start-remember-cancel" {
                        None
                    } else {
                        Some(mode == "start-remember")
                    },
                })
            } else {
                None
            },
        )
        .await
        .unwrap();
    if mode == "start-remember-cancel" {
        let end = Instant::now() + Duration::from_secs(30);
        while !matches!(handle.reply(), RegistrationReply::PendingNativePersistence) {
            assert!(Instant::now() < end);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        document.started(&url);
        document.finished(&url);
    }
    if mode == "start-cancel" {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < end,
                "startup never reached native key owners"
            );
            if opened.iter().all(|n| n.load(Ordering::Acquire) == 1) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        document.started(&url);
        document.finished(&url);
        // 不借旧 document 查询/Join；新的已鉴权请求必须自己回收已结束的原 task。
        let end = Instant::now() + Duration::from_secs(10);
        while !handle.finished() {
            assert!(Instant::now() < end);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let new_transports = Arc::new(TerminalTransports::new());
        let new_supervisor = Arc::new(NativeRunSupervisor::new(new_transports.clone()));
        let mut newer =
            Fixture::with_runtime_environment("input", new_supervisor, new_transports, true);
        let new_document = crate::cli::document::DocumentAuthority::new(
            newer.service.registry().clone(),
            url.clone(),
        )
        .unwrap();
        let mut new_resources = tauri::ResourceTable::default();
        let new_binding = Arc::new(new_document.attach(&mut new_resources).unwrap());
        new_document.started(&url);
        new_document.finished(&url);
        newer.caller = new_document.test_caller();
        let new_status = newer.start();
        let mut new_cleanup = Cleanup {
            supervisor: newer.supervisor.clone(),
            host: host.clone(),
            run: new_status.run.clone(),
            pin: Some(newer.supervisor.pin_shutdown(&new_status.run)),
            stop: None,
            proofs: Vec::new(),
        };
        let new_admission = newer
            .supervisor
            .admit_remote_run(
                newer
                    .service
                    .access(&newer.caller, &new_status.run)
                    .unwrap(),
                &new_status.run,
            )
            .unwrap();
        let new_frozen = FrozenRegistrationStart::parse(&candidate.to_string()).unwrap();
        let new_request = host
            .registration
            .test_request(
                new_binding,
                newer.caller.clone(),
                new_admission,
                new_frozen,
                controller.clone(),
                Some(false),
                true,
                None,
            )
            .await
            .unwrap();
        assert!(new_request.join().await);
        assert!(new_request.retired());
        assert!(
            handle.retired(),
            "new request did not consume the finished original handle"
        );
        drop(new_request);
        assert!(new_cleanup.close());
    }
    assert!(
        tokio::time::timeout(Duration::from_secs(125), handle.join())
            .await
            .unwrap()
    );
    assert!(
        handle.retired(),
        "original task and native key owners must really retire"
    );
    let outcome = handle.reply();
    let installed = matches!(outcome, RegistrationReply::InstalledLocked { .. });
    assert_eq!(
        installed,
        mode == "start-yes" || mode == "start-remember",
        "{outcome:?}"
    );
    let native_remembered = mode == "start-remember";
    if remember {
        let stored = crate::remote_native_pairing_store::tests::reopen_synthetic_file_store(
            native_store_dir.path(),
        )
        .unwrap();
        if native_remembered {
            let loaded = stored.load().unwrap();
            assert_eq!(loaded.devices.len(), 1);
            assert_eq!(
                loaded.devices[0].record.as_ref().unwrap().route.relay_mode,
                native_relay_mode
            );
        } else {
            assert_eq!(
                stored.load(),
                Err(crate::remote_native_pairing_store::StoreError::Missing)
            );
        }
    }
    if mode == "start-remember-deny" {
        assert_eq!(outcome, RegistrationReply::NativeStoreDeclined);
    }
    assert_eq!(controller.test_has_configuration(), installed);
    let counts = opened.each_ref().map(|n| n.load(Ordering::Acquire));
    assert_eq!(counts, if mode == "start-deny" { [0, 0] } else { [1, 1] });
    assert_eq!(host.test_network_attempts(), 0);
    assert!(
        host.test_activation_observation().is_none(),
        "registration must not consume the Host slot"
    );
    if mode != "start-cancel" && mode != "start-remember-cancel" {
        let inspect = fixture
            .supervisor
            .admit_remote_run(
                fixture
                    .service
                    .access(&fixture.caller, &status.run)
                    .unwrap(),
                &status.run,
            )
            .unwrap();
        assert_eq!(
            controller
                .current(&binding, &fixture.caller, &inspect)
                .unwrap(),
            HostControlReply::Locked
        );
        drop(inspect);
    }
    drop(handle);
    drop(controller);
    assert!(cleanup.close());
    emit(&serde_json::to_vec(&serde_json::json!({"kind":"finished","mode":mode,"nativeCandidateInstalled":installed,"locked":true,"hostNetworkAttempts":0,"hostKeyOpens":counts.iter().sum::<usize>(),"registrationKeyOpens":counts,"registrationTlsConnections":if installed {Some(1)}else if mode=="start-deny" {Some(0)}else{None},"actualRelay":true,"relayMode":if browser_relay_mode == "legacy" { "explicit-legacy" } else { browser_relay_mode },"nativeTransportRetired":true,"nativeKeysRetired":true,"nativeStartupOwner":true,"nativeRemembered":native_remembered,"nativePairingReloadLocked":native_remembered,"nativeElapsedMs":start.elapsed().as_millis(),"acknowledged":installed,"receipt":outcome,"genuineGuiApproval":false,"persistentTrust":false})).unwrap());
    let mut permission = String::new();
    std::io::stdin().read_line(&mut permission).unwrap();
    assert_eq!(permission.trim(), "TEST_ONLY_BROWSER_RETIRED");
    assert!(relay.close().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn NativePairingProduct_FreshRunLockedAndForgetFences_023() {
    use crate::{
        remote_host_approval::NativeForgetApproval,
        remote_native_pairing_store::{
            tests::{record, reopen_synthetic_file_store, synthetic_file_store},
            Mutation,
        },
        remote_pairing_product as product,
    };
    let root = tempfile::Builder::new()
        .prefix("pairing-product-")
        .tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".audit"),
        )
        .unwrap();
    // 公开测试点与synthetic sealer，绝不访问真实 DPAPI 或已有 key。
    let store = synthetic_file_store(root.path()).unwrap();
    struct InitialAuthorization;
    impl crate::remote_native_pairing_store::MutationAuthorization for InitialAuthorization {
        fn check(
            &mut self,
            _: &crate::remote_native_pairing_store::StoreTarget,
            m: &Mutation,
        ) -> Result<(), crate::remote_native_pairing_store::StoreError> {
            assert!(matches!(m, Mutation::Initialize { .. }));
            Ok(())
        }
    }
    let snapshot = store
        .apply(
            Mutation::Initialize {
                store_id: uuid::Uuid::new_v4().to_string(),
                record: record(),
            },
            &mut InitialAuthorization,
        )
        .unwrap();
    drop(store);
    let mut old: Option<FrozenHostApproval> = None;
    let mut old_scope = None;
    for iteration in 0..2 {
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
        if let Some(previous) = old.take() {
            assert!(!previous.matches(&admission));
            assert_ne!(old_scope.take().unwrap(), admission.scope().unwrap());
        }
        let store = reopen_synthetic_file_store(root.path()).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded, snapshot);
        let selection=serde_json::json!({"storeId":loaded.expected.store_id,"revision":loaded.expected.revision.to_string(),"deviceId":record().device_id}).to_string();
        let selected = product::selected(&loaded, &selection).unwrap();
        let frozen = product::freeze(&selected, &admission).unwrap();
        assert!(frozen.matches(&admission));
        let controller = RemoteHostController::default();
        controller
            .install_candidate(&binding, fixture.caller.clone(), &admission, frozen)
            .unwrap();
        assert_eq!(
            controller
                .current(&binding, &fixture.caller, &admission)
                .unwrap(),
            HostControlReply::Locked
        );
        assert_eq!(host.test_network_attempts(), 0);
        old = Some(product::freeze(&selected, &admission).unwrap());
        old_scope = Some(admission.scope().unwrap());
        if iteration == 1 {
            let mutation = Mutation::Forget {
                expected: loaded.expected.clone(),
                device_id: record().device_id,
            };
            let denied = NativeForgetApproval::test_pending(
                &binding,
                &fixture.caller,
                &admission,
                store.target(),
                mutation.clone(),
            );
            denied.test_complete(false);
            assert!(denied
                .commit(&admission, |_, _| panic!("No must not write"))
                .unwrap()
                .is_none());
            assert!(!denied.pending());
            let cancelled_id = uuid::Uuid::new_v4().to_string();
            let cancelled = product::forget_operation(
                cancelled_id.clone(),
                1,
                &binding,
                &fixture.caller,
                &admission,
            )
            .unwrap();
            cancelled.cancel();
            let late = product::forget_operation(
                cancelled_id.clone(),
                1,
                &binding,
                &fixture.caller,
                &admission,
            )
            .unwrap();
            assert!(late
                .show(|| panic!("early cancel must fence late dialog"))
                .is_err());
            let operation = product::forget_operation(
                uuid::Uuid::new_v4().to_string(),
                2,
                &binding,
                &fixture.caller,
                &admission,
            )
            .unwrap();
            operation
                .show(|| {
                    let a = NativeForgetApproval::test_pending(
                        &binding,
                        &fixture.caller,
                        &admission,
                        store.target(),
                        mutation.clone(),
                    );
                    a.test_complete(true);
                    Ok(a)
                })
                .unwrap();
            operation.cancel();
            assert!(matches!(
                operation.commit(&admission, &store).unwrap(),
                Some(product::ForgetReply::Declined)
            ));
            assert_eq!(store.load().unwrap(), loaded);
            for sequence in 3..=70 {
                let op = product::forget_operation(
                    uuid::Uuid::new_v4().to_string(),
                    sequence,
                    &binding,
                    &fixture.caller,
                    &admission,
                )
                .unwrap();
                op.cancel();
            }
            assert!(product::forget_operation(
                cancelled_id,
                1,
                &binding,
                &fixture.caller,
                &admission
            )
            .is_err());
            let approved = NativeForgetApproval::test_pending(
                &binding,
                &fixture.caller,
                &admission,
                store.target(),
                mutation,
            );
            approved.test_complete(true);
            assert!(approved
                .commit(&admission, |m, a| store.apply(m.clone(), a))
                .unwrap()
                .is_some());
            assert!(approved
                .commit(&admission, |_, _| panic!("one shot"))
                .unwrap()
                .is_none());
            assert!(product::selected(&store.load().unwrap(), &selection).is_err());
            // Forget 不撤销已经独立冻结的候选，更不接触运行连接。
            assert!(controller.test_has_configuration());
            assert_eq!(
                controller
                    .current(&binding, &fixture.caller, &admission)
                    .unwrap(),
                HostControlReply::Locked
            );
        }
        drop(store);
        drop(controller);
        drop(admission);
        assert!(cleanup.close());
    }
}
