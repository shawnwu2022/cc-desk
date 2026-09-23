//! Actual Wry/WebView2 -> formal D12 commands, with disposable roots and no model/CLI accounts.
use crate::cli::document::DOCUMENT_HEADER;
use crate::cli::launch_service::LaunchService;
use crate::cli::native_runtime::{take_main_config, NativeRuntime};
use crate::cli::profiles::{EnvValue, Override, Profile};
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::CliKind;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::ipc::{InvokeBody, Request};
use tauri::utils::config::WindowConfig;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, State, Webview, WebviewUrl, WebviewWindowBuilder};
const CASES: &[&str] = &[
    "independent-roots",
    "missing-proof",
    "raw-required",
    "forged-path",
    "forged-reference",
    "body-budget",
    "profile-revoked",
    "peer-rejected",
    "reload-rejected",
];
struct Probe {
    root: PathBuf,
    runtime: Arc<NativeRuntime>,
    repo: WorkspaceRepository,
    targets: Vec<Value>,
    proof: Mutex<Option<String>>,
    source: Mutex<Option<Value>>,
    records: Mutex<Vec<String>>,
    failure: Mutex<Option<String>>,
    loaded: AtomicBool,
}
impl Probe {
    fn fail(&self, app: &AppHandle) {
        *self.failure.lock() = Some("PROJECTION_LIVE_FAILED".into());
        app.exit(1);
    }
}
#[tauri::command]
fn d12_record(app: AppHandle, name: String, state: State<'_, Arc<Probe>>) -> Result<(), String> {
    let mut r = state.records.lock();
    if CASES.get(r.len()).copied() != Some(name.as_str()) {
        drop(r);
        state.fail(&app);
        return Err("CASE_ORDER".into());
    }
    r.push(name);
    Ok(())
}
#[tauri::command]
fn d12_save(
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<Probe>>,
) -> Result<String, String> {
    state
        .runtime
        .binding()
        .map_err(|e| e.code)?
        .admit_native(&webview, request.headers())
        .map_err(|e| e.code)?;
    let InvokeBody::Raw(body) = request.body() else {
        return Err("RAW_REQUIRED".into());
    };
    if body.len() > 8192 {
        return Err("BODY_LIMIT".into());
    }
    let value = serde_json::from_slice(body).map_err(|_| "INVALID".to_string())?;
    let proof = request.headers()[DOCUMENT_HEADER]
        .to_str()
        .map_err(|_| "PROOF".to_string())?
        .to_owned();
    *state.source.lock() = Some(value);
    *state.proof.lock() = Some(proof.clone());
    Ok(proof)
}
#[tauri::command]
fn d12_change_profile(state: State<'_, Arc<Probe>>) -> Result<(), String> {
    let d = state.repo.read().map_err(|e| e.code)?;
    state
        .repo
        .apply(
            d.revision,
            Patch::Update {
                id: "a".into(),
                changes: json!({"name":"changed"}).as_object().unwrap().clone(),
            },
        )
        .map_err(|e| e.code)?;
    Ok(())
}
#[tauri::command]
fn d12_peer(app: AppHandle, state: State<'_, Arc<Probe>>) -> Result<(), String> {
    WebviewWindowBuilder::new(&app, "peer", WebviewUrl::App("probe.html".into()))
        .visible(false)
        .data_directory(state.root.join("peer-view"))
        .build()
        .map_err(|_| "PEER_FAILED".to_string())?;
    Ok(())
}
#[tauri::command]
fn d12_reload(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or("MAIN_MISSING")?
        .eval("location.reload()")
        .map_err(|_| "RELOAD_FAILED".into())
}
#[tauri::command]
fn d12_end(app: AppHandle, state: State<'_, Arc<Probe>>) {
    if state.records.lock().len() == CASES.len() {
        app.exit(0)
    } else {
        state.fail(&app)
    }
}
#[tauri::command]
fn d12_abort(app: AppHandle, state: State<'_, Arc<Probe>>) {
    state.fail(&app)
}
#[test]
fn D12_Webview_FormalCommands_001() {
    let t = tempfile::tempdir().unwrap();
    let log = fs::File::create(t.path().join("log")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::native_cli_projection_live::D12_Webview_Worker_099",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("CC_DESK_D12_LIVE_ROOT", t.path())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("D12 worker timeout");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "{}",
        fs::read_to_string(t.path().join("log")).unwrap()
    );
    let report: Value =
        serde_json::from_slice(&fs::read(t.path().join("report.json")).unwrap()).unwrap();
    assert_eq!(report["observations"], json!(CASES));
    assert_eq!(report["failure"], Value::Null);
    assert!(report["engineVersion"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    println!("D12_WEBVIEW_EVIDENCE {report}");
}
#[test]
#[ignore = "isolated worker explicitly executed by D12_Webview_FormalCommands_001"]
fn D12_Webview_Worker_099() {
    let root = PathBuf::from(std::env::var_os("CC_DESK_D12_LIVE_ROOT").unwrap());
    let repo = WorkspaceRepository::open(root.join("metadata/workspace.json")).unwrap();
    let mut targets = vec![];
    for id in ["a", "b"] {
        let dir = root.join(id);
        fs::create_dir_all(dir.join("projects/p")).unwrap();
        fs::write(
            dir.join("projects/p/same.jsonl"),
            format!("{{\"type\":\"custom-title\",\"customTitle\":\"{id}\"}}\n"),
        )
        .unwrap();
        let mut p = Profile::new(id, CliKind::Claude);
        p.env.insert(
            "CLAUDE_CONFIG_DIR".into(),
            Override::Set(EnvValue::Literal {
                value: dir.to_str().unwrap().into(),
                non_secret: true,
            }),
        );
        let d = repo
            .apply(repo.read().unwrap().revision, Patch::Create { profile: p })
            .unwrap();
        targets.push(json!({"kind":"profile","profileId":id,"expectedProfileRevision":d.profiles[id].revision,"projectId":null}));
    }
    let launch = Arc::new(LaunchService::new(
        repo.clone(),
        Some(Default::default()),
        None,
    ));
    let runtime = Arc::new(NativeRuntime::new(launch));
    let probe = Arc::new(Probe {
        root: root.clone(),
        runtime: runtime.clone(),
        repo,
        targets,
        proof: Mutex::new(None),
        source: Mutex::new(None),
        records: Mutex::new(vec![]),
        failure: Mutex::new(None),
        loaded: AtomicBool::new(false),
    });
    let mut context = tauri::generate_context!("src/tests/fixtures/document/tauri.conf.json");
    context.config_mut().app.windows.push(WindowConfig {
        label: "main".into(),
        url: WebviewUrl::App("probe.html".into()),
        visible: false,
        data_directory: Some(root.join("main-view")),
        ..Default::default()
    });
    let main = take_main_config(context.config_mut()).unwrap();
    let setup = runtime.clone();
    let pages = probe.clone();
    let app=tauri::Builder::default().any_thread().manage(runtime).manage(probe.clone())
        .invoke_handler(tauri::generate_handler![crate::commands::native_get_scope,crate::commands::native_list_resources,d12_record,d12_save,d12_change_profile,d12_peer,d12_reload,d12_end,d12_abort])
        .setup(move|app|{setup.initialize_main(app,&main)?;Ok(())})
        .on_page_load(move|webview,payload|{
            if !matches!(payload.event(),PageLoadEvent::Finished){return;}
            let script=if webview.label()=="main" && !pages.loaded.swap(true,Ordering::SeqCst){
                format!("{}\nrunProjection({});",include_str!("fixtures/document/projection.js"),json!(pages.targets))
            }else{
                let proof=serde_json::to_string(&*pages.proof.lock()).unwrap();let source=serde_json::to_string(&*pages.source.lock()).unwrap();let target=pages.targets[0].to_string();
                let case=if webview.label()=="peer"{"peer-rejected"}else{"reload-rejected"};let end=if webview.label()=="peer"{"d12_reload"}else{"d12_end"};
                format!("(async()=>{{const n=window.__TAURI_INTERNALS__,h={{headers:{{'x-cc-desk-document':{proof}}}}},b=x=>new TextEncoder().encode(JSON.stringify(x));for(const [cmd,q] of [['native_get_scope',{target}],['native_list_resources',{{source:{source},resourceKind:'history',requestEpoch:'1'}}]]){{const e=await n.invoke(cmd,b(q),h).then(()=>null,e=>e.code);if(e!=='FORBIDDEN')throw Error();}}await n.invoke('d12_record',{{name:'{case}'}});await n.invoke('{end}');}})().catch(()=>window.__TAURI_INTERNALS__.invoke('d12_abort'));")
            };
            if webview.eval(script).is_err(){pages.fail(webview.app_handle());}
        }).build(context).unwrap();
    let exit = app.run_return(|_, _| {});
    let report = json!({"engineVersion":tauri::webview_version().unwrap(),"observations":probe.records.lock().clone(),"failure":probe.failure.lock().clone()});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    assert_eq!(exit, 0);
    assert_eq!(report["observations"], json!(CASES));
}
