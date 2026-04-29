//! 环境检测模块

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// 检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    fn fail_with_path(name: &str, message: String, path: Option<String>, action: &str, url: &str) -> Self {
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

/// 检测claude启动类型："direct" 或 "node"
fn detect_launcher_type(path: &str) -> String {
    // 1. 直接检查扩展名
    if path.ends_with(".js") {
        log::info!("Direct .js file: {}", path);
        return "node".to_string();
    }

    // 2. 检查文件内容
    if let Ok(content) = std::fs::read_to_string(path) {
        let first_lines = content.lines().take(5).collect::<Vec<_>>();

        // 检查node shebang
        if first_lines.iter().any(|line| line.contains("#!/usr/bin/env node")) {
            log::info!("Detected Node.js script by shebang: {}", path);
            return "node".to_string();
        }

        // 检查Anthropic版权信息（cli.js的特征）
        if first_lines.iter().any(|line|
            line.contains("// (c) Anthropic") && line.contains("Version:")
        ) {
            log::info!("Detected Anthropic CLI.js by content: {}", path);
            return "node".to_string();
        }
    }

    // 3. Mac/Linux: 解析符号链接
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(full_path) = std::fs::canonicalize(path) {
            let full_path_str = full_path.to_string_lossy();

            // 检查真实文件扩展名
            if full_path_str.ends_with(".js") {
                log::info!("Symlink points to .js file: {} -> {}", path, full_path_str);
                return "node".to_string();
            }

            // 检查真实文件内容
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let first_lines = content.lines().take(5).collect::<Vec<_>>();
                if first_lines.iter().any(|line| line.contains("#!/usr/bin/env node")) {
                    log::info!("Real file is Node.js script: {}", full_path_str);
                    return "node".to_string();
                }
                // 检查Anthropic版权信息
                if first_lines.iter().any(|line|
                    line.contains("// (c) Anthropic") && line.contains("Version:")
                ) {
                    log::info!("Real file is Anthropic CLI.js: {}", full_path_str);
                    return "node".to_string();
                }
            }
        }
    }

    "direct".to_string()
}

/// 检测node是否可用
fn find_node_executable() -> Option<String> {
    let cmd = if cfg!(target_os = "windows") { "where" } else { "which" };

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        std::process::Command::new(cmd)
            .arg("node")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                } else {
                    None
                }
            })
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new(cmd)
            .arg("node")
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                } else {
                    None
                }
            })
    }
}

/// 运行所有检查
pub fn run_checks() -> ChecksResult {
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
                        updates.insert("claudePath".to_string(), serde_json::Value::String(path.clone()));
                    }
                    "Git Bash" => {
                        updates.insert("gitBashPath".to_string(), serde_json::Value::String(path.clone()));
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
    let output = match run_locate(name) {
        Some(o) => o,
        None => {
            log::warn!("[Check] Failed to run locate for '{}'", name);
            return Vec::new();
        }
    };

    if !output.status.success() {
        log::warn!("[Check] locate '{}' exited with status: {}", name, output.status);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            log::warn!("[Check] stderr: {}", stderr.trim());
        }
        return Vec::new();
    }

    let results: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && Path::new(s).exists())
        .collect();

    log::debug!("[Check] locate '{}' found {} result(s): {:?}", name, results.len(), results);
    results
}

#[cfg(target_os = "windows")]
fn run_locate(name: &str) -> Option<std::process::Output> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    Command::new("cmd")
        .args(["/C", "where", name])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
}

#[cfg(not(target_os = "windows"))]
fn run_locate(name: &str) -> Option<std::process::Output> {
    Command::new("which").arg(name).output().ok()
}

/// 检查 Claude CLI
fn check_claude_cli(config_path: &Option<String>) -> CheckResult {
    // 1. 配置的自定义路径
    if let Some(ref path) = config_path {
        if Path::new(path).exists() {
            // 检测启动类型并保存
            let launcher_type = detect_launcher_type(path);
            save_launcher_type(&launcher_type);

            // 如果需要node，检查node是否可用
            if launcher_type == "node" && find_node_executable().is_none() {
                return CheckResult::fail_with_path(
                    "Claude CLI",
                    "Claude CLI is a Node.js script but 'node' not found.".to_string(),
                    Some(path.clone()),
                    "Install Node.js",
                    "https://nodejs.org/",
                );
            }

            return CheckResult::pass_with_path("Claude CLI", &format!("Found (custom): {}", path), path);
        }
    }

    // 2. where/which 查找
    let exe_name = if cfg!(target_os = "windows") { "claude.exe" } else { "claude" };
    let found = find_all_executables(exe_name);

    if let Some(path) = found.first() {
        // 检测启动类型并保存
        let launcher_type = detect_launcher_type(path);
        save_launcher_type(&launcher_type);

        // 如果需要node，检查node是否可用
        if launcher_type == "node" && find_node_executable().is_none() {
            return CheckResult::fail_with_path(
                "Claude CLI",
                "Claude CLI is a Node.js script but 'node' not found.".to_string(),
                Some(path.clone()),
                "Install Node.js",
                "https://nodejs.org/",
            );
        }

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

/// 保存启动类型到配置
fn save_launcher_type(launcher_type: &str) {
    let updates = serde_json::json!({
        "claudeLauncherType": launcher_type
    });
    match crate::store::update_app_config(updates) {
        Ok(()) => log::info!("[Check] Claude launcher type saved: {}", launcher_type),
        Err(e) => log::warn!("[Check] Failed to save launcher type: {}", e),
    }
}

/// 检查 Git Bash（仅 Windows）
#[cfg(target_os = "windows")]
fn check_git_bash(config_path: &Option<String>) -> CheckResult {
    // 1. 配置中保存的路径
    if let Some(ref path) = config_path {
        if Path::new(path).exists() {
            return CheckResult::pass_with_path("Git Bash", &format!("Found (config): {}", path), path);
        }
    }

    // 2. 环境变量
    if let Ok(path) = std::env::var("CLAUDE_CODE_GIT_BASH_PATH") {
        if Path::new(&path).exists() {
            return CheckResult::pass_with_path("Git Bash", &format!("Found (env): {}", path), &path);
        }
    }

    // 3. where git → 在 git 安装目录下找 bash.exe
    if let Some(bash_path) = detect_git_bash_from_git() {
        return CheckResult::pass_with_path("Git Bash", &format!("Found: {}", bash_path), &bash_path);
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
    let exe_name = if cfg!(target_os = "windows") { "git.exe" } else { "git" };
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
                    log::info!("[Check] Git Bash found via 'where git': {}", bash_path.display());
                    return Some(bash_path.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}
