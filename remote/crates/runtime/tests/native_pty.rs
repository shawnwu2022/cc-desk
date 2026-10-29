//! Default-off runtime typed PTY prerequisite; not a live desktop/remote path.
#![cfg(all(windows, feature = "conpty-qualification"))]
use cc_desk_native_runtime::{BackendPtyLaunch, NativePtyError, NativePtyPhase};
use cc_desk_native_runtime::{NativeOwnedPtyRun, NativePtySize};
use std::{
    io::{Read, Write},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
#[path = "native_pty/supervisor.rs"]
mod supervision;
#[test]
fn backend_pty_recipe_reuses_bounded_validation() {
    let result = BackendPtyLaunch::from_backend_recipe(
        "relative.exe".into(),
        "C:\\PUBLIC_TEST_ONLY".into(),
        Vec::new(),
        Vec::new(),
    );
    assert!(matches!(
        result,
        Err(NativePtyError::KnownNotStarted {
            phase: NativePtyPhase::Preparation
        })
    ));
}
#[test]
fn native_pty_size_is_checked_before_native_allocation() {
    for (rows, cols) in [(0, 80), (24, 0), (101, 80), (24, 241), (65535, 65535)] {
        assert!(matches!(
            NativePtySize::new(rows, cols),
            Err(NativePtyError::KnownNotStarted {
                phase: NativePtyPhase::Preparation
            })
        ));
    }
    assert!(NativePtySize::new(100, 240).is_ok());
}
#[test]
fn native_pty_owned_io_and_exact_root() {
    supervision::run("custody", "fixed_pty_runtime_worker");
}
#[test]
fn native_pty_reports_preparation_when_pinned_runtime_is_missing() {
    supervision::run("loader-reject", "fixed_pty_runtime_worker");
}
fn recipe(dir: &std::path::Path) -> BackendPtyLaunch {
    let root = std::env::var_os("SystemRoot").unwrap();
    let program = std::path::PathBuf::from(root.clone())
        .join("System32/cmd.exe")
        .canonicalize()
        .unwrap();
    BackendPtyLaunch::from_backend_recipe(
        program,
        dir.to_owned(),
        vec!["/D".into(), "/Q".into(), "/K".into()],
        vec![
            ("SystemRoot".into(), root),
            ("TEMP".into(), dir.as_os_str().into()),
            ("TMP".into(), dir.as_os_str().into()),
            ("HOME".into(), dir.as_os_str().into()),
            ("USERPROFILE".into(), dir.as_os_str().into()),
            ("APPDATA".into(), dir.as_os_str().into()),
            ("LOCALAPPDATA".into(), dir.as_os_str().into()),
        ],
    )
    .unwrap()
}
#[test]
#[ignore = "parent-held, contained TEST_ONLY worker; GO required"]
fn fixed_pty_runtime_worker() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(18));
        std::process::exit(90);
    });
    let mut go = [0];
    std::io::stdin().read_exact(&mut go).unwrap();
    assert_eq!(go, [1]);
    let end = Instant::now() + Duration::from_secs(12);
    let dir = std::path::PathBuf::from(std::env::var_os("PTY_CLI_TEST_DIR").unwrap());
    let mode = std::env::var("PTY_CLI_TEST_MODE").unwrap();
    let result = NativeOwnedPtyRun::launch(recipe(&dir), NativePtySize::new(24, 80).unwrap(), end);
    if mode == "loader-reject" {
        assert!(matches!(
            result,
            Err(NativePtyError::KnownNotStarted {
                phase: NativePtyPhase::Preparation
            })
        ));
        // The exact Preparation classification is observed here. Ordering before
        // target creation is additionally enforced by the wrapper's source path;
        // this fixture is not an independent process-creation witness.
        println!("PASS:NATIVE_PTY_TASK1");
        return;
    }
    assert_eq!(mode, "custody");
    let mut run = result.unwrap();
    let input = run.take_writer().expect("once input");
    assert!(run.take_writer().is_none());
    let mut output = run.take_reader().expect("once output");
    assert!(run.take_reader().is_none());
    let input = Arc::new(Mutex::new(input));
    let reply = input.clone();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        let mut da = false;
        let mut cpr = false;
        let result = loop {
            match output.read(&mut buf) {
                Ok(0) => break Ok(()),
                Err(e) => break Err(e),
                Ok(n) => {
                    if bytes.len() + n > 256 * 1024 {
                        break Err(std::io::Error::other("OUTPUT_CAP"));
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    if !da && !cpr && bytes.windows(4).any(|x| x == b"\x1b[6n") {
                        reply.lock().unwrap().write_all(b"\x1b[1;1R").unwrap();
                        cpr = true;
                    }
                    if !da
                        && (bytes.windows(3).any(|x| x == b"\x1b[c")
                            || bytes.windows(4).any(|x| x == b"\x1b[0c"))
                    {
                        reply.lock().unwrap().write_all(b"\x1b[?1;2c").unwrap();
                        da = true;
                    }
                }
            }
        };
        let _ = done_tx.send(result);
    });
    input
        .lock()
        .unwrap()
        .write_all(b"@echo off\recho TARGET>target.txt\r")
        .unwrap();
    while std::fs::read(dir.join("target.txt")).ok().as_deref() != Some(&b"TARGET\r\n"[..]) {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(run.try_wait_root().unwrap().is_none());
    // Holding the separately transferred input cannot block root control.
    let held = input.lock().unwrap();
    let stopped = run.stop_root_until(end).unwrap();
    assert_ne!(stopped.exit_code, 90);
    assert!(run.try_wait_root().unwrap().is_some());
    drop(held);
    drop(input);
    drop(run);
    done_rx
        .recv_timeout(end.saturating_duration_since(Instant::now()))
        .unwrap()
        .unwrap();
    while !reader.is_finished() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    reader.join().unwrap();
    assert!(Instant::now() < end);
    println!("PASS:NATIVE_PTY_TASK1");
}
