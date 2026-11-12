//! Direct owned-child evidence, independent of registries and output readers.
#![allow(non_snake_case)]
use super::*;
use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};

#[test]
fn HistoryRuntime_WaitSettlesExactOwnedChild_01() {
    let gate = AdmissionGate::new();
    #[cfg(windows)]
    let child = std::process::Command::new("cmd.exe")
        .args(["/C", "exit", "0"])
        .spawn()
        .unwrap();
    #[cfg(not(windows))]
    let child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let mut child = AdmittedChild::created(
        Box::new(child),
        gate.begin_start(RuntimeKind::Legacy).unwrap(),
    );
    assert!(gate.freeze(&uuid::Uuid::new_v4().to_string()).is_err());
    child.wait().unwrap();
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

#[cfg(windows)]
#[test]
fn HistoryRuntime_CachedExitWithoutSignalledHandleCannotSettle_02() {
    let gate = AdmissionGate::new();
    // A live child waits on its pipe. Inject only the cached observation: the
    // retained OS handle remains real and unsignalled throughout the assertion.
    let child = std::process::Command::new("cmd.exe")
        .args(["/C", "set /p WAIT="])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = AdmittedChild::created(
        Box::new(child),
        gate.begin_start(RuntimeKind::Native).unwrap(),
    );
    child.status = Some(ExitStatus::with_exit_code(0));
    let premature_exit = child.try_wait().unwrap();
    assert!(gate.check_codex_archive_idle().is_err());
    assert!(gate.freeze(&uuid::Uuid::new_v4().to_string()).is_err());
    child.kill().unwrap();
    // 保留同一进程句柄直到真正退出；由下一次非阻塞轮询而非 wait() 回收票据。
    assert_eq!(
        unsafe { wait_for_single_object(child.raw_handle(), 10_000) },
        0
    );
    let confirmed_exit = child.try_wait().unwrap();
    assert!(
        premature_exit.is_none(),
        "cached exit must not let the command owner abandon an unreaped admission"
    );
    assert!(confirmed_exit.is_some());
    assert!(gate.check_codex_archive_idle().is_ok());
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

#[cfg(unix)]
#[test]
fn HistoryRuntime_UnixTryWaitAlreadyReaps_03() {
    let gate = AdmissionGate::new();
    let child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let mut child = AdmittedChild::created(
        Box::new(child),
        gate.begin_start(RuntimeKind::Native).unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

#[cfg(windows)]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

#[test]
fn HistoryRuntime_NativeAdapterProvesSpawnErrorCreatesNoChild_04() {
    #[cfg(windows)]
    bundled_runtime::initialize().unwrap();
    let root = tempfile::tempdir().unwrap();
    let gate = AdmissionGate::new();
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let command = CommandBuilder::new(root.path().join("missing-program.exe"));
    assert!(AdmittedChild::spawn_native(
        pair.slave.as_ref(),
        command,
        gate.begin_start(RuntimeKind::Native).unwrap()
    )
    .is_err());
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}
