use crate::observer_registry::{ObserverDelivery, ObserverRegistry, ObserverRun, ObserverSource};
use std::io::{Read, Write};
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

const MAX_DIAGNOSTICS: usize = 64 * 1024;

struct Captured {
    bytes: Vec<u8>,
    total: usize,
}

struct Drain {
    result: std::sync::mpsc::Receiver<std::io::Result<Captured>>,
    count: Arc<std::sync::atomic::AtomicUsize>,
}

fn drain(mut stream: impl Read + Send + 'static) -> Drain {
    let (sender, receiver) = std::sync::mpsc::channel();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = count.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let mut captured = Captured {
                bytes: Vec::new(),
                total: 0,
            };
            let mut chunk = [0u8; 4096];
            loop {
                let count = stream.read(&mut chunk)?;
                if count == 0 {
                    break;
                }
                captured.total = captured.total.saturating_add(count);
                observed.store(captured.total, std::sync::atomic::Ordering::Relaxed);
                let retained = count.min(MAX_DIAGNOSTICS.saturating_sub(captured.bytes.len()));
                captured.bytes.extend_from_slice(&chunk[..retained]);
            }
            Ok(captured)
        })();
        let _ = sender.send(result);
    });
    Drain {
        result: receiver,
        count,
    }
}

fn phase_description(path: Option<&Path>) -> &'static str {
    let Some(path) = path else {
        return "none";
    };
    let mut bytes = Vec::new();
    let Ok(file) = std::fs::File::open(path) else {
        return "none";
    };
    if file.take(4097).read_to_end(&mut bytes).is_err() || bytes.len() > 4096 {
        return "unavailable";
    }
    let allowed = [
        "reporter-entered",
        "stdin-read",
        "capture-written",
        "ps-entered",
        "ps-info-created",
        "ps-process-created",
        "ps-before-start",
        "ps-started",
        "ps-copied",
        "ps-closed",
        "ps-waited",
        "ps-caught",
    ];
    String::from_utf8_lossy(&bytes)
        .split(';')
        .filter_map(|part| allowed.iter().copied().find(|value| *value == part))
        .next_back()
        .unwrap_or("none")
}

fn capture(mut process: Command, input: &[u8]) -> (std::process::ExitStatus, Captured, Captured) {
    let phase = process
        .get_envs()
        .find(|(key, _)| *key == "CC_DESK_TEST_PHASE")
        .and_then(|(_, value)| value.map(std::path::PathBuf::from));
    use std::os::windows::process::CommandExt;
    let shell_phase = process
        .get_envs()
        .find(|(key, _)| *key == "CC_DESK_TEST_PS_PHASE")
        .and_then(|(_, value)| value.map(std::path::PathBuf::from));
    let mut child = process
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    // 先并发排空两条管道；只保留有界数据，超限也继续读取，避免宿主 progress 反压。
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
    child.stdin.take().unwrap().write_all(input).unwrap();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "owned reporter fixture deadline exceeded; stdoutBytes={} stderrBytes={} reporterPhase={} shellPhase={}",
                stdout.count.load(std::sync::atomic::Ordering::Relaxed),
                stderr.count.load(std::sync::atomic::Ordering::Relaxed),
                phase_description(phase.as_deref()),
                phase_description(shell_phase.as_deref())
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().unwrap();
    let receive = |drain: Drain| {
        drain
            .result
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("owned reporter diagnostic drain deadline exceeded")
            .expect("owned reporter diagnostic read failed")
    };
    let stdout = receive(stdout);
    let stderr = receive(stderr);
    (status, stdout, stderr)
}

fn invoke(
    command: &str,
    executable: &Path,
    input: &[u8],
    environment: &[(String, String)],
) -> usize {
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
    let (status, stdout, stderr) = capture(process, input);
    assert!(status.success());
    assert!(
        stdout.total <= MAX_DIAGNOSTICS && stderr.total <= MAX_DIAGNOSTICS,
        "owned reporter diagnostics exceeded bounded retention"
    );
    assert_eq!(stdout.total, 0, "reporter must not emit stdout");
    let stderr_bytes = stderr.total;
    let stderr = String::from_utf8_lossy(&stderr.bytes);
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
    stderr_bytes
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
    let phase_path = root.path().join("phase.txt");
    environment.push((
        "CC_DESK_TEST_PHASE".into(),
        phase_path.to_string_lossy().into_owned(),
    ));
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
        std::fs::write(&phase_path, b"").unwrap();
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            invoke(&command, &executable, &probe, &environment)
        }));
        let stderr_bytes = match attempt {
            Ok(value) => value,
            Err(error) => {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    diagnose(&command, &executable, &probe, &environment, &phase_path)
                }));
                std::panic::resume_unwind(error);
            }
        };
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
            "WindowsReporter route={route} rawBytes={} bom={} hostStderrBytes={stderr_bytes}",
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

pub fn verify_pipe_drain() {
    use base64::Engine;
    let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    // 超过管道容量仍须排空，内存只保留固定上限；此处只使用合成字符。
    let script = "[Console]::Error.Write('x'*1048576);exit 0";
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut process = Command::new(shell);
    process.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        &base64::engine::general_purpose::STANDARD.encode(bytes),
    ]);
    process.env_clear();
    for name in ["SystemRoot", "WINDIR", "TEMP", "TMP", "ComSpec"] {
        if let Some(value) = std::env::var_os(name) {
            process.env(name, value);
        }
    }
    let (status, stdout, stderr) = capture(process, b"");
    assert!(status.success());
    assert_eq!(stdout.total, 0);
    assert_eq!(stderr.total, 1048576);
    assert_eq!(stderr.bytes.len(), MAX_DIAGNOSTICS);
    eprintln!(
        "WindowsReporter drainedSyntheticBytes={} retainedBytes={}",
        stderr.total,
        stderr.bytes.len()
    );
}

fn diagnose(
    command: &str,
    executable: &Path,
    input: &[u8],
    environment: &[(String, String)],
    phase: &Path,
) {
    use base64::Engine;
    let Some((shell, encoded)) =
        command.split_once(" -NoLogo -NoProfile -NonInteractive -EncodedCommand ")
    else {
        return;
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let words: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let mut script = String::from_utf16(&words).unwrap();
    let mark =
        |name: &str| format!("[IO.File]::AppendAllText($env:CC_DESK_TEST_PS_PHASE,'{name};');");
    for (before, after) in [
        ("try{".to_owned(), format!("try{{{}", mark("ps-entered"))),
        (
            "$i.FileName=".to_owned(),
            format!("{}$i.FileName=", mark("ps-info-created")),
        ),
        (
            "$p.StartInfo=".to_owned(),
            format!("{}$p.StartInfo=", mark("ps-process-created")),
        ),
        (
            "if($p.Start())".to_owned(),
            format!("{}if($p.Start())", mark("ps-before-start")),
        ),
        (
            "[Console]::OpenStandardInput()".to_owned(),
            format!("{}[Console]::OpenStandardInput()", mark("ps-started")),
        ),
        (
            "$p.StandardInput.Close();".to_owned(),
            format!(
                "{}$p.StandardInput.Close();{}",
                mark("ps-copied"),
                mark("ps-closed")
            ),
        ),
        (
            "$p.Dispose()".to_owned(),
            format!("{}$p.Dispose()", mark("ps-waited")),
        ),
        (
            "catch{}".to_owned(),
            format!("catch{{{}}}", mark("ps-caught")),
        ),
    ] {
        assert_eq!(script.matches(&before).count(), 1);
        script = script.replace(&before, &after);
    }
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let command = format!(
        "{shell} -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    // 原始尝试已失败；只在失败后做私有环境对照，诊断结果不能把原失败转成通过。
    for variable in [
        None,
        Some("PSModuleAnalysisCachePath"),
        Some("LOCALAPPDATA"),
        Some("APPDATA"),
        Some("PSModulePath"),
    ] {
        let mut environment = environment.to_vec();
        let phase_path =
            phase.with_file_name(format!("probe-{}.txt", variable.unwrap_or("baseline")));
        environment
            .iter_mut()
            .find(|(key, _)| key == "CC_DESK_TEST_PHASE")
            .unwrap()
            .1 = phase_path.to_string_lossy().into_owned();
        let shell_phase_path = phase_path.with_extension("shell.txt");
        environment.push((
            "CC_DESK_TEST_PS_PHASE".into(),
            shell_phase_path.to_string_lossy().into_owned(),
        ));
        let phase = phase_path.as_path();
        if let Some(variable) = variable {
            let path = if variable == "PSModulePath" {
                std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                    .join("System32/WindowsPowerShell/v1.0/Modules")
            } else {
                phase.parent().unwrap().join(variable)
            };
            if !matches!(variable, "PSModuleAnalysisCachePath" | "PSModulePath") {
                std::fs::create_dir_all(&path).unwrap();
            }
            environment.push((variable.into(), path.to_string_lossy().into_owned()));
        }
        std::fs::write(phase, b"").unwrap();
        std::fs::write(&shell_phase_path, b"").unwrap();
        let start = Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            invoke(&command, executable, input, &environment)
        }));
        eprintln!(
            "WindowsReporter postFailureProbe={} completed={} elapsedMs={} reporterPhase={} shellPhase={}",
            variable.unwrap_or("baseline"),
            result.is_ok(),
            start.elapsed().as_millis(),
            phase_description(Some(phase)),
            phase_description(Some(&shell_phase_path))
        );
    }
}
