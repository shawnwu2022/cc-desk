//! 原生公开 scope 的只读投影；不是 GUI、凭据或远程授权验收。
use super::*;
use crate::remote_host_ipc::read_pairing_scope;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn HostPairingScope_OriginalScopeOwnerAndReadOnly_001() {
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
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let access = fixture
        .service
        .access(&fixture.caller, &status.run)
        .unwrap();
    let admission = fixture
        .supervisor
        .admit_remote_run(access.clone(), &status.run)
        .unwrap();
    let original = admission.scope().unwrap();
    let controller = crate::remote_host_controller::RemoteHostController::default();
    for _ in 0..2 {
        let value = serde_json::to_value(
            read_pairing_scope(&binding, &fixture.caller, &admission).unwrap(),
        )
        .unwrap();
        assert_eq!(
            value,
            json!({"session":original.session,"run":original.run,"generation":original.generation})
        );
        assert_eq!(value.as_object().unwrap().len(), 3);
        assert_eq!(value["run"], status.run.run_id);
        assert_eq!(value["generation"], status.run.generation.to_string());
        assert_eq!(
            controller
                .current(&binding, &fixture.caller, &admission)
                .unwrap(),
            crate::remote_host_controller::HostControlReply::Locked
        );
    }
    for field in 0..3 {
        let mut wrong = fixture.caller.clone();
        match field {
            0 => wrong.window_label.push_str("foreign"),
            1 => wrong.instance_id.push_str("foreign"),
            _ => wrong.webview_epoch = crate::cli::types::WireU64::parse("999").unwrap(),
        }
        assert!(read_pairing_scope(&binding, &wrong, &admission).is_err());
    }
    assert!(host.test_activation_observation().is_none());
    assert_eq!(host.test_network_attempts(), 0);
    assert_eq!(admission.scope().unwrap(), original);
    // 查询不取走 producer，也不影响原本本地输入。
    assert_eq!(access.write_frame(b"native").unwrap().confirmed_bytes, 6);
    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert!(read_pairing_scope(&binding, &fixture.caller, &admission).is_err());
    drop(access);
    drop(admission);
    assert!(cleanup.close());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn HostPairingScope_DocumentRevocationRejects_002() {
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
        host,
        run: status.run.clone(),
        pin: Some(fixture.supervisor.pin_shutdown(&status.run)),
        stop: None,
        proofs: Vec::new(),
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
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
    assert!(read_pairing_scope(&binding, &fixture.caller, &admission).is_ok());
    binding.revoke();
    assert!(read_pairing_scope(&binding, &fixture.caller, &admission).is_err());
    drop(admission);
    assert!(cleanup.close());
}
