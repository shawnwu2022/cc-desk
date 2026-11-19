//! Hosted-only one-use preview worker: genuine document/runtime, no confirm handler.
use super::{
    Arc, AtomicBool, CliKind, LaunchService, NativeRuntime, Ordering, Override, PageLoadEvent,
    Patch, PathBuf, Profile, WebviewUrl, WindowConfig, WorkspaceRepository, fs, json,
    take_main_config,
};
use serde::Deserialize;
use std::io::{BufRead, Read};
use tauri::{AppHandle, Manager};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    live: PathBuf,
    codex: PathBuf,
    active: uuid::Uuid,
    archived: uuid::Uuid,
}

#[tauri::command]
fn preview_fixture_finish(app: AppHandle, webview: tauri::Webview, ok: bool, stage: String) {
    let phase = match stage.as_str() {
        "scope" => "scope",
        "before-list" => "before-list",
        "active-preview" => "active-preview",
        "archived-preview" => "archived-preview",
        "after-list" => "after-list",
        "complete" if ok && webview.label() == "main" => "complete",
        _ => "unexpected",
    };
    if phase == "complete" {
        println!("STAGE_B_WRY_PREVIEW_ONLY_PASS");
        app.exit(0);
    } else {
        println!("STAGE_B_WRY_PREVIEW_BLOCKED {phase}");
        app.exit(1);
    }
}

#[test]
#[ignore = "explicit hosted same-job Stage-A fixture; product preview only"]
fn PreviewOnlyOwned_098() {
    println!("PREVIEW_WRY_STEP enter");
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    assert_eq!(
        std::env::var("CC_DESK_PREVIEW_STAGE").as_deref(),
        Ok("codex-preview-only")
    );
    let mut raw = Vec::new();
    std::io::BufReader::new(std::io::stdin().lock().take(16_385))
        .read_until(b'\n', &mut raw)
        .expect("owned preview input");
    assert!(
        raw.ends_with(b"\n") && raw.len() <= 16_384,
        "owned preview input bound"
    );
    println!("PREVIEW_WRY_STEP descriptor-received");
    let fixture: Input = serde_json::from_slice(&raw).expect("owned preview schema");
    assert_ne!(fixture.active, fixture.archived);
    let runner_temp =
        fs::canonicalize(std::env::var_os("RUNNER_TEMP").expect("hosted temp")).expect("temp path");
    let live = fs::canonicalize(&fixture.live).expect("synthetic live");
    let root = live.parent().expect("synthetic parent");
    assert_eq!(root.parent(), Some(runner_temp.as_path()));
    assert!(
        root.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("pr101-restore-stage-a-"))
    );
    assert_eq!(
        live.file_name().and_then(|name| name.to_str()),
        Some("live")
    );
    let actual_codex = fs::canonicalize(&fixture.codex).expect("official provider path");
    assert_eq!(
        actual_codex,
        runner_temp
            .join("official-codex/extracted/package/vendor/x86_64-pc-windows-msvc/bin/codex.exe")
    );
    let project_dir = fixture.live.join("project");
    let home = fixture.live.join("home/codex");
    assert_eq!(
        fs::canonicalize(&project_dir).expect("synthetic project"),
        live.join("project")
    );
    assert_eq!(
        fs::canonicalize(&home).expect("synthetic Codex home"),
        live.join("home/codex")
    );
    assert_eq!(
        std::env::var_os("CODEX_HOME").as_deref(),
        Some(home.as_os_str())
    );
    assert_eq!(
        std::env::var_os("CODEX_SQLITE_HOME").as_deref(),
        Some(fixture.live.join("sqlite").as_os_str())
    );
    // The owned parent holds and hashes the pinned official provider, before passing the private fixture.
    println!("PREVIEW_WRY_STEP origin-bound");
    let state_dir = root.join("preview-product-state");
    fs::create_dir(&state_dir).expect("fresh preview product state");
    let repo =
        WorkspaceRepository::open(state_dir.join("workspace.json")).expect("preview repository");
    let project = crate::cli::workspace::register_project(&repo, &project_dir)
        .expect("preview project binding");
    let mut profile = Profile::new("codex-preview-only", CliKind::Codex);
    profile.program_path = Override::Set(
        fixture
            .codex
            .to_str()
            .expect("pinned provider UTF-8")
            .into(),
    );
    let saved = repo
        .apply(
            repo.read().expect("preview profile read").revision,
            Patch::Create { profile },
        )
        .expect("preview profile write");
    let target = json!({"kind":"profile","profileId":"codex-preview-only","expectedProfileRevision":saved.profiles["codex-preview-only"].revision,"projectIds":[project.project_id]});
    let ids = json!({"active":fixture.active,"archived":fixture.archived,"cwd":project_dir});
    let mut inherited = crate::cli::environment::EnvMap::new();
    for name in [
        "SystemRoot",
        "WINDIR",
        "SystemDrive",
        "ComSpec",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
        "CODEX_HOME",
        "CODEX_SQLITE_HOME",
        "HOMEDRIVE",
        "HOMEPATH",
        "GIT_CEILING_DIRECTORIES",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_GLOBAL",
    ] {
        inherited.insert(
            name.into(),
            std::env::var_os(name).expect("Stage-A isolated env"),
        );
    }
    println!("PREVIEW_WRY_STEP profile-bound");
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
        data_directory: Some(state_dir.join("webview")),
        ..Default::default()
    });
    let main = take_main_config(context.config_mut()).expect("preview main config");
    let setup = runtime.clone();
    let loaded = AtomicBool::new(false);
    let app = tauri::Builder::default()
        .any_thread()
        .manage(runtime)
        .invoke_handler(tauri::generate_handler![
            crate::commands::native_get_scope,
            crate::commands::native_list_resources,
            crate::cli::commands::cli_preview_session_delete,
            preview_fixture_finish
        ])
        .setup(move |app| {
            println!("PREVIEW_WRY_STEP main-build-start");
            setup.initialize_main(app, &main)?;
            println!("PREVIEW_WRY_STEP main-built");
            Ok(())
        })
        .on_page_load(move |view, event| {
            if view.label() != "main"
                || !matches!(event.event(), PageLoadEvent::Finished)
                || loaded.swap(true, Ordering::SeqCst)
            {
                return;
            }
            println!("PREVIEW_WRY_STEP page-finished");
            let script = format!(
                "{}\nrunOwnedDeletePreview({},{});",
                include_str!("fixtures/document/codex-delete-preview-only.js"),
                target,
                ids
            );
            if view.eval(script).is_err() {
                println!("PREVIEW_WRY_STEP eval-rejected");
                view.app_handle().exit(1);
            } else {
                println!("PREVIEW_WRY_STEP eval-queued");
            }
        })
        .build(context)
        .expect("real preview WebView start");
    println!("PREVIEW_WRY_STEP loop-enter");
    let exit = app.run_return(|_, _| {});
    println!("PREVIEW_WRY_STEP loop-exit");
    assert_eq!(exit, 0);
}
