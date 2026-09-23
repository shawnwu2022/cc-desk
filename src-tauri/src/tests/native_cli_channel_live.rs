//! The real native Channel boundary, isolated from production startup and agents.
use crate::cli::document::{native::build_main, DocumentBinding, DOCUMENT_HEADER};
use crate::cli::output_route::{OutputRoute, CHANNEL_HEADER};
use crate::cli::run_registry::RunRegistry;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, WireU64};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::Request;
use tauri::utils::config::WindowConfig;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, State, Webview, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const OBSERVATIONS: &[&str] = &[
    "bound",
    "duplicate-rejected",
    "peer-rejected",
    "bytes-received",
    "destroy-revoked",
];

struct Probe {
    registry: Arc<RunRegistry<usize>>,
    binding: Mutex<Option<Arc<DocumentBinding<usize>>>>,
    route: Mutex<Option<Arc<OutputRoute<Value>>>>,
    caller: Mutex<Option<CallerIdentity>>,
    headers: Mutex<Option<(String, String)>>,
    observations: Mutex<Vec<String>>,
    failure: Mutex<Option<String>>,
    root: PathBuf,
    loaded: AtomicBool,
    peer_loaded: AtomicBool,
    destroyed: AtomicBool,
}

impl Probe {
    fn fail(&self, app: &AppHandle, code: &str) -> String {
        self.failure.lock().get_or_insert_with(|| code.into());
        app.exit(1);
        code.into()
    }

    fn record(&self, app: &AppHandle, name: &str) -> Result<(), String> {
        let mut observations = self.observations.lock();
        if OBSERVATIONS.get(observations.len()).copied() != Some(name) {
            return Err(self.fail(app, "CHANNEL_OBSERVATION_ORDER"));
        }
        observations.push(name.into());
        Ok(())
    }
}

fn payload(sequence: usize) -> Value {
    json!({"runId":"native-channel-run","generation":9,"sequence":sequence,
        "offset":"9007199254740993","bytes":[0,255,27,91,50,48,48,126,228,184,173]})
}

#[tauri::command]
async fn d11_channel_open(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    let binding = state.binding.lock().clone().ok_or("NO_BINDING")?;
    let (caller, _) = binding
        .start_native(&webview, &request)
        .map_err(|_| state.fail(&app, "START_ADMISSION_FAILED"))?;
    let route = binding
        .channel_native::<_, Value>(&webview, request.headers())
        .map_err(|_| state.fail(&app, "CHANNEL_BIND_FAILED"))?;
    *state.caller.lock() = Some(caller);
    *state.headers.lock() = Some((
        request.headers()[DOCUMENT_HEADER].to_str().unwrap().into(),
        request.headers()[CHANNEL_HEADER].to_str().unwrap().into(),
    ));
    let route = Arc::new(route);
    *state.route.lock() = Some(route.clone());
    state.record(&app, "bound")?;
    for index in 0..2 {
        route
            .send(payload(index))
            .map_err(|_| state.fail(&app, "INITIAL_SEND_FAILED"))?;
    }
    Ok(())
}

#[tauri::command]
async fn d11_channel_duplicate(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    let binding = state.binding.lock().clone().ok_or("NO_BINDING")?;
    let result = binding.channel_native::<_, Value>(&webview, request.headers());
    if !matches!(result, Err(ref failure) if failure.code == "OUTPUT_CHANNEL_BUSY") {
        return Err(state.fail(&app, "DUPLICATE_CHANNEL_ACCEPTED"));
    }
    state.record(&app, "duplicate-rejected")?;
    let route = state.route.lock().clone().ok_or("NO_ROUTE")?;
    route
        .send(payload(2))
        .map_err(|_| state.fail(&app, "ORIGINAL_CHANNEL_LOST"))?;
    WebviewWindowBuilder::new(&app, "peer", WebviewUrl::App("probe.html".into()))
        .visible(false)
        .data_directory(state.root.join("peer-webview"))
        .build()
        .map_err(|_| state.fail(&app, "PEER_BUILD_FAILED"))?;
    Ok(())
}

#[tauri::command]
async fn d11_channel_foreign(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    let binding = state.binding.lock().clone().ok_or("NO_BINDING")?;
    let result = binding.channel_native::<_, Value>(&webview, request.headers());
    if !matches!(result, Err(ref failure) if failure.code == "FORBIDDEN") {
        return Err(state.fail(&app, "FOREIGN_CHANNEL_ACCEPTED"));
    }
    state.record(&app, "peer-rejected")?;
    let route = state.route.lock().clone().ok_or("NO_ROUTE")?;
    route
        .send(payload(3))
        .map_err(|_| state.fail(&app, "PEER_REPLACED_MAIN_CHANNEL"))
}

#[tauri::command]
async fn d11_channel_ack(
    app: AppHandle,
    events: Value,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    // This is a test-only byte equality report, not the future parser ACK API.
    let expected: Vec<Value> = (0..4)
        .map(|index| json!({"index":index,"message":payload(index)}))
        .collect();
    if events != json!(expected) {
        return Err(state.fail(&app, "CHANNEL_BYTES_OR_ORDER_CHANGED"));
    }
    state.record(&app, "bytes-received")?;
    let caller = state.caller.lock().clone().ok_or("NO_CALLER")?;
    let window = app.get_webview_window("main").ok_or("NO_MAIN")?;
    window
        .destroy()
        .map_err(|_| state.fail(&app, "DESTROY_FAILED"))?;
    let state = state.inner().clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if state.destroyed.load(Ordering::SeqCst)
                && state.registry.check_caller(&caller).is_err()
            {
                let route = state.route.lock().clone().unwrap();
                if !matches!(route.send(payload(4)), Err(failure) if failure.code == "FORBIDDEN") {
                    state.fail(&app, "REVOKED_CHANNEL_SENT");
                } else if state.record(&app, "destroy-revoked").is_ok() {
                    app.exit(0);
                }
                break;
            }
            if Instant::now() >= deadline {
                state.fail(&app, "REVOCATION_TIMEOUT");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    Ok(())
}

#[tauri::command]
fn d11_channel_abort(app: AppHandle, state: State<'_, Arc<Probe>>) {
    state.fail(&app, "CHANNEL_SCRIPT_FAILED");
}

// 检查实际 Channel 有序回传字节，拒绝重复绑定及另一窗口，销毁后拒绝发送。
#[test]
fn D11_Channel_Native_011() {
    let directory = tempfile::tempdir().unwrap();
    let log_path = directory.path().join("worker.log");
    let log = File::create(&log_path).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::native_cli_channel_live::D11_Channel_Worker_099",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("CC_DESK_D11_CHANNEL_ROOT", directory.path())
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
            panic!("native Channel worker timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "{}",
        fs::read_to_string(log_path).unwrap()
    );
    let path = directory.path().join("channel-report.json");
    assert!(fs::metadata(&path).unwrap().len() <= 65536);
    let report: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(report["observations"], json!(OBSERVATIONS), "{report}");
    assert_eq!(report["failure"], Value::Null);
    assert!(report["engineVersion"]
        .as_str()
        .is_some_and(|v| !v.is_empty()));
    writeln!(std::io::stdout().lock(), "D11_CHANNEL_EVIDENCE {report}").unwrap();
}

// 由监督测试显式启动，避免与其他单测共用原生 GUI 事件循环。
#[test]
#[ignore = "subprocess worker explicitly invoked by D11_Channel_Native_011"]
fn D11_Channel_Worker_099() {
    let root = PathBuf::from(std::env::var_os("CC_DESK_D11_CHANNEL_ROOT").unwrap());
    assert!(root.is_absolute());
    let request = LaunchRequest {
        request_id: "channel-request".into(),
        tab_id: "channel-tab".into(),
        run_id: "native-channel-run".into(),
        generation: 9,
        profile_id: "channel-profile".into(),
        expected_profile_revision: WireU64::parse("1").unwrap(),
        cli: CliKind::Codex,
        launch_cwd: root.to_str().unwrap().into(),
        action: LaunchAction::Raw { argv: vec![] },
        extra_args: vec![],
        cols: 80,
        rows: 24,
    };
    let probe = Arc::new(Probe {
        registry: Arc::new(RunRegistry::new(4)),
        binding: Mutex::new(None),
        route: Mutex::new(None),
        caller: Mutex::new(None),
        headers: Mutex::new(None),
        observations: Mutex::new(vec![]),
        failure: Mutex::new(None),
        root: root.clone(),
        loaded: AtomicBool::new(false),
        peer_loaded: AtomicBool::new(false),
        destroyed: AtomicBool::new(false),
    });
    let setup = probe.clone();
    let pages = probe.clone();
    let windows = probe.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .manage(probe.clone())
        .invoke_handler(tauri::generate_handler![
            d11_channel_open,
            d11_channel_duplicate,
            d11_channel_foreign,
            d11_channel_ack,
            d11_channel_abort
        ])
        .setup(move |app| {
            let config = WindowConfig {
                label: "main".into(),
                url: WebviewUrl::App("probe.html".into()),
                visible: false,
                data_directory: Some(setup.root.join("main-webview")),
                ..Default::default()
            };
            let bound = build_main(
                app,
                &config,
                "http://tauri.localhost/probe.html".parse().unwrap(),
                setup.registry.clone(),
            )?;
            *setup.binding.lock() = Some(Arc::new(bound.binding));
            Ok(())
        })
        .on_window_event(move |window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::Destroyed) {
                windows.destroyed.store(true, Ordering::SeqCst);
            }
        })
        .on_page_load(move |webview, payload| {
            if !matches!(payload.event(), PageLoadEvent::Finished) {
                return;
            }
            let script = if webview.label() == "main"
                && !pages.loaded.swap(true, Ordering::SeqCst)
            {
                format!(
                    "{}\nrunChannelProbe({});",
                    include_str!("fixtures/document/channel.js"),
                    serde_json::to_string(&request).unwrap(),
                )
            } else if webview.label() == "peer"
                && !pages.peer_loaded.swap(true, Ordering::SeqCst)
            {
                let (proof, channel) = pages.headers.lock().clone().unwrap();
                format!(
                    "window.__TAURI_INTERNALS__.invoke('d11_channel_foreign',new Uint8Array(),{{headers:{{'x-cc-desk-document':{},'x-cc-desk-output-channel':{}}}}}).catch(()=>window.__TAURI_INTERNALS__.invoke('d11_channel_abort'))",
                    serde_json::to_string(&proof).unwrap(),
                    serde_json::to_string(&channel).unwrap(),
                )
            } else {
                return;
            };
            if webview.eval(script).is_err() {
                pages.fail(webview.app_handle(), "CHANNEL_SCRIPT_EVAL_FAILED");
            }
        })
        .build(tauri::generate_context!("src/tests/fixtures/document/tauri.conf.json"))
        .expect("isolated native Channel application");
    let exit = app.run_return(|_, _| {});
    let report = json!({
        "engineVersion":tauri::webview_version().unwrap(),
        "observations":probe.observations.lock().clone(),
        "failure":probe.failure.lock().clone(),
    });
    fs::write(
        root.join("channel-report.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    assert_eq!(exit, 0, "{report}");
    assert_eq!(report["observations"], json!(OBSERVATIONS), "{report}");
    assert_eq!(report["failure"], Value::Null);
}
