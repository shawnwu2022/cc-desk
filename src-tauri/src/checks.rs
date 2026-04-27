//! 环境检测模块

use std::path::Path;
use std::env;

/// 检查结果
#[derive(Debug, Clone)]
pub struct CheckResult {
    name: String,
    passed: bool,
    message: String,
}

impl CheckResult {
    fn pass(name: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            message: message.to_string(),
        }
    }

    fn fail(name: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            message: message.to_string(),
        }
    }
}

/// 检查结果集合
#[derive(Debug)]
pub struct ChecksResult {
    checks: Vec<CheckResult>,
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
    let checks = vec![
        check_claude_config_dir(),
        check_git_bash(),
        check_claude_cli(),
    ];

    ChecksResult { checks }
}

/// 检查 Claude 配置目录
fn check_claude_config_dir() -> CheckResult {
    let claude_dir = dirs::home_dir()
        .map(|h| h.join(".claude"));

    match claude_dir {
        Some(dir) if dir.exists() => {
            CheckResult::pass("Claude config dir", &format!("Found: {}", dir.display()))
        }
        Some(dir) => {
            CheckResult::fail("Claude config dir", &format!("Not found: {}", dir.display()))
        }
        None => {
            CheckResult::fail("Claude config dir", "Could not find home directory")
        }
    }
}

/// 检查 Git Bash（仅 Windows）
fn check_git_bash() -> CheckResult {
    if !cfg!(target_os = "windows") {
        return CheckResult::pass("Git Bash", "Not required on non-Windows");
    }

    // 候选路径
    let candidates = [
        "D:\\Program Files\\Git\\bin\\bash.exe",
        "C:\\Program Files\\Git\\bin\\bash.exe",
        "C:\\Program Files (x86)\\Git\\bin\\bash.exe",
    ];

    for path in &candidates {
        if Path::new(path).exists() {
            return CheckResult::pass("Git Bash", &format!("Found: {}", path));
        }
    }

    // 从 PATH 查找
    let path_env = env::var("PATH").unwrap_or_default();
    for entry in path_env.split(';') {
        if entry.contains("Git") && entry.contains("bin") {
            let bash_path = format!("{}\\bash.exe", entry.trim_end_matches('\\'));
            if Path::new(&bash_path).exists() {
                return CheckResult::pass("Git Bash", &format!("Found in PATH: {}", bash_path));
            }
        }
    }

    // 检查环境变量
    if env::var("CLAUDE_CODE_GIT_BASH_PATH").is_ok() {
        return CheckResult::pass("Git Bash", "Set via CLAUDE_CODE_GIT_BASH_PATH");
    }

    CheckResult::fail("Git Bash", "Not found. Claude CLI may not work properly.")
}

/// 检查 Claude CLI
fn check_claude_cli() -> CheckResult {
    // Windows: 检查 ~/.local/bin/claude.exe
    if cfg!(target_os = "windows") {
        if let Some(home) = dirs::home_dir() {
            let claude_path = home.join(".local").join("bin").join("claude.exe");
            if claude_path.exists() {
                return CheckResult::pass("Claude CLI", &format!("Found: {}", claude_path.display()));
            }
        }
    }

    // 从 PATH 查找
    let path_env = env::var("PATH").unwrap_or_default();
    let separator = if cfg!(target_os = "windows") { ';' } else { ':' };

    for entry in path_env.split(separator) {
        let claude_name = if cfg!(target_os = "windows") { "claude.exe" } else { "claude" };
        let claude_path = format!("{}{}{}", entry.trim_end_matches('\\').trim_end_matches('/'),
            if cfg!(target_os = "windows") { "\\" } else { "/" }, claude_name);
        if Path::new(&claude_path).exists() {
            return CheckResult::pass("Claude CLI", &format!("Found in PATH: {}", claude_path));
        }
    }

    CheckResult::fail("Claude CLI", "Not found. Please install Claude CLI.")
}