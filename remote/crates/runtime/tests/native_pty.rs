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
fn native_pty_output_pressure_stop_retires_owned_io() {
    supervision::run("pressure-stop", "fixed_pty_runtime_worker");
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
    if mode == "pressure-stop" {
        pressure_stop(&dir, end);
        println!("PASS:NATIVE_PTY_TASK1");
        return;
    }
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

fn pressure_stop(dir: &std::path::Path, end: Instant) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let marker = dir.join("own-nonreading-worker-ready");
    let recipe = BackendPtyLaunch::from_backend_recipe(
        env!("CARGO_BIN_EXE_native-pipe-fixture").into(),
        dir.to_owned(),
        vec!["TEST_ONLY_PTY_OUTPUT".into(), marker.as_os_str().into()],
        Vec::new(),
    )
    .unwrap();
    let mut run =
        NativeOwnedPtyRun::launch(recipe, NativePtySize::new(24, 80).unwrap(), end).unwrap();
    let mut input = run.take_writer().unwrap();
    let mut output = run.take_reader().unwrap();
    let (drain_tx, drain_rx) = mpsc::sync_channel(1);
    let (reply_tx, reply_rx) = mpsc::sync_channel(2);
    let pause = Arc::new(AtomicBool::new(false));
    let reader_pause = pause.clone();
    let (paused_tx, paused_rx) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut buf = [0; 4096];
        let mut total = 0usize;
        let mut tail = Vec::new();
        let (mut cpr, mut da) = (false, false);
        let result = loop {
            match output.read(&mut buf) {
                Ok(0) => break Ok(total),
                Ok(n) => {
                    total += n;
                    if total > 2 * 1024 * 1024 {
                        break Err(std::io::Error::other("TEST_ONLY_DRAIN_CAP"));
                    }
                    tail.extend_from_slice(&buf[..n]);
                    if !cpr && tail.windows(4).any(|w| w == b"\x1b[6n") {
                        let _ = reply_tx.try_send(&b"\x1b[1;1R"[..]);
                        cpr = true;
                    }
                    if !da
                        && (tail.windows(3).any(|w| w == b"\x1b[c")
                            || tail.windows(4).any(|w| w == b"\x1b[0c"))
                    {
                        let _ = reply_tx.try_send(&b"\x1b[?1;2c"[..]);
                        da = true;
                    }
                    if tail.len() > 8 {
                        tail.drain(..tail.len() - 8);
                    }
                    if reader_pause.load(Ordering::Acquire) {
                        let _ = paused_tx.try_send(());
                        while reader_pause.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                }
                Err(e) => break Err(e),
            }
        };
        let _ = drain_tx.send(result);
    });
    let ready_end = (Instant::now() + Duration::from_secs(2)).min(end);
    while std::fs::read(&marker).ok().as_deref() != Some(b"TEST_ONLY_OUTPUT") {
        while let Ok(reply) = reply_rx.try_recv() {
            input.write_all(reply).unwrap();
        }
        assert!(
            Instant::now() < ready_end,
            "nonreading fixture never became ready"
        );
        assert!(
            run.try_wait_root().unwrap().is_none(),
            "fixture exited before readiness"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(std::fs::read(&marker).unwrap(), b"TEST_ONLY_OUTPUT");
    // Test-only startup replies have ended before pressure owns the writer.
    // The drain thread never takes a writer lock or waits for this channel.
    drop(reply_rx);
    struct ResumeDrain(Arc<AtomicBool>);
    impl Drop for ResumeDrain {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    // Declared after run: unwind resumes drain before run/master destruction.
    let _resume = ResumeDrain(pause.clone());
    pause.store(true, Ordering::Release);
    std::fs::write(marker.with_extension("go"), b"TEST_ONLY_GO").unwrap();
    paused_rx
        .recv_timeout(end.saturating_duration_since(Instant::now()))
        .unwrap();
    // Observe an entered, incomplete output call across a real interval with
    // reader pause acknowledged. No kernel-stack or pipe-capacity claim.
    let pressure_end = (Instant::now() + Duration::from_secs(2)).min(end);
    let observed = loop {
        assert!(
            Instant::now() < pressure_end,
            "bounded pressure did not stall a write"
        );
        let progress = || {
            let read_count = |extension| {
                std::fs::read_to_string(marker.with_extension(extension))
                    .ok()
                    .filter(|s| s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|s| s.parse::<usize>().ok())
            };
            read_count("entered").zip(read_count("progress"))
        };
        let before = progress();
        std::thread::sleep(Duration::from_millis(100));
        let after = progress();
        assert!(
            !matches!(after, Some((_, 64))),
            "bounded output completed without pressure"
        );
        if let Some((entered, completed)) = before {
            if entered <= 64 && entered > completed && before == after {
                break (entered, completed);
            }
        }
    };
    assert!(run.try_wait_root().unwrap().is_none());
    let stopped = run.stop_owned_job_until(end).unwrap();
    assert_eq!(stopped.observed_active_processes(), 0);
    assert_eq!(stopped.root_exit.exit_code, 1);
    assert!(matches!(run.job_active_processes(), Ok(None)));
    // Resume the independent reader before ClosePseudoConsole; input and master
    // resources are released on this thread, not abandoned to a detached timer.
    pause.store(false, Ordering::Release);
    drop(input);
    drop(run);
    assert!(
        Instant::now() < end,
        "master destructor returned after the original deadline"
    );
    let bytes = drain_rx
        .recv_timeout(end.saturating_duration_since(Instant::now()))
        .unwrap()
        .unwrap();
    while !reader.is_finished() {
        assert!(Instant::now() < end, "owned IO thread retirement unknown");
        std::thread::sleep(Duration::from_millis(1));
    }
    reader.join().unwrap();
    assert!(Instant::now() < end);
    eprintln!("TEST_ONLY_OUTPUT_PRESSURE entered_completed_at_pause={observed:?}; owned_job_zero=true; master_drop_returned=true; input_dropped=true; reader_eof_joined=true; drained_bytes={bytes}; openconsole_process_exit=NOT_PROVED");
}
