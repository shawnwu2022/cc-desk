//! Updater 模块
//! 通过 GitHub Releases API 检查更新，支持下载和安装

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

const GITHUB_REPO: &str = "orczh-hj/cc-box";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub has_update: bool,
    pub release_notes: String,
    pub download_url: String,
    pub platform_asset: Option<PlatformAsset>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percent: f64,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

/// 从 Cargo.toml 版本获取当前版本号
fn get_current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 比较语义化版本号，返回 true 如果 remote_version > current_version
fn is_newer_version(current: &str, remote: &str) -> bool {
    let parse_parts = |v: &str| -> Vec<u32> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };

    let cur = parse_parts(current);
    let rem = parse_parts(remote);

    for i in 0..rem.len().max(cur.len()) {
        let c = cur.get(i).unwrap_or(&0);
        let r = rem.get(i).unwrap_or(&0);
        if r > c {
            return true;
        }
        if r < c {
            return false;
        }
    }
    false
}

/// 根据当前平台匹配对应的安装包
fn find_platform_asset(assets: &[GitHubAsset]) -> Option<PlatformAsset> {
    assets
        .iter()
        .find(|a| {
            let name = a.name.to_lowercase();
            if cfg!(target_os = "windows") {
                name.ends_with(".exe") && !name.contains("update")
            } else if cfg!(target_os = "macos") {
                name.ends_with(".dmg")
            } else {
                name.ends_with(".appimage")
            }
        })
        .map(|a| PlatformAsset {
            name: a.name.clone(),
            url: a.browser_download_url.clone(),
            size: a.size,
        })
}

/// 检查 GitHub Releases 是否有新版本
pub async fn check_for_updates() -> Result<UpdateInfo> {
    let current = get_current_version();
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = reqwest::Client::builder()
        .user_agent("CC-Box-Updater")
        .build()?;

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Ok(UpdateInfo {
            version: current.clone(),
            current_version: current,
            has_update: false,
            release_notes: String::new(),
            download_url: String::new(),
            platform_asset: None,
        });
    }

    let release: GitHubRelease = response.json().await?;
    let remote_version = release.tag_name.trim_start_matches('v').to_string();
    let has_update = is_newer_version(&current, &remote_version);
    let platform_asset = find_platform_asset(&release.assets);

    Ok(UpdateInfo {
        version: remote_version,
        current_version: current,
        has_update,
        release_notes: release.body.unwrap_or_default(),
        download_url: release.html_url,
        platform_asset,
    })
}

/// 下载更新文件到临时目录，发送进度事件
pub async fn download_update(
    url: String,
    file_name: String,
    app_handle: AppHandle,
) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("cc-box-update");
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    let file_path = temp_dir.join(&file_name);

    // 如果文件已存在且大小 > 0，直接返回（支持断点续传可后续优化）
    if file_path.exists() {
        if let Ok(meta) = std::fs::metadata(&file_path) {
            if meta.len() > 0 {
                return Ok(file_path.to_string_lossy().to_string());
            }
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("CC-Box-Updater")
        .build()
        .map_err(|e| e.to_string())?;

    let mut response = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let total = response.content_length().unwrap_or(0);

    let mut file = std::fs::File::create(&file_path).map_err(|e| e.to_string())?;
    let mut downloaded: u64 = 0;
    let mut last_percent: u8 = 255; // 初始值确保第一次一定会发送

    use std::io::Write;
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        if total > 0 {
            let percent = (downloaded as f64 / total as f64 * 100.0) as u8;
            if percent != last_percent {
                last_percent = percent;
                let _ = app_handle.emit(
                    "update:download-progress",
                    DownloadProgress {
                        downloaded,
                        total,
                        percent: percent as f64,
                    },
                );
            }
        }
    }

    // 发送完成事件
    let _ = app_handle.emit(
        "update:download-progress",
        DownloadProgress {
            downloaded,
            total: if total == 0 { downloaded } else { total },
            percent: 100.0,
        },
    );

    Ok(file_path.to_string_lossy().to_string())
}

/// 安装更新（多平台）
pub async fn install_update(file_path: String, app_handle: AppHandle) -> Result<(), String> {
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(&file_path)
            .spawn()
            .map_err(|e| format!("Failed to start installer: {}", e))?;
        app_handle.exit(0);
    }

    #[cfg(target_os = "macos")]
    {
        let mount_point = "/tmp/cc-box-update";
        // 挂载 DMG
        std::process::Command::new("hdiutil")
            .args([
                "attach",
                &file_path,
                "-mountpoint",
                mount_point,
                "-nobrowse",
                "-quiet",
            ])
            .status()
            .map_err(|e| format!("Failed to mount DMG: {}", e))?;

        // 查找并复制 .app
        let entries = std::fs::read_dir(mount_point).map_err(|e| e.to_string())?;
        let mut found = false;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".app") {
                let src = entry.path();
                let dst = std::path::PathBuf::from("/Applications").join(&name);
                // 删除旧版本
                let _ = std::fs::remove_dir_all(&dst);
                std::process::Command::new("cp")
                    .args(["-R", &src.to_string_lossy(), &dst.to_string_lossy()])
                    .status()
                    .map_err(|e| format!("Failed to copy app: {}", e))?;
                // 卸载 DMG
                let _ = std::process::Command::new("hdiutil")
                    .args(["detach", mount_point, "-quiet"])
                    .status();
                // 启动新版本
                std::process::Command::new("open").arg(&dst).spawn().ok();
                found = true;
                break;
            }
        }
        if !found {
            let _ = std::process::Command::new("hdiutil")
                .args(["detach", mount_point, "-quiet"])
                .status();
            return Err("No .app found in DMG".to_string());
        }
        app_handle.exit(0);
    }

    #[cfg(target_os = "linux")]
    {
        let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let backup = current_exe.with_extension("appimage.bak");
        let _ = std::fs::copy(&current_exe, &backup);
        std::fs::rename(&file_path, &current_exe).map_err(|e| e.to_string())?;
        let _ = std::process::Command::new("chmod")
            .args(["+x", &current_exe.to_string_lossy()])
            .status();
        std::process::Command::new(&current_exe).spawn().ok();
        app_handle.exit(0);
    }

    Ok(())
}
