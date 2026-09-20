//! Opt-in real Claude Code input acceptance, not a raw-stdin mock.
//! CI feeds buildPastePayload output to the production writer, then captures the
//! actual submitted prompt with a local UserPromptSubmit hook. The hook blocks
//! model processing. A disposable config and dummy loopback API provide a second
//! guard; no user's config, hooks, clipboard or API credentials are used.

use crate::pty::write_pty_data;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

// Use the same secure bootstrap as the production executable. Disabling this
// test option alone does not establish a system-backend control: Cargo also
// stages app-local runtime files beside the test executable.
#[allow(dead_code)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

#[derive(Deserialize)]
struct Case {
    name: String,
    wire: String,
    expected: String,
    #[serde(default, rename = "isJson")]
    is_json: bool,
    #[serde(default, rename = "launchMode")]
    launch_mode: LaunchMode,
    #[serde(default = "one_copy")]
    copies: usize,
}

fn one_copy() -> usize {
    1
}

// Launch selection is explicit; unknown modes cannot silently test the control.
#[derive(Clone, Copy, Debug, Default, Deserialize, serde::Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum LaunchMode {
    #[default]
    Direct,
    ProductionShell,
}

const CI_GIT_BASH: &str = "C:/Program Files/Git/bin/bash.exe";

fn quoted_ci_path(path: &Path) -> Result<String, String> {
    let path = path.to_str().ok_or("CLI path must be Unicode")?;
    if path
        .chars()
        .any(|ch| matches!(ch, '"' | '$' | '`' | '\r' | '\n' | '\0'))
    {
        return Err("Unsupported shell metacharacter in CI executable path".into());
    }
    Ok(format!("\"{}\"", path.replace('\\', "/")))
}

fn launch_command(program: &Path, mode: LaunchMode) -> Result<CommandBuilder, String> {
    if mode == LaunchMode::Direct {
        return Ok(if program.extension().is_some_and(|ext| ext == "js") {
            let mut command = CommandBuilder::new(node_path());
            command.arg(program);
            command
        } else {
            CommandBuilder::new(program)
        });
    }
    if !Path::new(CI_GIT_BASH).is_file() {
        return Err("production-shell acceptance requires CI Git Bash; no fallback".into());
    }
    let cli_command = if program.extension().is_some_and(|ext| ext == "js") {
        format!(
            "{} {}",
            quoted_ci_path(&node_path())?,
            quoted_ci_path(program)?
        )
    } else {
        quoted_ci_path(program)?
    };
    // Same selector/arguments as PtyManager::spawn_claude, not a hand-built bash -c.
    // User plugin/config injection and the WebView remain outside this test.
    let (shell, args) = crate::platform::get_claude_shell(&cli_command, Some(CI_GIT_BASH));
    let mut command = CommandBuilder::new(shell);
    for arg in args {
        command.arg(arg);
    }
    Ok(command)
}

// A shell may keep its native child alive. Only terminate the process tree
// rooted at the PID returned by this test's spawn; never search by image name.
fn terminate_case_tree(pid: Option<u32>) {
    if let Some(pid) = pid {
        let _ = crate::platform::new_command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

#[test]
fn PasteAcceptance_LaunchModeIsExplicit_004() {
    let case: Case = serde_json::from_str(
        r#"{"name":"probe","wire":"text","expected":"text","launchMode":"production-shell"}"#,
    )
    .unwrap();
    assert_eq!(case.launch_mode, LaunchMode::ProductionShell);
    let case: Case =
        serde_json::from_str(r#"{"name":"probe","wire":"text","expected":"text"}"#).unwrap();
    assert_eq!(case.launch_mode, LaunchMode::Direct);
}

#[test]
fn PasteAcceptance_UnknownLaunchFails_005() {
    assert!(serde_json::from_str::<Case>(
        r#"{"name":"probe","wire":"text","expected":"text","launchMode":"invalid"}"#,
    )
    .is_err());
}

#[test]
fn PasteAcceptance_ShellPathIsData_006() {
    assert_eq!(
        quoted_ci_path(Path::new(r"C:\Program Files\claude.exe")).unwrap(),
        "\"C:/Program Files/claude.exe\""
    );
    for path in [
        "C:/$(command)/cli.exe",
        "C:/`command`/cli.exe",
        "C:/\"/cli.exe",
        "C:/\n/cli.exe",
    ] {
        assert!(quoted_ci_path(Path::new(path)).is_err());
    }
}

fn node_path() -> PathBuf {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|path| path.join("node.exe"))
        .find(|path| path.is_file())
        .expect("Node.js is required to run the local capture hook")
}

const CAPTURE_HOOK: &str = r#"
const fs = require('node:fs');
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const event = JSON.parse(input);
    if (event.hook_event_name !== 'UserPromptSubmit' || typeof event.prompt !== 'string') {
      throw new Error('expected UserPromptSubmit text');
    }
    const dest = process.env.CC_PASTE_CAPTURE;
    fs.appendFileSync(dest + '.events', 'submit\n');
    fs.writeFileSync(dest + '.tmp', event.prompt, 'utf8');
    fs.renameSync(dest + '.tmp', dest);
    process.stdout.write(JSON.stringify({decision: 'block', reason: 'CC_DESK_CI_CAPTURED'}));
  } catch (error) {
    process.stderr.write('CC_DESK_CAPTURE_ERROR:' + String(error));
    process.exitCode = 2;
  }
});
"#;

fn tail(output: &Arc<Mutex<String>>) -> String {
    let text = output.lock().unwrap();
    text.chars()
        .rev()
        .take(2000)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn compare_prompt(actual: &str, expected: &str, is_json: bool) -> Result<(), String> {
    if actual != expected {
        let prefix = actual
            .as_bytes()
            .iter()
            .zip(expected.as_bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = actual
            .as_bytes()
            .iter()
            .rev()
            .zip(expected.as_bytes().iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        // Classification only: tab expansion still fails strict equality.
        let tabs_only = actual == expected.replace('\t', "    ");
        return Err(format!(
            "prompt differs: expected_bytes={} actual_bytes={} first_mismatch={} common_suffix={} tab_expansion_only={}",
            expected.len(),
            actual.len(),
            prefix,
            suffix,
            tabs_only
        ));
    }
    if is_json {
        serde_json::from_str::<serde_json::Value>(actual)
            .map_err(|e| format!("submitted JSON is invalid: {e}"))?;
    }
    Ok(())
}

#[test]
fn PasteAcceptance_PlainTextIsNotJson_001() {
    let text = "Error: value\n\tat Widget.refresh\n(匿名) @ Item.ts:128";
    assert!(compare_prompt(text, text, false).is_ok());
    assert!(compare_prompt(text, text, true).is_err());
}

#[test]
fn PasteAcceptance_MissingSuffixHasOffset_002() {
    let error = compare_prompt("head", "head\ntail", false).unwrap_err();
    assert!(error.contains("first_mismatch=4"));
}

#[test]
fn PasteAcceptance_ClassifyTabsWithoutHidingLoss_003() {
    let error = compare_prompt("head\n    tail", "head\n\ttail", false).unwrap_err();
    assert!(error.contains("tab_expansion_only=true"));
    let error = compare_prompt("tail", "head\n\ttail", false).unwrap_err();
    assert!(error.contains("tab_expansion_only=false"));
}

fn run_case(program: &Path, case: &Case) -> Result<(), String> {
    if std::env::var("CC_PASTE_BUNDLED_RUNTIME").as_deref() == Ok("1") {
        bundled_runtime::initialize()?;
    }
    if !(1..=3).contains(&case.copies) {
        return Err("copies must be between 1 and 3".into());
    }
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let config = temp.path().join("config");
    let home = temp.path().join("home");
    for dir in [&project, &config, &home] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let capture = temp.path().join("submitted.txt");
    let events = temp.path().join("submitted.txt.events");
    let helper = temp.path().join("capture.cjs");
    std::fs::write(&helper, CAPTURE_HOOK).unwrap();
    let hook_command = format!(
        "\"{}\" \"{}\"",
        node_path().to_string_lossy().replace('\\', "/"),
        helper.to_string_lossy().replace('\\', "/")
    );
    let settings = serde_json::json!({
        "hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": hook_command, "timeout": 10}]}]},
        "enableAllProjectMcpServers": false
    });
    std::fs::write(
        config.join("settings.json"),
        serde_json::to_vec_pretty(&settings).unwrap(),
    )
    .unwrap();
    let key = "cc-desk-ci-only-not-a-real-api-key-00000000000000000000";
    let mut projects = serde_json::Map::new();
    let trusted =
        serde_json::json!({"hasTrustDialogAccepted": true, "hasCompletedProjectOnboarding": true});
    projects.insert(project.to_string_lossy().into_owned(), trusted.clone());
    projects.insert(project.to_string_lossy().replace('\\', "/"), trusted);
    let global = serde_json::json!({
        "hasCompletedOnboarding": true, "theme": "dark", "numStartups": 1,
        "customApiKeyResponses": {"approved": [&key[key.len()-20..]], "rejected": []},
        "projects": projects
    });
    for path in [
        config.join(".claude.json"),
        home.join(".claude.json"),
        temp.path().join(".claude.json"),
    ] {
        std::fs::write(path, serde_json::to_vec_pretty(&global).unwrap()).unwrap();
    }

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 35,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = launch_command(program, case.launch_mode)?;
    cmd.cwd(&project);
    for (name, value) in [
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("CI", ""),
        ("ANTHROPIC_API_KEY", key),
        ("ANTHROPIC_AUTH_TOKEN", ""),
        ("CLAUDE_CODE_OAUTH_TOKEN", ""),
        ("ANTHROPIC_BASE_URL", "http://127.0.0.1:9"),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
        ("DISABLE_AUTOUPDATER", "1"),
    ] {
        cmd.env(name, value);
    }
    cmd.env("CLAUDE_CONFIG_DIR", &config);
    cmd.env("HOME", &home);
    cmd.env("USERPROFILE", &home);
    cmd.env("CC_PASTE_CAPTURE", &capture);
    if Path::new("C:/Program Files/Git/bin/bash.exe").is_file() {
        cmd.env(
            "CLAUDE_CODE_GIT_BASH_PATH",
            "C:/Program Files/Git/bin/bash.exe",
        );
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    let child_pid = child.process_id();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let output = Arc::new(Mutex::new(String::new()));
    let collected = output.clone();
    let reader_thread = std::thread::spawn(move || {
        let mut buf = [0; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut text = collected.lock().unwrap();
            if text.len() < 4 * 1024 * 1024 {
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        }
    });
    let (done, receiver) = mpsc::channel::<()>();
    let mut killer = child.clone_killer();
    let watchdog = std::thread::spawn(move || {
        if matches!(
            receiver.recv_timeout(Duration::from_secs(120)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            terminate_case_tree(child_pid);
            let _ = killer.kill();
        }
    });
    let result = (|| -> Result<(), String> {
        let start = Instant::now();
        let mut trusted_once = false;
        let mut key_once = false;
        loop {
            let text = output.lock().unwrap().clone();
            if text.contains("shift+tab")
                || text.contains("? for shortcuts")
                || text.contains("alt+m")
            {
                break;
            }
            let lower = text.to_lowercase();
            if !trusted_once && lower.contains("yes, i trust this folder") {
                writer.write_all(b"1\r").map_err(|e| e.to_string())?;
                writer.flush().map_err(|e| e.to_string())?;
                trusted_once = true;
            }
            if !key_once && lower.contains("do you want to use this api key") {
                writer.write_all(b"1\r").map_err(|e| e.to_string())?;
                writer.flush().map_err(|e| e.to_string())?;
                key_once = true;
            }
            if start.elapsed() > Duration::from_secs(45) {
                return Err(format!(
                    "Claude did not reach its prompt: {}",
                    tail(&output)
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Separate actual writes for consecutive paste, not one concatenated frame.
        for _ in 0..case.copies {
            write_pty_data(&mut *writer, case.wire.as_bytes()).map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_millis(1000));
            if capture.exists() || events.exists() {
                return Err("Paste submitted before the explicit Enter key".into());
            }
        }
        writer.write_all(b"\r").map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        let start = Instant::now();
        while !capture.exists() {
            if start.elapsed() > Duration::from_secs(45) {
                return Err(format!("No UserPromptSubmit capture: {}", tail(&output)));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(250));
        let count = std::fs::read_to_string(&events)
            .map_err(|e| e.to_string())?
            .lines()
            .count();
        if count != 1 {
            return Err(format!("Expected one submit, got {count}"));
        }
        let actual = std::fs::read_to_string(&capture).map_err(|e| e.to_string())?;
        compare_prompt(&actual, &case.expected.repeat(case.copies), case.is_json)
    })();
    let _ = done.send(());
    terminate_case_tree(child_pid);
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    drop(pair.master);
    let _ = reader_thread.join();
    let _ = watchdog.join();
    result
}

#[test]
#[ignore = "requires explicit disposable Claude CLI acceptance environment"]
fn RealClaude_DevtoolsJsonSubmittedCompletely_001() {
    // Retain the old test name for existing callers; run text and JSON alike.
    let program =
        PathBuf::from(std::env::var_os("CC_E2E_CLAUDE_PATH").expect("set CC_E2E_CLAUDE_PATH"));
    let file = std::env::var_os("CC_PASTE_PAYLOAD_FILE")
        .expect("set CC_PASTE_PAYLOAD_FILE to real buildPastePayload fixtures");
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert!(
        !cases.is_empty(),
        "acceptance cases must not silently be empty"
    );
    let mut failures = Vec::new();
    let mut results = Vec::new();
    for case in cases {
        let outcome = run_case(&program, &case);
        let detail = match outcome.as_ref() {
            Ok(()) => "exact submitted text".to_string(),
            Err(error) if error.starts_with("prompt differs:") => error.clone(),
            Err(_) => "launch-or-capture-error; see isolated CI log".to_string(),
        };
        results.push(serde_json::json!({
            "name": case.name,
            "launchMode": case.launch_mode,
            "expectedBytes": case.expected.len() * case.copies,
            "passed": outcome.is_ok(),
            "detail": detail
        }));
        println!("[launch {:?}] {}", case.launch_mode, case.name);
        match outcome {
            Ok(()) => println!(
                "[PASS real Claude] {}: {} UTF-8 bytes, exact submitted text",
                case.name,
                case.expected.len() * case.copies
            ),
            Err(error) => {
                println!("[FAIL real Claude] {}: {error}", case.name);
                failures.push(case.name);
            }
        }
    }
    if let Some(file) = std::env::var_os("CC_PASTE_RESULT_FILE") {
        std::fs::write(file, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }
    assert!(failures.is_empty(), "Failed cases: {}", failures.join(", "));
}
