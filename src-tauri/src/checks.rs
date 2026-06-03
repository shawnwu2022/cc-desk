//! 环境检测模块
//!
//! 检查 Claude CLI 和 Git Bash 是否可用。
//! PTY 通过 shell 执行 `claude` 命令，与终端行为一致，
//! 不需要检测启动类型或 node 是否可用。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    /// 检测到的路径（无论通过与否，只要找到了就带上）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl CheckResult {
    fn pass_with_path(name: &str, message: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            message: message.to_string(),
            detected_path: Some(path.to_string()),
            action: None,
            url: None,
        }
    }

    fn fail_with_path(
        name: &str,
        message: String,
        path: Option<String>,
        action: &str,
        url: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            message,
            detected_path: path,
            action: Some(action.to_string()),
            url: Some(url.to_string()),
        }
    }
}

/// 检查结果集合
#[derive(Debug, Clone, Serialize)]
pub struct ChecksResult {
    pub checks: Vec<CheckResult>,
}

impl ChecksResult {
    pub fn all_passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }

    pub fn failed_checks(&self) -> Vec<&CheckResult> {
        self.checks.iter().filter(|c| !c.passed).collect()
    }
}

/// 运行所有检查
pub fn run_checks() -> ChecksResult {
    // 修复 GUI 应用 PATH 继承问题
    crate::platform::refresh_path();

    let (claude_path, git_bash_path) = read_config_paths();

    let checks = vec![
        check_claude_cli(&claude_path),
        #[cfg(target_os = "windows")]
        check_git_bash(&git_bash_path),
    ];

    // 将通过的检查项检测到的路径自动保存到配置
    save_detected_paths(&checks);

    ChecksResult { checks }
}

/// 将检测到的路径保存到配置文件（仅保存通过的检查项）
fn save_detected_paths(checks: &[CheckResult]) {
    let mut updates = serde_json::Map::new();

    for check in checks {
        if check.passed {
            if let Some(ref path) = check.detected_path {
                match check.name.as_str() {
                    "Claude CLI" => {
                        updates.insert(
                            "claudePath".to_string(),
                            serde_json::Value::String(path.clone()),
                        );
                    }
                    "Git Bash" => {
                        updates.insert(
                            "gitBashPath".to_string(),
                            serde_json::Value::String(path.clone()),
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    if !updates.is_empty() {
        match crate::store::update_app_config(serde_json::Value::Object(updates)) {
            Ok(()) => log::info!("[Check] Detected paths saved to config"),
            Err(e) => log::warn!("[Check] Failed to save detected paths: {}", e),
        }
    }
}

fn read_config_paths() -> (Option<String>, Option<String>) {
    match crate::store::get_app_config() {
        Ok(config) => (config.claude_path, config.git_bash_path),
        Err(_) => (None, None),
    }
}

/// 用 where (Windows) / which (Unix) 查找可执行文件，返回所有结果
fn find_all_executables(name: &str) -> Vec<String> {
    crate::platform::find_all_executables(name)
}

/// 检查 Claude CLI
///
/// 只检查 `claude` 命令是否存在，不检测启动类型。
/// PTY 通过 shell 执行，shell 自动处理 npm 安装或原生可执行文件。
fn check_claude_cli(config_path: &Option<String>) -> CheckResult {
    // 1. 配置的自定义路径（优先）
    // 如果配置了路径且存在，直接通过
    if let Some(ref path) = config_path {
        if Path::new(path).exists() {
            return CheckResult::pass_with_path("Claude CLI", &format!("Found: {}", path), path);
        }
        // 配置路径不存在，记录警告，继续尝试 where 查找
        log::warn!("[Check] Configured Claude path not found: {}, trying where...", path);
    }

    // 2. 自动检测（where/which）
    let exe_name = "claude";
    let found = find_all_executables(exe_name);

    if let Some(path) = found.first() {
        return CheckResult::pass_with_path("Claude CLI", &format!("Found: {}", path), path);
    }

    CheckResult::fail_with_path(
        "Claude CLI",
        "Claude CLI not found. You can set a custom path below.".to_string(),
        None,
        "View installation guide",
        "https://code.claude.com/docs",
    )
}

/// 检查 Git Bash（仅 Windows）
///
/// Git Bash 是 Windows 上 Claude CLI 的必需依赖。
#[cfg(target_os = "windows")]
fn check_git_bash(config_path: &Option<String>) -> CheckResult {
    // 1. 配置中保存的路径（优先）
    // 如果配置了路径且存在，直接通过
    if let Some(ref path) = config_path {
        if Path::new(path).exists() {
            return CheckResult::pass_with_path(
                "Git Bash",
                &format!("Found (config): {}", path),
                path,
            );
        }
        // 配置路径不存在，记录警告，继续尝试其他方式查找
        log::warn!("[Check] Configured Git Bash path not found: {}, trying other methods...", path);
    }

    // 2. 环境变量
    if let Ok(path) = std::env::var("CLAUDE_CODE_GIT_BASH_PATH") {
        if Path::new(&path).exists() {
            return CheckResult::pass_with_path(
                "Git Bash",
                &format!("Found (env): {}", path),
                &path,
            );
        }
    }

    // 3. where git → 在 git 安装目录下找 bash.exe
    if let Some(bash_path) = detect_git_bash_from_git() {
        return CheckResult::pass_with_path(
            "Git Bash",
            &format!("Found: {}", bash_path),
            &bash_path,
        );
    }

    CheckResult::fail_with_path(
        "Git Bash",
        "Git Bash not found. You can set a custom path below.".to_string(),
        None,
        "Install Git for Windows",
        "https://git-scm.com/download/win",
    )
}

/// 通过 where git 查找 Git 安装目录下的 bash.exe
#[cfg(target_os = "windows")]
fn detect_git_bash_from_git() -> Option<String> {
    let exe_name = if cfg!(target_os = "windows") {
        "git.exe"
    } else {
        "git"
    };
    let git_paths = find_all_executables(exe_name);

    for git_path in &git_paths {
        // git.exe 通常位于 <git-install>/cmd/git.exe 或 <git-install>/bin/git.exe
        let path = Path::new(git_path);
        if let Some(parent) = path.parent() {
            let git_install_dir = if parent.file_name().map(|n| n == "cmd").unwrap_or(false)
                || parent.file_name().map(|n| n == "bin").unwrap_or(false)
            {
                parent.parent()
            } else {
                Some(parent)
            };

            if let Some(install_dir) = git_install_dir {
                let bash_path = install_dir.join("bin").join("bash.exe");
                if bash_path.exists() {
                    log::info!(
                        "[Check] Git Bash found via 'where git': {}",
                        bash_path.display()
                    );
                    return Some(bash_path.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

