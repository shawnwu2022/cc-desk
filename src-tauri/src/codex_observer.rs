//! Codex 官方 session hooks 的临时元数据通道，不持有进程或历史写入权限。
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{ffi::OsString, path::Path};

pub(crate) use crate::cli::environment::CODEX_OBSERVER_ENV_NAMES as ENV_NAMES;
const EVENTS: &[(&str, &str)] = &[
    ("SessionStart", "session_start"),
    ("UserPromptSubmit", "user_prompt_submit"),
    ("Stop", "stop"),
];

/// 单调 nonce 只约束重放窗口，不表示 CLI 回合、hook 因果或授权顺序。
#[derive(Default)]
pub(crate) struct ReplayWindow {
    seen: std::collections::BTreeSet<String>,
    floor: Option<String>,
}

pub(crate) fn ordered_event_id(value: &str) -> bool {
    value.len() == 66
        && value.starts_with('m')
        && value.as_bytes()[33] == b'-'
        && value.as_bytes()[1..33]
            .iter()
            .chain(&value.as_bytes()[34..])
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}

impl ReplayWindow {
    pub(crate) fn accept(&mut self, event_id: &str, capacity: usize) -> bool {
        if self.floor.as_deref().is_some_and(|floor| event_id <= floor)
            || !self.seen.insert(event_id.to_owned())
        {
            return false;
        }
        if self.seen.len() > capacity {
            let oldest = self.seen.pop_first().expect("nonempty replay window");
            let retained = oldest != event_id;
            self.floor = Some(oldest);
            return retained;
        }
        true
    }
}

pub(crate) fn event_id() -> Result<String, SafeError> {
    // 跨 reporter 进程共享同一开机单调时钟，避免系统时间回拨冻结窗口。
    #[cfg(windows)]
    let ticks = {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn QueryPerformanceCounter(value: *mut i64) -> i32;
        }
        let mut value = 0i64;
        // 缓冲区在同步 Win32 调用期间有效；失败只关闭可选观察。
        if unsafe { QueryPerformanceCounter(&mut value) } == 0 || value < 0 {
            return Err(error("OBSERVER_UNAVAILABLE"));
        }
        value as u128
    };
    #[cfg(unix)]
    let ticks = {
        let mut value: libc::timespec = unsafe { std::mem::zeroed() };
        // CLOCK_MONOTONIC 为同一主机所有进程提供相同时间基准。
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) } != 0
            || value.tv_sec < 0
        {
            return Err(error("OBSERVER_UNAVAILABLE"));
        }
        value.tv_sec as u128 * 1_000_000_000 + value.tv_nsec as u128
    };
    #[cfg(not(any(windows, unix)))]
    return Err(error("OBSERVER_UNAVAILABLE"));
    #[cfg(any(windows, unix))]
    Ok(format!("m{ticks:032x}-{}", uuid::Uuid::new_v4().simple()))
}

fn sorted(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.sort_keys();
            for value in object.values_mut() {
                sorted(value)
            }
        }
        Value::Array(items) => {
            for value in items {
                sorted(value)
            }
        }
        _ => {}
    }
}

pub(crate) fn hook_args(executable: &Path) -> Result<Vec<OsString>, SafeError> {
    let path = executable
        .to_str()
        .filter(|path| executable.is_absolute() && !path.chars().any(char::is_control))
        .ok_or_else(|| error("OBSERVER_PREPARE_FAILED"))?;
    #[cfg(windows)]
    let command = {
        use base64::Engine;
        let shell = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| error("OBSERVER_PREPARE_FAILED"))?
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let shell = shell
            .to_str()
            .filter(|value| {
                !value
                    .chars()
                    .any(|c| c.is_whitespace() || "\"'%!;&|<>$`()".contains(c))
            })
            .ok_or_else(|| error("OBSERVER_PREPARE_FAILED"))?
            .replace('\\', "/");
        // Codex 可选择 PowerShell/cmd/Bash；首命令无空格，参数只含 base64，不改变用户 shell。
        let script = format!("$ErrorActionPreference='Stop';try{{$i=New-Object System.Diagnostics.ProcessStartInfo;$i.FileName='{}';$i.Arguments='--cc-desk-codex-observer';$i.UseShellExecute=$false;$i.RedirectStandardInput=$true;$i.CreateNoWindow=$true;$p=New-Object System.Diagnostics.Process;$p.StartInfo=$i;if($p.Start()){{[Console]::OpenStandardInput().CopyTo($p.StandardInput.BaseStream);$p.StandardInput.Close();if(-not $p.WaitForExit(2500)){{$p.Kill()}};$p.Dispose()}}}}catch{{}};exit 0", path.replace('\'', "''"));
        let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        if path
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-/.:\\".contains(&c))
        {
            format!("{} --cc-desk-codex-observer", path.replace('\\', "/"))
        } else {
            format!(
                "{shell} -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        }
    };
    #[cfg(not(windows))]
    let command = format!("'{}' --cc-desk-codex-observer", path.replace('\'', "'\\''"));
    let mut args = Vec::new();
    let mut states = toml::Table::new();
    for (event, key) in EVENTS {
        let handler = json!({"type":"command", "command":command, "timeout":5, "async":matches!(*event, "UserPromptSubmit" | "Stop")});
        let group = json!({"hooks":[handler]});
        let mut fingerprint = json!({"event_name":key, "hooks":[handler]});
        sorted(&mut fingerprint);
        let hash = format!(
            "sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&fingerprint).map_err(|_| error("OBSERVER_PREPARE_FAILED"))?
            )
        );
        let value =
            toml::Value::try_from(json!([group])).map_err(|_| error("OBSERVER_PREPARE_FAILED"))?;
        #[cfg(windows)]
        let source = r"C:\<session-flags>\config.toml";
        #[cfg(not(windows))]
        let source = "/<session-flags>/config.toml";
        states.insert(
            format!("{source}:{key}:0:0"),
            toml::Value::try_from(json!({"trusted_hash":hash}))
                .map_err(|_| error("OBSERVER_PREPARE_FAILED"))?,
        );
        args.extend(["-c".into(), format!("hooks.{event}={value}").into()]);
    }
    // -c 的 dotted key 不是 TOML quoted key 解析器，含 config.toml 的 key 必须位于内联表内。
    args.extend([
        "-c".into(),
        format!("hooks.state={}", toml::Value::Table(states)).into(),
    ]);
    Ok(args)
}

pub(crate) fn metadata(input: &Value) -> Result<Value, SafeError> {
    let event = input
        .get("hook_event_name")
        .and_then(Value::as_str)
        .filter(|name| EVENTS.iter().any(|(event, _)| event == name))
        .ok_or_else(|| error("OBSERVER_INVALID_EVENT"))?;
    if ["agent_id", "agent_type"]
        .iter()
        .any(|key| input.get(key).is_some_and(|value| !value.is_null()))
    {
        return Err(error("OBSERVER_INVALID_EVENT"));
    }
    let id = input
        .get("session_id")
        .and_then(Value::as_str)
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .ok_or_else(|| error("OBSERVER_INVALID_EVENT"))?;
    Ok(json!({"hook_event_name":event, "session_id":id.to_string()}))
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    #[test]
    fn CodexCompatibility_ReporterUsesOrderedReplayIds_081() {
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..100 {
            let id = event_id().expect("boot clock available");
            assert!(ordered_event_id(&id));
            assert!(ids.insert(id));
        }
        for invalid in [
            "",
            "m",
            "opaque-legacy",
            "m00000000000000000000000000000000-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            assert!(!ordered_event_id(invalid));
        }
    }
    #[test]
    fn CodexIdentity_HookArgumentsRetainNativePolicy_033() {
        #[cfg(windows)]
        let path = Path::new(r"C:\Program Files\CC Desk\cc-desk.exe");
        #[cfg(not(windows))]
        let path = Path::new("/tmp/CC Desk/cc-desk");
        let args: Vec<_> = hook_args(path)
            .unwrap()
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect();
        assert_eq!(args.len(), 8);
        for pair in args.as_chunks::<2>().0 {
            assert_eq!(pair[0], "-c");
        }
        let state = args
            .last()
            .unwrap()
            .strip_prefix("hooks.state=")
            .unwrap()
            .parse::<toml::Value>()
            .unwrap();
        assert_eq!(state.as_table().unwrap().len(), 3);
        for (key, value) in state.as_table().unwrap() {
            assert!(key.contains("<session-flags>"));
            assert!(key.ends_with(":0:0"));
            assert!(value["trusted_hash"]
                .as_str()
                .unwrap()
                .starts_with("sha256:"));
        }
        assert!(!args.join(" ").contains("features.hooks"));
        assert!(!args.join(" ").contains("bypass"));
        assert!(!args.join(" ").contains("no-daemon"));
    }
}
