use axum::body::Bytes;
use axum::extract::Json;
use axum::http::HeaderMap;
use axum::{routing::post, Router};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use parking_lot::Mutex as SyncMutex;

use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use tower::ServiceBuilder;

use crate::hook_events::HookPayload;
use crate::store::invalidate_project_path_mapping;

pub(crate) const MAX_OBSERVER_PAYLOAD: usize = 64 * 1024;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct ObserverRun {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObserverSource {
    ClaudeHook,
    Unknown,
}

#[derive(Clone)]
pub(crate) struct ObserverBinding {
    pub(crate) run: ObserverRun,
    pub(crate) capability: String,
    pub(crate) source: ObserverSource,
}

impl std::fmt::Debug for ObserverBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ObserverBinding(<redacted>)")
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct ValidatedObserverEvent {
    pub(crate) run: ObserverRun,
    pub(crate) event_id: String,
    pub(crate) source: ObserverSource,
    pub(crate) event: Value,
}

impl std::fmt::Debug for ValidatedObserverEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ValidatedObserverEvent(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ObserverAccept {
    Accepted(ValidatedObserverEvent),
    Duplicate,
}

struct ObserverSlot {
    capability: String,
    source: ObserverSource,
    seen: HashSet<String>,
}

pub(crate) struct ObserverRegistry {
    slots: SyncMutex<HashMap<ObserverRun, ObserverSlot>>,
}

impl ObserverRegistry {
    pub(crate) const MAX_SEEN_EVENTS: usize = 1024;

    pub(crate) fn new() -> Self {
        Self {
            slots: SyncMutex::new(HashMap::new()),
        }
    }

    pub(crate) fn attach(
        &self,
        run: ObserverRun,
        capability: String,
        source: ObserverSource,
    ) -> Result<ObserverBinding, SafeError> {
        validate_observer_run(&run)?;
        validate_capability(&capability)?;
        if source != ObserverSource::ClaudeHook {
            return Err(error("OBSERVER_SOURCE_UNSUPPORTED"));
        }

        self.slots.lock().insert(
            run.clone(),
            ObserverSlot {
                capability: capability.clone(),
                source,
                seen: HashSet::new(),
            },
        );
        Ok(ObserverBinding {
            run,
            capability,
            source,
        })
    }

    pub(crate) fn accept_event(
        &self,
        binding: &ObserverBinding,
        event_id: &str,
        payload: &[u8],
    ) -> Result<ObserverAccept, SafeError> {
        validate_observer_run(&binding.run)?;
        if binding.source != ObserverSource::ClaudeHook {
            return Err(error("OBSERVER_SOURCE_UNSUPPORTED"));
        }
        validate_event_id(event_id)?;
        if payload.len() > MAX_OBSERVER_PAYLOAD {
            return Err(error("OBSERVER_PAYLOAD_TOO_LARGE"));
        }

        self.check_binding(binding)?;
        let event: Value =
            serde_json::from_slice(payload).map_err(|_| error("OBSERVER_INVALID_EVENT"))?;
        let event_name = event
            .as_object()
            .and_then(|object| object.get("hook_event_name"))
            .and_then(Value::as_str)
            .ok_or_else(|| error("OBSERVER_INVALID_EVENT"))?;
        if !SUPPORTED_OBSERVER_EVENTS.contains(&event_name) {
            return Err(error("OBSERVER_INVALID_EVENT"));
        }

        let mut slots = self.slots.lock();
        let slot = slots
            .get_mut(&binding.run)
            .ok_or_else(|| error("OBSERVER_FORBIDDEN"))?;
        if slot.capability != binding.capability || slot.source != binding.source {
            return Err(error("OBSERVER_FORBIDDEN"));
        }
        if slot.seen.contains(event_id) {
            return Ok(ObserverAccept::Duplicate);
        }
        if slot.seen.len() >= Self::MAX_SEEN_EVENTS {
            return Err(error("OBSERVER_EVENT_CAPACITY"));
        }
        slot.seen.insert(event_id.to_string());

        Ok(ObserverAccept::Accepted(ValidatedObserverEvent {
            run: binding.run.clone(),
            event_id: event_id.to_string(),
            source: binding.source,
            event,
        }))
    }

    fn check_binding(&self, binding: &ObserverBinding) -> Result<(), SafeError> {
        let slots = self.slots.lock();
        let slot = slots
            .get(&binding.run)
            .ok_or_else(|| error("OBSERVER_FORBIDDEN"))?;
        if slot.capability != binding.capability || slot.source != binding.source {
            return Err(error("OBSERVER_FORBIDDEN"));
        }
        Ok(())
    }
}

const SUPPORTED_OBSERVER_EVENTS: [&str; 13] = [
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
];

fn validate_observer_run(run: &ObserverRun) -> Result<(), SafeError> {
    if run.generation == 0
        || run.run_id.is_empty()
        || run.run_id.len() > 128
        || run.run_id.chars().any(char::is_control)
    {
        return Err(SafeError::invalid("run"));
    }
    Ok(())
}

fn validate_capability(value: &str) -> Result<(), SafeError> {
    if !(32..=128).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err(SafeError::invalid("capability"));
    }
    Ok(())
}

fn validate_event_id(value: &str) -> Result<(), SafeError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err(SafeError::invalid("eventId"));
    }
    Ok(())
}

/// session_id → pty_id 映射
type SessionMap = Mutex<std::collections::HashMap<String, String>>;

/// 同步存储端口
static HOOK_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

/// 异步会话映射
static SESSIONS: once_cell::sync::Lazy<SessionMap> =
    once_cell::sync::Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

pub async fn init(app_handle: AppHandle) {
    match start_server(app_handle).await {
        Ok(port) => {
            HOOK_PORT.set(port).ok();
            log::info!("Hook server started on port {}", port);
        }
        Err(e) => {
            log::error!(
                "Failed to start hook server: {}. CC Desk will continue without hook monitoring.",
                e
            );
        }
    }
}

async fn start_server(app_handle: AppHandle) -> Result<u16, String> {
    let app = Router::new()
        .route("/hook", post(handle_hook))
        .layer(ServiceBuilder::new());

    let addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind failed: {}", e))?;

    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {}", e))?
        .port();

    tokio::spawn(async move {
        let app = app.layer(axum::extract::Extension(app_handle));
        if let Err(e) = axum::serve(listener, app).await {
            log::error!("Hook server error: {}", e);
        }
    });

    Ok(port)
}

pub fn get_port() -> Option<u16> {
    HOOK_PORT.get().copied()
}

async fn handle_hook(
    headers: HeaderMap,
    axum::extract::Extension(app_handle): axum::extract::Extension<AppHandle>,
    body: Bytes,
) -> Json<Value> {
    // 从原始字节转换为 UTF-8 字符串（处理可能的编码问题）
    let body_str = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[hook-server] UTF-8 decode error: {}", e);
            // 使用 lossy 转换作为 fallback
            String::from_utf8_lossy(&body).into_owned()
        }
    };

    let event: Value = match serde_json::from_str(&body_str) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "[hook-server] JSON parse error: {} (body length={})",
                e,
                body_str.len()
            );
            return Json(json!({}));
        }
    };

    // 从 header 获取 PTY ID
    let pty_id = headers
        .get("X-CC-Box-Session")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let event_name = event
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    log::info!(
        "[hook-server] received: {} pty={}",
        event_name,
        pty_id.as_deref().unwrap_or("?")
    );

    // 建立 session_id ↔ pty_id 映射
    if let Some(ref sid) = event
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
    {
        if let Some(ref pty) = pty_id {
            let mut sessions = SESSIONS.lock().await;
            sessions.insert(sid.to_string(), pty.clone());
        }
    }

    // 统一提取结构化数据并发送
    let payload = HookPayload::from_raw(pty_id, event);

    // SessionStart 时 invalidate 项目路径缓存（确保新项目会话可见）
    if event_name == "SessionStart" {
        invalidate_project_path_mapping();
        log::info!("[hook-server] SessionStart received, invalidated project path cache");
    }

    let _ = app_handle.emit("hook-event", &payload);

    Json(json!({}))
}
