//! Formal D11 commands -> real WebView2 -> actual PTY. No CLI accounts or product hooks.
use crate::cli::document::DOCUMENT_HEADER;
use crate::cli::launch_service::{LaunchService, NativeRun, RunAccess, RunSupervisor};
use crate::cli::native_runtime::{take_main_config, NativeRuntime};
use crate::cli::profiles::{error, Override, Profile};
use crate::cli::run_registry::{LaunchPhase, RunKey, RunRegistry};
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, SafeError, WireU64};
use crate::terminal_transport::OutputFrame;
use parking_lot::Mutex;
use portable_pty::PtySize;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::{InvokeBody, Request};
use tauri::utils::config::WindowConfig;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, State, Webview, WebviewUrl, WebviewWindowBuilder, WindowEvent};

#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

const READY: &[&str] = &[
    "single-child",
    "receipt-recovered",
    "profile-snapshot-frozen",
    "peer-rejected",
    "stale-generation-rejected",
    "bytes-received",
    "destroy-revoked",
    "owned-child-reaped",
];
const CLOSED: &[&str] = &["unready-rejected-before-io"];

#[derive(Default)]
struct Consumer {
    calls: AtomicUsize,
    runs: Mutex<Vec<(RunKey, Arc<NativeRun>)>>,
    readers: Mutex<Vec<std::thread::JoinHandle<()>>>,
    packets: Arc<Mutex<Vec<Value>>>,
    failed: Arc<AtomicBool>,
}
impl RunSupervisor for Consumer {
    fn adopt(
        &self,
        _registry: Arc<RunRegistry<NativeRun>>,
        run: &RunKey,
        resource: Arc<NativeRun>,
    ) -> Result<(), SafeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.runs.lock().push((run.clone(), resource.clone()));
        let mut reader = resource.process.pty.take_reader()?;
        let route = resource.route();
        let packets = self.packets.clone();
        let failed = self.failed.clone();
        let run = run.clone();
        // Test-only bounded small-frame consumer. Not the D14 event/ACK protocol.
        let thread = std::thread::Builder::new()
            .name("d11-fixture-reader".into())
            .spawn(move || {
                let mut total = 0usize;
                let mut buffer = [0u8; 128];
                loop {
                    let length = match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    total += length;
                    if total > 32768 {
                        failed.store(true, Ordering::SeqCst);
                        break;
                    }
                    let offset = total - length;
                    let event = json!({
                        "runId": run.run_id,
                        "generation": run.generation,
                        "streamEpoch": "1",
                        "offset": offset.to_string(),
                        "bytes": buffer[..length],
                    });
                    packets.lock().push(event);
                    let frame = OutputFrame {
                        run_id: run.run_id.clone(),
                        generation: run.generation,
                        stream_epoch: WireU64::parse("1").unwrap(),
                        offset: WireU64::parse(&offset.to_string()).unwrap(),
                        bytes: buffer[..length].to_vec(),
                    };
                    if route.send(frame).is_err() {
                        break;
                    }
                }
            })
            .map_err(|_| error("TEST_READER_FAILED"))?;
        self.readers.lock().push(thread);
        Ok(())
    }
}
impl Consumer {
    fn cleanup(&self, service: &LaunchService) -> Result<(), SafeError> {
        let runs = std::mem::take(&mut *self.runs.lock());
        for (key, resource) in runs {
            if resource.process.pty.try_wait()?.is_none() {
                resource.process.pty.terminate_root()?;
            }
            let deadline = Instant::now() + Duration::from_secs(15);
            while resource.process.pty.try_wait()?.is_none() {
                if Instant::now() > deadline {
                    return Err(error("TEST_REAP_TIMEOUT"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            resource.process.pty.wait()?;
            service.registry().mark_exited(&key)?;
            service.registry().retire(&key)?;
            drop(resource); // Close the master before joining its EOF reader.
        }
        for thread in std::mem::take(&mut *self.readers.lock()) {
            thread.join().map_err(|_| error("TEST_READER_PANIC"))?;
        }
        Ok(())
    }
}

struct Probe {
    root: PathBuf,
    mode: String,
    request: LaunchRequest,
    repository: WorkspaceRepository,
    service: Arc<LaunchService>,
    runtime: Arc<NativeRuntime>,
    consumer: Arc<Consumer>,
    access: Mutex<Option<RunAccess>>,
    proof: Mutex<Option<String>>,
    observations: Mutex<Vec<String>>,
    failure: Mutex<Option<String>>,
    loaded: AtomicBool,
    peer_loaded: AtomicBool,
    destroyed: AtomicBool,
}
impl Probe {
    fn expected(&self) -> &[&str] {
        if self.mode == "ready" {
            READY
        } else {
            CLOSED
        }
    }
    fn fail(&self, app: &AppHandle, code: &str) -> String {
        self.failure.lock().get_or_insert_with(|| code.into());
        app.exit(1);
        code.into()
    }
    fn record(&self, app: &AppHandle, name: &str) -> Result<(), String> {
        let mut observations = self.observations.lock();
        if self.expected().get(observations.len()).copied() != Some(name) {
            return Err(self.fail(app, "OBSERVATION_ORDER"));
        }
        observations.push(name.into());
        Ok(())
    }
    fn receipt(&self, webview: &Webview, request: &Request<'_>) -> Result<Value, String> {
        let binding = self.runtime.binding().map_err(|e| e.code)?;
        let caller = binding
            .admit_native(webview, request.headers())
            .map_err(|e| e.code)?;
        let status = self
            .service
            .registry()
            .status(&caller, &self.request.request_id)
            .map_err(|e| e.code)?;
        if status.phase != LaunchPhase::Running {
            return Err("NOT_RUNNING".into());
        }
        let expected = serde_json::to_value(&status).unwrap();
        if raw_value(request)? != expected {
            return Err("RECEIPT_CHANGED".into());
        }
        Ok(expected)
    }
}
fn raw_value(request: &Request<'_>) -> Result<Value, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("RAW_REQUIRED".into());
    };
    if bytes.len() > 65536 {
        return Err("TEST_BODY_TOO_LARGE".into());
    }
    serde_json::from_slice(bytes).map_err(|_| "BAD_TEST_BODY".into())
}
fn size() -> PtySize {
    PtySize {
        rows: 30,
        cols: 100,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[tauri::command]
async fn d11_launch_validate(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    state
        .receipt(&webview, &request)
        .map_err(|_| state.fail(&app, "BAD_RECOVERED_RECEIPT"))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let entries = loop {
        let entries: Vec<_> = fs::read_dir(state.root.join("work"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|v| v == "json"))
            .collect();
        if !entries.is_empty() {
            break entries;
        }
        if Instant::now() > deadline {
            return Err(state.fail(&app, "CHILD_NOT_READY"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    if entries.len() != 1 || state.consumer.calls.load(Ordering::SeqCst) != 1 {
        return Err(state.fail(&app, "DUPLICATE_CHILD"));
    }
    let child: Value = serde_json::from_slice(&fs::read(entries[0].path()).unwrap()).unwrap();
    if child
        != json!({"argv":["中文","","two words"],"marker":"frozen-service-value","stdin":true,"stdout":true})
    {
        return Err(state.fail(&app, "CHILD_INPUTS_CHANGED"));
    }
    state.record(&app, "single-child")?;
    state.record(&app, "receipt-recovered")?;
    let body = InvokeBody::Raw(
        serde_json::to_vec(
            &json!({"runId":state.request.run_id,"generation":state.request.generation}),
        )
        .unwrap(),
    );
    *state.access.lock() = Some(
        state
            .runtime
            .access(&webview, request.headers(), &body)
            .map_err(|_| state.fail(&app, "OWNER_ACCESS_FAILED"))?,
    );
    *state.proof.lock() = Some(request.headers()[DOCUMENT_HEADER].to_str().unwrap().into());
    let document = state.repository.read().unwrap();
    state
        .repository
        .apply(
            document.revision,
            Patch::Delete {
                id: state.request.profile_id.clone(),
            },
        )
        .unwrap();
    Ok(())
}
#[tauri::command]
async fn d11_launch_replayed(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    state
        .receipt(&webview, &request)
        .map_err(|_| state.fail(&app, "REPLAY_CHANGED"))?;
    let access = state.access.lock();
    let snapshot = access
        .as_ref()
        .unwrap()
        .snapshot()
        .map_err(|_| state.fail(&app, "SNAPSHOT_LOST"))?;
    if snapshot.request() != &state.request
        || snapshot
            .environment()
            .get(std::ffi::OsStr::new("CC_DESK_SERVICE_MARKER"))
            != Some(&"frozen-service-value".into())
        || state.consumer.calls.load(Ordering::SeqCst) != 1
    {
        return Err(state.fail(&app, "FROZEN_RUN_CHANGED"));
    }
    state.record(&app, "profile-snapshot-frozen")
}
#[tauri::command]
async fn d11_launch_peer(app: AppHandle, state: State<'_, Arc<Probe>>) -> Result<(), String> {
    WebviewWindowBuilder::new(&app, "peer", WebviewUrl::App("probe.html".into()))
        .visible(false)
        .data_directory(state.root.join("peer-webview"))
        .build()
        .map_err(|_| state.fail(&app, "PEER_BUILD_FAILED"))?;
    Ok(())
}
#[tauri::command]
async fn d11_launch_peer_ops(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    for _ in ["input", "resize", "stop", "snapshot"] {
        if !matches!(state.runtime.access(&webview,request.headers(),request.body()),Err(e) if e.code=="FORBIDDEN")
        {
            return Err(state.fail(&app, "PEER_ACCESS_GRANTED"));
        }
    }
    state.record(&app, "peer-rejected")?;
    app.get_webview_window("main")
        .unwrap()
        .eval("finishLaunchProbe()")
        .map_err(|_| state.fail(&app, "MAIN_CONTINUE_FAILED"))
}
#[tauri::command]
async fn d11_launch_stale(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    if !matches!(state.runtime.access(&webview,request.headers(),request.body()),Err(e) if e.code=="STALE_GENERATION")
    {
        return Err(state.fail(&app, "STALE_ACCESS_GRANTED"));
    }
    state.record(&app, "stale-generation-rejected")
}
#[tauri::command]
async fn d11_launch_bytes(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    state
        .runtime
        .binding()
        .unwrap()
        .admit_native(&webview, request.headers())
        .map_err(|_| state.fail(&app, "ACK_WRONG_OWNER"))?;
    let packets = state.consumer.packets.lock().clone();
    let bytes: Vec<u8> = packets
        .iter()
        .flat_map(|e| {
            e["bytes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
        })
        .collect();
    if raw_value(&request)? != json!(packets)
        || !bytes.windows(14).any(|v| v == b"D11_REAL_READY")
        || state.consumer.failed.load(Ordering::SeqCst)
    {
        return Err(state.fail(&app, "ACTUAL_PTY_BYTES_CHANGED"));
    }
    state.record(&app, "bytes-received")?;
    app.get_webview_window("main")
        .unwrap()
        .destroy()
        .map_err(|_| state.fail(&app, "DESTROY_FAILED"))?;
    let state = state.inner().clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !state.destroyed.load(Ordering::SeqCst) {
            if Instant::now() > deadline {
                state.fail(&app, "DESTROY_TIMEOUT");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let access = state.access.lock().take().unwrap();
        let forbidden = [
            access.with_writer::<()>(|_| panic!("revoked writer")),
            access.resize(size()),
            access.terminate_root(),
            access.snapshot().map(|_| ()),
        ]
        .into_iter()
        .all(|r| matches!(r,Err(e) if e.code=="FORBIDDEN"));
        drop(access);
        if !forbidden
            || state.consumer.runs.lock()[0]
                .1
                .process
                .pty
                .try_wait()
                .unwrap()
                .is_some()
        {
            state.fail(&app, "REVOKED_ACCESS_OR_OWNER_LOST");
            return;
        }
        if state.record(&app, "destroy-revoked").is_err() {
            return;
        }
        if state.consumer.cleanup(&state.service).is_err() {
            state.fail(&app, "REAP_FAILED");
            return;
        }
        if state.record(&app, "owned-child-reaped").is_ok() {
            app.exit(0);
        }
    });
    Ok(())
}
#[tauri::command]
async fn d11_launch_closed(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<(), String> {
    state
        .runtime
        .binding()
        .unwrap()
        .admit_native(&webview, request.headers())
        .map_err(|_| state.fail(&app, "GATE_WRONG_OWNER"))?;
    if state.root.join("metadata").exists() || state.consumer.calls.load(Ordering::SeqCst) != 0 {
        return Err(state.fail(&app, "GATE_DID_IO"));
    }
    state.record(&app, "unready-rejected-before-io")?;
    app.exit(0);
    Ok(())
}
#[tauri::command]
fn d11_launch_abort(app: AppHandle, stage: String, state: State<'_, Arc<Probe>>) {
    let safe = if [
        "initial",
        "gate",
        "concurrent-start",
        "status",
        "replay-after-delete",
        "stale-run",
        "native-bytes",
        "peer",
    ]
    .contains(&stage.as_str())
    {
        stage
    } else {
        "unknown".into()
    };
    state.fail(&app, &format!("SCRIPT_FAILED:{safe}"));
}

#[path = "native_cli_launch_worker.rs"]
mod worker;
