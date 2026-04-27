//! Tauri Commands 模块
//! 定义所有 IPC 命令

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::pty::get_pty_manager;
use crate::store::{AppConfig, Project, SessionInfo, SessionDetails, ProjectConfig, AgentInfo, McpServerInfo, PluginInfo, SkillInfo};

// ==================== PTY Commands ====================

#[derive(Debug, Deserialize)]
pub struct PtySpawnOptions {
    cwd: String,
    #[serde(rename = "type")]
    pty_type: String,  // "claude" | "shell"
    cols: Option<u16>,
    rows: Option<u16>,
    args: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct PtySpawnResult {
    id: String,
    #[serde(rename = "type")]
    pty_type: String,
    cwd: String,
}

/// 启动 PTY
#[tauri::command]
pub async fn pty_spawn(
    options: PtySpawnOptions,
    _app_handle: AppHandle,
) -> Result<Option<PtySpawnResult>, String> {
    let cols = options.cols.unwrap_or(80);
    let rows = options.rows.unwrap_or(24);

    let manager = get_pty_manager()
        .ok_or_else(|| "PTY manager not initialized".to_string())?;

    let result = if options.pty_type == "shell" {
        manager.spawn_shell(&options.cwd, cols, rows)
    } else {
        manager.spawn_claude(&options.cwd, cols, rows, options.args)
    };

    match result {
        Ok(info) => Ok(Some(PtySpawnResult {
            id: info.id,
            pty_type: info.pty_type,
            cwd: info.cwd,
        })),
        Err(e) => Err(e.to_string())
    }
}

/// 写入 PTY 输入
#[tauri::command]
pub async fn pty_input(id: String, data: String) -> Result<bool, String> {
    let manager = get_pty_manager()
        .ok_or_else(|| "PTY manager not initialized".to_string())?;

    manager.write(&id, &data)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// resize PTY
#[tauri::command]
pub async fn pty_resize(id: String, cols: u16, rows: u16) -> Result<bool, String> {
    let manager = get_pty_manager()
        .ok_or_else(|| "PTY manager not initialized".to_string())?;

    manager.resize(&id, cols, rows)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// 杀掉 PTY
#[tauri::command]
pub async fn pty_kill(id: String) -> Result<bool, String> {
    let manager = get_pty_manager()
        .ok_or_else(|| "PTY manager not initialized".to_string())?;

    manager.kill(&id)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// 杀掉所有 PTY
#[tauri::command]
pub async fn pty_kill_all() -> Result<(), String> {
    let manager = get_pty_manager()
        .ok_or_else(|| "PTY manager not initialized".to_string())?;

    manager.kill_all();
    Ok(())
}

// ==================== Store Commands ====================

/// 获取项目列表
#[tauri::command]
pub async fn get_projects() -> Result<Vec<Project>, String> {
    crate::store::get_projects().map_err(|e| e.to_string())
}

/// 获取项目信息
#[tauri::command]
pub async fn get_project_info(path: String) -> Result<Option<Project>, String> {
    crate::store::get_project_info(&path).map_err(|e| e.to_string())
}

/// 获取会话列表
#[tauri::command]
pub async fn get_sessions(
    project_path: String,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SessionInfo>, String> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);
    crate::store::get_sessions(&project_path, limit, offset)
        .map_err(|e| e.to_string())
}

/// 获取会话总数
#[tauri::command]
pub async fn get_session_count(project_path: String) -> Result<usize, String> {
    crate::store::get_session_count(&project_path).map_err(|e| e.to_string())
}

/// 获取所有项目的近期会话
#[tauri::command]
pub async fn get_all_recent_sessions(limit: Option<usize>) -> Result<Vec<SessionInfo>, String> {
    let limit = limit.unwrap_or(20);
    crate::store::get_all_recent_sessions(limit)
        .map_err(|e| e.to_string())
}

/// 获取会话详情
#[tauri::command]
pub async fn get_session_details(
    project_path: String,
    session_id: String,
) -> Result<Option<SessionDetails>, String> {
    crate::store::get_session_details(&project_path, &session_id).map_err(|e| e.to_string())
}

/// 获取应用配置
#[tauri::command]
pub async fn get_app_config() -> Result<AppConfig, String> {
    crate::store::get_app_config().map_err(|e| e.to_string())
}

/// 更新应用配置
#[tauri::command]
pub async fn update_app_config(updates: serde_json::Value) -> Result<(), String> {
    crate::store::update_app_config(updates).map_err(|e| e.to_string())
}

/// 获取默认 Claude 选项
#[tauri::command]
pub async fn get_default_claude_options() -> Result<crate::store::DefaultClaudeOptions, String> {
    crate::store::get_default_claude_options().map_err(|e| e.to_string())
}

/// 保存默认 Claude 选项
#[tauri::command]
pub async fn save_default_claude_options(
    options: crate::store::DefaultClaudeOptions,
) -> Result<(), String> {
    crate::store::save_default_claude_options(options).map_err(|e| e.to_string())
}

/// 保存最近打开项目
#[tauri::command]
pub async fn save_last_project(path: String) -> Result<(), String> {
    crate::store::save_last_project(&path).map_err(|e| e.to_string())
}

/// 选择目录对话框
#[tauri::command]
pub async fn select_directory() -> Result<Option<SelectedDirectory>, String> {
    // 使用 tauri-plugin-dialog 或 tauri API
    // 暂时返回 None，需要添加 dialog plugin
    Ok(None)
}

#[derive(Debug, Serialize)]
pub struct SelectedDirectory {
    path: String,
    name: String,
}

/// 在文件管理器中打开
#[tauri::command]
pub async fn open_in_file_manager(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        std::process::Command::new("explorer")
            .arg(&path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// 测试命令（验证通信）
#[tauri::command]
pub async fn test_communication(message: String) -> Result<String, String> {
    Ok(format!("Received: {}", message))
}

/// 获取项目配置
#[tauri::command]
pub async fn get_project_config(project_path: String) -> Result<ProjectConfig, String> {
    crate::store::get_project_config(&project_path).map_err(|e| e.to_string())
}

/// 获取所有 Agents（包括 built-in、plugin、user、project）
#[tauri::command]
pub async fn get_all_agents(project_path: String) -> Result<Vec<AgentInfo>, String> {
    crate::store::get_all_agents(&project_path).map_err(|e| e.to_string())
}

/// 获取所有 Skills（包括 project、user、plugin）
#[tauri::command]
pub async fn get_all_skills(project_path: String) -> Result<Vec<SkillInfo>, String> {
    crate::store::get_all_skills(&project_path).map_err(|e| e.to_string())
}

/// 获取所有 MCP Servers（包括 plugin 和配置的）
#[tauri::command]
pub async fn get_all_mcp_servers(project_path: String) -> Result<Vec<McpServerInfo>, String> {
    crate::store::get_all_mcp_servers(&project_path).map_err(|e| e.to_string())
}

/// 获取所有 Plugins（使用 --json）
#[tauri::command]
pub async fn get_all_plugins(project_path: String) -> Result<Vec<PluginInfo>, String> {
    crate::store::get_all_plugins(&project_path).map_err(|e| e.to_string())
}

/// 获取 MCP Server 详情（通过 MCP 协议）
#[tauri::command]
pub async fn get_mcp_server_detail(
    project_path: String,
    server_name: String,
    force_refresh: bool,
) -> Result<Option<crate::mcp::McpServerDetail>, String> {
    // 先从 store 获取 server 的 URL、command 和 headers
    let servers = crate::store::get_all_mcp_servers(&project_path).map_err(|e| e.to_string())?;
    let server = servers.iter().find(|s| s.name == server_name);

    if server.is_none() {
        return Ok(None);
    }

    let server = server.unwrap();
    let url = server.url.as_deref();
    let command = server.command.as_deref();
    let headers = server.headers.as_ref();

    crate::mcp::get_mcp_server_detail_cached(&server_name, url, command, headers, force_refresh).await
}

// ==================== Updater Commands ====================

/// 检查 GitHub Releases 是否有新版本
#[tauri::command]
pub async fn check_for_updates() -> Result<crate::updater::UpdateInfo, String> {
    crate::updater::check_for_updates().await.map_err(|e| e.to_string())
}