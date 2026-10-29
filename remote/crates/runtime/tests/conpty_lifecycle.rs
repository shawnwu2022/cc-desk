//! TEST_ONLY private handle-transfer harness. No PID lookup or production IPC.
#![cfg(windows)]
use std::{
    io::{Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{DuplicateHandle, DUPLICATE_HANDLE_OPTIONS, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, WaitForSingleObject, PROCESS_DUP_HANDLE,
    },
};
struct Supervisor {
    child: Child,
    _input: ChildStdin,
    lines: mpsc::Receiver<Result<String, &'static str>>,
    stderr: mpsc::Receiver<Result<Vec<u8>, &'static str>>,
    end: Instant,
    _temp: tempfile::TempDir,
    cleanup_witnesses: Vec<OwnedHandle>,
    cleanup: CleanupState,
}
#[derive(Clone, Copy)]
enum CleanupState {
    NotStarted,
    Pending(Instant),
    Completed,
}
#[derive(Debug, PartialEq, Eq)]
enum ObserveError {
    Timeout,
    Control(&'static str),
}
impl Supervisor {
    fn register_witness(&mut self, witness: OwnedHandle) -> Result<(), &'static str> {
        if !matches!(self.cleanup, CleanupState::NotStarted) {
            return Err("CLEANUP_WITNESSES_SEALED");
        }
        self.cleanup_witnesses.push(witness);
        Ok(())
    }
    fn observe_until(&self, end: Instant) -> Result<Option<String>, ObserveError> {
        let end = end.min(self.end);
        if Instant::now() >= end {
            return Err(ObserveError::Timeout);
        }
        let result = match self
            .lines
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(Ok(line)) => Ok(Some(line)),
            Ok(Err(code)) => Err(ObserveError::Control(code)),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
            Err(_) => Err(ObserveError::Timeout),
        };
        if Instant::now() >= end {
            Err(ObserveError::Timeout)
        } else {
            result
        }
    }
    fn kill_and_observe(&mut self, end: Instant) -> Result<(), &'static str> {
        let end = match self.cleanup {
            CleanupState::Completed => return Ok(()),
            CleanupState::NotStarted => end,
            CleanupState::Pending(previous) => previous.min(end),
        };
        self.cleanup = CleanupState::Pending(end);
        let _ = self.child.kill();
        if Instant::now() >= end {
            return Err("CLEANUP_DEADLINE_EXPIRED");
        }
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(5)),
                _ => return Err("WORKER_EXIT_UNKNOWN"),
            }
        }
        for witness in &self.cleanup_witnesses {
            let ms = end
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(u32::MAX as u128) as u32;
            if unsafe { WaitForSingleObject(HANDLE(witness.as_raw_handle()), ms) } != WAIT_OBJECT_0
            {
                return Err("READONLY_WITNESS_CLEANUP_UNKNOWN");
            }
        }
        if Instant::now() >= end {
            return Err("CLEANUP_DEADLINE_EXPIRED");
        }
        self.cleanup = CleanupState::Completed;
        Ok(())
    }
    fn new(mode: &str) -> Self {
        let end = Instant::now() + Duration::from_secs(20);
        let temp = tempfile::tempdir().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_conpty-qualification-worker"))
            .arg(mode)
            .arg(temp.path().join("TEST_ONLY"))
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (tx, lines) = mpsc::sync_channel(16);
        let mut output = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            let mut line = Vec::new();
            let mut total = 0;
            loop {
                let mut b = [0];
                match output.read(&mut b) {
                    Ok(0) => {
                        if !line.is_empty() {
                            let _ = tx.send(Err("CONTROL_TRUNCATED"));
                        }
                        return;
                    }
                    Ok(_) => {
                        total += 1;
                        if total > 4096 || line.len() >= 192 {
                            let _ = tx.send(Err("CONTROL_LIMIT"));
                            return;
                        }
                        if b[0] == b'\n' {
                            match String::from_utf8(std::mem::take(&mut line)) {
                                Ok(s) => {
                                    if tx.send(Ok(s.trim_end_matches('\r').to_owned())).is_err() {
                                        return;
                                    }
                                }
                                Err(_) => {
                                    let _ = tx.send(Err("CONTROL_ENCODING"));
                                    return;
                                }
                            }
                        } else {
                            line.push(b[0]);
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(Err("CONTROL_IO"));
                        return;
                    }
                }
            }
        });
        let (err_tx, stderr) = mpsc::sync_channel(1);
        let mut error = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buf = [0; 512];
            loop {
                match error.read(&mut buf) {
                    Ok(0) => {
                        let _ = err_tx.send(Ok(bytes));
                        return;
                    }
                    Ok(n) if bytes.len() + n <= 8192 => bytes.extend_from_slice(&buf[..n]),
                    Ok(_) => {
                        let _ = err_tx.send(Err("STDERR_LIMIT"));
                        return;
                    }
                    Err(_) => {
                        let _ = err_tx.send(Err("STDERR_IO"));
                        return;
                    }
                }
            }
        });
        let mut input = child.stdin.take().unwrap();
        let mut in_worker = HANDLE::default();
        // Source is our own process; target is the exact worker we just created.
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                GetCurrentProcess(),
                HANDLE(child.as_raw_handle()),
                &mut in_worker,
                PROCESS_DUP_HANDLE.0,
                false,
                DUPLICATE_HANDLE_OPTIONS(0),
            )
        }
        .unwrap();
        // Numeric value is valid only in this exact child. It is not a PID/auth token.
        let _ = writeln!(input, "{:x}", in_worker.0 as usize);
        Self {
            child,
            _input: input,
            lines,
            stderr,
            end,
            _temp: temp,
            cleanup_witnesses: Vec::new(),
            cleanup: CleanupState::NotStarted,
        }
    }
    fn next(&self) -> Option<String> {
        self.observe_until(self.end)
            .expect("control observation failed; cleanup unknown")
    }
    fn witness(&mut self, prefix: &str) -> Witness {
        let line = self
            .next()
            .expect("worker exited before exact process witness");
        let hex = line
            .strip_prefix(prefix)
            .expect("missing exact witness event");
        assert!(!hex.is_empty() && hex.len() <= 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()));
        let value = usize::from_str_radix(hex, 16).unwrap();
        assert!(value != 0 && value != usize::MAX);
        // Only our trusted worker's DuplicateHandle installed this owned handle in
        // us; arbitrary external wire values never reach this TEST_ONLY harness.
        let witness = unsafe { OwnedHandle::from_raw_handle(value as *mut _) };
        self.register_witness(witness.try_clone().unwrap())
            .expect("witness registration after cleanup start");
        Witness(witness)
    }
    fn finish(&mut self, expect_success: bool) {
        let status = loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                break s;
            }
            assert!(
                Instant::now() < self.end,
                "worker deadline; cleanup unknown"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        let stderr = self
            .stderr
            .recv_timeout(self.end.saturating_duration_since(Instant::now()))
            .expect("stderr EOF deadline")
            .unwrap();
        println!(
            "worker success={} stderr={}",
            status.success(),
            String::from_utf8_lossy(&stderr)
        );
        assert_eq!(status.success(), expect_success);
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        let end = Instant::now() + Duration::from_secs(2);
        // Kill only our exact worker first, then observe read-only copies under one
        // separate finite cleanup budget; observers never terminate a fixture.
        if let Err(code) = self.kill_and_observe(end) {
            eprintln!("{code}");
        }
    }
}
struct Witness(OwnedHandle);
impl Witness {
    fn wait(&self, ms: u32) -> bool {
        (unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), ms) }) == WAIT_OBJECT_0
    }
    fn assert_alive(&self) {
        assert_eq!(
            unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), 0) },
            WAIT_TIMEOUT
        );
    }
    fn exit_code(&self) -> u32 {
        let mut code = 0;
        unsafe { GetExitCodeProcess(HANDLE(self.0.as_raw_handle()), &mut code) }.unwrap();
        code
    }
}
#[test]
fn abrupt_worker_exit_signals_exact_owned_cli_witness() {
    let mut supervisor = Supervisor::new("lifecycle-crash");
    let witness = supervisor.witness("WITNESS:");
    witness.assert_alive();
    assert_eq!(supervisor.next().as_deref(), Some("READY:CRASH"));
    supervisor.child.kill().unwrap();
    let wait = supervisor
        .end
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(500) as u32;
    assert!(
        witness.wait(wait),
        "exact fixture did not exit after abrupt owned worker termination"
    );
    println!(
        "observed exact CLI exit={} after worker abrupt exit",
        witness.exit_code()
    );
    assert_ne!(
        witness.exit_code(),
        90,
        "natural fixture watchdog is not abrupt cleanup"
    );
    while supervisor.next().is_some() {}
    supervisor.finish(false);
}
#[test]
fn real_output_stall_recovers_with_drain_and_stops_while_stalled() {
    let mut supervisor = Supervisor::new("lifecycle-pressure");
    let witness = supervisor.witness("WITNESS:");
    witness.assert_alive();
    let mut events = Vec::new();
    while let Some(line) = supervisor.next() {
        events.push(line);
    }
    assert!(
        events.iter().any(|s| s.starts_with("PRESSURE:STALLED1:")),
        "no measured first output stall"
    );
    assert!(
        events.iter().any(|s| s.starts_with("PRESSURE:RESUMED:")),
        "no causal drain/resume control"
    );
    assert!(
        events.iter().any(|s| s.starts_with("PRESSURE:STALLED2:")),
        "no measured second output stall"
    );
    assert!(events
        .iter()
        .any(|s| s == "PASS:PRESSURE_STOP_ROOT_EXIT_DRAIN_EOF"));
    assert!(witness.wait(0), "root exit not independently signaled");
    assert_ne!(
        witness.exit_code(),
        90,
        "natural fixture watchdog is not pressure cleanup"
    );
    println!("{}", events.join("\n"));
    supervisor.finish(true);
}
#[test]
fn abrupt_worker_exit_signals_exact_root_and_known_grandchild_separately() {
    let mut supervisor = Supervisor::new("lifecycle-tree");
    let root = supervisor.witness("WITNESS:");
    root.assert_alive();
    let grandchild = supervisor.witness("GRANDCHILD:");
    grandchild.assert_alive();
    let mut in_worker = HANDLE::default();
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            HANDLE(grandchild.0.as_raw_handle()),
            HANDLE(supervisor.child.as_raw_handle()),
            &mut in_worker,
            (windows::Win32::System::Threading::PROCESS_SYNCHRONIZE
                | windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION)
                .0,
            false,
            DUPLICATE_HANDLE_OPTIONS(0),
        )
    }
    .unwrap();
    writeln!(supervisor._input, "{:x}", in_worker.0 as usize).unwrap();
    assert_eq!(
        supervisor.next().as_deref(),
        Some("READY:TREE_JOB_MEMBERSHIP_VERIFIED")
    );
    supervisor.child.kill().unwrap();
    let end = (Instant::now() + Duration::from_millis(500)).min(supervisor.end);
    assert!(
        root.wait(end.saturating_duration_since(Instant::now()).as_millis() as u32),
        "root not signaled"
    );
    assert!(
        grandchild.wait(end.saturating_duration_since(Instant::now()).as_millis() as u32),
        "known grandchild not signaled"
    );
    println!(
        "separate exact root/grandchild signaled: root={} grandchild={}",
        root.exit_code(),
        grandchild.exit_code()
    );
    assert_ne!(root.exit_code(), 90, "root exited from natural watchdog");
    assert_ne!(
        grandchild.exit_code(),
        90,
        "grandchild exited from natural watchdog"
    );
    while supervisor.next().is_some() {}
    supervisor.finish(false);
}
#[test]
fn forced_observation_timeout_stays_timeout_then_owned_cleanup_is_observed() {
    let mut supervisor = Supervisor::new("lifecycle-crash");
    let witness = supervisor.witness("WITNESS:");
    witness.assert_alive();
    assert_eq!(supervisor.next().as_deref(), Some("READY:CRASH"));
    let started = Instant::now();
    let result = supervisor.observe_until(started + Duration::from_millis(120));
    assert_eq!(
        result,
        Err(ObserveError::Timeout),
        "timeout must remain failure, not success/EOF"
    );
    witness.assert_alive();
    assert!(supervisor.child.try_wait().unwrap().is_none());
    supervisor
        .kill_and_observe(Instant::now() + Duration::from_secs(2))
        .expect("timeout cleanup remains unknown");
    assert!(witness.wait(0));
    assert!(
        supervisor.kill_and_observe(Instant::now()).is_ok(),
        "completed cleanup must stay completed after clock deadline"
    );
    assert_eq!(
        supervisor.register_witness(witness.0.try_clone().unwrap()),
        Err("CLEANUP_WITNESSES_SEALED")
    );
    assert_ne!(
        witness.exit_code(),
        90,
        "natural fixture watchdog is not cleanup evidence"
    );
    assert_eq!(result, Err(ObserveError::Timeout));
    println!(
        "typed TIMEOUT preserved after {:?}; exact root cleanup observed separately",
        started.elapsed()
    );
    while supervisor.next().is_some() {}
    supervisor.finish(false);
}

#[test]
fn expired_cleanup_budget_cannot_reopen_when_a_late_owned_witness_exits() {
    struct OwnedProbe(Child);
    impl Drop for OwnedProbe {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let end = Instant::now() + Duration::from_secs(2);
            while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    let mut supervisor = Supervisor::new("lifecycle-crash");
    let root = supervisor.witness("WITNESS:");
    root.assert_alive();
    assert_eq!(supervisor.next().as_deref(), Some("READY:CRASH"));
    // A separately owned finite probe is a deliberate pending cleanup obligation,
    // not a claim that this process is a descendant of the worker under test.
    let mut probe = OwnedProbe(
        Command::new(env!("CARGO_BIN_EXE_conpty-qualification-worker"))
            .arg("--known-grandchild")
            .arg(supervisor._temp.path().join("TEST_ONLY_PENDING_CLEANUP"))
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut readonly = HANDLE::default();
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            HANDLE(probe.0.as_raw_handle()),
            GetCurrentProcess(),
            &mut readonly,
            (windows::Win32::System::Threading::PROCESS_SYNCHRONIZE
                | windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION)
                .0,
            false,
            DUPLICATE_HANDLE_OPTIONS(0),
        )
    }
    .unwrap();
    supervisor
        .register_witness(unsafe { OwnedHandle::from_raw_handle(readonly.0) })
        .unwrap();
    let first = supervisor.kill_and_observe(Instant::now() + Duration::from_millis(80));
    assert!(
        first.is_err(),
        "pending readonly probe must exhaust the first budget"
    );
    assert!(
        probe.0.try_wait().unwrap().is_none(),
        "fault-injection probe must still be alive"
    );
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        let _ = probe.0.kill();
        let end = Instant::now() + Duration::from_secs(2);
        let exited = loop {
            match probe.0.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(5)),
                _ => break false,
            }
        };
        let _ = tx.send(exited);
    });
    let started = Instant::now();
    let repeated = supervisor.kill_and_observe(Instant::now() + Duration::from_secs(1));
    let elapsed = started.elapsed();
    assert!(rx
        .recv_timeout(Duration::from_secs(2))
        .expect("owned probe cleanup thread deadline"));
    println!(
        "first cleanup={first:?}; repeated cleanup={repeated:?}; repeated elapsed={elapsed:?}"
    );
    assert!(
        repeated.is_err(),
        "late witness exit cannot reopen an expired cleanup budget"
    );
    assert_ne!(root.exit_code(), 90);
    while supervisor.next().is_some() {}
    supervisor.finish(false);
}
