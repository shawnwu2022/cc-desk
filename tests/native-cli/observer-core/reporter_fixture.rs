use crate::observer_registry::{ObserverDelivery, ObserverRegistry, ObserverRun, ObserverSource};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn command_for(executable: &Path, key: &str) -> String {
    let args = crate::codex_observer::hook_args(executable).unwrap();
    let prefix = format!("hooks.{key}=");
    let value = args
        .iter()
        .filter_map(|arg| arg.to_str())
        .find_map(|arg| arg.strip_prefix(&prefix))
        .unwrap();
    let config: toml::Value = toml::from_str(&format!("value={value}")).unwrap();
    config["value"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn invoke(command: &str, executable: &Path, input: &[u8], environment: &[(String, String)]) {
    use std::os::windows::process::CommandExt;
    let mut process = if let Some((shell, encoded)) =
        command.split_once(" -NoLogo -NoProfile -NonInteractive -EncodedCommand ")
    {
        let mut process = Command::new(shell);
        process.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            encoded,
        ]);
        process
    } else {
        assert_eq!(
            command,
            format!(
                "{} --cc-desk-codex-observer",
                executable.to_string_lossy().replace('\\', "/")
            )
        );
        let mut process = Command::new(executable);
        process.arg("--cc-desk-codex-observer");
        process
    };
    process.env_clear();
    for name in ["SystemRoot", "WINDIR", "TEMP", "TMP", "ComSpec"] {
        if let Some(value) = std::env::var_os(name) {
            process.env(name, value);
        }
    }
    process.envs(environment.iter().map(|(key, value)| (key, value)));
    let mut child = process
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("owned reporter fixture deadline exceeded");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty(), "reporter must not emit stdout");
    let stderr = String::from_utf8_lossy(&result.stderr);
    // Windows PowerShell 首次载入模块可发宿主 progress；直连 reporter 仍须完全静默。
    let host_progress = command.contains(" -EncodedCommand ")
        && stderr.starts_with("#< CLIXML\r\n")
        && stderr.contains(" S=\"progress\"")
        && stderr
            .split(" S=\"")
            .skip(1)
            .all(|value| value.starts_with("progress\""));
    assert!(
        stderr.is_empty() || host_progress,
        "unexpected reporter or shell diagnostics"
    );
    assert!(!stderr.contains("SYNTHETIC_PRIVATE"));
}

fn fixture_root(original: &Path) -> tempfile::TempDir {
    // direct 分支要求完整绝对路径安全；TEMP 含特殊字符时使用夹具自身的构建目录。
    let parents = [
        std::env::temp_dir(),
        original.parent().unwrap().to_path_buf(),
    ];
    for parent in parents {
        let root = tempfile::tempdir_in(parent).unwrap();
        let executable = root.path().join("direct").join("cc-desk.exe");
        if !command_for(&executable, "SessionStart").contains(" -EncodedCommand ") {
            return root;
        }
    }
    panic!("no isolated absolute ASCII root available for required direct-route fixture");
}

pub fn verify(original: &Path) {
    let root = fixture_root(original);
    let registry = Arc::new(ObserverRegistry::new());
    let run_id = format!("run-{}", uuid::Uuid::new_v4().simple());
    let lease = registry
        .lease_source(
            ObserverRun {
                run_id: run_id.clone(),
                generation: 1,
            },
            ObserverDelivery::native("main"),
            Arc::new(|| true),
            ObserverSource::CodexHook,
        )
        .unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let output = received.clone();
    let app = crate::observer_http::router(
        registry,
        Arc::new(move |_, payload| {
            output
                .lock()
                .unwrap()
                .push(serde_json::to_value(payload).unwrap());
            Ok(())
        }),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown, ended) = tokio::sync::oneshot::channel::<()>();
    runtime.spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = ended.await;
            })
            .await
            .unwrap();
    });
    let mut environment = vec![
        ("CC_DESK_CODEX_PORT".into(), port.to_string()),
        ("CC_DESK_CODEX_RUN".into(), run_id.clone()),
        ("CC_DESK_CODEX_GENERATION".into(), "1".into()),
        (
            "CC_DESK_CODEX_CAPABILITY".into(),
            lease.binding().capability.clone(),
        ),
        ("HOME".into(), root.path().to_string_lossy().into_owned()),
        (
            "USERPROFILE".into(),
            root.path().to_string_lossy().into_owned(),
        ),
    ];
    // direct 根只使用生产准入允许的字符；其他四种路径必须进入真实 PowerShell 分支。
    let session = "11111111-2222-4333-8444-555555555555";
    let mut expected = 0;
    for (route, directory) in [
        ("space", "space path"),
        ("unicode", "中文"),
        ("apostrophe", "quote'path"),
        ("combined", "安装 路径'utf8"),
        ("direct", "direct"),
    ] {
        let directory = root.path().join(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("cc-desk.exe");
        std::fs::copy(original, &executable).unwrap();
        let capture = root.path().join(format!("{route}-capture.bin"));
        environment.push((
            "CC_DESK_TEST_STDIN_CAPTURE".into(),
            capture.to_string_lossy().into_owned(),
        ));
        let probe = serde_json::to_vec(
            &serde_json::json!({"hook_event_name":"SessionStart","session_id":session}),
        )
        .unwrap();
        let command = command_for(&executable, "SessionStart");
        assert_eq!(command.contains(" -EncodedCommand "), route != "direct");
        invoke(&command, &executable, &probe, &environment);
        environment.pop();
        let captured = std::fs::read(capture).unwrap();
        let normalized = captured
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(&captured);
        assert_eq!(
            normalized, probe,
            "wrapper must preserve the synthetic UTF-8 payload"
        );
        eprintln!(
            "WindowsReporter route={route} rawBytes={} bom={}",
            captured.len(),
            captured.starts_with(&[0xef, 0xbb, 0xbf])
        );
        let wrapper_boms = usize::from(captured.starts_with(&[0xef, 0xbb, 0xbf]));
        for event in ["SessionStart", "UserPromptSubmit", "Stop"] {
            for supplied_boms in [0, 1] {
                let payload = serde_json::to_vec(&serde_json::json!({"hook_event_name":event,"session_id":session,"prompt":"SYNTHETIC_PRIVATE","cwd":"SYNTHETIC_PRIVATE"})).unwrap();
                let mut input = [0xef, 0xbb, 0xbf].repeat(supplied_boms);
                input.extend(payload);
                let admitted = wrapper_boms + supplied_boms <= 1;
                invoke(
                    &command_for(&executable, event),
                    &executable,
                    &input,
                    &environment,
                );
                expected += usize::from(admitted);
                let deadline = Instant::now() + Duration::from_secs(2);
                while admitted
                    && received.lock().unwrap().len() < expected
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let values = received.lock().unwrap();
                assert_eq!(values.len(), expected, "actual {route} {event} supplied_boms={supplied_boms} wrapper_boms={wrapper_boms} must preserve input policy");
                if admitted {
                    let last = values.last().unwrap();
                    assert_eq!(last["sessionId"], session);
                    assert_eq!(last["eventName"], event);
                    assert_eq!(last["runId"], run_id);
                    assert_eq!(last["generation"], 1);
                    assert!(!last.to_string().contains("SYNTHETIC_PRIVATE"));
                }
            }
        }
        let mut rejected = environment.clone();
        rejected
            .iter_mut()
            .find(|(key, _)| key == "CC_DESK_CODEX_CAPABILITY")
            .unwrap()
            .1 = "ffffffffffffffffffffffffffffffff".into();
        invoke(&command, &executable, &probe, &rejected);
        assert_eq!(
            received.lock().unwrap().len(),
            expected,
            "BOM compatibility must not bypass capability admission"
        );
    }
    assert!(expected >= 15);
    lease.revoke();
    let _ = shutdown.send(());
}
