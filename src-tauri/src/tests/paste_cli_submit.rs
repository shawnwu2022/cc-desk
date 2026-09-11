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

#[derive(Deserialize)]
struct Case {
    name: String,
    wire: String,
    expected: String,
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

fn run_case(program: &Path, case: &Case) {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let config = temp.path().join("config");
    let home = temp.path().join("home");
    for dir in [&project, &config, &home] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let capture = temp.path().join("submitted.txt");
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
    let mut cmd = if program.extension().is_some_and(|ext| ext == "js") {
        let mut command = CommandBuilder::new(node_path());
        command.arg(program);
        command
    } else {
        CommandBuilder::new(program)
    };
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
                return Err("Claude did not reach its interactive prompt".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Real frontend wire, actual application writer; no hand-built transport.
        write_pty_data(&mut *writer, case.wire.as_bytes()).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(1000));
        if capture.exists() {
            return Err("Paste submitted before the explicit Enter key".into());
        }
        writer.write_all(b"\r").map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        let start = Instant::now();
        while !capture.exists() {
            if start.elapsed() > Duration::from_secs(45) {
                return Err("No complete UserPromptSubmit capture".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let actual = std::fs::read_to_string(&capture).map_err(|e| e.to_string())?;
        if actual != case.expected {
            let mismatch = actual
                .as_bytes()
                .iter()
                .zip(case.expected.as_bytes())
                .position(|(a, b)| a != b);
            return Err(format!(
                "submitted JSON differs: expected {} bytes, got {}, first mismatch {mismatch:?}",
                case.expected.len(),
                actual.len()
            ));
        }
        serde_json::from_str::<serde_json::Value>(&actual)
            .map_err(|e| format!("submitted JSON is invalid: {e}"))?;
        println!(
            "[PASS real Claude] {}: {} UTF-8 bytes, exact submitted JSON including all formatting",
            case.name,
            actual.len()
        );
        Ok(())
    })();
    let _ = done.send(());
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    drop(pair.master);
    let _ = reader_thread.join();
    let _ = watchdog.join();
    if let Err(error) = result {
        panic!(
            "{}: {error}\nSynthetic-session output tail: {}",
            case.name,
            tail(&output)
        );
    }
}

#[test]
#[ignore = "requires explicit disposable Claude CLI acceptance environment"]
fn RealClaude_DevtoolsJsonSubmittedCompletely_001() {
    let program =
        PathBuf::from(std::env::var_os("CC_E2E_CLAUDE_PATH").expect("set CC_E2E_CLAUDE_PATH"));
    let file = std::env::var_os("CC_PASTE_PAYLOAD_FILE")
        .expect("set CC_PASTE_PAYLOAD_FILE to real buildPastePayload fixtures");
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert!(
        !cases.is_empty(),
        "acceptance cases must not silently be empty"
    );
    for case in cases {
        run_case(&program, &case);
    }
}
