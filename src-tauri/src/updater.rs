//! Updater 模块
//! 通过 GitHub Releases API 检查更新

use anyhow::Result;
use serde::{Deserialize, Serialize};

const GITHUB_REPO: &str = "orczh-hj/cc-box";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub has_update: bool,
    pub release_notes: String,
    pub download_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    html_url: String,
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

/// 检查 GitHub Releases 是否有新版本
pub async fn check_for_updates() -> Result<UpdateInfo> {
    let current = get_current_version();
    let url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);

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
        });
    }

    let release: GitHubRelease = response.json().await?;
    let remote_version = release.tag_name.trim_start_matches('v').to_string();
    let has_update = is_newer_version(&current, &remote_version);

    Ok(UpdateInfo {
        version: remote_version,
        current_version: current,
        has_update,
        release_notes: release.body.unwrap_or_default(),
        download_url: release.html_url,
    })
}
