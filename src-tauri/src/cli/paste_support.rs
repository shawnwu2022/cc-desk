//! Codex Ctrl+V 原生文本粘贴能力探测（运行时查询，非 launch preflight）。
//!
//! Windows 上 codex 的 Ctrl+V 文本回退于 0.163.0 加入：chatwidget/interaction.rs
//! 对纯 Ctrl+V 构造 `PasteImage { text_fallback }`，无图时回退读剪贴板文本；
//! 0.162.x 的 `paste_image` 只处理图片，纯文本按键被吞。CC Desk 转发 `\x16`
//! 前必须确认该能力，否则用户文本无法粘贴。结果按可执行路径缓存进程生命周期。

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use super::profile_service::authorize_profile_window;
use super::profiles::error;
use super::snapshot::configured_program;
use super::storage::WorkspaceRepository;
use super::types::{CliKind, SafeError, WireU64};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PasteSupportRequest {
    pub(crate) profile_id: String,
    pub(crate) expected_revision: WireU64,
}

impl PasteSupportRequest {
    fn validate(&self) -> Result<(), SafeError> {
        if self.profile_id.is_empty()
            || self.profile_id.len() > 128
            || !self
                .profile_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(SafeError::invalid("profileId"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexNativePasteSupport {
    pub(crate) profile_id: String,
    pub(crate) native_text_paste: bool,
}

pub(crate) fn native_paste_support(
    repository: &WorkspaceRepository,
    caller: &str,
    request: &PasteSupportRequest,
) -> Result<CodexNativePasteSupport, SafeError> {
    authorize_profile_window(caller)?;
    request.validate()?;
    let profile = repository.get_profile(&request.profile_id)?;
    if profile.revision != request.expected_revision {
        return Err(error("REVISION_CONFLICT"));
    }
    // codex 的 Ctrl+V 文本回退本身是 cfg!(windows) 编译期分支（chatwidget/interaction.rs）；
    // 非 Windows 上纯 Ctrl+V 只触发图片粘贴，转发 \x16 对纯文本无效，恒走 Desk 剪贴板路径。
    let native_text_paste = if !cfg!(windows) {
        false
    } else if profile.cli == CliKind::Codex {
        let legacy = profile.read_legacy(&repository.metadata_directory().join("config.json"))?;
        // 未配置或不可用的 program 无法运行，也就没有可转发目标。
        configured_program(&profile, legacy.as_ref())
            .ok()
            .and_then(|program| program.to_str().map(str::to_owned))
            .map(|program| probe_program_cached(&program))
            .unwrap_or(false)
    } else {
        false
    };
    Ok(CodexNativePasteSupport {
        profile_id: profile.id,
        native_text_paste,
    })
}

/// Windows Ctrl+V 文本回退要求 codex ≥ 0.163.0。
const MIN_TEXT_FALLBACK_VERSION: (u64, u64, u64) = (0, 163, 0);

fn probe_program_cached(program: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cached = match cache.lock() {
        Ok(guard) => guard.get(program).copied(),
        Err(_) => None,
    };
    if let Some(supported) = cached {
        return supported;
    }
    let supported = run_version_probe(program).unwrap_or(false);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(program.to_owned(), supported);
    }
    supported
}

fn run_version_probe(program: &str) -> Option<bool> {
    let mut child = crate::platform::new_command(program)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // 独立线程读输出：管道写满会卡住子进程，必须边等退出边读。版本在首行，4KB 封顶。
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut collected = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    collected.extend_from_slice(&chunk[..n]);
                    if collected.len() >= 4096 {
                        break;
                    }
                }
            }
        }
        let _ = sender.send(collected);
    });
    // 有界等待：wrapper 忽略 --version 或挂起时终止并回收子进程，前端 producer
    // 等到确定结果后才能回退剪贴板路径，不能无限阻塞输入队列队首。
    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    let exited = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => break None,
        }
    };
    if exited.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // 孙进程可能继承管道让 read 不返回：取不到结果就放弃（线程随后自行结束）。
    let output = receiver.recv_timeout(Duration::from_secs(1)).ok()?;
    exited?
        .success()
        .then(|| parse_text_fallback_support(&String::from_utf8_lossy(&output)))
}

/// 探测期限：只发生一次（结果按路径缓存），失败即永久走 Desk 剪贴板路径。
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// 输出必须携带 `codex-cli` 标识 token 才接受版本；裸 semver（如启动器 npx --version
/// 打印的自身版本）不代表目标 Codex 的能力，一律视为不支持。
fn parse_text_fallback_support(version_output: &str) -> bool {
    parse_codex_version(version_output)
        .map(|version| version >= MIN_TEXT_FALLBACK_VERSION)
        .unwrap_or(false)
}

fn parse_codex_version(version_output: &str) -> Option<(u64, u64, u64)> {
    let mut tokens = version_output.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "codex-cli" {
            let version = tokens.next()?;
            let mut parts = version.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next()?.parse().ok()?;
            if parts.next().is_some() {
                return None;
            }
            return Some((major, minor, patch));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exact_minimum_version() {
        assert!(parse_text_fallback_support("codex-cli 0.163.0\n"));
    }

    #[test]
    fn rejects_version_without_text_fallback() {
        assert!(!parse_text_fallback_support("codex-cli 0.162.1\n"));
    }

    #[test]
    fn accepts_newer_version() {
        assert!(parse_text_fallback_support("codex-cli 1.0.0\n"));
    }

    #[test]
    fn rejects_unparseable_output() {
        assert!(!parse_text_fallback_support(""));
        assert!(!parse_text_fallback_support("internal error"));
        assert!(!parse_text_fallback_support("codex-cli 0.163"));
    }

    #[test]
    fn rejects_bare_semver_from_launcher_tools() {
        // npx.cmd --version 打印自身版本；启动器不是 Codex，不得据此放行转发。
        assert!(!parse_text_fallback_support("11.14.1\n"));
        assert!(!parse_text_fallback_support("my-codex-cli 11.14.1\n"));
    }

    #[test]
    fn accepts_exact_marker_before_version() {
        assert!(parse_text_fallback_support("codex-cli 0.163.0 (rc 1)\n"));
    }
}
