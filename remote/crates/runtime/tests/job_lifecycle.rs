#![cfg(windows)]
#![allow(non_snake_case)]
use cc_desk_native_runtime::*;
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    System::Threading::{
        GetCurrentProcess, GetProcessHandleCount, OpenProcess, WaitForSingleObject,
        PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION,
    },
};
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct FixtureChild(std::process::Child);
impl Drop for FixtureChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn command(role: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_owned-job-fixture"));
    c.arg(role)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}
fn ready(path: &std::path::Path) {
    let end = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        assert!(Instant::now() < end, "fixture did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn Host_UnreadOutputIndependentStop_001() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ready");
    let mut c = command("--write");
    c.arg(&marker).stdout(Stdio::piped());
    let mut run = OwnedJob::launch(BackendLaunch::from_backend_command(c)).expect("owned launch");
    ready(&marker);
    let stdout = run.take_stdout().unwrap();
    let mut available = 0;
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        unsafe {
            windows::Win32::System::Pipes::PeekNamedPipe(
                HANDLE(stdout.as_raw_handle()),
                None,
                0,
                None,
                Some(&mut available),
                None,
            )
        }
        .unwrap();
        if available >= 4096 {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(run.try_wait().unwrap().is_none());
    let stopped = Instant::now();
    run.stop(stopped + Duration::from_secs(2)).unwrap();
    assert!(stopped.elapsed() < Duration::from_secs(2));
    drop((stdout, run));
}
#[test]
fn Host_JobClosesChildAndGrandchild_001() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("descendant");
    let mut c = command("--grandchild");
    c.arg(&marker);
    let run = OwnedJob::launch(BackendLaunch::from_backend_command(c)).expect("owned launch");
    ready(&marker);
    let pid = std::fs::read_to_string(&marker).unwrap().parse().unwrap();
    // Query-only handle for the descendant just created by our own fixture; never PID kill.
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_ACCESS_RIGHTS(0x100000),
            false,
            pid,
        )
    }
    .unwrap();
    let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    drop(run);
    assert_eq!(
        unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 2000) },
        WAIT_OBJECT_0
    );
    drop(handle);
}
#[test]
fn Host_UnrelatedOwnedFixtureUnaffected_001() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut other = FixtureChild(command("--idle").spawn().unwrap());
    let mut run = OwnedJob::launch(BackendLaunch::from_backend_command(command("--idle")))
        .expect("owned launch");
    run.stop(Instant::now() + Duration::from_secs(2)).unwrap();
    assert!(other.0.try_wait().unwrap().is_none());
}
fn handles() -> u32 {
    let mut n = 0;
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut n) }.unwrap();
    n
}
#[test]
fn Host_TwentyLifecycleHandleProbe_001() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let before = handles();
    for _ in 0..20 {
        let mut run = OwnedJob::launch(BackendLaunch::from_backend_command(command("--exit")))
            .expect("owned launch");
        run.stop(Instant::now() + Duration::from_secs(2)).unwrap();
    }
    let after = handles();
    assert!(after <= before + 4, "handle growth: {before}->{after}");
    eprintln!("TEST_ONLY_JOB_CYCLES=20 HANDLES_BEFORE={before} HANDLES_AFTER={after}");
}
