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

/// Only the npm Cmd route needs hook TOML kept outside CALL's literal subset.
#[cfg(windows)]
pub(crate) struct CmdHookOverlay {
    pub(crate) hook_args: Vec<OsString>,
    pub(crate) default_args: Vec<OsString>,
    pub(crate) extra_args: Vec<OsString>,
}

#[cfg(windows)]
fn hook_configuration_error() -> SafeError {
    SafeError {
        field: Some("codexHookConfiguration".into()),
        ..error("OBSERVER_PREPARE_FAILED")
    }
}

#[cfg(windows)]
fn config_value(value: &str) -> Result<toml::Value, SafeError> {
    if value.len() > 64 * 1024 {
        return Err(hook_configuration_error());
    }
    let mut document: toml::Table = format!("value={value}")
        .parse()
        .map_err(|_| hook_configuration_error())?;
    if document.len() != 1 {
        return Err(hook_configuration_error());
    }
    document
        .remove("value")
        .ok_or_else(hook_configuration_error)
}

#[cfg(windows)]
fn collect_session_hooks(
    args: &[OsString],
    hooks: &mut toml::Table,
    changed: &mut bool,
) -> Result<Vec<(usize, OsString)>, SafeError> {
    let hook_assignment = |value: &str| {
        value
            .split_once('=')
            .is_some_and(|(key, _)| key.trim() == "hooks" || key.trim().starts_with("hooks."))
    };
    let has_hooks = args.iter().enumerate().any(|(index, arg)| {
        arg.to_str().is_some_and(|arg| {
            matches!(arg, "-c" | "--config")
                && args
                    .get(index + 1)
                    .and_then(|v| v.to_str())
                    .is_some_and(hook_assignment)
                || arg
                    .strip_prefix("--config=")
                    .or_else(|| {
                        arg.strip_prefix("-c")
                            .map(|v| v.strip_prefix('=').unwrap_or(v))
                    })
                    .is_some_and(hook_assignment)
        })
    });
    if !has_hooks {
        return Ok(args.iter().cloned().enumerate().collect());
    }
    let mut remaining = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_str().ok_or_else(hook_configuration_error)?;
        let (assignment, count) = if matches!(arg, "-c" | "--config") {
            (
                args.get(index + 1)
                    .and_then(|v| v.to_str())
                    .ok_or_else(hook_configuration_error)?,
                2,
            )
        } else if let Some(value) = arg.strip_prefix("--config=").or_else(|| {
            arg.strip_prefix("-c")
                .filter(|v| !v.is_empty())
                .map(|v| v.strip_prefix('=').unwrap_or(v))
        }) {
            (value, 1)
        } else {
            // Only known single-value options establish consumption. Unknown
            // syntax cannot authorize moving something that may be a value.
            let value_flags = [
                "--model",
                "-m",
                "--profile",
                "-p",
                "--cd",
                "-C",
                "--sandbox",
                "-s",
                "--ask-for-approval",
                "-a",
            ];
            let count = if value_flags.contains(&arg) {
                if index + 1 >= args.len() {
                    return Err(hook_configuration_error());
                }
                2
            } else if arg
                .split_once('=')
                .is_some_and(|(key, _)| value_flags.contains(&key))
                || !arg.starts_with('-')
            {
                1
            } else {
                return Err(hook_configuration_error());
            };
            remaining.extend((index..index + count).map(|i| (i, args[i].clone())));
            index += count;
            continue;
        };
        let Some((key, value)) = assignment.split_once('=') else {
            remaining.extend((index..index + count).map(|i| (i, args[i].clone())));
            index += count;
            continue;
        };
        let key = key.trim();
        if key != "hooks" && !key.starts_with("hooks.") {
            remaining.extend((index..index + count).map(|i| (i, args[i].clone())));
            index += count;
            continue;
        }
        let value = config_value(value)?;
        if key == "hooks" {
            *hooks = value
                .as_table()
                .ok_or_else(hook_configuration_error)?
                .clone();
        } else {
            // Quoted dotted trust keys are not the CLI override parser's syntax.
            // Preserve them in an inline hooks.state table instead of guessing.
            let field = key
                .strip_prefix("hooks.")
                .ok_or_else(hook_configuration_error)?;
            if field.is_empty()
                || !field
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(hook_configuration_error());
            }
            hooks.insert(field.to_owned(), value);
        }
        *changed = true;
        index += count;
    }
    Ok(remaining)
}

#[cfg(windows)]
pub(crate) fn cmd_hook_overlay(
    desk_args: &[OsString],
    defaults: &[OsString],
    extras: &[OsString],
) -> Result<CmdHookOverlay, SafeError> {
    let mut user = toml::Table::new();
    let mut changed = false;
    let combined: Vec<_> = defaults.iter().chain(extras).cloned().collect();
    let remaining = collect_session_hooks(&combined, &mut user, &mut changed)?;
    let (default_args, extra_args): (Vec<_>, Vec<_>) = remaining
        .into_iter()
        .partition(|(index, _)| *index < defaults.len());
    let default_args = default_args.into_iter().map(|(_, value)| value).collect();
    let extra_args = extra_args.into_iter().map(|(_, value)| value).collect();
    if !changed {
        return Ok(CmdHookOverlay {
            hook_args: desk_args.to_vec(),
            default_args,
            extra_args,
        });
    }
    if desk_args.len() != 8 {
        return Err(hook_configuration_error());
    }
    let mut desk = toml::Table::new();
    for pair in desk_args.as_chunks::<2>().0 {
        if pair[0] != "-c" {
            return Err(hook_configuration_error());
        }
        let (key, value) = pair[1]
            .to_str()
            .and_then(|v| v.split_once('='))
            .ok_or_else(hook_configuration_error)?;
        desk.insert(
            key.strip_prefix("hooks.")
                .ok_or_else(hook_configuration_error)?
                .to_owned(),
            config_value(value)?,
        );
    }
    let mut states = match user.remove("state") {
        Some(value) => value
            .as_table()
            .ok_or_else(hook_configuration_error)?
            .clone(),
        None => toml::Table::new(),
    };
    let desk_states = desk
        .get("state")
        .and_then(toml::Value::as_table)
        .ok_or_else(hook_configuration_error)?;
    for (event, label) in EVENTS {
        let mut groups = match user.remove(*event) {
            Some(value) => value
                .as_array()
                .ok_or_else(hook_configuration_error)?
                .clone(),
            None => Vec::new(),
        };
        let additions = desk
            .get(*event)
            .and_then(toml::Value::as_array)
            .filter(|values| values.len() == 1)
            .ok_or_else(hook_configuration_error)?;
        let suffix = format!(":{label}:0:0");
        let (original, state) = desk_states
            .iter()
            .find(|(key, _)| key.ends_with(&suffix))
            .ok_or_else(hook_configuration_error)?;
        let prefix = original
            .strip_suffix(":0:0")
            .ok_or_else(hook_configuration_error)?;
        let key = format!("{prefix}:{}:0", groups.len());
        // Never override an explicit user's enablement/trust entry, even for Desk.
        if states.contains_key(&key) {
            return Err(hook_configuration_error());
        }
        states.insert(key, state.clone());
        groups.extend_from_slice(additions);
        user.insert((*event).to_owned(), toml::Value::Array(groups));
    }
    user.insert("state".into(), toml::Value::Table(states));
    Ok(CmdHookOverlay {
        hook_args: vec![
            "-c".into(),
            format!("hooks={}", toml::Value::Table(user)).into(),
        ],
        default_args,
        extra_args,
    })
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

    #[cfg(windows)]
    #[test]
    fn CodexAdapter_AmbiguousBusinessValuesDoNotBecomeHooks_115() {
        let desk = hook_args(&std::env::current_exe().unwrap()).unwrap();
        let defaults = ["--future-option", "-chooks.SessionStart=[]"].map(OsString::from);
        assert!(
            cmd_hook_overlay(&desk, &defaults, &[]).is_err(),
            "unknown value arity cannot authorize rewriting argv"
        );
        let defaults = ["--future-option"].map(OsString::from);
        let extras = ["-chooks.SessionStart=[]"].map(OsString::from);
        assert!(
            cmd_hook_overlay(&desk, &defaults, &extras).is_err(),
            "default/extra boundary is not a CLI parser boundary"
        );
    }

    #[cfg(windows)]
    #[test]
    fn CodexAdapter_ConfigEqualsFormPreservesUserHook_116() {
        let desk = hook_args(&std::env::current_exe().unwrap()).unwrap();
        let defaults =
            [r#"-c=hooks.SessionStart=[{hooks=[{type="command",command="user-owned"}]}]"#]
                .map(OsString::from);
        let overlay = cmd_hook_overlay(&desk, &defaults, &[]).unwrap();
        assert!(
            overlay.default_args.is_empty(),
            "recognized global hook assignment must not override the composed plan later"
        );
        let hooks = overlay.hook_args[1]
            .to_str()
            .unwrap()
            .strip_prefix("hooks=")
            .unwrap();
        let document: toml::Table = format!("hooks={hooks}").parse().unwrap();
        assert_eq!(
            document["hooks"]["SessionStart"].as_array().unwrap().len(),
            2
        );
        assert_eq!(
            document["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str(),
            Some("user-owned")
        );
    }

    #[cfg(windows)]
    #[test]
    fn CodexAdapter_UserStateKeysAndDeskCollisionStayUntrusted_118() {
        let desk = hook_args(&std::env::current_exe().unwrap()).unwrap();
        let original: toml::Table = format!(
            "state={}",
            desk[7]
                .to_str()
                .unwrap()
                .strip_prefix("hooks.state=")
                .unwrap()
        )
        .parse()
        .unwrap();
        let key = original["state"]
            .as_table()
            .unwrap()
            .keys()
            .find(|key| key.ends_with(":session_start:0:0"))
            .unwrap();
        let user_state = toml::Value::Table(toml::Table::from_iter([(
            key.clone(),
            toml::Value::Table(toml::Table::from_iter([
                ("enabled".into(), toml::Value::Boolean(false)),
                (
                    "trusted_hash".into(),
                    toml::Value::String("user-trust".into()),
                ),
            ])),
        )]));
        let flags = [
            "-c".into(),
            "hooks.SessionStart=[{hooks=[{type=\"command\",command=\"user-owned\"}]}]".into(),
            "-c".into(),
            format!("hooks.state={user_state}").into(),
        ];
        let overlay = cmd_hook_overlay(&desk, &flags, &[]).unwrap();
        let merged: toml::Table = format!(
            "hooks={}",
            overlay.hook_args[1]
                .to_str()
                .unwrap()
                .strip_prefix("hooks=")
                .unwrap()
        )
        .parse()
        .unwrap();
        assert_eq!(
            merged["hooks"]["state"][key]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            merged["hooks"]["state"][key]["trusted_hash"].as_str(),
            Some("user-trust")
        );
        assert!(merged["hooks"]["state"]
            .as_table()
            .unwrap()
            .contains_key(&key.replace(":0:0", ":1:0")));
        let conflict = ["-c".into(), format!("hooks.state={user_state}").into()];
        assert!(
            cmd_hook_overlay(&desk, &conflict, &[]).is_err(),
            "Desk must not override an explicit disabled entry at its own target index"
        );
    }
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
