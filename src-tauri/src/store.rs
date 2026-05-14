//! Store 模块
//! Claude Code 原生数据读取

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 项目信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub path: String,
    pub name: String,
    pub last_session_id: Option<String>,
    pub last_cost: Option<f64>,
    #[serde(rename = "lastDuration")]
    pub last_duration: Option<u64>,
}

/// 会话信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub name: String,
    #[serde(rename = "projectPath")]
    pub project_path: String,
    #[serde(rename = "lastActiveAt")]
    pub last_active_at: u64,
}

/// 会话详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetails {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub name: String,
    #[serde(rename = "messageCount")]
    pub message_count: usize,
    #[serde(rename = "totalTokens")]
    pub total_tokens: Option<u64>,
    #[serde(rename = "totalCost")]
    pub total_cost: Option<f64>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<u64>,
    #[serde(rename = "lastActiveAt")]
    pub last_active_at: u64,
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(rename = "defaultContinue")]
    pub default_continue: Option<bool>,
    #[serde(rename = "defaultSkipPermissions")]
    pub default_skip_permissions: Option<bool>,
    #[serde(rename = "defaultCustomArgs")]
    pub default_custom_args: Option<String>,
    pub theme: Option<String>,
    #[serde(rename = "fontSize")]
    pub font_size: Option<u16>,
    #[serde(rename = "autoConnectIde")]
    pub auto_connect_ide: Option<bool>,
    #[serde(rename = "hiddenProjects")]
    pub hidden_projects: Option<Vec<String>>,
    #[serde(rename = "lastOpenedProject")]
    pub last_opened_project: Option<String>,
    #[serde(rename = "windowSize")]
    pub window_size: Option<WindowSize>,
    #[serde(rename = "claudePath")]
    pub claude_path: Option<String>,
    #[serde(rename = "gitBashPath")]
    pub git_bash_path: Option<String>,
    #[serde(rename = "claudeEnvVars")]
    pub claude_env_vars: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

/// 默认 Claude 选项
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DefaultClaudeOptions {
    #[serde(rename = "continue")]
    pub continue_opt: Option<bool>,
    pub resume: Option<String>,
    #[serde(rename = "skipPermissions")]
    pub skip_permissions: Option<bool>,
    #[serde(rename = "customArgs")]
    pub custom_args: Option<String>,
}

/// 配置来源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub label: String,
    pub path: Option<String>,
}

/// 基本配置项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicConfigItem {
    pub model: Option<String>,
    pub theme: Option<String>,
    #[serde(rename = "editorMode")]
    pub editor_mode: Option<String>,
    #[serde(rename = "autoConnectIde")]
    pub auto_connect_ide: Option<bool>,
    pub permissions: Option<PermissionsConfig>,
    pub env: Option<serde_json::Value>,
    pub source: ConfigSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    pub allow: Option<Vec<String>>,
    pub deny: Option<Vec<String>>,
}

/// MCP 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerItem {
    pub name: String,
    #[serde(rename = "type")]
    pub server_type: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<serde_json::Value>,
    pub url: Option<String>,
    pub source: ConfigSource,
}

/// Skill 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillItem {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
    pub source: ConfigSource,
}

/// Agent 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentItem {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
    pub source: ConfigSource,
}

/// Hook 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookItem {
    pub event: String,
    pub matcher: Option<String>,
    pub command: Option<String>,
    #[serde(rename = "hookType")]
    pub hook_type: Option<String>,
    pub source: ConfigSource,
}

/// 项目配置结果
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    pub basic: Vec<BasicConfigItem>,
    pub mcp: Vec<McpServerItem>,
    pub skills: Vec<SkillItem>,
    pub agents: Vec<AgentItem>,
    pub hooks: Vec<HookItem>,
}

/// 真实项目路径 → Claude 项目目录列表的缓存
/// 同一 real_path 可能对应多个 Claude 项目目录（如编码规则变更后新旧目录共存）
static PROJECT_PATH_MAPPING: Mutex<Option<HashMap<String, Vec<PathBuf>>>> = Mutex::new(None);

// ==================== 辅助函数 ====================

/// 获取 Claude 配置目录
fn get_claude_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".claude"))
        .context("Home directory not found")
}

/// 获取 GUI 配置目录
fn get_gui_config_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".cc-box"))
        .context("Home directory not found")
}

/// 获取 GUI 配置文件路径
fn get_gui_config_path() -> Result<PathBuf> {
    get_gui_config_dir().map(|d| d.join("config.json"))
}

// ==================== Claude 环境变量注入 ====================

/// 将 cc-box 管理的环境变量合并写入 ~/.claude/settings.json
/// user_env: 要写入的键值对
/// removed_keys: 需要从 Claude settings env 中删除的 key（用户改名/删除操作产生）
pub fn sync_claude_env(
    user_env: std::collections::HashMap<String, String>,
    removed_keys: Vec<String>,
) -> Result<()> {
    let home = dirs::home_dir().context("Home directory not found")?;
    let settings_path = home.join(".claude").join("settings.json");

    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)?;
        }
        serde_json::json!({})
    };

    if settings.is_null() || !settings.is_object() {
        settings = serde_json::json!({});
    }
    let settings_obj = settings
        .as_object_mut()
        .context("settings.json is not an object")?;

    if !settings_obj.contains_key("env") {
        settings_obj.insert("env".to_string(), serde_json::json!({}));
    }
    let env_obj = settings_obj
        .get_mut("env")
        .and_then(|v| v.as_object_mut())
        .context("settings.json env is not an object")?;

    // 删除不再管理的 key
    for key in &removed_keys {
        env_obj.remove(key);
    }

    // 写入/更新 key
    for (key, value) in &user_env {
        env_obj.insert(key.clone(), serde_json::Value::String(value.clone()));
    }

    let content = serde_json::to_string_pretty(&settings)?;
    fs::write(&settings_path, content)?;

    Ok(())
}

/// 扫描 ~/.claude/projects/ 构建真实路径到项目目录的映射
/// 每个目录通过读取 JSONL 中的 cwd 字段获取真实项目路径
fn build_project_path_mapping() -> HashMap<String, Vec<PathBuf>> {
    let claude_dir = match get_claude_dir() {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        return HashMap::new();
    }

    let mut mapping: HashMap<String, Vec<PathBuf>> = HashMap::new();

    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if let Some(real_path) = extract_project_path_from_jsonl(&path) {
                mapping.entry(real_path).or_default().push(path);
            }
        }
    }

    mapping
}

/// 根据真实项目路径查找对应的 Claude 项目目录列表
/// 使用缓存避免重复扫描
fn get_project_dirs(project_path: &str) -> Vec<PathBuf> {
    let mut cache = PROJECT_PATH_MAPPING
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    if cache.is_none() {
        *cache = Some(build_project_path_mapping());
    }

    cache
        .as_ref()
        .and_then(|m| m.get(project_path))
        .cloned()
        .unwrap_or_default()
}

/// 清除项目路径映射缓存（供外部调用以强制刷新）
pub fn invalidate_project_path_mapping() {
    let mut cache = PROJECT_PATH_MAPPING
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *cache = None;
}

/// 从 JSONL 文件提取真实项目路径
fn extract_project_path_from_jsonl(project_dir: &Path) -> Option<String> {
    if !project_dir.exists() {
        return None;
    }

    // 从 JSONL 内容提取 cwd
    for entry in fs::read_dir(project_dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();

        // 只读取非 agent 开头的 jsonl 文件
        if path.extension().map(|e| e == "jsonl").unwrap_or(false)
            && !path
                .file_name()
                .map(|n| n.to_str().unwrap_or("").starts_with("agent-"))
                .unwrap_or(false)
        {
            if let Ok(content) = fs::read_to_string(&path) {
                // 读取所有行直到找到 cwd（通常在前几行，但不确定具体位置）
                for line in content.lines() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(cwd) = json.get("cwd").and_then(|v| v.as_str()) {
                            return Some(cwd.to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

/// 获取项目目录最后修改时间
fn get_project_last_modified(project_dir: &Path) -> u64 {
    if !project_dir.exists() {
        return 0;
    }

    let mut max_time = 0u64;

    if let Ok(entries) = fs::read_dir(project_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "jsonl").unwrap_or(false)
                && !path
                    .file_name()
                    .map(|n| n.to_str().unwrap_or("").starts_with("agent-"))
                    .unwrap_or(false)
            {
                if let Ok(meta) = fs::metadata(&path) {
                    if let Ok(modified) = meta.modified() {
                        let millis = modified
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        max_time = max_time.max(millis);
                    }
                }
            }
        }
    }

    if max_time == 0 {
        if let Ok(meta) = fs::metadata(project_dir) {
            if let Ok(modified) = meta.modified() {
                max_time = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
            }
        }
    }

    max_time
}

// ==================== 公开函数 ====================

/// 首页数据（一次遍历同时返回项目和近期会话）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeData {
    pub projects: Vec<Project>,
    pub recent_sessions: Vec<SessionInfo>,
    pub has_more: bool,
}

/// 一次遍历获取首页所需全部数据，避免重复 IO
pub fn get_home_data(project_limit: usize, session_limit: usize) -> Result<HomeData> {
    let claude_dir = get_claude_dir()?;
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        return Ok(HomeData {
            projects: Vec::new(),
            recent_sessions: Vec::new(),
            has_more: false,
        });
    }

    let mut projects = Vec::new();
    let mut all_sessions = Vec::new();

    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let real_path = match extract_project_path_from_jsonl(&path) {
            Some(p) => p,
            None => continue,
        };

        // 跳过原始路径已不存在的项目
        if !Path::new(&real_path).exists() {
            continue;
        }

        let last_modified = get_project_last_modified(&path);

        projects.push(Project {
            path: real_path.clone(),
            name: Path::new(&real_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&real_path)
                .to_string(),
            last_session_id: None,
            last_cost: None,
            last_duration: Some(last_modified),
        });

        // 同一次遍历中收集每个项目的前 3 条会话
        if let Ok(sessions) = get_sessions(&real_path, 3, 0) {
            all_sessions.extend(sessions);
        }
    }

    // 按最后修改时间排序
    projects.sort_by(|a, b| {
        b.last_duration
            .unwrap_or(0)
            .cmp(&a.last_duration.unwrap_or(0))
    });

    let has_more = projects.len() > project_limit;
    let paginated_projects: Vec<Project> = projects.into_iter().take(project_limit).collect();

    all_sessions.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
    all_sessions.truncate(session_limit);

    Ok(HomeData {
        projects: paginated_projects,
        recent_sessions: all_sessions,
        has_more,
    })
}

/// 获取项目列表（支持分页）
pub fn get_projects(limit: Option<usize>, offset: Option<usize>) -> Result<Vec<Project>> {
    let claude_dir = get_claude_dir()?;
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();

    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let real_path = match extract_project_path_from_jsonl(&path) {
            Some(p) => p,
            None => {
                log::warn!("Could not extract path from JSONL for {:?}", path);
                continue;
            }
        };

        // 跳过原始路径已不存在的项目
        if !Path::new(&real_path).exists() {
            continue;
        }

        let last_modified = get_project_last_modified(&path);

        let project = Project {
            path: real_path.clone(),
            name: Path::new(&real_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&real_path)
                .to_string(),
            last_session_id: None,
            last_cost: None,
            last_duration: Some(last_modified),
        };

        projects.push(project);
    }

    // 按最后修改时间排序
    projects.sort_by(|a, b| {
        b.last_duration
            .unwrap_or(0)
            .cmp(&a.last_duration.unwrap_or(0))
    });

    // 分页
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(projects.len());
    let start = offset.min(projects.len());
    let end = (offset + limit).min(projects.len());

    Ok(projects[start..end].to_vec())
}

/// 获取项目信息
pub fn get_project_info(path: &str) -> Result<Option<Project>> {
    let projects = get_projects(None, None)?;
    Ok(projects.iter().find(|p| p.path == path).cloned())
}

/// 从 JSONL 提取会话名称
pub(crate) fn extract_session_name(jsonl_path: &Path) -> String {
    if let Ok(content) = fs::read_to_string(jsonl_path) {
        let mut custom_title: Option<String> = None;
        let mut last_user_message: Option<String> = None;

        for line in content.lines() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                // 查找 custom-title（优先级最高）
                if json.get("type").and_then(|v| v.as_str()) == Some("custom-title") {
                    if let Some(title) = json.get("customTitle").and_then(|v| v.as_str()) {
                        custom_title = Some(title.to_string());
                    }
                }

                // 查找用户消息
                if json.get("type").and_then(|v| v.as_str()) == Some("user") {
                    if let Some(msg_content) = json
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        let is_meta = json
                            .get("isMeta")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        // 过滤所有系统注入消息：以 < 开头的都是系统标记
                        // 如 <task-notification>, <local-command-stdout>, <system-reminder>, <command-name> 等
                        let is_system_inject = msg_content.trim_start().starts_with('<');

                        if !is_meta && !is_system_inject {
                            let truncated: String = msg_content.chars().take(50).collect();
                            last_user_message = if msg_content.chars().count() > 50 {
                                Some(format!("{}...", truncated))
                            } else {
                                Some(msg_content.to_string())
                            };
                        }
                    }
                }
            }
        }

        if let Some(title) = custom_title {
            return title;
        }

        if let Some(msg) = last_user_message {
            return msg;
        }
    }

    "Unnamed session".to_string()
}

/// 轻量会话条目（仅文件元数据，不读内容）
struct SessionEntry {
    session_id: String,
    path: PathBuf,
    last_active_at: u64,
}

/// 获取会话列表
pub fn get_sessions(project_path: &str, limit: usize, offset: usize) -> Result<Vec<SessionInfo>> {
    let project_dirs = get_project_dirs(project_path);

    if project_dirs.is_empty() {
        return Ok(Vec::new());
    }

    // 第一遍：只扫文件名和元数据（不读 JSONL 内容）
    let mut entries: Vec<SessionEntry> = Vec::new();

    for project_dir in &project_dirs {
        if !project_dir.exists() {
            continue;
        }

        for entry in fs::read_dir(project_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "jsonl").unwrap_or(false)
                && !path
                    .file_name()
                    .map(|n| n.to_str().unwrap_or("").starts_with("agent-"))
                    .unwrap_or(false)
            {
                let session_id = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                let last_active_at = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);

                entries.push(SessionEntry {
                    session_id,
                    path,
                    last_active_at,
                });
            }
        }
    }

    // 按最后活跃时间排序
    entries.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));

    // 分页
    let start = offset.min(entries.len());
    let end = (offset + limit).min(entries.len());

    // 第二遍：只读分页后的少量文件提取名称
    let sessions: Vec<SessionInfo> = entries[start..end]
        .iter()
        .map(|e| SessionInfo {
            session_id: e.session_id.clone(),
            name: extract_session_name(&e.path),
            project_path: project_path.to_string(),
            last_active_at: e.last_active_at,
        })
        .collect();

    Ok(sessions)
}

/// 获取所有项目的近期会话（跨项目，按 lastActiveAt 降序排列）
pub fn get_all_recent_sessions(limit: usize) -> Result<Vec<SessionInfo>> {
    let projects = get_projects(None, None)?;
    let mut all_sessions = Vec::new();

    // 每个项目只取前 3 条，减少 IO 开销
    let per_project = 3.min(limit);

    for project in &projects {
        if let Ok(sessions) = get_sessions(&project.path, per_project, 0) {
            all_sessions.extend(sessions);
        }
    }

    // 按 lastActiveAt 降序排列
    all_sessions.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));

    // 截断到 limit 条
    all_sessions.truncate(limit);

    Ok(all_sessions)
}

/// 获取会话总数
pub fn get_session_count(project_path: &str) -> Result<usize> {
    let project_dirs = get_project_dirs(project_path);

    if project_dirs.is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    for project_dir in &project_dirs {
        if !project_dir.exists() {
            continue;
        }

        count += fs::read_dir(project_dir)?
            .filter(|e| {
                e.as_ref()
                    .ok()
                    .map(|entry| {
                        let path = entry.path();
                        path.extension().map(|e| e == "jsonl").unwrap_or(false)
                            && !path
                                .file_name()
                                .map(|n| n.to_str().unwrap_or("").starts_with("agent-"))
                                .unwrap_or(false)
                    })
                    .unwrap_or(false)
            })
            .count();
    }

    Ok(count)
}

/// 获取会话详情
pub fn get_session_details(project_path: &str, session_id: &str) -> Result<Option<SessionDetails>> {
    let project_dirs = get_project_dirs(project_path);

    let session_file = project_dirs
        .iter()
        .map(|dir| dir.join(format!("{}.jsonl", session_id)))
        .find(|f| f.exists());

    let session_file = match session_file {
        Some(f) => f,
        None => return Ok(None),
    };

    let content = fs::read_to_string(&session_file)?;
    let mut message_count = 0;
    let mut total_tokens = 0u64;
    let mut total_cost = 0.0;
    let mut created_at: Option<u64> = None;
    let mut name = "Unnamed session".to_string();

    for line in content.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            // 创建时间
            if created_at.is_none() {
                if let Some(ts) = json.get("timestamp").and_then(|v| v.as_str()) {
                    created_at = Some(parse_timestamp(ts));
                }
            }

            // 消息计数
            let msg_type = json.get("type").and_then(|v| v.as_str());
            if msg_type == Some("user") || msg_type == Some("assistant") {
                message_count += 1;
            }

            // Token 统计
            if let Some(usage) = json.get("usage") {
                if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                    total_tokens += input;
                }
                if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                    total_tokens += output;
                }
            }

            // 成本
            if let Some(cost) = json.get("costUSD").and_then(|v| v.as_f64()) {
                total_cost += cost;
            }

            // 名称
            if msg_type == Some("custom-title") {
                if let Some(title) = json.get("customTitle").and_then(|v| v.as_str()) {
                    name = title.to_string();
                }
            }
        }
    }

    if name == "Unnamed session" {
        name = extract_session_name(&session_file);
    }

    let last_active_at = fs::metadata(&session_file)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        })
        .unwrap_or(0);

    Ok(Some(SessionDetails {
        session_id: session_id.to_string(),
        name,
        message_count,
        total_tokens: if total_tokens > 0 {
            Some(total_tokens)
        } else {
            None
        },
        total_cost: if total_cost > 0.0 {
            Some(total_cost)
        } else {
            None
        },
        created_at,
        last_active_at,
    }))
}

/// 会话搜索结果（含消息片段）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSearchResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub name: String,
    #[serde(rename = "projectPath")]
    pub project_path: String,
    #[serde(rename = "lastActiveAt")]
    pub last_active_at: u64,
    pub snippet: String,
}

/// 搜索会话消息内容
pub fn search_session_messages(
    project_path: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<SessionSearchResult>> {
    let project_dirs = get_project_dirs(project_path);
    if project_dirs.is_empty() {
        return Ok(Vec::new());
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for project_dir in &project_dirs {
        if !project_dir.exists() {
            continue;
        }

        for entry in fs::read_dir(project_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "jsonl").unwrap_or(false)
                && !path
                    .file_name()
                    .map(|n| n.to_str().unwrap_or("").starts_with("agent-"))
                    .unwrap_or(false)
            {
                let session_id = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                if let Ok(content) = fs::read_to_string(&path) {
                    let name = extract_session_name(&path);
                    let lines: Vec<&str> = content.lines().collect();
                    let mut matched_snippet: Option<String> = None;

                    // 从末尾开始读取（最新消息优先）
                    for line in lines.iter().rev().take(200) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                            let msg_type = json.get("type").and_then(|v| v.as_str());

                            if msg_type == Some("user") || msg_type == Some("assistant") {
                                if let Some(msg_content) = json
                                    .get("message")
                                    .and_then(|m| m.get("content"))
                                    .and_then(|c| c.as_str())
                                {
                                    if msg_content.to_lowercase().contains(&query_lower) {
                                        let chars: Vec<char> = msg_content.chars().collect();
                                        let lower_content: String =
                                            chars.iter().collect::<String>().to_lowercase();
                                        let match_pos =
                                            lower_content.find(&query_lower).unwrap_or(0);
                                        let char_match_pos =
                                            lower_content[..match_pos].chars().count();
                                        let start = char_match_pos.saturating_sub(30);
                                        let end = (char_match_pos + query.chars().count() + 70)
                                            .min(chars.len());
                                        let snippet_raw: String =
                                            chars[start..end].iter().collect();
                                        matched_snippet = Some(if start > 0 || end < chars.len() {
                                            format!("...{}...", snippet_raw)
                                        } else {
                                            snippet_raw
                                        });
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if let Some(snippet) = matched_snippet {
                        let last_active_at = fs::metadata(&path)
                            .and_then(|m| m.modified())
                            .map(|t| {
                                t.duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0)
                            })
                            .unwrap_or(0);

                        results.push(SessionSearchResult {
                            session_id,
                            name,
                            project_path: project_path.to_string(),
                            last_active_at,
                            snippet,
                        });
                    }
                }
            }
        }
    }

    results.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
    results.truncate(limit);

    Ok(results)
}

/// 解析 ISO 时间戳
pub(crate) fn parse_timestamp(ts: &str) -> u64 {
    // 简单解析 ISO 格式时间戳
    // 2024-01-01T00:00:00Z -> milliseconds
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        dt.timestamp_millis() as u64
    } else {
        0
    }
}

/// 获取应用配置
pub fn get_app_config() -> Result<AppConfig> {
    let config_path = get_gui_config_path()?;

    if !config_path.exists() {
        return Ok(AppConfig {
            default_continue: Some(true),
            default_skip_permissions: Some(false),
            default_custom_args: Some("".to_string()),
            theme: Some("light".to_string()),
            font_size: Some(12),
            hidden_projects: Some(Vec::new()),
            ..Default::default()
        });
    }

    let content = fs::read_to_string(&config_path)?;
    let config: AppConfig =
        serde_json::from_str(&content).context("Failed to parse config.json")?;

    Ok(config)
}

/// 更新应用配置
pub fn update_app_config(updates: serde_json::Value) -> Result<()> {
    let config_path = get_gui_config_path()?;
    let config_dir = config_path
        .parent()
        .context("Could not get parent directory of config path")?;

    if !config_dir.exists() {
        fs::create_dir_all(config_dir)?;
    }

    let existing = get_app_config()?;
    let existing_json = serde_json::to_value(existing)?;

    let merged = merge_json_values(existing_json, updates);

    let content = serde_json::to_string_pretty(&merged)?;
    fs::write(&config_path, content)?;

    Ok(())
}

/// 合并 JSON 值
pub(crate) fn merge_json_values(base: serde_json::Value, updates: serde_json::Value) -> serde_json::Value {
    match (base, updates) {
        (serde_json::Value::Object(mut base_map), serde_json::Value::Object(updates_map)) => {
            for (key, value) in updates_map {
                if value.is_null() {
                    base_map.remove(&key);
                } else {
                    base_map.insert(key, value);
                }
            }
            serde_json::Value::Object(base_map)
        }
        (_, updates) => updates,
    }
}

/// 获取默认 Claude 选项
pub fn get_default_claude_options() -> Result<DefaultClaudeOptions> {
    let config = get_app_config()?;
    Ok(DefaultClaudeOptions {
        continue_opt: config.default_continue,
        resume: None,
        skip_permissions: config.default_skip_permissions,
        custom_args: config.default_custom_args,
    })
}

/// 保存默认 Claude 选项
pub fn save_default_claude_options(options: DefaultClaudeOptions) -> Result<()> {
    let updates = serde_json::json!({
        "defaultContinue": options.continue_opt,
        "defaultSkipPermissions": options.skip_permissions,
        "defaultCustomArgs": options.custom_args,
    });
    update_app_config(updates)?;
    Ok(())
}

/// 保存最近打开项目
pub fn save_last_project(path: &str) -> Result<()> {
    let updates = serde_json::json!({
        "lastOpenedProject": path,
    });
    update_app_config(updates)?;
    Ok(())
}

/// 获取项目配置
pub fn get_project_config(project_path: &str) -> Result<ProjectConfig> {
    let mut config = ProjectConfig::default();

    // 读取基本配置
    config.basic = read_basic_config(project_path)?;

    // 读取 MCP 配置
    config.mcp = read_mcp_config(project_path)?;

    // 读取 Skills 配置
    config.skills = read_skills_config(project_path)?;

    // 读取 Agents 配置
    config.agents = read_agents_config(project_path)?;

    // 读取 Hooks 配置
    config.hooks = read_hooks_config(project_path)?;

    Ok(config)
}

// ==================== 配置读取辅助函数 ====================

fn read_basic_config(project_path: &str) -> Result<Vec<BasicConfigItem>> {
    let mut result = Vec::new();
    let home = dirs::home_dir().context("Home directory not found")?;

    // 用户级 ~/.claude/settings.json
    let user_settings = home.join(".claude").join("settings.json");
    if user_settings.exists() {
        if let Ok(content) = fs::read_to_string(&user_settings) {
            if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
                result.push(BasicConfigItem {
                    model: settings
                        .get("model")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    theme: settings
                        .get("theme")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    editor_mode: settings
                        .get("editorMode")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    auto_connect_ide: settings.get("autoConnectIde").and_then(|v| v.as_bool()),
                    permissions: parse_permissions(&settings),
                    env: settings.get("env").cloned(),
                    source: ConfigSource {
                        source_type: "user".to_string(),
                        label: "User".to_string(),
                        path: Some(user_settings.to_string_lossy().to_string()),
                    },
                });
            }
        }
    }

    // 项目级 .claude/settings.json
    let project_settings = PathBuf::from(project_path)
        .join(".claude")
        .join("settings.json");
    if project_settings.exists() {
        if let Ok(content) = fs::read_to_string(&project_settings) {
            if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
                result.push(BasicConfigItem {
                    model: settings
                        .get("model")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    theme: settings
                        .get("theme")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    editor_mode: settings
                        .get("editorMode")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    auto_connect_ide: settings.get("autoConnectIde").and_then(|v| v.as_bool()),
                    permissions: parse_permissions(&settings),
                    env: settings.get("env").cloned(),
                    source: ConfigSource {
                        source_type: "project".to_string(),
                        label: "Project".to_string(),
                        path: Some(project_settings.to_string_lossy().to_string()),
                    },
                });
            }
        }
    }

    // 本地级 .claude/settings.local.json
    let local_settings = PathBuf::from(project_path)
        .join(".claude")
        .join("settings.local.json");
    if local_settings.exists() {
        if let Ok(content) = fs::read_to_string(&local_settings) {
            if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
                result.push(BasicConfigItem {
                    model: None,
                    theme: None,
                    editor_mode: None,
                    auto_connect_ide: None,
                    permissions: parse_permissions(&settings),
                    env: settings.get("env").cloned(),
                    source: ConfigSource {
                        source_type: "local".to_string(),
                        label: "Local".to_string(),
                        path: Some(local_settings.to_string_lossy().to_string()),
                    },
                });
            }
        }
    }

    Ok(result)
}

fn parse_permissions(settings: &serde_json::Value) -> Option<PermissionsConfig> {
    settings.get("permissions").map(|p| PermissionsConfig {
        allow: p.get("allow").and_then(|v| v.as_array()).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        }),
        deny: p.get("deny").and_then(|v| v.as_array()).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        }),
    })
}

fn read_mcp_config(project_path: &str) -> Result<Vec<McpServerItem>> {
    let mut result = Vec::new();

    // 项目级 .mcp.json
    let project_mcp = PathBuf::from(project_path).join(".mcp.json");
    if project_mcp.exists() {
        if let Ok(content) = fs::read_to_string(&project_mcp) {
            if let Ok(mcp) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(servers) = mcp.get("mcpServers").and_then(|v| v.as_object()) {
                    for (name, config) in servers {
                        result.push(parse_mcp_server(name, config, "project", &project_mcp));
                    }
                }
            }
        }
    }

    // 用户级 ~/.claude.json 的 mcpServers
    let home = dirs::home_dir().context("Home directory not found")?;
    let user_config_path = home.join(".claude.json");
    if user_config_path.exists() {
        if let Ok(content) = fs::read_to_string(&user_config_path) {
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(servers) = config.get("mcpServers").and_then(|v| v.as_object()) {
                    for (name, server_config) in servers {
                        result.push(parse_mcp_server(
                            name,
                            server_config,
                            "user",
                            &user_config_path,
                        ));
                    }
                }
            }
        }
    }

    Ok(result)
}

fn parse_mcp_server(
    name: &str,
    config: &serde_json::Value,
    source_type: &str,
    path: &Path,
) -> McpServerItem {
    McpServerItem {
        name: name.to_string(),
        server_type: config
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        command: config
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        args: config.get("args").and_then(|v| v.as_array()).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        }),
        env: config.get("env").cloned(),
        url: config
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        source: ConfigSource {
            source_type: source_type.to_string(),
            label: source_type.to_string(),
            path: Some(path.to_string_lossy().to_string()),
        },
    }
}

fn read_skills_config(project_path: &str) -> Result<Vec<SkillItem>> {
    let mut result = Vec::new();

    // 项目级
    let project_skills_dir = PathBuf::from(project_path).join(".claude").join("skills");
    if project_skills_dir.exists() {
        for entry in fs::read_dir(&project_skills_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                let skill_file = entry.path().join("SKILL.md");
                if skill_file.exists() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let description = extract_md_description(&skill_file);
                    result.push(SkillItem {
                        name,
                        description,
                        path: skill_file.to_string_lossy().to_string(),
                        source: ConfigSource {
                            source_type: "project".to_string(),
                            label: "Project".to_string(),
                            path: Some(skill_file.to_string_lossy().to_string()),
                        },
                    });
                }
            }
        }
    }

    // 用户级 ~/.claude/skills/<name>/SKILL.md
    let home = dirs::home_dir().context("Home directory not found")?;
    let user_skills_dir = home.join(".claude").join("skills");
    if user_skills_dir.exists() {
        for entry in fs::read_dir(&user_skills_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                let skill_file = entry.path().join("SKILL.md");
                if skill_file.exists() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let description = extract_md_description(&skill_file);
                    result.push(SkillItem {
                        name,
                        description,
                        path: skill_file.to_string_lossy().to_string(),
                        source: ConfigSource {
                            source_type: "user".to_string(),
                            label: "User".to_string(),
                            path: Some(skill_file.to_string_lossy().to_string()),
                        },
                    });
                }
            }
        }
    }

    Ok(result)
}

fn read_agents_config(project_path: &str) -> Result<Vec<AgentItem>> {
    let mut result = Vec::new();

    // 项目级
    let project_agents_dir = PathBuf::from(project_path).join(".claude").join("agents");
    if project_agents_dir.exists() {
        for entry in fs::read_dir(&project_agents_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let name = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let description = extract_md_description(&path);
                result.push(AgentItem {
                    name,
                    description,
                    path: path.to_string_lossy().to_string(),
                    source: ConfigSource {
                        source_type: "project".to_string(),
                        label: "Project".to_string(),
                        path: Some(path.to_string_lossy().to_string()),
                    },
                });
            }
        }
    }

    // 用户级 ~/.claude/agents/*.md
    let home = dirs::home_dir().context("Home directory not found")?;
    let user_agents_dir = home.join(".claude").join("agents");
    if user_agents_dir.exists() {
        for entry in fs::read_dir(&user_agents_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let name = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let description = extract_md_description(&path);
                result.push(AgentItem {
                    name,
                    description,
                    path: path.to_string_lossy().to_string(),
                    source: ConfigSource {
                        source_type: "user".to_string(),
                        label: "User".to_string(),
                        path: Some(path.to_string_lossy().to_string()),
                    },
                });
            }
        }
    }

    Ok(result)
}

fn read_hooks_config(project_path: &str) -> Result<Vec<HookItem>> {
    let mut result = Vec::new();

    let home = dirs::home_dir().context("Home directory not found")?;

    let settings_paths: Vec<(PathBuf, &str)> = vec![
        (home.join(".claude").join("settings.json"), "user"),
        (
            PathBuf::from(project_path)
                .join(".claude")
                .join("settings.json"),
            "project",
        ),
        (
            PathBuf::from(project_path)
                .join(".claude")
                .join("settings.local.json"),
            "local",
        ),
    ];

    for (path, source_type) in settings_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(hooks) = settings.get("hooks").and_then(|v| v.as_object()) {
                        for (event, hook_list) in hooks {
                            if let Some(list) = hook_list.as_array() {
                                for hook in list {
                                    result.push(HookItem {
                                        event: event.to_string(),
                                        matcher: hook
                                            .get("matcher")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                        command: hook
                                            .get("command")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string())
                                            .or_else(|| {
                                                hook.get("hooks")
                                                    .and_then(|h| h.as_array())
                                                    .and_then(|a| a.first())
                                                    .and_then(|h| h.get("command"))
                                                    .and_then(|v| v.as_str())
                                                    .map(|s| s.to_string())
                                            }),
                                        hook_type: hook
                                            .get("type")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                        source: ConfigSource {
                                            source_type: source_type.to_string(),
                                            label: source_type.to_string(),
                                            path: Some(path.to_string_lossy().to_string()),
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

/// 从 Markdown 提取描述
/// 优先从 YAML frontmatter 的 description 字段读取，其次从正文提取第一行非空非标题内容
pub(crate) fn extract_md_description(path: &Path) -> Option<String> {
    if let Ok(content) = fs::read_to_string(path) {
        let lines = content.lines().collect::<Vec<_>>();

        // 检查是否有 YAML frontmatter
        if lines.first().map(|l| l.trim() == "---") == Some(true) {
            // 在 frontmatter 中查找 description 字段
            for line in lines.iter().skip(1) {
                if line.trim() == "---" {
                    break;
                }
                if let Some(desc) = line.strip_prefix("description:") {
                    let desc = desc.trim();
                    if !desc.is_empty() {
                        let desc_chars: String = desc.chars().take(200).collect();
                        return Some(if desc.chars().count() > 200 {
                            format!("{}...", desc_chars)
                        } else {
                            desc.to_string()
                        });
                    }
                }
            }
        }

        // 后备：跳过 frontmatter，从正文提取第一行非空非标题
        let mut start = 0;
        if lines.first().map(|l| l.trim() == "---") == Some(true) {
            for (i, line) in lines.iter().skip(1).enumerate() {
                if line.trim() == "---" {
                    start = i + 2;
                    break;
                }
            }
        }

        for line in lines.iter().skip(start) {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("---") {
                let desc_chars: String = trimmed.chars().take(100).collect();
                return Some(if trimmed.chars().count() > 100 {
                    format!("{}...", desc_chars)
                } else {
                    trimmed.to_string()
                });
            }
        }
    }

    Some("No description".to_string())
}

// ==================== Agent 和 MCP 动态获取 ====================

/// Skill 信息（用于面板显示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    /// Skill 名称（如 "deploy" 或 "paper-tool:paper-search"）
    pub name: String,
    /// 显示名称（去除前缀）
    #[serde(rename = "displayName")]
    pub display_name: String,
    /// 描述
    pub description: Option<String>,
    /// 来源类型：project、user、plugin
    #[serde(rename = "sourceType")]
    pub source_type: String,
    /// 来源标签
    #[serde(rename = "sourceLabel")]
    pub source_label: String,
    /// 调用格式（如 "/deploy" 或 "/paper-tool:paper-search"）
    #[serde(rename = "invokeFormat")]
    pub invoke_format: String,
}

/// Plugin 内部 Skill 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSkill {
    /// Skill 名称（不含 plugin 前缀）
    pub name: String,
    /// 描述
    pub description: Option<String>,
    /// 调用格式
    #[serde(rename = "invokeFormat")]
    pub invoke_format: String,
}

/// Plugin 内部 Agent 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAgent {
    /// Agent 名称
    pub name: String,
    /// 描述
    pub description: Option<String>,
    /// 调用格式
    #[serde(rename = "invokeFormat")]
    pub invoke_format: String,
}

/// Agent 信息（用于面板显示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Agent 名称（如 "code-reviewer" 或 "paper-tool:paper-search"）
    pub name: String,
    /// 显示名称（去除前缀）
    #[serde(rename = "displayName")]
    pub display_name: String,
    /// 描述
    pub description: Option<String>,
    /// 来源类型：builtin、plugin、user、project
    #[serde(rename = "sourceType")]
    pub source_type: String,
    /// 来源标签
    #[serde(rename = "sourceLabel")]
    pub source_label: String,
    /// 模型（如 haiku、sonnet、inherit）
    pub model: Option<String>,
    /// 调用格式（如 "@agent-code-reviewer" 或 "@agent-paper-tool:paper-search"）
    #[serde(rename = "invokeFormat")]
    pub invoke_format: String,
}

/// MCP Server 信息（用于面板显示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    /// Server 名称
    pub name: String,
    /// 显示名称（去除 plugin: 前缀）
    #[serde(rename = "displayName")]
    pub display_name: String,
    /// 描述
    pub description: Option<String>,
    /// 来源类型：builtin、plugin、user、project
    #[serde(rename = "sourceType")]
    pub source_type: String,
    /// 来源标签
    #[serde(rename = "sourceLabel")]
    pub source_label: String,
    /// 类型：stdio、http、sse
    #[serde(rename = "serverType")]
    pub server_type: Option<String>,
    /// 连接状态
    pub status: Option<String>,
    /// URL（HTTP/SSE server）
    pub url: Option<String>,
    /// 命令（stdio server）
    pub command: Option<String>,
    /// HTTP Headers（用于认证）
    pub headers: Option<HashMap<String, String>>,
    /// 可用的 prompts 列表
    pub prompts: Vec<McpPromptInfo>,
}

/// MCP Prompt 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptInfo {
    /// Prompt 名称
    pub name: String,
    /// 描述
    pub description: Option<String>,
    /// 调用格式（如 "/mcp__github__list_prs"）
    #[serde(rename = "invokeFormat")]
    pub invoke_format: String,
}

/// 获取所有 Agents（包括 built-in、plugin、user、project）
pub fn get_all_agents(project_path: &str) -> Result<Vec<AgentInfo>> {
    let mut agents = Vec::new();

    // 1. 通过 claude agents list 获取 built-in 和 plugin agents
    if let Ok(output) = run_claude_command("agents list") {
        parse_agents_list_output(&output, &mut agents);
    }

    // 2. 从 plugins 获取 plugin agents 的描述
    let plugins = get_all_plugins(project_path)?;
    for agent in &mut agents {
        if agent.source_type == "plugin" {
            // agent.name 格式为 "plugin-name:agent-name"
            // 需要分别匹配 plugin 名称和 agent 名称
            let name_parts: Vec<&str> = agent.name.split(':').collect();
            if name_parts.len() >= 2 {
                let plugin_name = name_parts[0];
                let agent_short_name = name_parts[1];

                // 查找对应的 plugin
                for plugin in &plugins {
                    // plugin.name 是去除 @publisher 后的名称
                    if plugin.name == plugin_name {
                        if let Some(plugin_agents) = &plugin.agents {
                            for plugin_agent in plugin_agents {
                                if plugin_agent.name == agent_short_name {
                                    agent.description = plugin_agent.description.clone();
                                    break;
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    // 3. 添加 user 和 project agents（从文件系统读取）
    let user_project_agents = read_agents_config(project_path)?;
    for agent in user_project_agents {
        let invoke_format = format!("@\"{} (agent)\"", agent.name);
        agents.push(AgentInfo {
            name: agent.name.clone(),
            display_name: agent.name,
            description: agent.description,
            source_type: agent.source.source_type,
            source_label: agent.source.label,
            model: None,
            invoke_format,
        });
    }

    Ok(agents)
}

/// 获取所有 Skills（包括 project、user、plugin）
/// 注意：不包含 builtin skills（如 /simplify、/debug 等）
pub fn get_all_skills(project_path: &str) -> Result<Vec<SkillInfo>> {
    let mut skills = Vec::new();

    // 1. 从项目目录读取 skills
    let project_skills_dir = PathBuf::from(project_path).join(".claude").join("skills");
    if project_skills_dir.exists() {
        for entry in fs::read_dir(&project_skills_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                let skill_file = entry.path().join("SKILL.md");
                if skill_file.exists() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let description = extract_md_description(&skill_file);
                    skills.push(SkillInfo {
                        name: name.clone(),
                        display_name: name.clone(),
                        description,
                        source_type: "project".to_string(),
                        source_label: "Project".to_string(),
                        invoke_format: format!("/{}", name),
                    });
                }
            }
        }
    }

    // 2. 从用户目录读取 skills
    let home = dirs::home_dir().context("Home directory not found")?;
    let user_skills_dir = home.join(".claude").join("skills");
    if user_skills_dir.exists() {
        for entry in fs::read_dir(&user_skills_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                let skill_file = entry.path().join("SKILL.md");
                if skill_file.exists() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let description = extract_md_description(&skill_file);
                    skills.push(SkillInfo {
                        name: name.clone(),
                        display_name: name.clone(),
                        description,
                        source_type: "user".to_string(),
                        source_label: "User".to_string(),
                        invoke_format: format!("/{}", name),
                    });
                }
            }
        }
    }

    // 3. 从 plugins 获取 skills（使用 get_all_plugins）
    let plugins = get_all_plugins(project_path)?;
    for plugin in plugins {
        if let Some(plugin_skills) = plugin.skills {
            for skill in plugin_skills {
                // invoke_format 是 "/pluginName:skillName"，去掉 "/" 得到完整名称
                let full_name = skill
                    .invoke_format
                    .strip_prefix('/')
                    .unwrap_or(&skill.invoke_format);
                skills.push(SkillInfo {
                    name: full_name.to_string(),
                    display_name: skill.name.clone(),
                    description: skill.description,
                    source_type: "plugin".to_string(),
                    source_label: "Plugin".to_string(),
                    invoke_format: skill.invoke_format,
                });
            }
        }
    }

    Ok(skills)
}

/// 获取所有 MCP Servers（包括 plugin 和配置的）
pub fn get_all_mcp_servers(_project_path: &str) -> Result<Vec<McpServerInfo>> {
    let mut servers = Vec::new();

    // 1. 从 ~/.claude.json 读取 MCP 配置（包括 headers）
    let mcp_configs = read_mcp_configs_from_claude_json();

    // 2. 使用 claude mcp list 获取实时信息
    if let Ok(output) = run_claude_command("mcp list") {
        let parsed_servers = parse_mcp_list_output(&output);
        for parsed in parsed_servers {
            // 从配置中获取 headers
            let headers = mcp_configs
                .get(&parsed.name)
                .and_then(|c| c.headers.clone());

            servers.push(McpServerInfo {
                name: parsed.name.clone(),
                display_name: parsed.display_name.clone(),
                description: None,
                source_type: parsed.scope.clone(),
                source_label: capitalize_scope(&parsed.scope),
                server_type: Some(parsed.server_type.clone()),
                status: Some(parsed.status.clone()),
                url: parsed.url.clone(),
                command: parsed.command.clone(),
                headers,
                prompts: Vec::new(),
            });
        }
    }

    Ok(servers)
}

/// MCP 配置结构（从 ~/.claude.json 读取）
struct McpConfigEntry {
    headers: Option<HashMap<String, String>>,
}

/// 从 ~/.claude.json 读取 MCP 配置
fn read_mcp_configs_from_claude_json() -> HashMap<String, McpConfigEntry> {
    let mut configs = HashMap::new();

    let home = dirs::home_dir();
    if home.is_none() {
        return configs;
    }

    let config_path = home.unwrap().join(".claude.json");
    if !config_path.exists() {
        return configs;
    }

    if let Ok(content) = fs::read_to_string(&config_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            // 读取顶级 mcpServers
            if let Some(mcp_servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
                for (name, server_config) in mcp_servers {
                    let headers = server_config
                        .get("headers")
                        .and_then(|v| v.as_object())
                        .map(|obj| {
                            obj.iter()
                                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                                .collect()
                        });

                    configs.insert(name.clone(), McpConfigEntry { headers });
                }
            }
        }
    }

    configs
}

/// 解析 claude mcp list 输出
pub(crate) fn parse_mcp_list_output(output: &str) -> Vec<ParsedMcpServer> {
    let mut servers = Vec::new();

    for line in output.lines() {
        if line.contains(" - ") {
            let parts: Vec<&str> = line.splitn(2, " - ").collect();
            if parts.len() == 2 {
                let server_info = parts[0].trim();
                let status = parts[1].trim();

                if let Some(parsed) = parse_server_info_line(server_info, status) {
                    servers.push(parsed);
                }
            }
        }
    }

    servers
}

/// 解析后的 MCP Server（临时结构）
pub(crate) struct ParsedMcpServer {
    pub(crate) name: String,
    pub(crate) display_name: String,
    pub(crate) scope: String,
    pub(crate) server_type: String,
    pub(crate) status: String,
    pub(crate) url: Option<String>,
    pub(crate) command: Option<String>,
}

/// 解析单个服务器信息行
fn parse_server_info_line(info: &str, status: &str) -> Option<ParsedMcpServer> {
    // 格式示例：
    // plugin:paper-tool:paper-search: uv run --directory C:/claude-plugins/paper-tool/paper-search mcp_server.py
    // zread: https://open.bigmodel.cn/api/mcp/zread/mcp (HTTP)

    let type_marker_pos = info.find('(');
    let type_end_pos = info.find(')');

    let server_type = if type_marker_pos.is_some() && type_end_pos.is_some() {
        info[type_marker_pos.unwrap() + 1..type_end_pos.unwrap()]
            .trim()
            .to_string()
    } else {
        if info.contains("http://") || info.contains("https://") {
            "HTTP".to_string()
        } else {
            "stdio".to_string()
        }
    };

    let colon_pos = find_name_separator(info, &server_type);
    if colon_pos.is_none() {
        return None;
    }

    let colon_idx = colon_pos.unwrap();
    let name = info[..colon_idx].trim();

    let after_colon = &info[colon_idx + 1..];

    let command_or_url = if type_marker_pos.is_some() {
        let type_rel_pos = type_marker_pos.unwrap() - colon_idx - 1;
        if type_rel_pos > 0 && type_rel_pos < after_colon.len() {
            after_colon[..type_rel_pos].trim()
        } else {
            after_colon.trim()
        }
    } else {
        after_colon.trim()
    };

    let (url, command) =
        if command_or_url.starts_with("http://") || command_or_url.starts_with("https://") {
            (Some(command_or_url.to_string()), None)
        } else {
            (None, Some(command_or_url.to_string()))
        };

    let scope = if name.starts_with("plugin:") {
        "plugin"
    } else if name.starts_with("managed:") {
        "managed"
    } else {
        "user"
    };

    let display_name = if scope == "plugin" {
        // plugin:paper-tool:paper-search -> paper-search
        name.split(':').last().unwrap_or(name).to_string()
    } else {
        name.to_string()
    };

    Some(ParsedMcpServer {
        name: name.to_string(),
        display_name,
        scope: scope.to_string(),
        server_type,
        status: status.to_string(),
        url,
        command,
    })
}

/// 找到分割 name 和 command/url 的冒号位置
pub(crate) fn find_name_separator(info: &str, server_type: &str) -> Option<usize> {
    if server_type == "HTTP" || server_type == "SSE" {
        if let Some(http_pos) = info.find("https://").or_else(|| info.find("http://")) {
            let before_url = &info[..http_pos];
            before_url.rfind(':')
        } else {
            info.find(':')
        }
    } else {
        // stdio server: 找最后一个冒号（但要排除 Windows 路径中的冒号）
        let mut candidate: Option<usize> = None;
        let chars = info.chars().collect::<Vec<_>>();

        for i in (0..chars.len()).rev() {
            if chars[i] == ':' {
                let after_colon = if i + 1 < chars.len() {
                    &info[i + 1..]
                } else {
                    ""
                };

                // 排除 Windows 路径中的冒号（如 C:/, D:/）
                let is_path_colon = after_colon.starts_with('/') || after_colon.starts_with('\\');
                let is_url_colon = after_colon.starts_with("//");

                if is_path_colon || is_url_colon {
                    continue;
                }

                // 找到合适的分隔冒号
                if after_colon.starts_with(' ')
                    || after_colon.is_empty()
                    || after_colon.find(':').is_none()
                {
                    return Some(i);
                }

                candidate = Some(i);
            }
        }

        candidate.or_else(|| info.rfind(':'))
    }
}

/// 转换 scope 为显示标签
fn capitalize_scope(scope: &str) -> String {
    match scope {
        "plugin" => "Plugin".to_string(),
        "managed" => "Managed".to_string(),
        "user" => "User".to_string(),
        "project" => "Project".to_string(),
        _ => scope.to_string(),
    }
}

/// 执行 claude 命令并获取输出（设置 git-bash 环境）
fn run_claude_command(args: &str) -> Result<String> {
    // Windows 上需要设置 git-bash 环境
    let mut cmd = std::process::Command::new("claude");
    cmd.args(args.split_whitespace());

    // 设置 git-bash 环境
    if cfg!(target_os = "windows") {
        // 尝试从环境变量获取
        if let Ok(git_bash_path) = std::env::var("CLAUDE_CODE_GIT_BASH_PATH") {
            if Path::new(&git_bash_path).exists() {
                cmd.env("CLAUDE_CODE_GIT_BASH_PATH", git_bash_path);
            }
        } else {
            // 检测 git-bash 路径
            let git_bash_path = detect_git_bash_path();
            if let Some(path) = git_bash_path {
                cmd.env("CLAUDE_CODE_GIT_BASH_PATH", path);
            }
        }
    }

    // Windows 上禁止子进程创建控制台窗口
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().context("Failed to run claude command")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        // 即使失败也返回输出，可能包含有用信息
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// 检测 Git Bash 路径（Windows）
fn detect_git_bash_path() -> Option<String> {
    if !cfg!(target_os = "windows") {
        return None;
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        // where git → 同目录下找 bash.exe
        let output = std::process::Command::new("where")
            .arg("git")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let git_path = stdout.lines().next()?.trim();
        let parent = Path::new(git_path).parent()?;
        let bash_path = parent.join("bash.exe");

        if bash_path.exists() {
            Some(bash_path.to_string_lossy().to_string())
        } else {
            None
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// 解析 claude agents list 输出
pub(crate) fn parse_agents_list_output(output: &str, agents: &mut Vec<AgentInfo>) {
    // 输出格式示例：
    // Plugin agents:
    //   paper-tool:paper-search · inherit
    // Built-in agents:
    //   claude-code-guide · haiku
    //   Explore · haiku
    //   general-purpose · inherit
    //   Plan · inherit
    //   statusline-setup · sonnet

    let mut current_section = "";

    for line in output.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Plugin agents:") {
            current_section = "plugin";
            continue;
        }
        if trimmed.starts_with("Built-in agents:") {
            current_section = "builtin";
            continue;
        }
        if trimmed.starts_with("User agents:") {
            current_section = "user";
            continue;
        }
        if trimmed.starts_with("Project agents:") {
            current_section = "project";
            continue;
        }

        // 解析 agent 行：name · model
        if trimmed.starts_with("·") || trimmed.is_empty() || !trimmed.contains("·") {
            continue;
        }

        let parts: Vec<&str> = trimmed.splitn(2, '·').collect();
        if parts.len() == 2 {
            let name = parts[0].trim();
            let model = parts[1].trim();

            // 调用格式：@"name (agent)" 或 @"plugin:agent (agent)"
            let invoke_format = format!("@\"{} (agent)\"", name);

            // 显示名称：去除前缀
            let display_name = if name.contains(':') {
                name.split(':').last().unwrap_or(name)
            } else {
                name
            };

            let source_label = if current_section == "plugin" {
                "Plugin"
            } else if current_section == "builtin" {
                "Built-in"
            } else {
                current_section
            };

            agents.push(AgentInfo {
                name: name.to_string(),
                display_name: display_name.to_string(),
                description: None,
                source_type: current_section.to_string(),
                source_label: source_label.to_string(),
                model: Some(model.to_string()),
                invoke_format,
            });
        }
    }
}

// ==================== Plugin 信息获取 ====================

/// Plugin 信息（用于面板显示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Plugin ID（如 "paper-tool@orczh"）
    pub id: String,
    /// 显示名称（去除 @publisher 后缀）
    pub name: String,
    /// 版本
    pub version: String,
    /// 作用域：user、project
    pub scope: String,
    /// 是否启用
    pub enabled: bool,
    /// 安装路径（修正后的有效路径）
    #[serde(rename = "installPath")]
    pub install_path: String,
    /// 安装时间
    #[serde(rename = "installedAt")]
    pub installed_at: Option<String>,
    /// 最后更新时间
    #[serde(rename = "lastUpdated")]
    pub last_updated: Option<String>,
    /// 项目路径（仅 project scope）
    #[serde(rename = "projectPath")]
    pub project_path: Option<String>,
    /// Plugin 提供的 skills（详细列表）
    pub skills: Option<Vec<PluginSkill>>,
    /// Plugin 提供的 agents（详细列表）
    pub agents: Option<Vec<PluginAgent>>,
    /// Plugin 提供的 MCP servers
    #[serde(rename = "mcpServers")]
    pub mcp_servers: Option<serde_json::Value>,
}

/// CLI 返回的 Plugin JSON 结构
#[derive(Debug, Clone, Deserialize)]
struct CliPluginInfo {
    id: String,
    version: String,
    scope: String,
    enabled: bool,
    #[serde(rename = "installPath")]
    install_path: String,
    #[serde(rename = "installedAt")]
    installed_at: Option<String>,
    #[serde(rename = "lastUpdated")]
    last_updated: Option<String>,
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
    #[serde(rename = "mcpServers")]
    mcp_servers: Option<serde_json::Value>,
}

/// 获取所有 Plugins（使用 CLI 命令），包含内部 skills/agents
pub fn get_all_plugins(project_path: &str) -> Result<Vec<PluginInfo>> {
    // 使用 CLI 命令获取 plugin 信息
    let output = run_claude_command("plugins list --json")?;
    let cli_plugins: Vec<CliPluginInfo> =
        serde_json::from_str(&output).context("Failed to parse plugins list JSON")?;

    let mut plugins = Vec::new();

    for cli_plugin in cli_plugins {
        // 过滤当前项目相关的 plugins
        let is_relevant = if cli_plugin.scope == "user" {
            true
        } else if cli_plugin.scope == "project" {
            cli_plugin
                .project_path
                .as_ref()
                .map(|p| p == project_path)
                .unwrap_or(false)
        } else {
            false
        };

        if !is_relevant || !cli_plugin.enabled {
            continue;
        }

        // Plugin 名称（去除 @publisher）
        let plugin_name = cli_plugin.id.split('@').next().unwrap_or(&cli_plugin.id);

        // 验证/修正 install_path
        let valid_path = find_valid_plugin_path(&cli_plugin.install_path, &cli_plugin.id);

        if valid_path.is_none() {
            log::warn!(
                "Plugin {} install path not valid: {}",
                cli_plugin.id,
                cli_plugin.install_path
            );
            continue;
        }

        let install_path = valid_path.unwrap();

        // 解析内部 skills 和 agents
        let plugin_skills = parse_plugin_skills(&install_path, plugin_name);
        let plugin_agents = parse_plugin_agents(&install_path, plugin_name);

        // 读取 mcpServers（优先从 CLI 输出，后备从文件读取）
        let mcp_servers = cli_plugin
            .mcp_servers
            .clone()
            .or_else(|| read_plugin_mcp_servers(&install_path));

        plugins.push(PluginInfo {
            id: cli_plugin.id.clone(),
            name: plugin_name.to_string(),
            version: cli_plugin.version.clone(),
            scope: cli_plugin.scope.clone(),
            enabled: cli_plugin.enabled,
            install_path: install_path.clone(),
            installed_at: cli_plugin.installed_at.clone(),
            last_updated: cli_plugin.last_updated.clone(),
            project_path: cli_plugin.project_path.clone(),
            skills: plugin_skills,
            agents: plugin_agents,
            mcp_servers,
        });
    }

    Ok(plugins)
}

/// 查找有效的 plugin 安装路径
/// 如果 installPath 不存在，在父目录下找最新版本
/// 不检查 .orphaned_at，只看路径是否存在
fn find_valid_plugin_path(original_path: &str, _plugin_id: &str) -> Option<String> {
    // 先检查原始路径是否存在
    let original = PathBuf::from(original_path);
    if original.exists() {
        return Some(original_path.to_string());
    }

    // 原始路径不存在，尝试在父目录下找最新版本
    // 例如: ~/.claude/plugins/cache/orczh/paper-tool/ 下找最新版本目录
    let parent = original.parent()?;
    if !parent.exists() {
        return None;
    }

    // 遍历子目录找最新版本（不检查 orphaned）
    let mut versions: Vec<(String, std::time::SystemTime)> = Vec::new();

    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        versions.push((path.to_string_lossy().to_string(), modified));
                    }
                }
            }
        }
    }

    // 按修改时间排序，返回最新的
    versions.sort_by(|a, b| b.1.cmp(&a.1));

    versions.first().map(|(path, _)| path.clone())
}

/// 读取 plugin 的 MCP servers 配置（从 .mcp.json）
fn read_plugin_mcp_servers(install_path: &str) -> Option<serde_json::Value> {
    // 先尝试读取 .mcp.json（plugin 标准位置）
    let mcp_file = PathBuf::from(install_path).join(".mcp.json");
    if mcp_file.exists() {
        if let Ok(content) = fs::read_to_string(&mcp_file) {
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                // .mcp.json 直接是 servers 的映射，不需要 mcpServers 包装
                return Some(config);
            }
        }
    }

    // 后备：读取 .claude-plugin/plugin.json
    let plugin_config = PathBuf::from(install_path)
        .join(".claude-plugin")
        .join("plugin.json");
    if plugin_config.exists() {
        if let Ok(content) = fs::read_to_string(&plugin_config) {
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                return config.get("mcpServers").cloned();
            }
        }
    }
    None
}

/// 解析 plugin 目录中的 skills
fn parse_plugin_skills(install_path: &str, plugin_name: &str) -> Option<Vec<PluginSkill>> {
    let skills_dir = PathBuf::from(install_path).join("skills");
    if !skills_dir.exists() {
        return None;
    }

    let mut skills = Vec::new();
    if let Ok(entries) = fs::read_dir(&skills_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let skill_file = entry.path().join("SKILL.md");
                if skill_file.exists() {
                    let skill_name = entry.file_name().to_string_lossy().to_string();
                    let description = extract_md_description(&skill_file);
                    skills.push(PluginSkill {
                        name: skill_name.clone(),
                        description,
                        invoke_format: format!("/{}:{}", plugin_name, skill_name),
                    });
                }
            }
        }
    }

    if skills.is_empty() {
        None
    } else {
        Some(skills)
    }
}

/// 解析 plugin 目录中的 agents
fn parse_plugin_agents(install_path: &str, plugin_name: &str) -> Option<Vec<PluginAgent>> {
    let agents_dir = PathBuf::from(install_path).join("agents");
    if !agents_dir.exists() {
        return None;
    }

    let mut agents = Vec::new();
    if let Ok(entries) = fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let agent_name = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let description = extract_md_description(&path);
                agents.push(PluginAgent {
                    name: agent_name.clone(),
                    description,
                    invoke_format: format!("@\"{}:{} (agent)\"", plugin_name, agent_name),
                });
            }
        }
    }

    if agents.is_empty() {
        None
    } else {
        Some(agents)
    }
}

