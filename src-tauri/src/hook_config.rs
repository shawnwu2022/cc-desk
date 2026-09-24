use anyhow::Result;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use once_cell::sync::Lazy;

// 编译时嵌入 plugin 源文件
const PLUGIN_JSON: &str = include_str!("../plugin/.claude-plugin/plugin.json");
const HOOKS_JSON: &str = include_str!("../plugin/hooks/hooks.json");
const REPORT_HOOK_SH: &str = include_str!("../plugin/scripts/report-hook.sh");

/// Fingerprint all embedded plugin bytes. A script change must redeploy even
/// when the human-facing plugin version was not bumped.
pub(crate) fn deployment_id(plugin_json: &str, hooks_json: &str, report_hook: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in [plugin_json, hooks_json, report_hook] {
        for byte in part.as_bytes().iter().copied().chain(std::iter::once(0xff)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("content-{hash:016x}")
}

static PLUGIN_DEPLOYMENT_ID: Lazy<String> =
    Lazy::new(|| deployment_id(PLUGIN_JSON, HOOKS_JSON, REPORT_HOOK_SH));

/// Plugin 目标路径（~/.cc-box/claude-plugin/）
pub fn plugin_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Home directory not found")
        .join(".cc-box")
        .join("claude-plugin")
}

/// 确保 plugin 文件存在于目标路径
pub fn ensure_plugin_files() -> Result<()> {
    let dir = plugin_dir();
    let version_file = dir.join(".version");

    // 版本匹配时跳过
    if version_file.exists() {
        if let Ok(existing_version) = fs::read_to_string(&version_file) {
            if existing_version.trim() == *PLUGIN_DEPLOYMENT_ID {
                log::info!(
                    "Plugin deployment {} matches, skipping deployment",
                    *PLUGIN_DEPLOYMENT_ID
                );
                return Ok(());
            }
        }
    }

    // 版本不匹配或不存在，需要部署
    log::info!("Deploying plugin deployment {}", *PLUGIN_DEPLOYMENT_ID);

    fs::create_dir_all(dir.join(".claude-plugin"))?;
    fs::create_dir_all(dir.join("hooks"))?;
    fs::create_dir_all(dir.join("scripts"))?;

    write_file(dir.join(".claude-plugin").join("plugin.json"), PLUGIN_JSON)?;
    write_file(dir.join("hooks").join("hooks.json"), HOOKS_JSON)?;
    write_executable(dir.join("scripts").join("report-hook.sh"), REPORT_HOOK_SH)?;
    write_file(version_file, &PLUGIN_VERSION)?;

    log::info!("Plugin deployed successfully");
    Ok(())
}

fn write_file(path: PathBuf, content: &str) -> Result<()> {
    fs::write(&path, content)?;
    Ok(())
}

/// 写入可执行文件（Unix 系统设置 0755 权限）
fn write_executable(path: PathBuf, content: &str) -> Result<()> {
    fs::write(&path, content)?;

    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;

    Ok(())
}
