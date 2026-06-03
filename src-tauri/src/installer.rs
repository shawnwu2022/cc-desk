//! 依赖安装模块
//!
//! 从 OSS 下载 Claude CLI 和 Git 便携版，放置到用户目录并添加到 PATH。
//!
//! 安装路径：
//! - Claude: %USERPROFILE%\.local\bin\claude.exe (Windows) / ~/.local/bin/claude (Unix)
//! - Git: %LOCALAPPDATA%\PortableGit\bin\bash.exe

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "windows"))]
use std::process::Command;
use tauri::{AppHandle, Emitter};

/// OSS 配置
const OSS_BASE_URL: &str = "https://cc-box.oss-cn-beijing.aliyuncs.com";

/// 安装路径
#[cfg(target_os = "windows")]
fn get_install_dirs() -> (PathBuf, PathBuf) {
    let user_profile = std::env::var("USERPROFILE")
        .unwrap_or_else(|_| format!("C:\\Users\\{}", std::env::var("USERNAME").unwrap_or_default()));
    let claude_dir = PathBuf::from(&user_profile).join(".local").join("bin");

    let local_app_data = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| format!("{}\\AppData\\Local", user_profile));
    let git_dir = PathBuf::from(&local_app_data).join("PortableGit");
    (claude_dir, git_dir)
}

#[cfg(not(target_os = "windows"))]
fn get_install_dirs() -> (PathBuf, PathBuf) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let claude_dir = PathBuf::from(&home).join(".local").join("bin");
    let git_dir = PathBuf::from(&home).join("PortableGit"); // macOS/Linux 不需要 Git 便携版
    (claude_dir, git_dir)
}

/// Latest.json 结构
#[derive(Debug, Deserialize, Serialize)]
pub struct ClaudeLatestInfo {
    pub version: String,
    pub release_date: String,
    pub platforms: std::collections::HashMap<String, PlatformInfo>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PlatformInfo {
    pub url: String,
    pub checksum: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GitLatestInfo {
    pub version: String,
    pub release_date: String,
    pub file: String,
    pub url: String,
    pub size: u64,
}

/// 最新版本信息
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestVersions {
    pub claude: ClaudeLatestInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitLatestInfo>,
}

/// 下载进度事件
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub item: String,       // "claude" | "git"
    pub stage: String,      // "fetching" | "downloading" | "extracting" | "placing" | "done" | "error"
    pub progress: u8,       // 0-100
    pub message: String,
}

// ============================================
// HTTP 下载
// ============================================

/// 从 URL 下载文件（无代理，直接访问 OSS）
fn download_file(url: &str, output_path: &Path, app: &AppHandle, item: &str) -> io::Result<()> {
    log::info!("[Installer] Downloading {} from {}", item, url);

    // 发送进度事件
    emit_progress(app, item, "downloading", 0, "开始下载...");

    let mut response = reqwest::blocking::get(url)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    if !response.status().is_success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("HTTP error: {}", response.status()),
        ));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut file = fs::File::create(output_path)?;
    let mut downloaded: u64 = 0;
    let mut buffer = vec![0u8; 8192];

    loop {
        let bytes_read = response.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        file.write_all(&buffer[..bytes_read])?;
        downloaded += bytes_read as u64;

        if total_size > 0 {
            let percent = ((downloaded as f64 / total_size as f64) * 100.0) as u8;
            emit_progress(
                app,
                item,
                "downloading",
                percent,
                format!("下载中 {}%", percent),
            );
        }
    }

    emit_progress(app, item, "downloading", 100, "下载完成");
    log::info!("[Installer] Downloaded {} bytes", downloaded);

    Ok(())
}

/// 发送进度事件
fn emit_progress(app: &AppHandle, item: &str, stage: &str, progress: u8, message: impl Into<String>) {
    let progress_event = DownloadProgress {
        item: item.to_string(),
        stage: stage.to_string(),
        progress,
        message: message.into(),
    };
    if let Err(e) = app.emit("download-progress", progress_event.clone()) {
        log::warn!("[Installer] Failed to emit progress: {}", e);
    }
}

// ============================================
// Claude CLI 安装
// ============================================

/// 获取 Claude 最新版本信息
fn fetch_claude_latest() -> io::Result<ClaudeLatestInfo> {
    let url = format!("{}/deps/claude/latest.json", OSS_BASE_URL);
    log::info!("[Installer] Fetching Claude latest.json from {}", url);

    let response = reqwest::blocking::get(&url)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    if !response.status().is_success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to fetch latest.json: {}", response.status()),
        ));
    }

    let info: ClaudeLatestInfo = response
        .json()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    log::info!("[Installer] Claude latest version: {}", info.version);
    Ok(info)
}

/// 获取当前平台标识
fn get_current_platform() -> String {
    crate::platform::get_platform_id()
}

/// 下载并安装 Claude CLI
#[tauri::command]
pub async fn download_and_install_claude(app: AppHandle) -> Result<(), String> {
    log::info!("[Installer] Starting Claude CLI installation");

    emit_progress(&app, "claude", "fetching", 0, "获取版本信息...");

    // 获取最新版本信息
    let latest = tokio::task::spawn_blocking(|| fetch_claude_latest())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    emit_progress(&app, "claude", "fetching", 100, format!("版本: {}", latest.version));

    // 获取平台信息
    let platform = get_current_platform();
    let platform_info = latest
        .platforms
        .get(&platform)
        .ok_or_else(|| format!("Platform {} not supported", platform))?;

    let download_url = format!("{}/{}", OSS_BASE_URL, platform_info.url);
    let filename = if platform.starts_with("win32") {
        "claude.exe"
    } else {
        "claude"
    };

    // 创建安装目录
    let (claude_dir, _) = get_install_dirs();
    fs::create_dir_all(&claude_dir)
        .map_err(|e| format!("Failed to create Claude dir: {}", e))?;

    let claude_path = claude_dir.join(filename);

    // 下载文件
    emit_progress(&app, "claude", "downloading", 0, "开始下载...");
    let claude_path_clone = claude_path.clone();
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || download_file(&download_url, &claude_path_clone, &app_clone, "claude"))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    // 验证 checksum（可选）
    emit_progress(&app, "claude", "verifying", 50, "验证文件...");
    // TODO: 实现 checksum 验证

    // 设置权限（Unix）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&claude_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }

    // 添加到 PATH（进程级 + 用户级持久化）
    emit_progress(&app, "claude", "placing", 80, "添加到 PATH...");
    add_to_path(&claude_dir);
    add_to_user_path_permanent(&claude_dir);

    // 保存路径到配置文件
    save_install_path_to_config("claudePath", &claude_path);

    emit_progress(&app, "claude", "done", 100, "安装完成");

    log::info!("[Installer] Claude CLI installed to {}", claude_path.display());
    Ok(())
}

// ============================================
// Git 便携版安装（Windows）
// ============================================

#[cfg(target_os = "windows")]
/// 获取 Git 最新版本信息
fn fetch_git_latest() -> io::Result<GitLatestInfo> {
    let url = format!("{}/deps/git/latest.json", OSS_BASE_URL);
    log::info!("[Installer] Fetching Git latest.json from {}", url);

    let response = reqwest::blocking::get(&url)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    if !response.status().is_success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to fetch Git latest.json: {}", response.status()),
        ));
    }

    let info: GitLatestInfo = response
        .json()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    log::info!("[Installer] Git latest version: {}", info.version);
    Ok(info)
}

#[cfg(target_os = "windows")]
/// 解压 PortableGit.7z.exe
fn extract_portable_git(archive_path: &Path, target_dir: &Path, app: &AppHandle) -> io::Result<()> {
    log::info!("[Installer] Extracting PortableGit to {}", target_dir.display());

    emit_progress(app, "git", "extracting", 0, "开始解压...");

    // PortableGit.7z.exe 自解压后内容直接放在输出目录
    // 所以需要：解压到临时目录 → 移动到目标目录

    let local_app_data = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| format!("{}\\AppData\\Local", std::env::var("USERPROFILE").unwrap_or_default()));

    // 创建临时解压目录
    let temp_extract_dir = PathBuf::from(&local_app_data).join("PortableGit-temp");
    if temp_extract_dir.exists() {
        fs::remove_dir_all(&temp_extract_dir)?;
    }
    fs::create_dir_all(&temp_extract_dir)?;

    // 解压到临时目录
    // PortableGit.7z.exe 参数：-y (自动确认), -o"path" (输出目录)
    let temp_extract_str = temp_extract_dir.to_string_lossy().to_string();

    let mut cmd = crate::platform::new_command(&archive_path.to_string_lossy());
    cmd.args(["-y", &format!("-o{}", temp_extract_str)]);
    let status = cmd
        .spawn()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
        .wait()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    if !status.success() {
        // 清理临时目录
        fs::remove_dir_all(&temp_extract_dir).ok();
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Extract failed with status: {}", status),
        ));
    }

    emit_progress(app, "git", "extracting", 50, "移动文件...");

    // 如果目标目录已存在，先删除
    if target_dir.exists() {
        log::info!("[Installer] Removing existing PortableGit directory");
        fs::remove_dir_all(target_dir)?;
    }

    // 移动临时目录到目标目录
    fs::rename(&temp_extract_dir, target_dir)?;
    log::info!("[Installer] PortableGit moved to {}", target_dir.display());

    // 验证解压结果
    let bash_path = target_dir.join("bin").join("bash.exe");
    if !bash_path.exists() {
        // 某些版本 bash.exe 在 usr/bin
        let alt_bash_path = target_dir.join("usr").join("bin").join("bash.exe");
        if alt_bash_path.exists() {
            log::info!("[Installer] bash.exe found in usr/bin");
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("bash.exe not found after extraction"),
            ));
        }
    }

    emit_progress(app, "git", "extracting", 100, "解压完成");
    log::info!("[Installer] PortableGit extracted successfully");
    Ok(())
}

#[cfg(target_os = "windows")]
/// 下载并安装 Git 便携版
#[tauri::command]
pub async fn download_and_install_git(app: AppHandle) -> Result<(), String> {
    log::info!("[Installer] Starting Git portable installation");

    emit_progress(&app, "git", "fetching", 0, "获取版本信息...");

    // 获取最新版本信息
    let latest = tokio::task::spawn_blocking(|| fetch_git_latest())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    emit_progress(&app, "git", "fetching", 100, format!("版本: {}", latest.version));

    let download_url = format!("{}/{}", OSS_BASE_URL, latest.url);

    // 创建安装目录
    let (_, git_dir) = get_install_dirs();
    let _parent_dir = git_dir.parent().unwrap();

    // 下载到临时目录
    let temp_dir = std::env::temp_dir();
    let archive_path = temp_dir.join(&latest.file);

    emit_progress(&app, "git", "downloading", 0, "开始下载...");
    let archive_path_clone = archive_path.clone();
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || download_file(&download_url, &archive_path_clone, &app_clone, "git"))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    // 解压
    emit_progress(&app, "git", "extracting", 0, "开始解压...");
    let archive_path_clone = archive_path.clone();
    let git_dir_clone = git_dir.clone();
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || extract_portable_git(&archive_path_clone, &git_dir_clone, &app_clone))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    // 添加到 PATH（进程级 + 用户级持久化）
    let git_bin_dir = git_dir.join("bin");
    emit_progress(&app, "git", "placing", 80, "添加到 PATH...");
    add_to_path(&git_bin_dir);
    add_to_user_path_permanent(&git_bin_dir);

    // 保存 bash.exe 路径到配置文件
    let bash_path = git_bin_dir.join("bash.exe");
    save_install_path_to_config("gitBashPath", &bash_path);

    // 清理临时文件
    fs::remove_file(&archive_path).ok();

    emit_progress(&app, "git", "done", 100, "安装完成");

    log::info!("[Installer] Git portable installed to {}", git_dir.display());
    Ok(())
}

// ============================================
// PATH 管理
// ============================================

/// 保存安装路径到配置文件
fn save_install_path_to_config(key: &str, path: &Path) {
    let path_str = path.to_string_lossy().to_string();

    // 构建更新 JSON
    let mut updates = serde_json::Map::new();
    updates.insert(key.to_string(), serde_json::Value::String(path_str));

    // 调用 store 模块的更新函数
    match crate::store::update_app_config(serde_json::Value::Object(updates)) {
        Ok(()) => log::info!("[Installer] Saved {} to config: {}", key, path.display()),
        Err(e) => log::warn!("[Installer] Failed to save {} to config: {}", key, e),
    }
}

/// 清理 PATH 字符串并将目录添加到开头（纯函数）
///
/// 移除已有条目（大小写不敏感）后添加到开头，确保最高优先级。
pub(crate) fn clean_and_prepend_path(path: &str, dir: &str, sep: char) -> String {
    let dir_lower = dir.to_lowercase();
    let entries: Vec<&str> = path
        .split(sep)
        .filter(|e| e.trim().to_lowercase() != dir_lower)
        .collect();
    let clean_path = entries.join(&sep.to_string());

    if clean_path.is_empty() {
        dir.to_string()
    } else {
        format!("{}{}{}", dir, sep, clean_path)
    }
}

/// 清理 rc 文件内容并将 export 行追加到末尾（纯函数）
///
/// 移除包含任一 marker 的旧行（大小写不敏感），追加新的 export 行。
/// markers 用于同时匹配绝对路径和 $HOME 相对路径两种格式。
pub(crate) fn clean_rc_content(content: &str, markers: &[&str], export_line: &str) -> String {
    let markers_lower: Vec<String> = markers.iter().map(|m| m.to_lowercase()).collect();
    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            let line_lower = line.to_lowercase();
            !markers_lower.iter().any(|m| line_lower.contains(m.as_str()))
        })
        .collect();
    let mut result = filtered.join("\n");
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&format!("{}\n", export_line));
    result
}

/// 添加目录到进程 PATH（立即生效）
///
/// 始终将目录移到 PATH 最前面，确保最高优先级。
fn add_to_path(dir: &Path) {
    let dir_str = dir.to_string_lossy().to_string();
    let current_path = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ';' } else { ':' };

    let new_path = clean_and_prepend_path(&current_path, &dir_str, sep);
    std::env::set_var("PATH", &new_path);
    log::info!("[Installer] Ensured {} at PATH beginning", dir_str);
}

/// 添加目录到用户环境变量 PATH（持久化）
#[cfg(target_os = "windows")]
fn add_to_user_path_permanent(dir: &Path) {
    let dir_str = dir.to_string_lossy().to_string();

    // 获取当前用户 PATH
    let mut cmd = crate::platform::new_command("powershell");
    cmd.args([
        "-Command",
        "[Environment]::GetEnvironmentVariable('PATH', 'User')",
    ]);
    let output = cmd.output();

    let current_user_path = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(e) => {
            log::warn!("[Installer] Failed to get user PATH: {}", e);
            return;
        }
    };

    // 移除已有条目后添加到开头，确保最高优先级
    let new_user_path = clean_and_prepend_path(&current_user_path, &dir_str, ';');

    // 使用 PowerShell 设置用户 PATH
    let mut set_cmd = crate::platform::new_command("powershell");
    set_cmd.args([
        "-Command",
        &format!(
            "[Environment]::SetEnvironmentVariable('PATH', '{}', 'User')",
            new_user_path
        ),
    ]);
    let set_result = set_cmd.output();

    match set_result {
        Ok(o) => {
            if o.status.success() {
                log::info!("[Installer] Permanently added {} to user PATH", dir_str);
            } else {
                log::warn!(
                    "[Installer] Failed to set user PATH: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
        }
        Err(e) => {
            log::warn!("[Installer] Failed to set user PATH: {}", e);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn add_to_user_path_permanent(dir: &Path) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir_str = dir.to_string_lossy();

    // 使用 $HOME 相对路径，避免硬编码绝对路径
    let rel_path = if dir_str.starts_with(&home) {
        format!("$HOME{}", &dir_str[home.len()..])
    } else {
        dir_str.to_string()
    };
    let export_line = format!("export PATH=\"{}:$PATH\"", rel_path);

    // macOS 默认 zsh → 写入 ~/.zshenv（所有 zsh shell 都加载）
    // Linux 默认 bash → 写入 ~/.bashrc（交互式 shell 加载）
    let shell = std::env::var("SHELL").unwrap_or_default();
    let rc_file = if shell.contains("zsh") {
        PathBuf::from(&home).join(".zshenv")
    } else {
        PathBuf::from(&home).join(".bashrc")
    };

    // 移除已有的该路径相关行，然后追加新行确保最高优先级
    // 同时匹配绝对路径和 $HOME 相对路径格式
    let existing = fs::read_to_string(&rc_file).unwrap_or_default();
    let marker_abs = rel_path.replace("$HOME", &home);
    let markers = [marker_abs.as_str(), rel_path.as_str()];
    let new_content = clean_rc_content(&existing, &markers, &export_line);

    match fs::write(&rc_file, &new_content) {
        Ok(()) => log::info!("[Installer] Ensured {} at PATH beginning in {}", rel_path, rc_file.display()),
        Err(e) => log::warn!("[Installer] Failed to write to {}: {}", rc_file.display(), e),
    }
}

/// 获取最新版本信息（用于前端显示）
#[tauri::command]
pub async fn get_latest_versions() -> Result<LatestVersions, String> {
    log::info!("[Installer] Fetching latest versions info");

    // 获取 Claude 信息
    let claude = tokio::task::spawn_blocking(|| fetch_claude_latest())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    // 获取 Git 信息（Windows only）
    #[cfg(target_os = "windows")]
    let git = Some(
        tokio::task::spawn_blocking(|| fetch_git_latest())
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?,
    );

    #[cfg(not(target_os = "windows"))]
    let git: Option<GitLatestInfo> = None;

    Ok(LatestVersions { claude, git })
}

// ============================================
// Claude CLI 更新检查
// ============================================

/// Claude CLI 更新检查结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCliUpdateInfo {
    pub installed_version: Option<String>,
    pub latest_version: String,
    pub has_update: bool,
    pub not_installed: bool,
}

/// 获取已安装的 Claude CLI 版本号
fn get_installed_claude_version() -> Option<String> {
    let exe_name = if cfg!(target_os = "windows") {
        "claude.exe"
    } else {
        "claude"
    };

    // 1. 优先使用配置路径
    if let Ok(config) = crate::store::get_app_config() {
        if let Some(ref path) = config.claude_path {
            if Path::new(path).exists() {
                if let Some(v) = run_version_command(Path::new(path)) {
                    return Some(v);
                }
            }
        }
    }

    // 2. 检查标准安装目录
    let (claude_dir, _) = get_install_dirs();
    let claude_path = claude_dir.join(exe_name);
    if claude_path.exists() {
        if let Some(v) = run_version_command(&claude_path) {
            return Some(v);
        }
    }

    // 3. 检查 PATH 中的 claude
    run_version_command(Path::new("claude"))
}

/// 执行 claude --version 并解析版本号
#[cfg(target_os = "windows")]
fn run_version_command(program: &Path) -> Option<String> {
    let program_str = program.to_string_lossy();
    let mut cmd = crate::platform::new_command("cmd");
    cmd.args(["/C", &*program_str, "--version"]);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = crate::platform::decode_output(&output.stdout);
    parse_version_output(&stdout)
}

#[cfg(not(target_os = "windows"))]
fn run_version_command(program: &Path) -> Option<String> {
    let output = Command::new(program)
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = crate::platform::decode_output(&output.stdout);
    parse_version_output(&stdout)
}

/// 解析版本输出，提取 x.y.z 格式的版本号
pub(crate) fn parse_version_output(output: &str) -> Option<String> {
    for part in output.split_whitespace() {
        if let Some(v) = extract_semver(part) {
            return Some(v);
        }
    }
    None
}

/// 从字符串中提取 semver 版本号 (x.y.z)
pub(crate) fn extract_semver(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut dot_count = 0u8;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                if bytes[i] == b'.' {
                    dot_count += 1;
                    // 取到第3段时停止（2个点 = x.y.z）
                    if dot_count == 3 {
                        break;
                    }
                }
                i += 1;
            }
            if dot_count >= 2 {
                // 回退到最后一个数字（去除尾部点号）
                let mut j = i;
                while j > start && !bytes[j - 1].is_ascii_digit() {
                    j -= 1;
                }
                let version = &s[start..j];
                if !version.is_empty() && version.ends_with(|c: char| c.is_ascii_digit()) {
                    return Some(version.to_string());
                }
            }
            // 跳过剩余数字/点号
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    None
}

/// 简单的 semver 比较：latest > current 则返回 true
pub(crate) fn is_newer_version(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    let l = parse(latest);
    let c = parse(current);
    for i in 0..std::cmp::max(l.len(), c.len()) {
        let lv = l.get(i).unwrap_or(&0);
        let cv = c.get(i).unwrap_or(&0);
        if lv > cv {
            return true;
        }
        if lv < cv {
            return false;
        }
    }
    false
}

/// 检查 Claude CLI 是否有更新
#[tauri::command]
pub async fn check_claude_cli_update() -> Result<ClaudeCliUpdateInfo, String> {
    // 1. 获取已安装版本
    let installed = tokio::task::spawn_blocking(|| get_installed_claude_version())
        .await
        .map_err(|e| e.to_string())?;

    // 2. 获取最新版本信息
    let latest = tokio::task::spawn_blocking(|| fetch_claude_latest())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let not_installed = installed.is_none();
    let has_update = match &installed {
        Some(v) => is_newer_version(&latest.version, v),
        None => false,
    };

    Ok(ClaudeCliUpdateInfo {
        installed_version: installed,
        latest_version: latest.version,
        has_update,
        not_installed,
    })
}

// ============================================
// Claude CLI 进程管理
// ============================================

/// 检测是否有 claude 进程在运行
#[tauri::command]
pub async fn check_claude_running() -> Result<bool, String> {
    tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "windows")]
        let name = "claude.exe";
        #[cfg(not(target_os = "windows"))]
        let name = "claude";
        crate::platform::is_process_running(name)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 杀死所有 claude 进程
#[tauri::command]
pub async fn kill_claude_processes() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "windows")]
        let name = "claude.exe";
        #[cfg(not(target_os = "windows"))]
        let name = "claude";
        crate::platform::kill_processes_by_name(name)?;
        log::info!("[Installer] Killed claude processes");
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 检查已安装版本
#[tauri::command]
pub async fn check_installed_versions() -> Result<std::collections::HashMap<String, bool>, String> {
    let mut result = std::collections::HashMap::new();

    #[cfg(target_os = "windows")]
    let (claude_dir, git_dir) = get_install_dirs();
    #[cfg(not(target_os = "windows"))]
    let (claude_dir, _git_dir) = get_install_dirs();

    // 检查 Claude
    let platform = get_current_platform();
    let claude_file = if platform.starts_with("win32") {
        "claude.exe"
    } else {
        "claude"
    };
    result.insert("claude".to_string(), claude_dir.join(claude_file).exists());

    // 检查 Git（Windows only）
    #[cfg(target_os = "windows")]
    {
        result.insert("git".to_string(), git_dir.join("bin").join("bash.exe").exists());
    }

    Ok(result)
}