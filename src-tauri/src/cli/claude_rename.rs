use super::*;
use crate::cli::{
    environment::{lookup, EnvMap},
    profiles::error,
};
use std::ffi::OsStr;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) fn node_program(environment: &EnvMap) -> Result<PathBuf, SafeError> {
    let path =
        lookup(environment, OsStr::new("PATH")).ok_or_else(|| error("NODE_RUNTIME_UNAVAILABLE"))?;
    for directory in std::env::split_paths(path)
        .take(256)
        .filter(|directory| directory.is_absolute())
    {
        let candidate = directory.join(if cfg!(windows) { "node.exe" } else { "node" });
        if let Ok(metadata) = std::fs::metadata(&candidate) {
            if !metadata.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            return std::fs::canonicalize(candidate).map_err(|_| error("NODE_RUNTIME_UNAVAILABLE"));
        }
    }
    Err(error("NODE_RUNTIME_UNAVAILABLE"))
}
fn permission_path(path: &Path) -> Result<(), SafeError> {
    let text = path
        .to_str()
        .ok_or_else(|| error("NATIVE_RENAME_UNSUPPORTED"))?;
    if !path.is_absolute() || text.chars().any(char::is_control) || text.contains(['*', '?', ',']) {
        return Err(error("NATIVE_RENAME_UNSUPPORTED"));
    }
    Ok(())
}
pub(crate) fn embedded_bridge() -> &'static [u8] {
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/native-session-sdk/bridge.cjs"
    ))
}
pub(crate) fn command(
    environment: &EnvMap,
    bridge: &Path,
    namespace: &Path,
) -> Result<std::process::Command, SafeError> {
    permission_path(bridge)?;
    permission_path(namespace)?;
    if !bridge.is_file() {
        return Err(error("SDK_HELPER_UNAVAILABLE"));
    }
    let node = node_program(environment)?;
    // 只使用冻结环境中解析并核验的绝对运行时；不继承 NODE_OPTIONS 或 CLI 凭证。
    let mut command = std::process::Command::new(node);
    command.arg("--permission");
    for (flag, path) in [
        ("--allow-fs-read=", bridge),
        ("--allow-fs-read=", namespace),
        ("--allow-fs-write=", namespace),
    ] {
        let mut value = std::ffi::OsString::from(flag);
        value.push(path);
        command.arg(value);
    }
    command.arg("-e").arg("const [a,b]=process.versions.node.split('.').map(Number);if(a<22||(a===22&&b<13)||!process.permission){process.stdout.write('{\"state\":\"failed\",\"code\":\"NODE_RUNTIME_UNAVAILABLE\"}\\n');process.exit(1)}process.stdout.write('{\"state\":\"ready\"}\\n');require(process.argv[1]);").arg(bridge);
    command
        .env_clear()
        .env("CLAUDE_CONFIG_DIR", namespace)
        .current_dir(namespace);
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = lookup(environment, OsStr::new(name)) {
            command.env(name, value);
        }
    }
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::platform::configure_command(&mut command);
    Ok(command)
}
#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "lowercase", deny_unknown_fields)]
enum Response {
    Ready,
    Failed {
        code: String,
    },
    Renamed {
        #[serde(rename = "nativeSessionId")]
        native_session_id: String,
        title: String,
        cwd: String,
    },
}
pub(crate) fn process(
    child: &mut std::process::Child,
    authority: &crate::cli::native_projection::registry::RenameAuthority,
    title: &str,
    current: &dyn Fn() -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    let mut input = child.stdin.take().ok_or_else(|| error("RENAME_FAILED"))?;
    let output = child.stdout.take().ok_or_else(|| error("RENAME_FAILED"))?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(2);
    std::thread::Builder::new()
        .name("claude-rename-read".into())
        .spawn(move || {
            let mut output = std::io::BufReader::new(output);
            for _ in 0..2 {
                let mut line = Vec::new();
                if !matches!(
                    output.by_ref().take(32769).read_until(b'\n', &mut line),
                    Ok(1..)
                ) || line.len() > 32768
                {
                    break;
                }
                let Ok(value) = serde_json::from_slice::<Response>(&line) else {
                    break;
                };
                if sender.send(value).is_err() {
                    break;
                }
            }
        })
        .map_err(|_| error("RENAME_FAILED"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let receive = || -> Result<Response, SafeError> {
        loop {
            current()?;
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| error("RENAME_FAILED"))?;
            match receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
                Ok(value) => return Ok(value),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => return Err(error("RENAME_FAILED")),
            }
        }
    };
    if !matches!(
        receive().map_err(|_| error("NODE_RUNTIME_UNAVAILABLE"))?,
        Response::Ready
    ) {
        return Err(error("NODE_RUNTIME_UNAVAILABLE"));
    }
    current()?;
    authority.verify_claude(None)?;
    let request=serde_json::to_vec(&serde_json::json!({"nativeSessionId":authority.native_id,"title":title,"expectedCwd":authority.cwd})).map_err(|_| error("RENAME_FAILED"))?;
    if request.len() > 32768 {
        return Err(SafeError::invalid("title"));
    }
    input
        .write_all(&request)
        .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?;
    drop(input);
    match receive().map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))? {
        Response::Renamed {
            native_session_id,
            title: observed,
            cwd,
        } if native_session_id == authority.native_id
            && observed == title
            && cwd == authority.cwd =>
        {
            authority
                .verify_claude(Some(title))
                .map_err(|_| error("RENAME_VERIFICATION_FAILED"))
        }
        Response::Failed { code } => Err(error(match code.as_str() {
            "INVALID_REQUEST" => "INVALID_REQUEST",
            "SESSION_NOT_FOUND" => "SESSION_NOT_FOUND",
            "RENAME_FAILED" => "RENAME_FAILED",
            "RENAME_VERIFICATION_FAILED" => "RENAME_VERIFICATION_FAILED",
            _ => "RENAME_OUTCOME_UNKNOWN",
        })),
        _ => Err(error("RENAME_VERIFICATION_FAILED")),
    }
}
