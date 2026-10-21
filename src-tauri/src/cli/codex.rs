//! Codex action mapping. Do not inject model, sandbox, approval or TUI defaults.
#![allow(dead_code)] // Consumed by the staged D10 platform launcher.

use super::invocation::validate_locator;
use super::profiles::error;
use super::snapshot::LaunchSnapshot;
use super::types::{LaunchAction, ResumeScope, SafeError};
use std::ffi::OsString;

/// 可选 hook 先在隔离配置中询问真实能力；不启动模型、不重试用户会话。
pub(super) fn identity_hooks_supported(snapshot: &LaunchSnapshot, args: &[OsString]) -> bool {
    #[cfg(windows)]
    use super::profiles::Dialect;
    use super::profiles::Launcher;
    match snapshot.launcher() {
        // cmd transport 已验证精确 npm 包及入口；不能把长 flags 重新交给 cmd。
        #[cfg(windows)]
        Launcher::Shim {
            dialect: Dialect::Cmd,
            ..
        } => return true,
        Launcher::Native => {}
        _ => return false,
    }
    probe_identity_hooks(snapshot.program(), snapshot.environment(), args).unwrap_or(false)
}

fn hook_metadata_supported(value: &serde_json::Value) -> bool {
    let Some(data) = value["result"]["data"].as_array() else {
        return false;
    };
    ["sessionStart", "userPromptSubmit", "stop"]
        .iter()
        .all(|event| {
            data.iter()
                .filter_map(|row| row["hooks"].as_array())
                .flatten()
                .any(|hook| {
                    hook["eventName"] == *event
                        && hook["source"] == "sessionFlags"
                        && hook["trustStatus"] == "trusted"
                        && hook["handlerType"] == "command"
                        && hook["enabled"] == true
                })
        })
}

fn probe_identity_hooks(
    program: &std::path::Path,
    environment: &super::environment::EnvMap,
    args: &[OsString],
) -> Option<bool> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    // 自定义 wrapper 可能忽略 app-server 参数或生成无所有权的后代，不能用它探测。
    // 字节准入只约束可选探测；未知/升级目标继续原始启动，不做第二次会话启动。
    let _audited = audited_probe_target(program)?;
    let root = tempfile::tempdir().ok()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .arg("app-server")
        .current_dir(root.path())
        .env_clear();
    // 探测不读用户配置、项目或凭证；完整环境继承仅用于正常被选中的 CLI 运行。
    for name in [
        "SystemRoot",
        "WINDIR",
        "PATH",
        "ComSpec",
        "PATHEXT",
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
    ] {
        if let Some(value) = super::environment::lookup(environment, name.as_ref()) {
            command.env(name, value);
        }
    }
    for name in [
        "HOME",
        "USERPROFILE",
        "CODEX_HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
    ] {
        command.env(name, root.path());
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().ok()?;
    // 所有退出路径都回收本次探测；失败只撤销增强，基础启动仍只执行一次。
    let result = (|| {
        let mut stdout = child.stdout.take()?;
        let mut input = child.stdin.take()?;
        let request =
            serde_json::json!({"id":2,"method":"hooks/list","params":{"cwds":[root.path()]}});
        let requests = format!("{{\"id\":1,\"method\":\"initialize\",\"params\":{{\"clientInfo\":{{\"name\":\"cc_desk_probe\",\"version\":\"1\"}}}}}}\n{{\"method\":\"initialized\"}}\n{request}\n");
        input.write_all(requests.as_bytes()).ok()?;
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut collected = Vec::new();
        let mut start = 0;
        let mut lines = 0;
        let result = loop {
            if Instant::now() >= deadline || collected.len() >= 64 * 1024 || lines >= 128 {
                break Some(false);
            }
            let mut bytes = [0; 1024];
            match read_probe_output(&mut stdout, &mut bytes) {
                Ok(0) => break Some(false),
                Ok(n) => collected.extend_from_slice(&bytes[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => break Some(false),
            }
            let mut response = None;
            while let Some(offset) = collected[start..].iter().position(|byte| *byte == b'\n') {
                let end = start + offset;
                lines += 1;
                if let Ok(value) =
                    serde_json::from_slice::<serde_json::Value>(&collected[start..end])
                {
                    if value["id"] == 2 {
                        response = Some(hook_metadata_supported(&value));
                    }
                }
                start = end + 1;
                if response.is_some() || lines >= 128 {
                    break;
                }
            }
            if response.is_some() {
                break response;
            }
        };
        drop(input);
        result
    })();
    let _ = child.kill();
    child.wait().ok()?;
    result
}

/// 保持打开已核验的目标；不会执行未知名称、版本或 wrapper 来猜测能力。
#[cfg(windows)]
fn audited_probe_target(program: &std::path::Path) -> Option<Vec<std::fs::File>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    {
        let mut guards = super::codex_cmd::hold_probe_target(program).ok()?;
        let file = guards.last_mut()?;
        // 官方 Windows 可执行文件包含符号；先按已核实的大小拒绝未知目标，
        // 再对保持打开的完整字节核对 hash，不能误把 128 MiB 当作其大小上限。
        const AUDITED_SIZE: u64 = 341_270_320;
        if file.metadata().ok()?.len() != AUDITED_SIZE {
            return None;
        }
        let mut digest = Sha256::new();
        let mut remaining = AUDITED_SIZE as usize;
        let mut bytes = [0; 65536];
        loop {
            let read = file.read(&mut bytes).ok()?;
            if read == 0 {
                break;
            }
            remaining = remaining.checked_sub(read)?;
            digest.update(&bytes[..read]);
        }
        // 官方 rust-v0.163.0 Windows x64；扩展平台/版本须先验证其进程及 hook 契约。
        (format!("{:x}", digest.finalize())
            == "2692c8c7bb2cf8b0aefe638741fc90236f4b824c59578ce84994f9de58200993")
            .then_some(guards)
    }
}

#[cfg(not(windows))]
fn audited_probe_target(_: &std::path::Path) -> Option<Vec<std::fs::File>> {
    None
}

#[cfg(windows)]
fn read_probe_output(
    stdout: &mut std::process::ChildStdout,
    bytes: &mut [u8],
) -> std::io::Result<usize> {
    use std::io::Read;
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            handle: *mut std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }
    let mut available = 0;
    // stdout 只有当前读取者；先查询可用字节，避免持管道后代留下不可取消的 reader。
    if unsafe {
        PeekNamedPipe(
            stdout.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(109) {
            Ok(0)
        } else {
            Err(error)
        };
    }
    if available == 0 {
        return Err(std::io::ErrorKind::WouldBlock.into());
    }
    let size = bytes.len().min(available as usize);
    stdout.read(&mut bytes[..size])
}

#[cfg(not(windows))]
fn read_probe_output(_: &mut std::process::ChildStdout, _: &mut [u8]) -> std::io::Result<usize> {
    // 当前没有该平台可探测目标的字节准入，绝不进入阻塞读取。
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
#[allow(non_snake_case)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn CodexCompatibility_UnknownHookMetadataDegradesSafely_075() {
        let hooks = ["sessionStart", "userPromptSubmit", "stop"].map(|event| {
            serde_json::json!({
                "eventName":event,"source":"sessionFlags","trustStatus":"trusted",
                "handlerType":"command","enabled":true,"futureField":{"extra":1}})
        });
        let mut value =
            serde_json::json!({"id":2,"future":true,"result":{"data":[{"hooks":hooks}]}});
        assert!(hook_metadata_supported(&value));
        value["result"]["data"][0]["hooks"][2]["trustStatus"] = "futureTrust".into();
        assert!(!hook_metadata_supported(&value));
        assert!(!hook_metadata_supported(
            &serde_json::json!({"id":2,"error":{"code":-32601}})
        ));
    }

    #[test]
    fn CodexCompatibility_UnknownExecutableIsNeverProbed_079() {
        // 测试 executable 也是有效原生程序，但不是已审计的 Codex 字节。
        assert!(audited_probe_target(&std::env::current_exe().unwrap()).is_none());
        assert!(
            !probe_identity_hooks(&std::env::current_exe().unwrap(), &Default::default(), &[])
                .unwrap_or(false)
        );
    }
}

pub(super) fn action_args(snapshot: &LaunchSnapshot) -> Result<Vec<OsString>, SafeError> {
    match &snapshot.request().action {
        LaunchAction::New => Ok(Vec::new()),
        LaunchAction::ResumePicker {
            scope: ResumeScope::CurrentProject,
        } => Ok(vec!["resume".into()]),
        LaunchAction::ResumePicker {
            scope: ResumeScope::All,
        } => Ok(vec!["resume".into(), "--all".into()]),
        LaunchAction::ResumeId { native_session_id } => {
            validate_locator(native_session_id)?;
            Ok(vec!["resume".into(), native_session_id.into()])
        }
        // codex 拒绝 resume 自己归档的会话（要求先 unarchive），因此恢复 CLI 归档
        // 会话必须走 codex 自己的 unarchive 子命令，而不是搬动 ~/.codex 下的文件。
        LaunchAction::Unarchive { native_session_id } => {
            validate_locator(native_session_id)?;
            Ok(vec!["unarchive".into(), native_session_id.into()])
        }
        LaunchAction::Raw { .. } => Err(error("UNSUPPORTED_ACTION")),
    }
}
