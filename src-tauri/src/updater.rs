//! Updater 模块
//! 通过 GitHub Releases API 检查更新，支持下载和安装
//! 使用 ETag 缓存避免 GitHub API 速率限制

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

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

/// ETag 缓存，避免重复请求 GitHub API
#[derive(Serialize, Deserialize)]
struct CachedResponse {
    etag: Option<String>,
    remote_version: String,
    release_notes: String,
    download_url: String,
    platform_asset: Option<PlatformAsset>,
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
    let (preferred, fallback): (&[&str], &[&str]) = if cfg!(target_os = "windows") {
        (&["-setup.exe"], &[".exe"])
    } else if cfg!(target_os = "macos") {
        (&[".dmg"], &[])
    } else {
        (&[".appimage"], &[])
    };

    let name_lower = |a: &GitHubAsset| a.name.to_lowercase();

    for pattern in preferred {
        if let Some(a) = assets.iter().find(|a| name_lower(a).ends_with(pattern)) {
            return Some(PlatformAsset {
                name: a.name.clone(),
                url: a.browser_download_url.clone(),
                size: a.size,
            });
        }
    }

    for pattern in fallback {
        if let Some(a) = assets.iter().find(|a| name_lower(a).ends_with(pattern)) {
            return Some(PlatformAsset {
                name: a.name.clone(),
                url: a.browser_download_url.clone(),
                size: a.size,
            });
        }
    }

    None
}

fn get_cache_path(app_handle: &AppHandle) -> Result<std::path::PathBuf> {
    let dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("update-cache.json"))
}

fn read_cache(app_handle: &AppHandle) -> Option<CachedResponse> {
    let path = get_cache_path(app_handle).ok()?;
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_cache(app_handle: &AppHandle, cache: &CachedResponse) -> Result<()> {
    let path = get_cache_path(app_handle)?;
    let data = serde_json::to_string(cache)?;
    std::fs::write(&path, data)?;
    Ok(())
}

/// 检查 GitHub Releases 是否有新版本，使用 ETag 缓存减少 API 调用
pub async fn check_for_updates(app_handle: AppHandle) -> Result<UpdateInfo> {
    let current = get_current_version();
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let cache = read_cache(&app_handle);

    let client = reqwest::Client::builder()
        .user_agent("CC-Box-Updater")
        .build()?;

    let mut request = client.get(&url);

    // 附加 If-None-Match 头，命中 304 时不计入速率限制
    if let Some(ref cached) = cache {
        if let Some(ref etag) = cached.etag {
            request = request.header("If-None-Match", etag);
        }
    }

    let response = request.send().await?;

    // 304 Not Modified — 使用缓存数据，重新比较版本号（应用可能已更新）
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        if let Some(cached) = cache {
            let has_update = is_newer_version(&current, &cached.remote_version);
            return Ok(UpdateInfo {
                version: cached.remote_version,
                current_version: current,
                has_update,
                release_notes: cached.release_notes,
                download_url: cached.download_url,
                platform_asset: cached.platform_asset,
            });
        }
    }

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

    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let release: GitHubRelease = response.json().await?;
    let remote_version = release.tag_name.trim_start_matches('v').to_string();
    let has_update = is_newer_version(&current, &remote_version);
    let platform_asset = find_platform_asset(&release.assets);

    let result = UpdateInfo {
        version: remote_version.clone(),
        current_version: current,
        has_update,
        release_notes: release.body.unwrap_or_default(),
        download_url: release.html_url.clone(),
        platform_asset: platform_asset.clone(),
    };

    // 缓存响应供后续 ETag 请求使用
    let cached = CachedResponse {
        etag,
        remote_version,
        release_notes: result.release_notes.clone(),
        download_url: release.html_url,
        platform_asset,
    };
    let _ = write_cache(&app_handle, &cached);

    Ok(result)
}

/// 下载更新文件到临时目录，发送进度事件
/// expected_size 用于校验已有缓存文件的完整性
pub async fn download_update(
    url: String,
    file_name: String,
    expected_size: u64,
    app_handle: AppHandle,
) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("cc-box-update");
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    let file_path = temp_dir.join(&file_name);

    // 校验已有缓存：大小必须与预期完全匹配，否则删除重新下载
    if file_path.exists() {
        if let Ok(meta) = std::fs::metadata(&file_path) {
            if expected_size > 0 && meta.len() == expected_size {
                return Ok(file_path.to_string_lossy().to_string());
            }
            // 大小不匹配（下载中断或版本变更），删除旧文件
            let _ = std::fs::remove_file(&file_path);
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
    let mut last_percent: u8 = 255;

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

        let entries = std::fs::read_dir(mount_point).map_err(|e| e.to_string())?;
        let mut found = false;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".app") {
                let src = entry.path();
                let dst = std::path::PathBuf::from("/Applications").join(&name);
                let _ = std::fs::remove_dir_all(&dst);
                std::process::Command::new("cp")
                    .args(["-R", &src.to_string_lossy(), &dst.to_string_lossy()])
                    .status()
                    .map_err(|e| format!("Failed to copy app: {}", e))?;
                let _ = std::process::Command::new("hdiutil")
                    .args(["detach", mount_point, "-quiet"])
                    .status();
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

        if std::fs::rename(&file_path, &current_exe).is_err() {
            let dest = current_exe
                .parent()
                .ok_or_else(|| "Cannot determine exe directory".to_string())?
                .join(path.file_name().ok_or_else(|| "Invalid file name".to_string())?);
            std::fs::copy(&file_path, &dest)
                .map_err(|e| format!("Failed to copy update: {}", e))?;
        }

        let _ = std::process::Command::new("chmod")
            .args(["+x", &current_exe.to_string_lossy()])
            .status();
        std::process::Command::new(&current_exe).spawn().ok();
        app_handle.exit(0);
    }

    Ok(())
}
