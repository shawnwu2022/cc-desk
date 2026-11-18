//! 手动原生 UI 验收。没有产品启动、命名密钥加载或网络 start 注册。
#![allow(non_snake_case)]
use super::*;
use crate::cli::native_runtime::NativeRuntime;
use crate::remote_host_controller::RemoteHostController;
use crate::run_supervisor::NativeShutdownObservation;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use tauri::utils::config::WindowConfig;
use tauri::webview::PageLoadEvent;
use tauri::WebviewUrl;

// 显式执行此 worker；不能把人工操作缺失算成无人值守测试通过。
#[test]
#[ignore = "manual isolated WebView UI worker; requires official computer-use interaction"]
fn HostUi_Manual_Worker_099() {
    run_host_ui(false);
}

#[test]
#[ignore = "isolated real WebView scope observation; explicit owned scenario required"]
fn HostScope_Ipc_Worker_100() {
    run_host_ui(true);
}

struct ScopeObservation {
    enabled: bool,
    root: PathBuf,
    received: AtomicBool,
}

#[tauri::command]
fn host_scope_observed(
    app: tauri::AppHandle,
    state: tauri::State<ScopeObservation>,
    outcome: String,
) -> Result<(), String> {
    const CODES: &[&str] = &[
        "OK",
        "REMOTE_HOST_UNAVAILABLE",
        "NATIVE_CANONICAL_UNAVAILABLE",
        "RUN_NOT_READY",
        "FORBIDDEN",
        "DOCUMENT_BRIDGE_UNAVAILABLE",
        "INVALID_REQUEST",
        "UNKNOWN",
        "START_FAILED",
        "SCOPE_SHAPE_INVALID",
    ];
    if !state.enabled
        || !CODES.contains(&outcome.as_str())
        || state.received.swap(true, Ordering::SeqCst)
    {
        return Err("INVALID_TEST_RESULT".into());
    }
    fs::write(state.root.join("scope-observation.json"), serde_json::to_vec(&json!({"outcome":outcome,"source":"isolated-original-WebView-IPC","userInstanceEvidence":false})).unwrap()).map_err(|_| "RECEIPT_WRITE_FAILED")?;
    eprintln!("HOST_SCOPE_IPC_OBSERVATION outcome={outcome}");
    app.exit(0);
    Ok(())
}

fn run_host_ui(automatic_scope: bool) {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap();
    let root = PathBuf::from(
        std::env::var_os("CC_DESK_HOST_UI_ROOT").expect("explicit project fixture root"),
    )
    .canonicalize()
    .unwrap();
    let _scenario_claim =
        claim_scenario(&project, &root).expect("new empty project scenario required");
    for name in ["TEMP", "TMP", "TMPDIR"] {
        assert!(PathBuf::from(std::env::var_os(name).unwrap())
            .canonicalize()
            .unwrap()
            .starts_with(project.join(".audit")));
    }
    let js = if automatic_scope {
        String::new()
    } else {
        fs::read_to_string(project.join(".audit/host-ui-isolation-20261114/ui/host-ui.js")).unwrap()
    };
    let css = if automatic_scope {
        String::new()
    } else {
        fs::read_to_string(project.join(".audit/host-ui-isolation-20261114/ui/cc-desk.css"))
            .unwrap()
    };
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let controller = Arc::new(RemoteHostController::default());
    let candidate = public_candidate();
    fs::write(root.join("public-candidate.json"), &candidate).unwrap();
    fs::write(
        root.join("isolation.json"),
        serde_json::to_vec_pretty(&json!({
            "pid":std::process::id(),"root":root,"fixtureRoot":fixture.root.path(),
            "webviewRoot":root.join("webview"),"temp":std::env::temp_dir(),
            "engineVersion":tauri::webview_version().unwrap(),"productionStartup":false,
            "remoteHostStartCommandRegistered":false,"nodeEnvironment":"isolated allowlist",
            "processAdmission":"independent Builder; no product maintenance or singleton path",
        }))
        .unwrap(),
    )
    .unwrap();
    let loaded = Arc::new(AtomicBool::new(false));
    let setup = runtime.clone();
    let request = serde_json::to_string(&fixture.request).unwrap();
    let candidate = serde_json::to_string(&candidate).unwrap();
    let css = serde_json::to_string(&css).unwrap();
    let main = WindowConfig {
        label: "main".into(),
        title: "CC Desk TEST ONLY - Host UI isolated".into(),
        url: WebviewUrl::App("probe.html".into()),
        width: 1200.0,
        height: 820.0,
        visible: true,
        data_directory: Some(root.join("webview")),
        ..Default::default()
    };
    let app = tauri::Builder::default().any_thread()
        .manage(runtime.clone()).manage(controller.clone())
        .manage(ScopeObservation { enabled: automatic_scope, root: root.clone(), received: AtomicBool::new(false) })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            crate::cli::commands::cli_start,
            crate::commands::remote_host_configure,
            crate::commands::remote_host_discard_candidate,
            crate::commands::remote_host_request,
            crate::commands::remote_host_current,
            crate::commands::remote_host_cancel,
            crate::commands::remote_host_pairing_scope,
            host_scope_observed,
        ])
        .setup(move |app| { setup.initialize_main(app, &main)?; Ok(()) })
        .on_page_load(move |webview, payload| {
            if webview.label() == "main" && matches!(payload.event(), PageLoadEvent::Finished)
                && !loaded.swap(true, Ordering::SeqCst) {
                let script = if automatic_scope {
                    format!("({})({request});", include_str!("fixtures/document/host-scope.js"))
                } else { format!("const style=document.createElement('style');style.textContent={css};document.head.append(style);{js}\nHostUiFixture.mount({request},{candidate}).catch(()=>{{document.body.textContent='FIXTURE_MOUNT_FAILED';}});") };
                webview.eval(&script).unwrap();
            }
        })
        .build(tauri::generate_context!("src/tests/fixtures/document/tauri.conf.json"))
        .expect("isolated test application");
    let watchdog = app.handle().clone();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if finish_rx
            .recv_timeout(Duration::from_secs(if automatic_scope { 60 } else { 1800 }))
            .is_err()
        {
            watchdog.exit(2);
        }
    });
    let exit = app.run_return(|_, _| {});
    let _ = finish_tx.send(());
    watchdog.join().expect("owned watchdog join");
    controller.begin_shutdown();
    let run = fixture.request_run();
    let mut stop = fixture
        .supervisor
        .begin_shutdown_owned(vec![fixture.supervisor.pin_shutdown(&run)])
        .unwrap();
    runtime.shutdown();
    let end = Instant::now() + Duration::from_secs(15);
    let mut observed = stop.observe_run(&run);
    let mut approval = controller.test_native_approval_observation();
    while Instant::now() < end {
        if local_shutdown_facts_complete(observed) && !approval.0 && approval.1 != Some(true) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        observed = stop.observe_run(&run);
        approval = controller.test_native_approval_observation();
    }
    fs::write(
        root.join("shutdown.json"),
        serde_json::to_vec_pretty(&shutdown_evidence(exit, observed, approval)).unwrap(),
    )
    .unwrap();
    assert_eq!(exit, 0);
    if automatic_scope {
        let observation: Value = serde_json::from_slice(
            &fs::read(root.join("scope-observation.json")).expect("missing actual IPC observation"),
        )
        .unwrap();
        assert!(
            !matches!(
                observation["outcome"].as_str(),
                Some("START_FAILED" | "SCOPE_SHAPE_INVALID" | "UNKNOWN") | None
            ),
            "fixture did not observe a valid scope response: {observation}"
        );
    }
    assert!(
        local_shutdown_facts_complete(observed),
        "local shutdown facts incomplete: {observed:?}"
    );
    assert!(
        !approval.0 && approval.1 != Some(true),
        "native approval callback has not finished"
    );
}

fn claim_scenario(project: &Path, root: &Path) -> std::io::Result<fs::File> {
    if !root.starts_with(project.join(".audit")) || root == project.join(".audit") {
        return Err(std::io::Error::other(
            "scenario must be below project .audit",
        ));
    }
    if fs::read_dir(root)?.next().is_some() {
        return Err(std::io::Error::other("scenario directory must be empty"));
    }
    // 保留占用标记；拒绝复用场景目录，不能覆盖前次失败或超时证据。
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("fixture-owner.marker"))
}

fn public_candidate() -> String {
    // P-256 基点和 RFC 6979 的公开点仅为公开测试输入；不创建或打开命名 key。
    let spki = concat!("3059301306072a8648ce3d020106082a8648ce3d030107034200",
        "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5");
    let relay_spki = concat!("3059301306072a8648ce3d020106082a8648ce3d030107034200",
        "0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299");
    let public_spki = |hex: &str| -> [u8; 91] {
        std::array::from_fn(|index| u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap())
    };
    crate::native_crypto::host::FrozenHostBinding {
        native_profile: crate::native_crypto::host::HostKeyProfileIdentity {
            id: [2; 16],
            revision: 1,
        },
        endpoint_spki: public_spki(spki),
        relay_spki: public_spki(relay_spki),
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(
            "wss://desk.invalid/relay",
        )
        .unwrap(),
        relay_id: [1; 16],
        profile_cap: Instant::now() + Duration::from_secs(60),
    }
    .validate()
    .expect("valid distinct public fixture bindings");
    json!({"version":1,"address":"127.0.0.1:9","origin":"wss://desk.invalid/relay","relayMode":"rendezvousV1","tunnel":"03".repeat(16),"relayId":"01".repeat(16),"profileId":"02".repeat(16),"profileRevision":"1","endpoint":{"keyName":"TEST_ONLY_NEVER_OPEN_ENDPOINT","spki":spki},"relay":{"keyName":"TEST_ONLY_NEVER_OPEN_RELAY","spki":relay_spki},"phoneSpki":spki,"issuerSpki":spki,"principal":"04".repeat(32),"activation":"33".repeat(32),"hostKeyEpoch":"5","clientKeyEpoch":"7","policyEpoch":"13","issuerEpoch":"17","incarnation":"11","browser":{"origin":"https://desk.invalid","rpId":"desk.invalid","credentialId":"07".repeat(32),"spki":spki}}).to_string()
}

fn local_shutdown_facts_complete(observed: NativeShutdownObservation) -> bool {
    matches!(
        observed,
        NativeShutdownObservation::Unknown {
            direct_child_reaped: true,
            output_reader_joined: true,
            wait_workers_joined: true,
            original_io_drop_returned: true,
            stop_job_joined: true,
            ..
        }
    )
}

fn shutdown_evidence(
    exit: i32,
    observed: NativeShutdownObservation,
    approval: (bool, Option<bool>),
) -> Value {
    // 逐项事实不证明 PTY 后代或整体 I/O 退出，保留 supervisor 的 Unknown。
    let run_observation = match observed {
        NativeShutdownObservation::Pending => json!({"overall":"Pending"}),
        NativeShutdownObservation::OwnedSessionReleased => {
            json!({"overall":"OwnedSessionReleased"})
        }
        NativeShutdownObservation::Unknown {
            direct_child_reaped,
            output_reader_joined,
            wait_workers_joined,
            original_io_drop_returned,
            io_threads_joined,
            stop_job_joined,
        } => json!({"overall":"Unknown", "directChildReaped":direct_child_reaped,
            "outputReaderJoined":output_reader_joined, "waitWorkersJoined":wait_workers_joined,
            "originalIoDropReturned":original_io_drop_returned, "ioThreadsJoined":io_threads_joined,
            "stopJobJoined":stop_job_joined}),
    };
    json!({
        "exit":exit, "ownedRun":run_observation,
        "nativeApprovalRequestPending":approval.0,
        "nativeApprovalCallbackOutstanding":approval.1,
        "nativeApprovalCallbackCompletionConfirmed":!approval.0 && approval.1 == Some(false),
    })
}

#[test]
fn HostUi_PublicCandidatePreflight_096() {
    // 直接执行夹具的生产 binding 预检，不需要 NativeRun、key opener 或 WebView。
    let candidate: Value = serde_json::from_str(&public_candidate()).unwrap();
    assert_ne!(candidate["endpoint"]["spki"], candidate["relay"]["spki"]);
    assert_eq!(candidate["endpoint"]["spki"].as_str().unwrap().len(), 182);
    assert_eq!(candidate["relay"]["spki"].as_str().unwrap().len(), 182);
}

#[test]
fn HostUi_ScenarioReusePreservesEvidence_097() {
    let fixture = tempfile::tempdir().unwrap();
    let project = fixture.path().canonicalize().unwrap();
    let audit = project.join(".audit");
    fs::create_dir(&audit).unwrap();
    let scenario = audit.join("scenario");
    fs::create_dir(&scenario).unwrap();
    assert!(claim_scenario(&project, &project).is_err());
    assert!(claim_scenario(&project, &audit).is_err());
    let claim = claim_scenario(&project, &scenario).unwrap();
    fs::write(
        scenario.join("first-failure.json"),
        b"preserve first failure",
    )
    .unwrap();
    drop(claim);
    assert!(claim_scenario(&project, &scenario).is_err());
    assert_eq!(
        fs::read(scenario.join("first-failure.json")).unwrap(),
        b"preserve first failure"
    );
}

#[test]
fn HostUi_ShutdownUnknownRetainsPartialFacts_098() {
    let observed = NativeShutdownObservation::Unknown {
        direct_child_reaped: true,
        output_reader_joined: true,
        wait_workers_joined: true,
        original_io_drop_returned: true,
        io_threads_joined: false,
        stop_job_joined: true,
    };
    assert!(local_shutdown_facts_complete(observed));
    assert!(!local_shutdown_facts_complete(
        NativeShutdownObservation::Pending
    ));
    for approval in [
        (false, None),
        (false, Some(true)),
        (true, Some(false)),
        (false, Some(false)),
    ] {
        let evidence = shutdown_evidence(0, observed, approval);
        assert_eq!(evidence["ownedRun"]["overall"], "Unknown");
        assert_eq!(evidence["ownedRun"]["ioThreadsJoined"], false);
        assert_eq!(
            evidence["nativeApprovalCallbackOutstanding"],
            json!(approval.1)
        );
        assert_eq!(
            evidence["nativeApprovalCallbackCompletionConfirmed"],
            !approval.0 && approval.1 == Some(false)
        );
        assert!(evidence.get("ownedRunPhysicallyRetired").is_none());
        assert!(evidence.get("clean").is_none());
    }
}

// Regression: a never-remotely-admitted ordinary run must retain the desktop
// direct-root exit contract; this does not attest process-tree retirement.
#[tokio::test]
async fn HostExit_OrdinaryLocalRunProductionGate_101() {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let status = fixture.start();
    let resource = fixture
        .service
        .registry()
        .resource(&fixture.caller, &status.run)
        .unwrap();
    assert!(resource.process.managed_job.is_none());
    drop(resource);
    let host = Arc::new(crate::remote_host::RemoteHostService::default());
    crate::remote_host::begin_app_shutdown(&host, &RemoteHostController::default());
    assert_eq!(
        host.request_exit(0, false),
        crate::remote_host::ExitDecision::Wait(0)
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    host.start_exit(
        0,
        runtime.clone(),
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |code| {
            let _ = tx.send(code);
        },
    );
    let result = tokio::time::timeout(Duration::from_secs(15), rx).await;
    // Preserve original owner cleanup even when the regression assertion fails.
    runtime.shutdown();
    fixture.wait_registry_retired();
    assert_eq!(
        result,
        Ok(Ok(0)),
        "production exit gate did not release a local-only desktop run"
    );
}

#[tokio::test]
async fn HostExit_RemoteResponsibilityNeverUsesLocalDesktopContract_102() {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    fixture.start();
    let host = Arc::new(crate::remote_host::RemoteHostService::default());
    let controller = RemoteHostController::default();
    controller.reserve_external_responsibility().unwrap();
    crate::remote_host::begin_app_shutdown(&host, &controller);
    assert!(!controller.sealed_never_remote());
    assert!(controller.reserve_external_responsibility().is_err());
    assert_eq!(
        host.request_exit(0, false),
        crate::remote_host::ExitDecision::Wait(0)
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    host.start_exit(
        0,
        runtime.clone(),
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |code| {
            let _ = tx.send(code);
        },
    );
    let result = tokio::time::timeout(Duration::from_secs(15), rx).await;
    runtime.shutdown();
    fixture.wait_registry_retired();
    assert!(
        matches!(result, Ok(Err(_))),
        "remote responsibility must retain Unknown refusal"
    );
    assert_eq!(
        host.request_exit(0, false),
        crate::remote_host::ExitDecision::Prevent
    );
}
