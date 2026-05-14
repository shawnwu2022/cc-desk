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
    #[cfg(target_os = "windows")]
    {
        if std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_default() == "ARM64" {
            "win32-arm64".to_string()
        } else {
            "win32-x64".to_string()
        }
    }
    #[cfg(target_os = "macos")]
    {
        // 检测架构
        if std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_default() == "ARM64"
            || std::env::var("HOSTTYPE").unwrap_or_default().contains("aarch64") {
            "darwin-arm64".to_string()
        } else {
            "darwin-x64".to_string()
        }
    }
    #[cfg(target_os = "linux")]
    {
        // 检测架构
        let arch = std::env::var("HOSTTYPE").unwrap_or_default();
        if arch.contains("aarch64") || arch.contains("arm64") {
            // 检测 musl/glibc
            if std::path::Path::new("/lib/libc.musl-aarch64.so.1").exists() {
                "linux-arm64-musl".to_string()
            } else {
                "linux-arm64".to_string()
            }
        } else {
            // x64
            if std::path::Path::new("/lib/libc.musl-x86_64.so.1").exists() {
                "linux-x64-musl".to_string()
            } else {
                "linux-x64".to_string()
            }
        }
    }
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

    let status = Command::new(archive_path)
        .args(["-y", &format!("-o{}", temp_extract_str)])
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

/// 添加目录到进程 PATH（立即生效）
fn add_to_path(dir: &Path) {
    let dir_str = dir.to_string_lossy().to_string();
    let current_path = std::env::var("PATH").unwrap_or_default();

    // 避免重复添加
    if current_path.contains(&dir_str) {
        log::info!("[Installer] {} already in PATH", dir_str);
        return;
    }

    // 添加到 PATH 开头（优先级更高）
    let new_path = format!("{};{}", dir_str, current_path);
    std::env::set_var("PATH", &new_path);

    log::info!("[Installer] Added {} to PATH", dir_str);
}

/// 添加目录到用户环境变量 PATH（持久化）
#[cfg(target_os = "windows")]
fn add_to_user_path_permanent(dir: &Path) {
    use std::process::Command;

    let dir_str = dir.to_string_lossy().to_string();

    // 获取当前用户 PATH
    let output = Command::new("powershell")
        .args([
            "-Command",
            "[Environment]::GetEnvironmentVariable('PATH', 'User')",
        ])
        .output();

    let current_user_path = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(e) => {
            log::warn!("[Installer] Failed to get user PATH: {}", e);
            return;
        }
    };

    // 避免重复添加
    let current_lower = current_user_path.to_lowercase();
    let dir_lower = dir_str.to_lowercase();
    if current_lower.contains(&dir_lower) {
        log::info!("[Installer] {} already in user PATH", dir_str);
        return;
    }

    // 添加到用户 PATH 开头
    let new_user_path = if current_user_path.is_empty() {
        dir_str.clone()
    } else {
        format!("{};{}", dir_str, current_user_path)
    };

    // 使用 PowerShell 设置用户 PATH
    let set_result = Command::new("powershell")
        .args([
            "-Command",
            &format!(
                "[Environment]::SetEnvironmentVariable('PATH', '{}', 'User')",
                new_user_path
            ),
        ])
        .output();

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
    // macOS/Linux: 写入 ~/.bashrc 或 ~/.zshrc
    // 暂不实现，Unix 用户通常已安装 Claude CLI
    log::info!("[Installer] User PATH persistence not implemented for non-Windows");
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

/// 检查已安装版本
#[tauri::command]
pub async fn check_installed_versions() -> Result<std::collections::HashMap<String, bool>, String> {
    let mut result = std::collections::HashMap::new();

    let (claude_dir, git_dir) = get_install_dirs();

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