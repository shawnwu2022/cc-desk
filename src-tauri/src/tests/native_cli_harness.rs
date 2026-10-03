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
            return Err("probe did not become ready for terminal input".to_string());
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
    // 仅在 Windows 查询退出状态，避免 Unix try_wait 回收子进程并改变后续清理语义。
    let (
        child_state_before_kill,
        child_exit_before_kill,
        output_bytes_before_kill,
        ready_before_kill,
    ) = if write_result.is_err() {
        #[cfg(windows)]
        let (state, exit_code) = match child.try_wait() {
            Ok(Some(status)) => ("exited", Some(status.exit_code())),
            Ok(None) => ("running", None),
            Err(_) => ("query-failed", None),
        };
        #[cfg(not(windows))]
        let (state, exit_code) = ("not-sampled", None::<u32>);
        (
            state,
            exit_code,
            output.lock().ok().map(|bytes| bytes.len()),
            Some(ready_path.exists()),
        )
    } else {
        ("not-sampled", None, None, None)
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
    if let Err(failure) = write_result {
        // The fixture output may contain paths or inherited environment error
        // text. Report only bounded observations, never raw terminal contents.
        let bytes = output.lock().map_err(|_| "output lock poisoned")?;
        return Err(format!(
            "{failure}; ready_timeout_ms={}; child_state_before_kill={}; child_exit_before_kill={:?}; output_bytes_before_kill={:?}; ready_before_kill={:?}; ready_after_wait={}; report_exists={}; child_exit={:?}; output_bytes={}; reader_ok={}; probe_error_marker={}; node_error_marker={}; cursor_query={}; da1_query={}; focus_enable={}; focus_disable={}; win32_input_enable={}; win32_input_disable={}; window_show={}; window_hide={}",
            PROBE_READY_TIMEOUT.as_millis(),
            child_state_before_kill,
            child_exit_before_kill,
            output_bytes_before_kill,
            ready_before_kill,
            ready_path.exists(),
            report_path.exists(),
            status.as_ref().ok().map(|status| status.exit_code()),
            bytes.len(),
            reader_result.is_ok(),
            find_subslice(&bytes, b"probe error:").is_some(),
            find_subslice(&bytes, b"node:internal").is_some(),
            find_subslice(&bytes, b"\x1b[6n").is_some(),
            find_subslice(&bytes, b"\x1b[c").is_some()
                || find_subslice(&bytes, b"\x1b[0c").is_some(),
            find_subslice(&bytes, b"\x1b[?1004h").is_some(),
            find_subslice(&bytes, b"\x1b[?1004l").is_some(),
            find_subslice(&bytes, b"\x1b[?9001h").is_some(),
            find_subslice(&bytes, b"\x1b[?9001l").is_some(),
            find_subslice(&bytes, b"\x1b[1t").is_some(),
            find_subslice(&bytes, b"\x1b[2t").is_some(),
        ));
    }
    reader_result?;
    let status = status?;
    let exit_code = status.exit_code();
    let stdout = output
        .lock()
        .map_err(|_| "output lock poisoned".to_string())?
        .clone();

    let report_text = std::fs::read_to_string(report_path).map_err(|error| {
        let preview_len = stdout.len().min(512);
        format!(
            "failed to read probe report after exit {exit_code}: {error}; stdout={:?}",
            String::from_utf8_lossy(&stdout[..preview_len])
        )
    })?;
    let report = serde_json::from_str(&report_text).map_err(|error| error.to_string())?;

    Ok(ProbeExecution {
        report,
        stdout,
        exit_code,
    })
}

fn strings(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn extract_marked_output(output: &[u8], marker: &str) -> Result<Vec<u8>, String> {
    let begin = format!("<<CC_DESK_PROBE_OUTPUT_BEGIN:{marker}>>").into_bytes();
    let end = format!("<<CC_DESK_PROBE_OUTPUT_END:{marker}>>").into_bytes();
    let begin_at =
        find_subslice(output, &begin).ok_or_else(|| "output begin marker missing".to_string())?;
    let payload_at = begin_at + begin.len();
    let end_relative = find_subslice(&output[payload_at..], &end)
        .ok_or_else(|| "output end marker missing".to_string())?;
    Ok(output[payload_at..payload_at + end_relative].to_vec())
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
fn NativeCliHarness_PtyCapturesTerminalBytes_002() {
    let temp = tempfile::tempdir().expect("tempdir");
    let report = temp.path().join("capture.json");
    // ESC [ A, DEL, and printable input are valid terminal byte sequences.
    // Arbitrary invalid UTF-8 bytes are tested before the OS PTY boundary in W4/W5.
    let input = [0x1b, 0x5b, 0x41, 0x7f, 0x78];
    let arguments = strings(&["--capture-input", "--capture-bytes", "5"]);

    let execution = spawn_probe(
        temp.path(),
        temp.path(),
        &arguments,
        &[("CC_DESK_FIXTURE_VALUE", "capture")],
        Some(&input),
        &report,
    )
    .expect("terminal input through PTY");

    assert_eq!(execution.exit_code, 0);
    assert_eq!(
        execution.report.captured_base64.as_deref(),
        Some("G1tBf3g=")
    );
}

#[test]
fn NativeCliHarness_PtyReportsNonzeroExitAndTailOutput_003() {
    let temp = tempfile::tempdir().expect("tempdir");
    let report = temp.path().join("exit.json");
    let marker = "d03-tail-003";
    let arguments = strings(&[
        "--output-bytes",
        "257",
        "--output-marker",
        marker,
        "--exit-code",
        "7",
    ]);

    let execution = spawn_probe(
        temp.path(),
        temp.path(),
        &arguments,
        &[("CC_DESK_FIXTURE_VALUE", "exit")],
        None,
        &report,
    )
    .expect("nonzero probe through PTY");

    let payload = extract_marked_output(&execution.stdout, marker).expect("marked probe output");
    assert_eq!(execution.exit_code, 7);
    assert_eq!(execution.report.requested_exit_code, 7);
    assert_eq!(payload.len(), 257);
    assert!(payload.iter().all(|byte| *byte == b'x'));
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
