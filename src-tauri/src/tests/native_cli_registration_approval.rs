//! 原 document/run + 原验证器 + 原批准状态机；平台响应和 Yes 回调仅为 TEST_ONLY。
#![allow(non_snake_case)]
use super::*;
use crate::remote_host_approval::FrozenHostApproval;
use crate::remote_host_controller::{
    HostControlReply, NativeRegistrationOwner, RemoteHostController,
};
use cc_desk_remote_security::{
    browser_registration::BrowserRegistrationContext, BrowserProfile, P256Spki, PeerPin, Role,
};
use std::io::{BufRead, Write};

fn key(n: usize) -> P256Spki {
    P256Spki::new(&key_bytes(n)).unwrap()
}
fn key_bytes(n: usize) -> Vec<u8> {
    const POINTS: [&str; 4] = [
        "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        "047cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc4766997807775510db8ed040293d9ac69f7430dbba7dade63ce982299e04b79d227873d1",
        "045ecbe4d1a6330a44c8f7ef951d4bf165e6c6b721efada985fb41661bc6e7fd6c8734640c4998ff7e374b06ce1a64a2ecd82ab036384fb83d9a79b127a27d5032",
        "04e2534a3532d08fbba02dde659ee62bd0031fe2db785596ef509302446b030852e0f1575a4c633cc719dfee5fda862d764efc96c3f30ee0055c42c23f184ed8c6",
    ];
    let mut bytes = network_hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    bytes.extend(network_hex(POINTS[n - 1]));
    bytes
}
fn emit(bytes: &[u8]) {
    println!(
        "CC_DESK_REGISTRATION_ORACLE:{}",
        std::str::from_utf8(bytes).unwrap()
    );
    std::io::stdout().flush().unwrap();
}

#[path = "native_cli_registration_channel.rs"]
mod channel;
#[path = "native_cli_registration_wasm.rs"]
mod registration_wasm;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "TEST_ONLY protocol oracle requires an actual browser producer on stdin"]
async fn NativeRegistration_BrowserOracle_900() {
    let mode = std::env::var("CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_MODE").unwrap();
    assert!([
        "yes",
        "deny",
        "cancel",
        "expired",
        "expired-host",
        "expired-clock",
        "install-failed",
        "run",
        "foreign",
        "document",
        "mismatch",
        "repeat-create"
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
    let relay_point=network_hex("046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5");
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
    let mut owner =
        NativeRegistrationOwner::begin(&binding, &fixture.caller, &admission, context).unwrap();
    let mut second_pin = None;
    emit(
        &owner
            .request(&binding, &fixture.caller, &admission)
            .unwrap(),
    );
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let create = lines.next().unwrap().unwrap();
    owner
        .receive_create(&binding, &fixture.caller, &admission, create.as_bytes())
        .unwrap();
    if mode == "repeat-create" {
        assert!(owner
            .receive_create(&binding, &fixture.caller, &admission, create.as_bytes())
            .is_err());
        assert!(owner
            .request(&binding, &fixture.caller, &admission)
            .is_err());
    } else {
        emit(
            &owner
                .request(&binding, &fixture.caller, &admission)
                .unwrap(),
        );
        let assertion = lines.next().unwrap().unwrap();
        owner
            .receive_assertion(&binding, &fixture.caller, &admission, assertion.as_bytes())
            .unwrap();
        assert!(!controller.test_has_configuration());
        assert!(owner
            .install(&binding, &fixture.caller, &admission, &controller)
            .is_err());
        let clock_ms = if mode == "expired-clock" { 61000 } else { 1000 };
        let mut fields =
            super::approval_keys::description(&relay, &admission, Arc::new(move || Ok(clock_ms)));
        fields.phone_pin = PeerPin::new(Role::Client, key(1));
        fields.issuer = key(2);
        fields.browser = BrowserProfile::enrolled(
            "https://desk.invalid",
            "desk.invalid",
            vec![if mode == "mismatch" { 9 } else { 7 }; 32],
            key(4),
            false,
            false,
        )
        .unwrap();
        let supplier = crate::native_crypto::test_existing_host_supplier(
            cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin).unwrap(),
            Instant::now() + Duration::from_secs(60),
            opened.clone(),
        );
        let cap = Instant::now()
            + if mode == "expired-host" {
                Duration::from_millis(60)
            } else {
                Duration::from_secs(60)
            };
        let frozen = FrozenHostApproval::supplied(fields, &admission, cap, supplier).unwrap();
        let approval = owner.test_request_approval(&binding, &fixture.caller, &admission, frozen);
        if mode == "mismatch" || mode == "expired-clock" {
            assert!(approval.is_err());
        } else {
            let approval = approval.unwrap();
            assert!(owner
                .install(&binding, &fixture.caller, &admission, &controller)
                .is_err());
            assert!(!controller.test_has_configuration());
            if mode == "cancel" {
                owner.cancel();
            }
            if mode == "expired" {
                owner.test_expire();
            }
            approval.test_complete(mode != "deny");
            if mode == "expired-host" {
                tokio::time::sleep(Duration::from_millis(80)).await;
            }
            let mut caller = fixture.caller.clone();
            if mode == "foreign" {
                caller.window_label.push_str("-FOREIGN");
            }
            if mode == "document" {
                document.started(&url);
                document.finished(&url);
            }
            if mode == "install-failed" {
                controller.begin_shutdown();
            }
            let other_admission = if mode == "run" {
                let mut request = fixture.request.clone();
                request.request_id.push_str("-SECOND");
                request.run_id.push_str("-SECOND");
                request.tab_id.push_str("-SECOND");
                let second = fixture.start_request(&request);
                second_pin = Some((
                    second.run.clone(),
                    fixture.supervisor.pin_shutdown(&second.run),
                ));
                Some(
                    fixture
                        .supervisor
                        .admit_remote_run(
                            fixture
                                .service
                                .access(&fixture.caller, &second.run)
                                .unwrap(),
                            &second.run,
                        )
                        .unwrap(),
                )
            } else {
                None
            };
            let selected = other_admission.as_ref().unwrap_or(&admission);
            let installed = owner.install(&binding, &caller, selected, &controller);
            assert_eq!(installed.is_ok(), mode == "yes", "mode={mode}");
            assert!(owner
                .install(&binding, &caller, selected, &controller)
                .is_err());
            if mode == "install-failed" {
                let fresh = RemoteHostController::default();
                assert!(owner
                    .install(&binding, &caller, &admission, &fresh)
                    .is_err());
                assert!(!fresh.test_has_configuration());
            }
            drop(other_admission);
        }
    }
    let installed = controller.test_has_configuration();
    assert_eq!(installed, mode == "yes");
    if mode != "document" {
        assert_eq!(
            controller
                .current(&binding, &fixture.caller, &admission)
                .unwrap(),
            HostControlReply::Locked
        );
    }
    assert_eq!(host.test_network_attempts(), 0);
    assert_eq!(opened[0].load(std::sync::atomic::Ordering::Acquire), 0);
    assert_eq!(opened[1].load(std::sync::atomic::Ordering::Acquire), 0);
    drop(owner);
    drop(controller);
    drop(admission);
    // 同一 supervisor 的全部原 pin 一起移交；不能先关闭全局 owner 再补交另一 run。
    let second_run = second_pin.map(|(run, pin)| {
        cleanup.stop = Some(
            fixture
                .supervisor
                .begin_shutdown_owned(vec![cleanup.pin.take().unwrap(), pin])
                .unwrap(),
        );
        run
    });
    assert!(cleanup.close());
    if let Some(run) = second_run {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let observed = cleanup.stop.as_mut().unwrap().observe_run(&run);
            if matches!(
                observed,
                NativeShutdownObservation::OwnedSessionReleased
                    | NativeShutdownObservation::Unknown {
                        direct_child_reaped: true,
                        output_reader_joined: true,
                        wait_workers_joined: true,
                        original_io_drop_returned: true,
                        stop_job_joined: true,
                        ..
                    }
            ) {
                break;
            }
            assert!(
                Instant::now() < end,
                "original second run did not retire: {observed:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    assert!(relay.close().await);
    emit(&serde_json::to_vec(&serde_json::json!({"mode":mode,"nativeCandidateInstalled":installed,"locked":true,"networkAttempts":0,"keyOpens":0,"genuineGuiApproval":false,"persistentTrust":false})).unwrap());
}
