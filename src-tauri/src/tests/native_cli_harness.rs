use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const PROBE_READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) struct ProbeEnvironment {
    pub(crate) cc_desk_fixture_value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProbeReport {
    pub(crate) argv: Vec<String>,
    pub(crate) cwd: String,
    #[serde(rename = "stdinIsTTY")]
    pub(crate) stdin_is_tty: bool,
    #[serde(rename = "stdoutIsTTY")]
    pub(crate) stdout_is_tty: bool,
    pub(crate) env: ProbeEnvironment,
    pub(crate) captured_base64: Option<String>,
    pub(crate) requested_exit_code: u32,
}

pub(crate) struct ProbeExecution {
    pub(crate) report: ProbeReport,
    pub(crate) stdout: Vec<u8>,
    pub(crate) exit_code: u32,
}

fn node_path() -> Result<PathBuf, String> {
    crate::platform::find_executable(if cfg!(windows) { "node.exe" } else { "node" })
        .map(PathBuf::from)
        .ok_or_else(|| "Node.js is required for the native CLI probe".to_string())
}

fn probe_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a repository parent")
        .join("tests/fixtures/native-cli/probe.mjs")
}

fn wait_for_probe_ready(path: &Path) -> Result<(), String> {
    let started = Instant::now();
    loop {
        if path.exists() {
            return Ok(());
        }
        if started.elapsed() >= PROBE_READY_TIMEOUT {
            return Err("probe did not become ready for raw input".to_string());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn spawn_probe(
    test_root: &Path,
    cwd: &Path,
    probe_args: &[OsString],
    fixture_env: &[(&str, &str)],
    input: Option<&[u8]>,
    report_path: &Path,
) -> Result<ProbeExecution, String> {
    #[cfg(windows)]
    bundled_runtime::initialize()?;

    let ready_path = report_path.with_extension("ready");
    let mut command = CommandBuilder::new(node_path()?);
    command.arg(probe_path());
    command.arg("--report");
    command.arg(report_path);
    if input.is_some() {
        command.arg("--ready-file");
        command.arg(&ready_path);
    }
    for arg in probe_args {
        command.arg(arg);
    }
    command.cwd(cwd);
    for (name, value) in std::env::vars_os() {
        command.env(name, value);
    }
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("CC_DESK_TEST_ROOT", test_root);
    for (name, value) in fixture_env {
        command.env(name, value);
    }

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 35,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())?;
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| error.to_string())?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;

    let output = Arc::new(Mutex::new(Vec::new()));
    let collected = output.clone();
    let reader_thread = thread::spawn(move || -> Result<(), String> {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let mut bytes = collected.lock().map_err(|_| "output lock poisoned")?;
                    if bytes.len().saturating_add(count) > OUTPUT_LIMIT {
                        return Err("probe output exceeded 4 MiB".to_string());
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) if crate::pty::is_pty_stream_end(&error) => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    });

    let (done_tx, done_rx) = mpsc::channel();
    let mut killer = child.clone_killer();
    let watchdog = thread::spawn(move || {
        if matches!(
            done_rx.recv_timeout(PROBE_TIMEOUT),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            let _ = killer.kill();
        }
    });

    let write_result = if let Some(bytes) = input {
        wait_for_probe_ready(&ready_path).and_then(|()| {
            crate::pty::write_pty_data(&mut *writer, bytes).map_err(|error| error.to_string())
        })
    } else {
        Ok(())
    };
    if write_result.is_err() {
        let _ = child.kill();
    }

    let status = child.wait().map_err(|error| error.to_string());
    let _ = done_tx.send(());
    drop(writer);
    drop(pair.master);

    let reader_result = reader_thread
        .join()
        .map_err(|_| "probe reader thread panicked".to_string())?;
    watchdog
        .join()
        .map_err(|_| "probe watchdog thread panicked".to_string())?;
    write_result?;
    reader_result?;
    let status = status?;

    let report_text = std::fs::read_to_string(report_path).map_err(|error| error.to_string())?;
    let report = serde_json::from_str(&report_text).map_err(|error| error.to_string())?;
    let stdout = output
        .lock()
        .map_err(|_| "output lock poisoned".to_string())?
        .clone();

    Ok(ProbeExecution {
        report,
        stdout,
        exit_code: status.exit_code(),
    })
}

fn strings(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn NativeCliHarness_PtyPreservesIdentityAndArgv_001() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("工作 目录");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let report = temp.path().join("identity.json");
    let arguments = strings(&["--", "--future", "a b", "", "中文", "--", "-literal"]);

    let execution = spawn_probe(
        temp.path(),
        &cwd,
        &arguments,
        &[
            ("CC_DESK_FIXTURE_VALUE", "fixture-only"),
            ("CC_DESK_SECRET_SHOULD_NOT_APPEAR", "never-copy-this"),
        ],
        None,
        &report,
    )
    .expect("probe through PTY");

    assert_eq!(execution.exit_code, 0);
    assert!(execution.report.stdin_is_tty);
    assert!(execution.report.stdout_is_tty);
    assert_eq!(
        execution.report.argv,
        ["--future", "a b", "", "中文", "--", "-literal"]
    );
    assert_eq!(
        execution.report.env.cc_desk_fixture_value.as_deref(),
        Some("fixture-only")
    );
    assert_eq!(
        PathBuf::from(&execution.report.cwd)
            .canonicalize()
            .expect("reported cwd"),
        cwd.canonicalize().expect("expected cwd")
    );
    assert!(!std::fs::read_to_string(report)
        .expect("raw report")
        .contains("never-copy-this"));
}

#[test]
fn NativeCliHarness_PtyCapturesRawBytes_002() {
    let temp = tempfile::tempdir().expect("tempdir");
    let report = temp.path().join("capture.json");
    let input = [0x00, 0x1b, 0x7f, 0x80, 0xff];
    let arguments = strings(&["--capture-input", "--capture-bytes", "5"]);

    let execution = spawn_probe(
        temp.path(),
        temp.path(),
        &arguments,
        &[("CC_DESK_FIXTURE_VALUE", "capture")],
        Some(&input),
        &report,
    )
    .expect("raw input through PTY");

    assert_eq!(execution.exit_code, 0);
    assert_eq!(
        execution.report.captured_base64.as_deref(),
        Some("ABt/gP8=")
    );
}

#[test]
fn NativeCliHarness_PtyReportsNonzeroExitAndTailOutput_003() {
    let temp = tempfile::tempdir().expect("tempdir");
    let report = temp.path().join("exit.json");
    let arguments = strings(&["--output-bytes", "257", "--exit-code", "7"]);

    let execution = spawn_probe(
        temp.path(),
        temp.path(),
        &arguments,
        &[("CC_DESK_FIXTURE_VALUE", "exit")],
        None,
        &report,
    )
    .expect("nonzero probe through PTY");

    assert_eq!(execution.exit_code, 7);
    assert_eq!(execution.report.requested_exit_code, 7);
    assert_eq!(execution.stdout.len(), 257);
    assert!(execution.stdout.iter().all(|byte| *byte == b'x'));
}

#[test]
fn NativeCliHarness_PtyBuffersInputBeforeDelayedRead_004() {
    let temp = tempfile::tempdir().expect("tempdir");
    let report = temp.path().join("delayed.json");
    let input = b"late";
    let arguments = strings(&[
        "--capture-input",
        "--capture-bytes",
        "4",
        "--delay-read-ms",
        "50",
    ]);

    let execution = spawn_probe(
        temp.path(),
        temp.path(),
        &arguments,
        &[("CC_DESK_FIXTURE_VALUE", "delay")],
        Some(input),
        &report,
    )
    .expect("delayed read through PTY");

    assert_eq!(execution.exit_code, 0);
    assert_eq!(
        execution.report.captured_base64.as_deref(),
        Some("bGF0ZQ==")
    );
}
