//! Updater 模块
//! 从阿里云 OSS 检查更新、下载和安装
//! 国内用户使用 OSS 加速下载，文件保存在 downloads 目录

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

const OSS_LATEST_URL: &str = "https://cc-box.oss-cn-beijing.aliyuncs.com/cc-box/latest.json";

/// 下载中断标志
static DOWNLOAD_CANCEL_FLAG: AtomicBool = AtomicBool::new(false);

/// 取消当前下载
pub fn cancel_download() {
    DOWNLOAD_CANCEL_FLAG.store(true, Ordering::SeqCst);
}

/// 重置下载标志（新下载开始时调用）
fn reset_download_flag() {
    DOWNLOAD_CANCEL_FLAG.store(false, Ordering::SeqCst);
}

/// 检查是否被取消
fn is_download_cancelled() -> bool {
    DOWNLOAD_CANCEL_FLAG.load(Ordering::SeqCst)
}

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

/// OSS latest.json 响应结构
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OssLatestInfo {
    version: String,
    #[serde(rename = "release_date")]
    release_date: String,
    release_notes: String,
    #[serde(rename = "release_notes_url")]
    release_notes_url: String,
    assets: OssAssets,
}

#[derive(Debug, Deserialize)]
struct OssAssets {
    windows: OssAssetInfo,
    macos: OssAssetInfo,
    linux: OssAssetInfo,
}

#[derive(Debug, Deserialize)]
struct OssAssetInfo {
    url: String,
    size: u64,
}

/// 从编译时注入的环境变量获取版本号（源自 package.json）
/// 这是版本号的唯一来源，确保前后端版本一致
fn get_current_version(_app_handle: &AppHandle) -> String {
    env!("APP_VERSION").to_string()
}

/// 比较语义化版本号，返回 true 如果 remote_version > current_version
pub(crate) fn is_newer_version(current: &str, remote: &str) -> bool {
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

/// 从 URL 提取文件名
pub(crate) fn extract_filename(url: &str) -> String {
    url.rsplit('/').next().unwrap_or("unknown").to_string()
}

/// 从 OSS 检查更新
async fn check_oss_updates() -> Result<(String, String, String, PlatformAsset)> {
    let client = reqwest::Client::builder()
        .user_agent("CC-Box-Updater")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = client.get(OSS_LATEST_URL).send().await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("OSS request failed: {}", response.status()));
    }

    let info: OssLatestInfo = response.json().await?;

    let (asset_info, name) = if cfg!(target_os = "windows") {
        (&info.assets.windows, extract_filename(&info.assets.windows.url))
    } else if cfg!(target_os = "macos") {
        (&info.assets.macos, extract_filename(&info.assets.macos.url))
    } else {
        (&info.assets.linux, extract_filename(&info.assets.linux.url))
    };

    Ok((
        info.version,
        info.release_notes,
        info.release_notes_url,
        PlatformAsset {
            name,
            url: asset_info.url.clone(),
            size: asset_info.size,
        },
    ))
}

/// 检查更新：仅从 OSS 获取
pub async fn check_for_updates(app_handle: AppHandle) -> Result<UpdateInfo> {
    let current = get_current_version(&app_handle);

    // 从 OSS 检查更新
    let (version, release_notes, release_url, asset) = check_oss_updates().await.map_err(|e| {
        eprintln!("[Updater] check_oss_updates failed: {:?}", e);
        e
    })?;

    let has_update = is_newer_version(&current, &version);
    Ok(UpdateInfo {
        version,
        current_version: current,
        has_update,
        release_notes,
        download_url: release_url,
        platform_asset: if has_update { Some(asset) } else { None },
    })
}

/// 下载更新文件到 downloads 目录，发送进度事件
/// expected_size 用于校验已有文件的完整性
pub async fn download_update(
    url: String,
    file_name: String,
    expected_size: u64,
    app_handle: AppHandle,
) -> Result<String, String> {
    // 重置中断标志
    reset_download_flag();

    // 使用 downloads 目录保存下载文件（用户缓存）
    let downloads_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("downloads");
    std::fs::create_dir_all(&downloads_dir).map_err(|e| e.to_string())?;
    let file_path = downloads_dir.join(&file_name);

    // 校验已有文件：大小必须与预期完全匹配，否则重新下载
    if file_path.exists() {
        if let Ok(meta) = std::fs::metadata(&file_path) {
            if expected_size > 0 && meta.len() == expected_size {
                return Ok(file_path.to_string_lossy().to_string());
            }
            // 大小不匹配（下载中断或版本变更），删除旧文件重新下载
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
        // 检查是否被取消
        if is_download_cancelled() {
            // 保留部分下载的文件，下次可续传
            return Err("Download cancelled".to_string());
        }

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

    // 最终检查（防止在最后时刻被取消）
    if is_download_cancelled() {
        return Err("Download cancelled".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_newer_version(current, remote) → true if remote > current ──

    #[test]
    fn patch_升级返回_true() {
        assert!(is_newer_version("0.6.3", "0.6.4"));
    }

    #[test]
    fn 降级返回_false() {
        assert!(!is_newer_version("0.6.4", "0.6.3"));
    }

    #[test]
    fn 相同版本返回_false() {
        assert!(!is_newer_version("0.6.4", "0.6.4"));
    }

    #[test]
    fn major_升级返回_true() {
        assert!(is_newer_version("0.6.4", "1.0.0"));
    }

    #[test]
    fn minor_升级返回_true() {
        assert!(is_newer_version("0.6.4", "0.7.0"));
    }

    #[test]
    fn v_前缀版本正确处理() {
        assert!(is_newer_version("v0.6.4", "v0.7.0"));
    }

    #[test]
    fn 混合_v_前缀正确处理() {
        assert!(is_newer_version("v0.6.4", "0.7.0"));
    }

    #[test]
    fn 不等长段数用_0_补齐() {
        assert!(is_newer_version("0.6", "0.6.4"));
    }

    #[test]
    fn 非数字段被跳过() {
        assert!(!is_newer_version("0.6.4-beta", "0.6.4"));
    }

    // ── extract_filename(url) → last path segment or "unknown" ──

    #[test]
    fn 从简单_url_提取文件名() {
        assert_eq!(extract_filename("https://example.com/file.zip"), "file.zip");
    }

    #[test]
    fn 从嵌套路径提取文件名() {
        assert_eq!(
            extract_filename("https://example.com/a/b/c/file.msi"),
            "file.msi"
        );
    }

    #[test]
    fn url_以_斜杠_结尾返回_unknown() {
        assert_eq!(extract_filename("https://example.com/"), "unknown");
    }

    #[test]
    fn 空字符串返回_unknown() {
        assert_eq!(extract_filename(""), "unknown");
    }
}
