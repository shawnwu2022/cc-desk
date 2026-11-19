//! Draft for a real production-document Codex rename acceptance, not an installer test.
use super::{
    Arc, AtomicBool, CliKind, LaunchService, NativeRuntime, Ordering, Override, PageLoadEvent,
    Patch, PathBuf, Profile, Value, WebviewUrl, WindowConfig, WorkspaceRepository, fs, json,
    take_main_config,
};
use tauri::{AppHandle, Manager};

const FIXTURE_ID: &str = "99fe1910-cb93-42eb-9b8f-967722ad2101";
const OLD_TITLE: &str = "Pr101OldRenameCanary";
const NEW_TITLE: &str = "Pr101NewRenameCanary";

#[tauri::command]
fn codex_rename_finish(app: AppHandle, webview: tauri::Webview, status: String, stage: String) {
    let good = webview.label() == "main" && status == "passed" && stage == "complete";
    let stage = match stage.as_str() {
        "initial-list" => "initial-list",
        "capture-start" => "capture-start",
        "rename" => "rename",
        "post-rename-list" => "post-rename-list",
        "trace" => "trace",
        "redaction" => "redaction",
        "capture-stop" => "capture-stop",
        "script-failed" => "script-failed",
        "complete" if good => "complete",
        _ => "unexpected",
    };
    eprintln!("CODEX_RENAME_WRY_STAGE {stage}");
    app.exit(if good { 0 } else { 1 });
}

// The eventual parent test must construct the isolated root, hold the exact
// published Codex byte lease, supervise this worker, then independently verify
// thread/read against its fresh fixture. This worker is never run implicitly.
#[test]
#[ignore = "verification draft: requires a dedicated pinned hosted supervisor"]
fn D12_Codex_Rename_Narrow_Worker_098() {
    eprintln!("CODEX_RENAME_WRY_STEP enter");
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    assert_eq!(
        std::env::var("CC_DESK_CODEX_RENAME_WRY_GO").as_deref(),
        Ok("isolated-codex-rename")
    );
    let root = PathBuf::from(
        std::env::var_os("CC_DESK_CODEX_RENAME_WRY_ROOT").expect("isolated fixture required"),
    );
    let codex = PathBuf::from(
        std::env::var_os("CC_DESK_CODEX_RENAME_WRY_BIN").expect("pinned binary required"),
    );
    let runner_temp =
        fs::canonicalize(std::env::var_os("RUNNER_TEMP").expect("hosted temp required")).unwrap();
    let canonical_root = fs::canonicalize(&root).unwrap();
    assert_eq!(canonical_root.parent(), Some(runner_temp.as_path()));
    assert!(
        root.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("pr101-rename-wry-"))
    );
    let codex_root = runner_temp.join("official-codex");
    assert_eq!(fs::canonicalize(&codex_root).unwrap(), codex_root);
    let codex = fs::canonicalize(codex).unwrap();
    assert_eq!(
        codex,
        codex_root.join("extracted/package/vendor/x86_64-pc-windows-msvc/bin/codex.exe")
    );
    eprintln!("CODEX_RENAME_WRY_STEP source-paths-checked");
    let lease = pin_codex(&codex);
    eprintln!("CODEX_RENAME_WRY_STEP official-binary-pinned");

    let workspace = root.join("project");
    let home = root.join("codex-home");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    assert_eq!(
        fs::canonicalize(&workspace).unwrap().parent(),
        Some(canonical_root.as_path())
    );
    let cwd = workspace.to_str().unwrap().to_string();
    let rollout = home.join("sessions/2025/01/01");
    fs::create_dir_all(&rollout).unwrap();
    let metadata = json!({"timestamp":"2025-01-01T00:00:00Z","type":"session_meta","payload":{"id":FIXTURE_ID,"session_id":FIXTURE_ID,"cwd":cwd,"source":"cli","originator":"cc_desk_acceptance","cli_version":"0.169.1","timestamp":"2025-01-01T00:00:00Z"}});
    let user = json!({"timestamp":"2025-01-01T00:00:01Z","type":"event_msg","payload":{"type":"user_message","message":OLD_TITLE,"images":[],"local_images":[],"text_elements":[]}});
    fs::write(
        rollout.join(format!("rollout-2025-01-01T00-00-00-{FIXTURE_ID}.jsonl")),
        format!("{metadata}\n{user}\n"),
    )
    .unwrap();

    eprintln!("CODEX_RENAME_WRY_STEP rollout-written");
    let repo = WorkspaceRepository::open(root.join("metadata/workspace.json")).unwrap();
    let project = crate::cli::workspace::register_project(&repo, &workspace).unwrap();
    let mut profile = Profile::new("codex-narrow", CliKind::Codex);
    profile.program_path = Override::Set(codex.to_str().unwrap().into());
    let current = repo.read().unwrap();
    let saved = repo
        .apply(current.revision, Patch::Create { profile })
        .unwrap();
    let target = json!({"kind":"profile","profileId":"codex-narrow","expectedProfileRevision":saved.profiles["codex-narrow"].revision,"projectIds":[project.project_id]});
    let fixture = json!({"id":FIXTURE_ID,"cwd":cwd,"oldTitle":OLD_TITLE,"newTitle":NEW_TITLE});

    eprintln!("CODEX_RENAME_WRY_STEP document-fixture-ready");
    let inherited = isolated_env(&root);
    let runtime = Arc::new(NativeRuntime::new(Arc::new(LaunchService::new(
        repo,
        Some(inherited),
        None,
    ))));
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
    let loaded = AtomicBool::new(false);
    let app = tauri::Builder::default()
        .any_thread()
        .manage(runtime)
        .invoke_handler(tauri::generate_handler![
            crate::cli::commands::cli_diagnostic_capture,
            crate::cli::commands::cli_rename_session,
            crate::commands::native_get_scope,
            crate::commands::native_list_resources,
            codex_rename_finish
        ])
        .setup(move |app| {
            eprintln!("CODEX_RENAME_WRY_STEP main-build-start");
            setup.initialize_main(app, &main)?;
            eprintln!("CODEX_RENAME_WRY_STEP main-built");
            Ok(())
        })
        .on_page_load(move |webview, payload| {
            if webview.label() != "main"
                || !matches!(payload.event(), PageLoadEvent::Finished)
                || loaded.swap(true, Ordering::SeqCst)
            {
                return;
            }
            eprintln!("CODEX_RENAME_WRY_STEP page-finished");
            let script = format!(
                "{}\nrunCodexRenameProbe({},{}).catch(() => window.__TAURI_INTERNALS__?.invoke('codex_rename_finish', {{status:'failed', stage:'script-failed'}}));",
                include_str!("fixtures/document/codex-rename-narrow.js"),
                target,
                fixture
            );
            if webview.eval(script).is_err() {
                eprintln!("CODEX_RENAME_WRY_STEP script-rejected");
                webview.app_handle().exit(1);
            } else {
                eprintln!("CODEX_RENAME_WRY_STEP script-queued");
            }
        })
        .build(context)
        .unwrap();
    eprintln!("CODEX_RENAME_WRY_STEP loop-enter");
    let exit = app.run_return(|_, _| {});
    eprintln!("CODEX_RENAME_WRY_STEP loop-exit");
    assert_eq!(exit, 0);
    // This is a Wry-side pass only; no final receipt before independent thread/read.
    drop(lease);
}

fn pin_codex(codex: &std::path::Path) -> fs::File {
    // Same digest as the installed-package verifier, with a live read-only lease.
    use sha2::{Digest, Sha256};
    use std::{
        io::Read,
        os::windows::fs::{MetadataExt, OpenOptionsExt},
    };
    let mut lease = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x00200000)
        .open(codex)
        .unwrap();
    let file = lease.metadata().unwrap();
    assert!(file.is_file() && file.file_attributes() & 0x400 == 0 && file.len() == 382_647_088);
    let mut digest = Sha256::new();
    let mut block = [0u8; 32768];
    loop {
        let read = lease.read(&mut block).unwrap();
        if read == 0 {
            break;
        }
        digest.update(&block[..read]);
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "f76f94d08b0793bfa129ae62c09341ee32ae96d57e69968ae6b8d7c71a20d8c4"
    );
    lease
}

#[test]
#[ignore = "explicit hosted-only official Codex and real Wry"]
fn D12_Codex_Rename_Narrow_097() {
    use crate::tests::fixture_process::FixtureChild;
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    assert_eq!(
        std::env::var("CC_DESK_CODEX_RENAME_WRY_GO").as_deref(),
        Ok("isolated-codex-rename")
    );
    let temp = PathBuf::from(std::env::var_os("RUNNER_TEMP").expect("hosted temp required"));
    let root = tempfile::Builder::new()
        .prefix("pr101-rename-wry-")
        .tempdir_in(&temp)
        .expect("isolated temp directory");
    let program =
        temp.join("official-codex/extracted/package/vendor/x86_64-pc-windows-msvc/bin/codex.exe");
    let log = fs::File::create(root.path().join("worker.log")).unwrap();
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "tests::native_cli_projection_live::codex_rename_narrow::D12_Codex_Rename_Narrow_Worker_098", "--ignored", "--nocapture", "--test-threads=1"]);
    cmd.env_clear()
        .envs(isolated_env(root.path()))
        .env("GITHUB_ACTIONS", "true")
        .env("RUNNER_ENVIRONMENT", "github-hosted")
        .env("RUNNER_TEMP", &temp)
        .env("CC_DESK_CODEX_RENAME_WRY_GO", "isolated-codex-rename")
        .env("CC_DESK_CODEX_RENAME_WRY_ROOT", root.path())
        .env("CC_DESK_CODEX_RENAME_WRY_BIN", &program)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log));
    let mut child = FixtureChild::new(cmd.spawn().expect("real Wry worker start"));
    let exited = child
        .wait_bounded(Duration::from_secs(90))
        .expect("bounded Wry worker did not exit");
    use std::io::Read;
    let mut raw = Vec::new();
    fs::File::open(root.path().join("worker.log"))
        .unwrap()
        .take(65_537)
        .read_to_end(&mut raw)
        .unwrap();
    assert!(raw.len() <= 65_536, "worker log budget");
    let allowed = [
        "initial-list",
        "capture-start",
        "rename",
        "post-rename-list",
        "trace",
        "redaction",
        "capture-stop",
        "script-failed",
        "complete",
        "unexpected",
    ];
    let log = String::from_utf8_lossy(&raw);
    let receipts: Vec<_> = log
        .lines()
        .filter_map(|line| line.strip_prefix("CODEX_RENAME_WRY_STAGE "))
        .filter(|stage| allowed.contains(stage))
        .take(2)
        .collect();
    let fixed_steps = [
        "enter",
        "source-paths-checked",
        "official-binary-pinned",
        "rollout-written",
        "document-fixture-ready",
        "main-build-start",
        "main-built",
        "page-finished",
        "script-queued",
        "script-rejected",
        "loop-enter",
        "loop-exit",
    ];
    let checkpoints: Vec<_> = log
        .lines()
        .filter_map(|line| {
            line.split_once("CODEX_RENAME_WRY_STEP ")
                .map(|(_, step)| step)
        })
        .filter(|step| fixed_steps.contains(step))
        .take(16)
        .collect();
    assert!(
        exited.success() && receipts == ["complete"],
        "Wry stage: {receipts:?}; checkpoints: {checkpoints:?}; exit: {:?}",
        exited.code()
    );
    // The observer cannot write metadata. Its only methods are initialize,
    // initialized and thread/read, over a fresh process in the same isolated home.
    let _lease = pin_codex(&program);
    verify_read_only(&program, root.path());
    println!(
        "CODEX_RENAME_NARROW_PASS bridge=true uniqueProjection=true readOnlyThreadRead=true nativeIdProjectTitleMatch=true installerGuiAccepted=false clipboardAccepted=false"
    );
}

fn verify_read_only(program: &std::path::Path, root: &std::path::Path) {
    use crate::tests::fixture_process::FixtureChild;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        process::{Command, Stdio},
        sync::mpsc,
        time::{Duration, Instant},
    };
    let mut child = FixtureChild::new(
        Command::new(program)
            .args(["app-server", "--strict-config"])
            .current_dir(root.join("project"))
            .env_clear()
            .envs(isolated_env(root))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("independent owned fixture root start"),
    );
    let mut input = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, recv) = mpsc::sync_channel(16);
    let reader = std::thread::spawn(move || {
        let mut stdout = BufReader::new(stdout.take(262_145));
        loop {
            let mut line = Vec::new();
            match stdout.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(n) if n <= 65_536 && line.ends_with(b"\n") => {
                    let value = serde_json::from_slice::<Value>(&line).ok();
                    if send.send(value).is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = send.send(None);
                    break;
                }
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut send = |body: Value| {
        writeln!(input, "{body}").expect("app-server fixture request");
        input.flush().expect("app-server fixture flush");
    };
    let reply = |id: u64| -> Value {
        for _ in 0..32 {
            let message = recv
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("app-server fixture timed out")
                .expect("invalid or oversized app-server fixture line");
            if message["id"].as_u64() == Some(id) {
                assert!(
                    message.get("error").is_none() && message.get("result").is_some(),
                    "app-server fixture rejected request"
                );
                return message["result"].clone();
            }
        }
        panic!("app-server fixture notification budget");
    };
    send(
        json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"cc_desk_acceptance","version":"0.0.0"},"capabilities":{"explicitGatewayOauth":true,"experimentalApi":false}}}),
    );
    let hello = reply(1);
    let actual_home = hello["codexHome"]
        .as_str()
        .expect("app-server fixture has no reported home");
    let expected_home =
        fs::canonicalize(root.join("codex-home")).expect("expected isolated home unavailable");
    assert!(
        fs::canonicalize(actual_home).ok().as_ref() == Some(&expected_home),
        "app-server fixture home mismatch"
    );
    send(json!({"method":"initialized"}));
    send(
        json!({"id":2,"method":"thread/read","params":{"threadId":FIXTURE_ID,"includeTurns":false}}),
    );
    let result = reply(2);
    let thread = &result["thread"];
    let project = root.join("project");
    assert!(
        thread["id"] == FIXTURE_ID
            && thread["cwd"].as_str() == project.to_str()
            && thread["name"] == NEW_TITLE,
        "app-server fixture identity/project/name mismatch"
    );
    drop(input);
    assert!(
        child
            .wait_bounded(deadline.saturating_duration_since(Instant::now()))
            .expect("app-server fixture exit deadline")
            .success()
    );
    drop(recv);
    // Do not use an unbounded join: exit/reap is the gate, the reader owns only
    // the bounded pipe and observes closed receiver after the root exits.
    if reader.is_finished() {
        reader.join().expect("app-server fixture reader");
    }
}

fn isolated_env(root: &std::path::Path) -> crate::cli::environment::EnvMap {
    let mut env = crate::cli::environment::EnvMap::new();
    for key in ["SystemRoot", "WINDIR", "SystemDrive", "ComSpec"] {
        if let Some(value) = std::env::var_os(key) {
            env.insert(key.into(), value);
        }
    }
    for (key, leaf) in [
        ("HOME", "home"),
        ("USERPROFILE", "home"),
        ("APPDATA", "roaming"),
        ("LOCALAPPDATA", "local"),
        ("TEMP", "tmp"),
        ("TMP", "tmp"),
        ("CODEX_HOME", "codex-home"),
        ("CODEX_SQLITE_HOME", "sqlite"),
        ("XDG_CONFIG_HOME", "xdg-config"),
        ("XDG_DATA_HOME", "xdg-data"),
    ] {
        let path = root.join(leaf);
        if leaf != "codex-home" {
            fs::create_dir_all(&path).unwrap();
        }
        env.insert(key.into(), path.into_os_string());
    }
    env
}
