//! Windows subprocess owns a real Tauri AppHandle and pinned ConPTY.
#![allow(non_snake_case)]
use super::*;
use std::cell::{Cell, RefCell};
use std::sync::Barrier;

thread_local! {
    static FAIL_IO: Cell<Option<&'static str>> = const { Cell::new(None) };
    static HOLD_WAITER: RefCell<Option<Arc<Barrier>>> = const { RefCell::new(None) };
    static FAIL_THREAD: Cell<Option<&'static str>> = const { Cell::new(None) };
}
pub(super) fn check_io(stage: &str) -> anyhow::Result<()> {
    if FAIL_IO.with(|fault| fault.get() == Some(stage)) {
        return Err(anyhow!("injected PTY handle acquisition failure"));
    }
    Ok(())
}
pub(super) fn waiter_barrier(name: &str) -> Option<Arc<Barrier>> {
    if name.starts_with("pty-waiter-") { HOLD_WAITER.with(|barrier| barrier.borrow().clone()) } else { None }
}
pub(super) fn fail_thread(name: &str) -> bool {
    FAIL_THREAD.with(|fault| fault.get().is_some_and(|prefix| name.starts_with(prefix)))
}

#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

#[test]
fn HistoryRuntime_LegacyRealPaths_01() {
    let root = tempfile::tempdir().unwrap();
    let log_path = root.path().join("worker.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut worker = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "pty::maintenance_tests::HistoryRuntime_LegacyWorker_99", "--ignored", "--test-threads=1", "--nocapture"])
        .env("CC_DESK_MAINTENANCE_TEST_ROOT", root.path())
        .stdout(log.try_clone().unwrap()).stderr(log).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = worker.try_wait().unwrap() {
            assert!(status.success(), "{}", std::fs::read_to_string(log_path).unwrap());
            break;
        }
        if Instant::now() > deadline {
            let _ = worker.kill(); let _ = worker.wait();
            panic!("Legacy maintenance worker timed out");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "isolated subprocess owned by HistoryRuntime_LegacyRealPaths_01"]
fn HistoryRuntime_LegacyWorker_99() {
    bundled_runtime::initialize().unwrap();
    let root = std::path::PathBuf::from(std::env::var_os("CC_DESK_MAINTENANCE_TEST_ROOT").unwrap());
    let app = tauri::Builder::default().any_thread()
        .build(tauri::generate_context!("src/tests/fixtures/document/tauri.conf.json")).unwrap();
    let gate = AdmissionGate::new();
    let manager = Arc::new(PtyManager::new(app.handle().clone(), gate.clone()));
    let frozen = gate.freeze(&Uuid::new_v4().to_string()).unwrap();
    let id = Uuid::new_v4().to_string();
    assert_eq!(manager.spawn_shell(id.clone(), root.to_str().unwrap(), 80, 24).unwrap_err().to_string(), "HISTORY_MAINTENANCE_ACTIVE");
    assert_eq!(manager.spawn_claude(id, root.to_str().unwrap(), 80, 24, None).unwrap_err().to_string(), "HISTORY_MAINTENANCE_ACTIVE");
    assert!(manager.instances.lock().is_empty());
    frozen.release_review().unwrap();

    // Both thread-creation errors happen after real CreateProcess. Existing
    // cleanup must carry the ticket into actual wait, not infer exit from removal.
    for thread_kind in ["pty-reader-", "pty-waiter-"] {
        let id = Uuid::new_v4().to_string();
        let mut command = CommandBuilder::new("cmd.exe");
        command.args(["/C", "set /p WAIT="]);
        command.cwd(&root);
        FAIL_THREAD.with(|fault| fault.set(Some(thread_kind)));
        let result = manager.spawn_command(gate.begin_start(RuntimeKind::Legacy).unwrap().preparing(),
            id, root.to_str().unwrap(), 80, 24, "shell", command, "maintenance fixture", None);
        FAIL_THREAD.with(|fault| fault.set(None));
        assert!(result.is_err());
        assert!(manager.instances.lock().is_empty());
        gate.freeze(&Uuid::new_v4().to_string()).unwrap().release_review().unwrap();
    }

    for stage in ["writer", "reader"] {
        let id = Uuid::new_v4().to_string();
        let mut command = CommandBuilder::new("cmd.exe");
        command.args(["/C", "set /p WAIT="]);
        command.cwd(&root);
        FAIL_IO.with(|fault| fault.set(Some(stage)));
        let result = manager.spawn_command(gate.begin_start(RuntimeKind::Legacy).unwrap().preparing(),
            id, root.to_str().unwrap(), 80, 24, "shell", command, "maintenance fixture", None);
        FAIL_IO.with(|fault| fault.set(None));
        assert!(result.is_err());
        gate.freeze(&Uuid::new_v4().to_string()).unwrap().release_review().unwrap();
    }

    // Hold the actual waiter before observing synthetic map removal.
    let barrier = Arc::new(Barrier::new(2));
    HOLD_WAITER.with(|held| *held.borrow_mut() = Some(barrier.clone()));
    // A production waiter must settle even when its weak manager is gone.
    let id = Uuid::new_v4().to_string();
    let mut command = CommandBuilder::new("cmd.exe");
    command.args(["/C", "set /p WAIT="]);
    command.cwd(&root);
    manager.spawn_command(gate.begin_start(RuntimeKind::Legacy).unwrap().preparing(),
        id.clone(), root.to_str().unwrap(), 80, 24, "shell", command, "maintenance fixture", None).unwrap();
    HOLD_WAITER.with(|held| held.borrow_mut().take());
    assert!(gate.freeze(&Uuid::new_v4().to_string()).is_err());
    let mut instance = manager.remove_registration(&id).unwrap();
    assert!(manager.instances.lock().is_empty());
    assert!(gate.freeze(&Uuid::new_v4().to_string()).is_err());
    drop(manager);
    let _ = instance.killer.kill();
    drop(instance);
    barrier.wait();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(frozen) = gate.freeze(&Uuid::new_v4().to_string()) {
            frozen.release_review().unwrap(); break;
        }
        assert!(Instant::now() < deadline, "waiter lost its maintenance ticket");
        thread::sleep(Duration::from_millis(10));
    }
}
