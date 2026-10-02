//! Actual Tauri/Wry/WebView2 probe in a disposable child event loop. Uses only
//! freshly created test directories and no ordinary CC Desk startup or sessions.
use crate::version_history::windows::{
    durability::DurableRecord,
    fence::ImageFence,
    files::{ComponentName, Directory, FileAccess, PrivateDirectory},
    process::ExactProcess,
    security::CurrentUser,
    webview::{
        actual_udf, reject_manager_overrides, verify_manager_udf, SourceWebViews,
        WebViewExitReceipt,
    },
};
use std::{
    cell::RefCell,
    ffi::OsStr,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

thread_local! { static EXIT_PROBE: RefCell<Option<SourceWebViews>> = const { RefCell::new(None) }; }
const WORKER: &str = "tests::version_history_webview::HistoryWebView_Worker_099";

// 检查真实 WebView 使用指定 UDF，关闭控制器后仅在 BrowserProcessExited 到达时允许宿主退出。
#[test]
fn HistoryWebView_Exit_001() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source-webview.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &source).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let name = ComponentName::new(OsStr::new("source-webview.exe")).unwrap();
    let image = parent.open_file(name.clone(), FileAccess::Read).unwrap();
    let identity = image.identity().clone();
    let digest = image.digest().unwrap();
    drop(image);
    let mut child = Command::new(&source)
        .args(["--exact", WORKER, "--ignored", "--nocapture"])
        .env("CC_DESK_HISTORY_WEBVIEW_ROOT", temporary.path())
        .spawn()
        .unwrap();
    let host = ExactProcess::capture_observed(child.id()).unwrap();
    std::fs::write(temporary.path().join("allow-close"), b"captured exact host").unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "real WebView worker failed");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("real WebView exit receipt never arrived");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let receipts: Vec<_> = std::fs::read_dir(temporary.path().join("receipts"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        receipts.len(),
        1,
        "one real, durable browser-exit receipt required"
    );
    let bytes = std::fs::read(temporary.path().join("receipts").join(&receipts[0])).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["browsers"].as_array().unwrap().len(), 1);
    assert_ne!(value["host"]["pid"], value["browsers"][0]["pid"]);
    let user = CurrentUser::capture().unwrap();
    let receipt_root = Arc::new(
        PrivateDirectory::open_existing(
            parent.clone(),
            ComponentName::new(OsStr::new("receipts")).unwrap(),
            &user,
        )
        .unwrap(),
    );
    let record = DurableRecord::open(
        receipt_root.clone(),
        ComponentName::new(&receipts[0]).unwrap(),
        &crate::version_history::verified_package::sha256(&bytes),
        &user,
    )
    .unwrap();
    let evidence = WebViewExitReceipt::open(record)
        .unwrap()
        .after_host_exit(host)
        .unwrap();
    assert!(
        ImageFence::acquire(parent.clone(), name.clone(), &identity, &digest).is_err(),
        "retained read image guard must conflict with an exclusive fence"
    );
    let evidence = evidence.release_image_for_fence().unwrap();
    let mut fence = ImageFence::acquire(parent, name, &identity, &digest)
        .expect("terminal source evidence must compose with image fencing");
    fence
        .rename_to(
            receipt_root.directory().clone(),
            ComponentName::new(OsStr::new("sealed-source.exe")).unwrap(),
        )
        .unwrap();
    evidence.verify().unwrap();
    fence.verify().unwrap();
}

// 监督测试显式执行此独立进程，避免在共享测试进程创建第二个 GUI 事件循环。
#[test]
#[ignore = "explicitly invoked by HistoryWebView_Exit_001"]
fn HistoryWebView_Worker_099() {
    reject_manager_overrides().expect("ambient WebView overrides must be absent, never removed");
    let root = PathBuf::from(std::env::var_os("CC_DESK_HISTORY_WEBVIEW_ROOT").unwrap());
    let parent = Directory::open_absolute(&root).unwrap();
    let user = CurrentUser::capture().unwrap();
    let udf = Arc::new(
        PrivateDirectory::create_new(
            parent.clone(),
            ComponentName::new(OsStr::new("udf")).unwrap(),
            &user,
        )
        .unwrap(),
    );
    let receipts = Arc::new(
        PrivateDirectory::create_new(
            parent,
            ComponentName::new(OsStr::new("receipts")).unwrap(),
            &user,
        )
        .unwrap(),
    );
    let complete = Arc::new(AtomicBool::new(false));
    let completion = complete.clone();
    let receipt_root = receipts.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .setup(move |app| {
            let window = WebviewWindowBuilder::new(
                app,
                "history-probe",
                WebviewUrl::External("about:blank".parse().unwrap()),
            )
            .visible(false)
            .data_directory(root.join("udf"))
            .build()?;
            let expected = udf.clone();
            window.with_webview(move |platform| {
                let environment = platform.environment();
                verify_manager_udf(&environment, &expected).unwrap();
                let actual = actual_udf(&environment).unwrap();
                assert_eq!(actual.identity(), expected.directory().identity());
                let mut source =
                    SourceWebViews::capture(vec![(environment, platform.controller())], actual)
                        .unwrap();
                assert!(source
                    .persist_when_exited(receipt_root.clone(), &CurrentUser::capture().unwrap())
                    .is_err());
                EXIT_PROBE.with(|cell| *cell.borrow_mut() = Some(source));
            })?;
            let close_signal = root.join("allow-close");
            let closed = Arc::new(AtomicBool::new(false));
            let handle = app.handle().clone();
            let root = receipts.clone();
            let complete = completion.clone();
            std::thread::spawn(move || {
                while !complete.load(Ordering::SeqCst) {
                    let app = handle.clone();
                    let root = root.clone();
                    let complete = complete.clone();
                    let close_signal = close_signal.clone();
                    let closed = closed.clone();
                    handle
                        .run_on_main_thread(move || {
                            if complete.load(Ordering::SeqCst) || !close_signal.exists() {
                                return;
                            }
                            EXIT_PROBE.with(|cell| {
                                if let Some(source) = cell.borrow_mut().as_mut() {
                                    if !closed.swap(true, Ordering::SeqCst) {
                                        source.close_controllers().unwrap();
                                    }
                                    if source
                                        .persist_when_exited(root, &CurrentUser::capture().unwrap())
                                        .unwrap()
                                        .is_some()
                                    {
                                        complete.store(true, Ordering::SeqCst);
                                        app.exit(0);
                                    }
                                }
                            });
                        })
                        .unwrap();
                    std::thread::sleep(Duration::from_millis(50));
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!(
            "src/tests/fixtures/document/tauri.conf.json"
        ))
        .unwrap();
    let exit = app.run_return(move |_, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            if !complete.load(Ordering::SeqCst) {
                api.prevent_exit();
            }
        }
    });
    EXIT_PROBE.with(|cell| {
        cell.borrow_mut().take();
    });
    assert_eq!(exit, 0);
}
