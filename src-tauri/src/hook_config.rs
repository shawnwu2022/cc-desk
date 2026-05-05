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

/// Plugin 版本（独立于应用版本，从 plugin.json 解析）
static PLUGIN_VERSION: Lazy<String> = Lazy::new(|| {
    serde_json::from_str::<serde_json::Value>(PLUGIN_JSON)
        .ok()
        .and_then(|v| v.get("version").map(|s| s.as_str().unwrap_or("unknown").to_string()))
        .unwrap_or_else(|| "unknown".to_string())
});

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
            if existing_version.trim() == *PLUGIN_VERSION {
                log::info!("Plugin version {} matches, skipping deployment", *PLUGIN_VERSION);
                return Ok(());
            }
        }
    }

    // 版本不匹配或不存在，需要部署
    log::info!("Deploying plugin version {}", *PLUGIN_VERSION);

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