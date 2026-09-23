//! Real Wry/WebView2 boundary tests. Never calls the application's startup.
use super::native_cli_document_report::{expected, verify, Evidence};
use crate::cli::document::{native::build_main, DocumentBinding, DOCUMENT_HEADER};
use crate::cli::run_registry::RunRegistry;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, WireU64};
use parking_lot::Mutex;
use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::{InvokeBody, Request};
use tauri::utils::config::WindowConfig;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, State, Webview, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const CASE_HEADER: &str = "x-cc-desk-test-case";
const QUERY: &[u8] = br#"{"requestId":"probe-request"}"#;
const WORKER: &str = "tests::native_cli_document_live::D11_Webview_Worker_099";

struct Probe {
    registry: Arc<RunRegistry<usize>>,
    binding: Mutex<Option<Arc<DocumentBinding<usize>>>>,
    caller: Mutex<Option<CallerIdentity>>,
    proof: Mutex<Option<String>>,
    request: LaunchRequest,
    report: Mutex<Evidence>,
    root: PathBuf,
    mode: String,
    main_loaded: AtomicBool,
    peer_loaded: AtomicBool,
    ending: AtomicBool,
    destroyed: AtomicBool,
}

impl Probe {
    fn fail(&self, app: &AppHandle, code: &str) {
        self.report.lock().failure.get_or_insert_with(|| code.into());
        app.exit(1);
    }

    fn record(&self, name: &str, actual: &str) -> Result<(), String> {
        let expected = expected(&self.mode);
        let mut report = self.report.lock();
        let index = report.observations.len();
        if expected.get(index).copied() != Some((name, actual)) {
            let code = format!("PROBE_MISMATCH:{index}:{name}:{actual}");
            report.failure.get_or_insert_with(|| code.clone());
            return Err(code);
        }
        report.observations.push((name.into(), actual.into()));
        Ok(())
    }
}

#[tauri::command]
async fn d11_probe(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    let case = request
        .headers()
        .get(CASE_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing-case");
    let binding = state.binding.lock().clone().ok_or("BINDING_NOT_ATTACHED")?;
    let actual = if matches!(
        case,
        "query" | "query-boundary" | "query-overflow" | "peer" | "reload"
    ) {
        match binding.query_native(&webview, &request) {
            Ok(_) => "UNEXPECTED_LAUNCH".into(),
            Err(failure) => failure.code.to_string(),
        }
    } else {
        match binding.start_native(&webview, &request) {
            Ok((caller, parsed)) => {
                let exact = matches!(request.body(), InvokeBody::Raw(bytes)
                    if *bytes == serde_json::to_vec(&state.request).unwrap());
                if parsed != state.request || !exact {
                    "WIRE_MISMATCH".into()
                } else {
                    *state.caller.lock() = Some(caller);
                    *state.proof.lock() = request
                        .headers()
                        .get(DOCUMENT_HEADER)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned);
                    "ADMITTED".into()
                }
            }
            Err(failure) => failure.code.to_string(),
        }
    };
    if let Err(failure) = state.record(case, &actual) {
        state.fail(&app, &failure);
        return Err(failure);
    }
    Ok(())
}

#[tauri::command]
async fn d11_peer(app: AppHandle, state: State<'_, Arc<Probe>>) -> Result<(), String> {
    if state.report.lock().observations.len() != 9 {
        state.fail(&app, "PEER_BEFORE_MAIN_CASES");
        return Err("PEER_BEFORE_MAIN_CASES".into());
    }
    WebviewWindowBuilder::new(&app, "peer", WebviewUrl::App("probe.html".into()))
        .visible(false)
        .data_directory(state.root.join("peer-webview"))
        .build()
        .map_err(|_| {
            state.fail(&app, "PEER_BUILD_FAILED");
            "PEER_BUILD_FAILED".to_owned()
        })?;
    Ok(())
}

#[tauri::command]
async fn d11_end(app: AppHandle, state: State<'_, Arc<Probe>>) -> Result<(), String> {
    if state.report.lock().observations.len() != 10
        || state.ending.swap(true, Ordering::SeqCst)
    {
        state.fail(&app, "LIFECYCLE_OUT_OF_ORDER");
        return Err("LIFECYCLE_OUT_OF_ORDER".into());
    }
    let caller = state.caller.lock().clone().ok_or("NO_ADMITTED_CALLER")?;
    if state.registry.check_caller(&caller).is_err() {
        state.fail(&app, "PEER_REVOKED_MAIN");
        return Err("PEER_REVOKED_MAIN".into());
    }
    let window = app.get_webview_window("main").ok_or("MAIN_MISSING")?;
    let action = if state.mode == "reload" {
        window.eval("location.reload()")
    } else {
        window.destroy()
    };
    if action.is_err() {
        state.fail(&app, "NATIVE_LIFECYCLE_ACTION_FAILED");
        return Err("NATIVE_LIFECYCLE_ACTION_FAILED".into());
    }
    let state = state.inner().clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            // Observe real callbacks; the test never calls authority.revoke().
            let revoked = state.registry.check_caller(&caller).is_err();
            if revoked && (state.mode == "reload" || state.destroyed.load(Ordering::SeqCst)) {
                if state.mode == "reload" {
                    let proof = state.proof.lock().clone().unwrap();
                    let script = format!(
                        "window.__TAURI_INTERNALS__.invoke('d11_probe',new TextEncoder().encode({}),{{headers:{{'x-cc-desk-document':{},'x-cc-desk-test-case':'reload'}}}}).then(()=>window.__TAURI_INTERNALS__.invoke('d11_finish')).catch(()=>window.__TAURI_INTERNALS__.invoke('d11_abort'))",
                        serde_json::to_string(std::str::from_utf8(QUERY).unwrap()).unwrap(),
                        serde_json::to_string(&proof).unwrap(),
                    );
                    if window.eval(script).is_err() {
                        state.fail(&app, "REVOKED_PAGE_EVAL_FAILED");
                    }
                } else if state.record("destroy", "FORBIDDEN").is_ok() {
                    app.exit(0);
                } else {
                    state.fail(&app, "DESTROY_RESULT_FAILED");
                }
                break;
            }
            if Instant::now() >= deadline {
                state.fail(&app, "LIFECYCLE_REVOCATION_TIMEOUT");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    Ok(())
}

#[tauri::command]
fn d11_finish(app: AppHandle, state: State<'_, Arc<Probe>>) {
    let result = verify(&state.report.lock(), &state.mode);
    match result {
        Ok(()) => app.exit(0),
        Err(failure) => state.fail(&app, failure),
    }
}

#[tauri::command]
fn d11_abort(app: AppHandle, state: State<'_, Arc<Probe>>) {
    state.fail(&app, "SCRIPT_FAILURE");
}

// 检查真实 WebView 的字节/请求头传输、错窗口及刷新/销毁撤权。
#[test]
fn D11_Webview_Live_001() {
    for mode in ["reload", "destroy"] {
        let directory = tempfile::tempdir().unwrap();
        let log_path = directory.path().join("worker.log");
        let log = File::create(&log_path).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", WORKER, "--ignored", "--nocapture", "--test-threads=1"])
            .env("CC_DESK_D11_NATIVE_ROOT", directory.path())
            .env("CC_DESK_D11_NATIVE_MODE", mode)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("native WebView worker timed out: {mode}");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let log = fs::read_to_string(log_path).unwrap();
        assert!(status.success(), "native worker {mode} failed:\n{log}");
        let path = directory.path().join("report.json");
        assert!(fs::metadata(&path).unwrap().len() <= 65536);
        let report: Evidence = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(verify(&report, mode), Ok(()), "{report:?}");
        println!("D11_NATIVE_EVIDENCE {}", serde_json::to_string(&report).unwrap());
    }
}

// 仅由上面的监督进程执行，避免在整个 Rust 测试进程里运行 GUI 事件循环。
#[test]
#[ignore = "subprocess worker explicitly invoked by D11_Webview_Live_001"]
fn D11_Webview_Worker_099() {
    let root = PathBuf::from(std::env::var_os("CC_DESK_D11_NATIVE_ROOT").unwrap());
    let mode = std::env::var("CC_DESK_D11_NATIVE_MODE").unwrap();
    assert!(root.is_absolute() && matches!(mode.as_str(), "reload" | "destroy"));
    let request = LaunchRequest {
        request_id: "probe-request".into(),
        tab_id: "probe-tab".into(),
        run_id: "probe-run".into(),
        generation: 1,
        profile_id: "probe-profile".into(),
        expected_profile_revision: WireU64::parse("1").unwrap(),
        cli: CliKind::Codex,
        launch_cwd: root.to_str().unwrap().into(),
        action: LaunchAction::Raw {
            argv: vec!["a b".into(), "".into(), "中文🧪".into(), "$HOME".into()],
        },
        extra_args: vec![],
        cols: 80,
        rows: 24,
    };
    let state = Arc::new(Probe {
        registry: Arc::new(RunRegistry::new(4)),
        binding: Mutex::new(None),
        caller: Mutex::new(None),
        proof: Mutex::new(None),
        request,
        report: Mutex::new(Evidence {
            schema: 1,
            mode: mode.clone(),
            target: format!("windows-{}", std::env::consts::ARCH),
            engine: "tauri-wry-webview2".into(),
            engine_version: tauri::webview_version().expect("WebView2 runtime required"),
            events: vec![],
            observations: vec![],
            failure: None,
        }),
        root: root.clone(),
        mode: mode.clone(),
        main_loaded: AtomicBool::new(false),
        peer_loaded: AtomicBool::new(false),
        ending: AtomicBool::new(false),
        destroyed: AtomicBool::new(false),
    });
    let setup_state = state.clone();
    let pages = state.clone();
    let windows = state.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            d11_probe, d11_peer, d11_end, d11_finish, d11_abort
        ])
        .setup(move |app| {
            let config = WindowConfig {
                label: "main".into(),
                url: WebviewUrl::App("probe.html".into()),
                visible: false,
                data_directory: Some(setup_state.root.join("main-webview")),
                ..Default::default()
            };
            let bound = build_main(
                app,
                &config,
                "http://tauri.localhost/probe.html".parse().unwrap(),
                setup_state.registry.clone(),
            )
            .map_err(|_| "NATIVE_MAIN_BUILD_FAILED")?;
            *setup_state.binding.lock() = Some(Arc::new(bound.binding));
            Ok(())
        })
        .on_window_event(move |window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::Destroyed) {
                windows.destroyed.store(true, Ordering::SeqCst);
            }
        })
        .on_page_load(move |webview, payload| {
            if webview.label() == "main" {
                let event = match payload.event() {
                    PageLoadEvent::Started => "started",
                    PageLoadEvent::Finished => "finished",
                };
                let mut report = pages.report.lock();
                if report.events.len() >= 128 {
                    drop(report);
                    pages.fail(webview.app_handle(), "PAGE_EVENT_BUDGET");
                    return;
                }
                report.events.push(event.into());
            }
            if !matches!(payload.event(), PageLoadEvent::Finished) {
                return;
            }
            let script = if webview.label() == "main"
                && !pages.main_loaded.swap(true, Ordering::SeqCst)
            {
                format!(
                    "{}\nrunD11({});",
                    include_str!("fixtures/document/probe.js"),
                    serde_json::to_string(&pages.request).unwrap(),
                )
            } else if webview.label() == "peer"
                && !pages.peer_loaded.swap(true, Ordering::SeqCst)
            {
                let proof = pages.proof.lock().clone().unwrap();
                format!(
                    "window.__TAURI_INTERNALS__.invoke('d11_probe',new TextEncoder().encode({}),{{headers:{{'x-cc-desk-document':{},'x-cc-desk-test-case':'peer'}}}}).then(()=>window.__TAURI_INTERNALS__.invoke('d11_end')).catch(()=>window.__TAURI_INTERNALS__.invoke('d11_abort'))",
                    serde_json::to_string(std::str::from_utf8(QUERY).unwrap()).unwrap(),
                    serde_json::to_string(&proof).unwrap(),
                )
            } else {
                return;
            };
            if webview.eval(script).is_err() {
                pages.fail(webview.app_handle(), "INITIAL_PAGE_EVAL_FAILED");
            }
        })
        .build(tauri::generate_context!("src/tests/fixtures/document/tauri.conf.json"))
        .expect("disposable Tauri test application");
    let exit = app.run_return(|_, _| {});
    let report = state.report.lock().clone();
    fs::write(root.join("report.json"), serde_json::to_vec(&report).unwrap()).unwrap();
    assert_eq!(exit, 0, "{report:?}");
    assert_eq!(verify(&report, &mode), Ok(()), "{report:?}");
}
