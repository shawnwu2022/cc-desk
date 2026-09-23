use super::*;

// 正式命令验收同时覆盖启用消费方和默认禁启两种后端状态。
#[test]
fn D11_Launch_Native_001() {
    for mode in ["ready", "closed"] {
        let root = tempfile::tempdir().unwrap();
        let log_path = root.path().join("worker.log");
        let log = File::create(&log_path).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::native_cli_launch_live::worker::D11_Launch_Worker_099",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("CC_DESK_D11_LAUNCH_ROOT", root.path())
            .env("CC_DESK_D11_LAUNCH_MODE", mode)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("native launch worker timeout");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            status.success(),
            "{}",
            fs::read_to_string(log_path).unwrap()
        );
        let report: Value =
            serde_json::from_slice(&fs::read(root.path().join("report.json")).unwrap()).unwrap();
        assert_eq!(
            report["observations"],
            json!(if mode == "ready" { READY } else { CLOSED }),
            "{report}"
        );
        assert_eq!(report["failure"], Value::Null, "{report}");
        assert_eq!(report["mode"], mode);
        assert!(report["engineVersion"]
            .as_str()
            .is_some_and(|v| !v.is_empty()));
        writeln!(std::io::stdout().lock(), "D11_LAUNCH_EVIDENCE {report}").unwrap();
    }
}
// 使用隔离临时配置与数据目录，不访问实际 CLI 身份或生产启动副作用。
#[test]
#[ignore = "subprocess worker explicitly invoked by D11_Launch_Native_001"]
fn D11_Launch_Worker_099() {
    bundled_runtime::initialize().unwrap();
    let root = PathBuf::from(std::env::var_os("CC_DESK_D11_LAUNCH_ROOT").unwrap());
    let mode = std::env::var("CC_DESK_D11_LAUNCH_MODE").unwrap();
    assert!(["ready", "closed"].contains(&mode.as_str()));
    fs::create_dir(root.join("work")).unwrap();
    let repository = WorkspaceRepository::open(root.join("metadata/workspace.json")).unwrap();
    let mut profile = Profile::new("live-profile", CliKind::Codex);
    profile.program_path = Override::Set(crate::platform::find_executable("node").unwrap());
    let revision = if mode == "ready" {
        repository
            .apply(
                WireU64::parse("0").unwrap(),
                Patch::Create {
                    profile: profile.clone(),
                },
            )
            .unwrap()
            .profiles[&profile.id]
            .revision
    } else {
        WireU64::parse("1").unwrap()
    };
    let request = LaunchRequest {
        request_id: "native-service-request".into(),
        tab_id: "native-service-tab".into(),
        run_id: "native-service-run".into(),
        generation: 3,
        profile_id: profile.id,
        expected_profile_revision: revision,
        cli: CliKind::Codex,
        launch_cwd: root.join("work").to_str().unwrap().into(),
        action: LaunchAction::Raw {
            argv: vec![
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .join("tests/fixtures/native-cli/service-probe.mjs")
                    .to_str()
                    .unwrap()
                    .into(),
                "中文".into(),
                "".into(),
                "two words".into(),
            ],
        },
        extra_args: vec![],
        cols: 80,
        rows: 24,
    };
    let mut env = std::env::vars_os().collect::<crate::cli::environment::EnvMap>();
    env.insert(
        "CC_DESK_TEST_ROOT".into(),
        root.join("work").into_os_string(),
    );
    env.insert(
        "CC_DESK_SERVICE_MARKER".into(),
        "frozen-service-value".into(),
    );
    let consumer = Arc::new(Consumer::default());
    let supervisor = if mode == "ready" {
        Some(consumer.clone() as Arc<dyn RunSupervisor>)
    } else {
        None
    };
    let service = Arc::new(LaunchService::new(
        repository.clone(),
        Some(env),
        supervisor,
    ));
    let runtime = Arc::new(NativeRuntime::new(service.clone()));
    let probe = Arc::new(Probe {
        root: root.clone(),
        mode,
        request,
        repository,
        service,
        runtime: runtime.clone(),
        consumer,
        access: Mutex::new(None),
        proof: Mutex::new(None),
        observations: Mutex::new(vec![]),
        failure: Mutex::new(None),
        loaded: AtomicBool::new(false),
        peer_loaded: AtomicBool::new(false),
        destroyed: AtomicBool::new(false),
    });
    let mut context = tauri::generate_context!("src/tests/fixtures/document/tauri.conf.json");
    context.config_mut().app.windows.push(WindowConfig {
        label: "main".into(),
        url: WebviewUrl::App("probe.html".into()),
        visible: false,
        data_directory: Some(root.join("main-webview")),
        ..Default::default()
    });
    let main = take_main_config(context.config_mut()).unwrap();
    let setup = runtime.clone();
    let pages = probe.clone();
    let windows = probe.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .manage(runtime)
        .manage(probe.clone())
        .invoke_handler(tauri::generate_handler![
            crate::cli::commands::cli_start,
            crate::cli::commands::cli_get_launch_status,
            super::d11_launch_validate,
            super::d11_launch_replayed,
            super::d11_launch_peer,
            super::d11_launch_peer_ops,
            super::d11_launch_stale,
            super::d11_launch_bytes,
            super::d11_launch_closed,
            super::d11_launch_abort,
        ])
        .setup(move |app| {
            setup.initialize_main(app, &main)?;
            Ok(())
        })
        .on_window_event(move |window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::Destroyed) {
                windows.destroyed.store(true, Ordering::SeqCst);
            }
        })
        .on_page_load(move |webview, payload| {
            if !matches!(payload.event(), PageLoadEvent::Finished) { return; }
            let script = if webview.label() == "main" && !pages.loaded.swap(true, Ordering::SeqCst) {
                format!("{}\nrunLaunchProbe({},{});",
                    include_str!("fixtures/document/launch.js"),
                    serde_json::to_string(&pages.request).unwrap(),
                    serde_json::to_string(&pages.mode).unwrap())
            } else if webview.label() == "peer" && !pages.peer_loaded.swap(true, Ordering::SeqCst) {
                let proof = serde_json::to_string(pages.proof.lock().as_ref().unwrap()).unwrap();
                let id = serde_json::to_string(&json!({"requestId":pages.request.request_id})).unwrap();
                let key = serde_json::to_string(&json!({"runId":pages.request.run_id,"generation":pages.request.generation})).unwrap();
                format!("(async()=>{{const n=window.__TAURI_INTERNALS__,h={{headers:{{'x-cc-desk-document':{proof}}}}},b=x=>new TextEncoder().encode(JSON.stringify(x));const code=await n.invoke('cli_get_launch_status',b({id}),h).then(()=>'ACCEPTED',e=>e.code);if(code!=='FORBIDDEN')throw Error();await n.invoke('d11_launch_peer_ops',b({key}),h);}})().catch(()=>window.__TAURI_INTERNALS__.invoke('d11_launch_abort',{{stage:'peer'}}));")
            } else { return; };
            if webview.eval(script).is_err() { pages.fail(webview.app_handle(), "EVAL_FAILED"); }
        })
        .build(context)
        .expect("isolated native launch application");
    let exit = app.run_return(|_, _| {});
    drop(probe.access.lock().take());
    let cleanup = probe.consumer.cleanup(&probe.service);
    let report = json!({"mode":probe.mode,"engineVersion":tauri::webview_version().unwrap(),"observations":probe.observations.lock().clone(),"failure":probe.failure.lock().clone()});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    assert!(cleanup.is_ok(), "cleanup failed");
    assert_eq!(exit, 0, "{report}");
    assert_eq!(report["observations"], json!(probe.expected()), "{report}");
    assert_eq!(report["failure"], Value::Null, "{report}");
}
