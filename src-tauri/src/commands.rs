//! Tauri Commands 模块
//! 定义所有 IPC 命令

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::checks::CheckResult;
use crate::pty::get_pty_manager;
use crate::store::{
    AgentInfo, AppConfig, HomeData, McpServerInfo, PluginInfo, Project, ProjectConfig,
    ProjectsState, SessionDetails, SessionInfo, SessionSearchResult, SkillInfo,
};

// ==================== PTY Commands ====================

#[derive(Debug, Deserialize)]
pub struct PtySpawnOptions {
    cwd: String,
    #[serde(rename = "type")]
    pty_type: String, // "claude" | "shell"
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

    let manager = get_pty_manager().ok_or_else(|| "PTY manager not initialized".to_string())?;

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
        Err(e) => Err(e.to_string()),
    }
}

/// 写入 PTY 输入
#[tauri::command]
pub async fn pty_input(id: String, data: String) -> Result<bool, String> {
    let manager = get_pty_manager().ok_or_else(|| "PTY manager not initialized".to_string())?;

    manager
        .write(&id, &data)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// resize PTY
#[tauri::command]
pub async fn pty_resize(id: String, cols: u16, rows: u16) -> Result<bool, String> {
    let manager = get_pty_manager().ok_or_else(|| "PTY manager not initialized".to_string())?;

    manager
        .resize(&id, cols, rows)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// 杀掉 PTY
#[tauri::command]
pub async fn pty_kill(id: String) -> Result<bool, String> {
    let manager = get_pty_manager().ok_or_else(|| "PTY manager not initialized".to_string())?;

    manager.kill(&id).map(|_| true).map_err(|e| e.to_string())
}

/// 杀掉所有 PTY
#[tauri::command]
pub async fn pty_kill_all() -> Result<(), String> {
    let manager = get_pty_manager().ok_or_else(|| "PTY manager not initialized".to_string())?;

    manager.kill_all();
    Ok(())
}

// ==================== Store Commands ====================

pub(crate) async fn spawn_blocking_store<F, R>(command: &'static str, f: F) -> Result<R, String>
where
    F: FnOnce() -> anyhow::Result<R> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|error| format!("{command} blocking task failed: {error}"))?
        .map_err(|error| error.to_string())
}

pub(crate) async fn dispatch_indexed_store<F, R, G>(
    command: &'static str,
    load: F,
    flush: G,
) -> Result<R, String>
where
    F: FnOnce() -> anyhow::Result<crate::session_name_index::IndexedResult<R>> + Send + 'static,
    R: Send + 'static,
    G: FnOnce(crate::session_name_index::PendingIndexFlush) -> anyhow::Result<()> + Send + 'static,
{
    let indexed = spawn_blocking_store(command, load).await?;
    if let Some(pending_flush) = indexed.pending_flush {
        let _flush_task = tokio::task::spawn_blocking(move || {
            let _ = flush(pending_flush);
        });
    }
    Ok(indexed.value)
}

fn flush_session_name_index(
    pending: crate::session_name_index::PendingIndexFlush,
) -> anyhow::Result<()> {
    let store = crate::session_name_index::SessionNameIndexStore::production()?;
    store.flush_pending(pending)?;
    Ok(())
}

/// 获取环境检查结果
#[tauri::command]
pub async fn get_check_results() -> Result<Vec<CheckResult>, String> {
    Ok(crate::get_check_results())
}

/// 重新运行环境检查
#[tauri::command]
pub async fn run_checks() -> Result<Vec<CheckResult>, String> {
    Ok(crate::rerun_checks())
}

/// 一次获取首页数据（项目列表 + 近期会话 + 启动摘要），合并原 get_project_startup_state，
/// 避免启动时重复全扫 ~/.claude/projects/。
#[tauri::command]
pub async fn get_home_data(
    project_limit: Option<usize>,
    session_limit: Option<usize>,
    last_opened: String,
    hidden: Vec<String>,
) -> Result<HomeData, String> {
    let project_limit = project_limit.unwrap_or(12);
    let session_limit = session_limit.unwrap_or(20);
    dispatch_indexed_store(
        "get_home_data",
        move || {
            crate::store::get_home_data_indexed(project_limit, session_limit, &last_opened, &hidden)
        },
        flush_session_name_index,
    )
    .await
}

/// 获取项目列表（支持分页）
#[tauri::command]
pub async fn get_projects(
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<Project>, String> {
    crate::store::get_projects(limit, offset).map_err(|e| e.to_string())
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
    dispatch_indexed_store(
        "get_sessions",
        move || crate::store::get_sessions_indexed(&project_path, limit, offset),
        flush_session_name_index,
    )
    .await
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
    dispatch_indexed_store(
        "get_all_recent_sessions",
        move || crate::store::get_all_recent_sessions_indexed(limit),
        flush_session_name_index,
    )
    .await
}

/// 获取会话详情
#[tauri::command]
pub async fn get_session_details(
    project_path: String,
    session_id: String,
) -> Result<Option<SessionDetails>, String> {
    crate::store::get_session_details(&project_path, &session_id).map_err(|e| e.to_string())
}

/// 搜索会话消息内容
#[tauri::command]
pub async fn search_session_messages(
    project_path: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SessionSearchResult>, String> {
    let limit = limit.unwrap_or(20);
    crate::store::search_session_messages(&project_path, &query, limit).map_err(|e| e.to_string())
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

/// 在 spawn_blocking 内执行 projects.json 锁操作（共享 data_and_lock_paths 路径解析），
/// 统一 JoinError + anyhow::Error → String 转换，避免读/写路径重复 envelope。
async fn spawn_blocking_locked<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&std::path::Path, &std::path::Path) -> anyhow::Result<R> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let (d, l) = crate::store::data_and_lock_paths()?;
        f(&d, &l)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// 获取 projects 状态（共享锁读，与写入事务互斥并返回 canonical 状态）
#[tauri::command]
pub async fn get_projects_state() -> Result<ProjectsState, String> {
    spawn_blocking_locked(crate::store::read_projects_state_locked).await
}

/// 别名校验（与前端 validateDisplayName/input maxlength 同规则）：原始输入 ≤ 32 UTF-16 code unit、无控制字符。
pub(crate) fn validate_display_name_inner(alias: &str) -> Result<()> {
    let len = alias.encode_utf16().count();
    if len > 32 {
        bail!("alias too long ({} > 32)", len);
    }
    if alias.chars().any(|c| c.is_control()) {
        bail!("alias contains control characters");
    }
    Ok(())
}

async fn apply_projects_state_blocking<F>(apply: F) -> Result<ProjectsState, String>
where
    F: FnOnce(&mut ProjectsState) -> anyhow::Result<()> + Send + 'static,
{
    spawn_blocking_locked(move |d, l| crate::store::with_projects_state_locked(d, l, apply)).await
}

/// 置顶项目（锁内幂等：已含 normalized 等价则不重复）
#[tauri::command]
pub async fn pin_project(path: String) -> Result<ProjectsState, String> {
    apply_projects_state_blocking(move |s| {
        let n = crate::store::normalize_path_str(&path);
        if !s.pinned_projects.contains(&n) {
            s.pinned_projects.push(n);
        }
        Ok::<(), anyhow::Error>(())
    })
    .await
}

/// 取消置顶（锁内 normalized 过滤移除）
#[tauri::command]
pub async fn unpin_project(path: String) -> Result<ProjectsState, String> {
    apply_projects_state_blocking(move |s| {
        let n = crate::store::normalize_path_str(&path);
        s.pinned_projects.retain(|p| *p != n);
        Ok::<(), anyhow::Error>(())
    })
    .await
}

/// 存档会话（锁内归并到 canonical key 数组，sessionId 去重）
#[tauri::command]
pub async fn archive_session(
    project_path: String,
    session_id: String,
) -> Result<ProjectsState, String> {
    apply_projects_state_blocking(move |s| {
        // canonicalize 已保证 key 为 normalized，直接用 normalized key
        let n = crate::store::normalize_path_str(&project_path);
        let arr = s.archived_sessions.entry(n).or_default();
        if !arr.contains(&session_id) {
            arr.push(session_id);
        }
        Ok::<(), anyhow::Error>(())
    })
    .await
}

/// 恢复会话（锁内从数组移除，空数组清理 key；未存档幂等）
#[tauri::command]
pub async fn restore_session(
    project_path: String,
    session_id: String,
) -> Result<ProjectsState, String> {
    apply_projects_state_blocking(move |s| {
        let n = crate::store::normalize_path_str(&project_path);
        if let Some(arr) = s.archived_sessions.get_mut(&n) {
            arr.retain(|id| *id != session_id);
            if arr.is_empty() {
                s.archived_sessions.remove(&n);
            }
        }
        Ok::<(), anyhow::Error>(())
    })
    .await
}

/// 设别名（锁内校验 + 删 canonical 等价 key + 非空 set / 空 clear）
#[tauri::command]
pub async fn set_display_name(path: String, alias: String) -> Result<ProjectsState, String> {
    apply_projects_state_blocking(move |s| {
        validate_display_name_inner(&alias)?; // 校验失败返 Err（前后端同规则）
        let n = crate::store::normalize_path_str(&path);
        // canonicalize 已合并等价 key，此处 key 已是 normalized，直接覆盖/删除
        let trimmed = alias.trim();
        if trimmed.is_empty() {
            s.display_names.remove(&n);
        } else {
            s.display_names.insert(n, trimmed.to_string());
        }
        Ok::<(), anyhow::Error>(())
    })
    .await
}

/// 永久删除已存档会话(尽力批,非原子):删文件 + 清标记。薄壳,核心在 store::delete_sessions_inner。
#[tauri::command]
pub async fn delete_sessions(
    project_path: String,
    session_ids: Vec<String>,
) -> Result<ProjectsState, String> {
    tokio::task::spawn_blocking(move || {
        let (data, lock) = crate::store::data_and_lock_paths()?;
        let root = crate::store::claude_projects_root()?;
        crate::store::delete_sessions_inner(&data, &lock, &root, &project_path, &session_ids)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
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

/// 在文件管理器中打开
#[tauri::command]
pub async fn open_in_file_manager(path: String) -> Result<(), String> {
    crate::platform::open_in_file_manager(&path)
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

// ==================== Logging Commands ====================

/// 前端日志写入
#[tauri::command]
pub async fn log_message(level: String, message: String) {
    match level.as_str() {
        "error" => log::error!("[Frontend] {}", message),
        "warn" => log::warn!("[Frontend] {}", message),
        "info" => log::info!("[Frontend] {}", message),
        "debug" => log::debug!("[Frontend] {}", message),
        _ => log::info!("[Frontend] {}", message),
    }
}

/// 获取当前应用可执行文件路径（用于启动新实例）
#[tauri::command]
pub fn get_app_path() -> String {
    std::env::current_exe()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

/// 启动新的应用实例
#[tauri::command]
pub fn spawn_new_instance() -> Result<(), String> {
    let app_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = crate::platform::new_command(&app_path.to_string_lossy());
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}
