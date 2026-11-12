//! 绑定来源的两阶段永久删除；预览阶段不启动原生助手。
use super::{
    native_projection::wire::SourceRef,
    profiles::error,
    snapshot::CallerIdentity,
    types::{CliKind, SafeError},
};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewRequest {
    pub operation_id: String,
    pub source: SourceRef,
    pub session_key: String,
}
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfirmRequest {
    pub operation_id: String,
    pub preview_id: String,
    pub acknowledge_permanent_delete: bool,
    pub acknowledge_native_subtree: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InspectRequest {
    pub operation_id: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preview {
    pub operation_id: String,
    pub preview_id: Option<String>,
    pub cli: CliKind,
    pub state: &'static str,
    pub native_session_id: Option<String>,
    pub root_archived: Option<bool>,
    pub scope: &'static str,
    pub known_descendant_count: Option<u32>,
    pub reason: Option<&'static str>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Receipt {
    pub operation_id: String,
    pub cli: CliKind,
    pub state: &'static str,
    pub root_present: Option<bool>,
    pub descendants: &'static str,
    pub reason: Option<&'static str>,
}
impl PreviewRequest {
    pub fn validate(&self) -> Result<(), SafeError> {
        validate_id(&self.operation_id, "operationId")?;
        if self.session_key.is_empty() || self.session_key.len() > 16384 {
            return Err(SafeError::invalid("sessionKey"));
        }
        self.source.target.validate()
    }
    pub fn preview(&self, state: &'static str, reason: Option<&'static str>) -> Preview {
        Preview {
            operation_id: self.operation_id.clone(),
            preview_id: None,
            cli: self.source.cli,
            state,
            native_session_id: None,
            root_archived: None,
            scope: "native-subtree",
            known_descendant_count: None,
            reason,
        }
    }
    pub fn receipt(
        &self,
        state: &'static str,
        root_present: Option<bool>,
        reason: Option<&'static str>,
    ) -> Receipt {
        Receipt {
            operation_id: self.operation_id.clone(),
            cli: self.source.cli,
            state,
            root_present,
            descendants: "unverified",
            reason,
        }
    }
}
fn validate_id(id: &str, field: &str) -> Result<(), SafeError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| SafeError::invalid(field))
}
impl ConfirmRequest {
    pub fn validate(&self) -> Result<(), SafeError> {
        validate_id(&self.operation_id, "operationId")?;
        validate_id(&self.preview_id, "previewId")?;
        if !self.acknowledge_permanent_delete || !self.acknowledge_native_subtree {
            return Err(error("DELETE_CONFIRMATION_REQUIRED"));
        }
        Ok(())
    }
}
#[derive(Clone)]
pub(crate) struct Entry {
    owner: CallerIdentity,
    pub request: PreviewRequest,
    pub preview: Preview,
    pub observation: String,
    pub created: Instant,
    pub confirmed: Option<ConfirmRequest>,
    pub receipt: Option<Receipt>,
    pub active: Arc<AtomicBool>,
    pub outcome: OwnerOutcome,
}
impl Entry {
    fn block_before_dispatch(&mut self) -> Receipt {
        debug_assert!(self.confirmed.is_none());
        self.preview.preview_id = None;
        self.preview.state = "blocked";
        self.preview.reason = Some("DELETE_PREVIEW_CHANGED");
        let receipt = self
            .request
            .receipt("blocked", None, Some("DELETE_PREVIEW_CHANGED"));
        self.receipt = Some(receipt.clone());
        receipt
    }
}
pub(crate) type OwnerOutcome = Arc<Mutex<Option<Result<(), SafeError>>>>;
pub(crate) fn pre_dispatch_reason(code: &str) -> Option<&'static str> {
    match code {
        "DELETE_INITIALIZE_FAILED" => Some("DELETE_INITIALIZE_FAILED"),
        "DELETE_READ_FAILED" => Some("DELETE_READ_FAILED"),
        "DELETE_PREPARE_FAILED" => Some("DELETE_PREPARE_FAILED"),
        _ => None,
    }
}
pub(crate) fn pre_dispatch_failure(failure: SafeError) -> SafeError {
    if pre_dispatch_reason(&failure.code).is_some()
        || matches!(
            failure.code.as_str(),
            "SOURCE_BUSY"
                | "DELETE_ARGUMENTS_UNSUPPORTED"
                | "DELETE_ROOT_NOT_ARCHIVED"
                | "DELETE_SOURCE_UNAVAILABLE"
        )
    {
        failure
    } else {
        error("DELETE_PREPARE_FAILED")
    }
}
pub(crate) fn reconcile(entry: &Entry, root: Option<bool>) -> Receipt {
    if entry.active.load(Ordering::Acquire) {
        return entry
            .request
            .receipt("unknown", None, Some("DELETE_OWNER_PENDING"));
    }
    let stage = entry.outcome.lock().ok().and_then(|stored| {
        stored
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .and_then(|failure| pre_dispatch_reason(&failure.code))
    });
    if let Some(stage) = stage {
        return entry.request.receipt("blocked", root, Some(stage));
    }
    let old = entry.receipt.clone().unwrap_or_else(|| {
        entry
            .request
            .receipt("unknown", None, Some("DELETE_OUTCOME_UNKNOWN"))
    });
    let acknowledged = entry
        .outcome
        .lock()
        .ok()
        .is_some_and(|stored| matches!(*stored, Some(Ok(()))));
    if (old.state == "acknowledged" || (old.state == "unknown" && acknowledged))
        && root == Some(true)
    {
        entry
            .request
            .receipt("partial", root, Some("DELETE_ROOT_PRESENT"))
    } else if old.state == "unknown" && acknowledged {
        entry.request.receipt("acknowledged", root, None)
    } else if old.state == "unknown" && root == Some(true) {
        entry
            .request
            .receipt("partial", root, Some("DELETE_NOT_ACKNOWLEDGED"))
    } else {
        Receipt {
            root_present: root,
            ..old
        }
    }
}
#[derive(Default)]
pub(crate) struct Operations(Mutex<Vec<Entry>>);
impl Operations {
    pub fn invalidate(&self, caller: &CallerIdentity, id: &str) -> Result<Receipt, SafeError> {
        let mut entries = self.0.lock().map_err(|_| error("DELETE_UNAVAILABLE"))?;
        let entry = entries
            .iter_mut()
            .find(|e| &e.owner == caller && e.request.operation_id == id)
            .ok_or_else(|| error("DELETE_OPERATION_UNKNOWN"))?;
        if entry.confirmed.is_some() {
            return Ok(entry.receipt.clone().unwrap_or_else(|| {
                entry
                    .request
                    .receipt("unknown", None, Some("DELETE_IN_PROGRESS"))
            }));
        }
        Ok(entry.block_before_dispatch())
    }
    pub fn register(
        &self,
        caller: &CallerIdentity,
        request: &PreviewRequest,
        native_id: &str,
        observation: String,
    ) -> Result<Preview, SafeError> {
        // 其他提供方只有只读来源诊断；后续适配不能仅凭有效 ID 绕过执行能力门。
        if request.source.cli != CliKind::Codex {
            return Err(error("NATIVE_DELETE_UNSUPPORTED"));
        }
        let native_id = uuid::Uuid::parse_str(native_id)
            .map_err(|_| error("DELETE_SOURCE_UNAVAILABLE"))?
            .to_string();
        let mut entries = self.0.lock().map_err(|_| error("DELETE_UNAVAILABLE"))?;
        if let Some(entry) = entries
            .iter()
            .find(|e| &e.owner == caller && e.request.operation_id == request.operation_id)
        {
            if &entry.request != request {
                return Err(error("DELETE_OPERATION_CONFLICT"));
            }
            return Ok(entry.preview.clone());
        }
        if entries.len() >= 4096
            || entries.iter().any(|e| {
                e.request.source.cli == request.source.cli
                    && e.request.source.source_root_key == request.source.source_root_key
                    && e.preview.native_session_id.as_deref() == Some(native_id.as_str())
                    && (e.active.load(Ordering::Acquire)
                        || e.receipt.as_ref().is_some_and(|r| r.state == "unknown"))
            })
        {
            return Err(error("SOURCE_BUSY"));
        }
        let mut preview = request.preview("ready", None);
        preview.preview_id = Some(uuid::Uuid::new_v4().to_string());
        preview.native_session_id = Some(native_id);
        preview.root_archived = Some(true);
        entries.push(Entry {
            owner: caller.clone(),
            request: request.clone(),
            preview: preview.clone(),
            observation,
            created: Instant::now(),
            confirmed: None,
            receipt: None,
            active: Arc::new(AtomicBool::new(false)),
            outcome: Arc::new(Mutex::new(None)),
        });
        Ok(preview)
    }
    pub fn get(&self, caller: &CallerIdentity, id: &str) -> Result<Entry, SafeError> {
        self.0
            .lock()
            .map_err(|_| error("DELETE_UNAVAILABLE"))?
            .iter()
            .find(|e| &e.owner == caller && e.request.operation_id == id)
            .cloned()
            .ok_or_else(|| error("DELETE_OPERATION_UNKNOWN"))
    }
    pub fn begin(
        &self,
        caller: &CallerIdentity,
        request: &ConfirmRequest,
        observation: &str,
    ) -> Result<Option<Receipt>, SafeError> {
        request.validate()?;
        let mut entries = self.0.lock().map_err(|_| error("DELETE_UNAVAILABLE"))?;
        let index = entries
            .iter()
            .position(|e| &e.owner == caller && e.request.operation_id == request.operation_id)
            .ok_or_else(|| error("DELETE_OPERATION_UNKNOWN"))?;
        let entry = &entries[index];
        if entry.preview.preview_id.as_deref() != Some(request.preview_id.as_str()) {
            if entry.confirmed.is_some() {
                return Err(error("DELETE_PREVIEW_CHANGED"));
            }
            return Ok(Some(entries[index].block_before_dispatch()));
        }
        if let Some(old) = &entry.confirmed {
            if old != request {
                return Err(error("DELETE_OPERATION_CONFLICT"));
            }
            return Ok(Some(entry.receipt.clone().unwrap_or_else(|| {
                entry
                    .request
                    .receipt("unknown", None, Some("DELETE_IN_PROGRESS"))
            })));
        }
        if entry.created.elapsed() > Duration::from_secs(120) || entry.observation != observation {
            return Ok(Some(entries[index].block_before_dispatch()));
        }
        if entries.iter().enumerate().any(|(i, e)| {
            i != index
                && e.request.source.source_root_key == entry.request.source.source_root_key
                && e.preview.native_session_id == entry.preview.native_session_id
                && (e.active.load(Ordering::Acquire)
                    || e.receipt.as_ref().is_some_and(|r| r.state == "unknown"))
        }) {
            return Err(error("SOURCE_BUSY"));
        }
        let entry = &mut entries[index];
        entry.confirmed = Some(request.clone());
        entry.active.store(true, Ordering::Release);
        Ok(None)
    }
    pub fn finish(&self, caller: &CallerIdentity, id: &str, receipt: Receipt) {
        if let Ok(mut entries) = self.0.lock() {
            if let Some(entry) = entries
                .iter_mut()
                .find(|e| &e.owner == caller && e.request.operation_id == id)
            {
                if receipt.reason != Some("DELETE_OWNER_PENDING") {
                    entry.active.store(false, Ordering::Release);
                }
                entry.receipt = Some(receipt);
            }
        }
    }
}
pub(crate) fn exchange(
    id: &str,
    send: &mut dyn FnMut(serde_json::Value) -> Result<(), SafeError>,
    receive: &mut dyn FnMut(u64) -> Result<serde_json::Value, SafeError>,
    check: &dyn Fn(&serde_json::Value) -> Result<(), SafeError>,
    check_startup: &dyn Fn(&serde_json::Value) -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    use serde_json::json;
    fn result(value: serde_json::Value, id: u64) -> Result<serde_json::Value, SafeError> {
        if value["id"] != id || value.get("error").is_some() {
            return Err(error("DELETE_NOT_ACKNOWLEDGED"));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| error("DELETE_OUTCOME_UNKNOWN"))
    }
    // 元数据删除不请求登录，初始化不能恢复原生自动认证。
    send(
        json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"cc_desk","version":env!("CARGO_PKG_VERSION")},"capabilities":{"explicitGatewayOauth":true}}}),
    ).map_err(|_| error("DELETE_INITIALIZE_FAILED"))?;
    let startup = receive(1)
        .and_then(|value| result(value, 1))
        .map_err(|_| error("DELETE_INITIALIZE_FAILED"))?;
    check_startup(&startup).map_err(|_| error("DELETE_SOURCE_UNAVAILABLE"))?;
    send(json!({"method":"initialized"})).map_err(|_| error("DELETE_READ_FAILED"))?;
    send(json!({"id":2,"method":"thread/read","params":{"threadId":id,"includeTurns":false}}))
        .map_err(|_| error("DELETE_READ_FAILED"))?;
    let thread = receive(2)
        .and_then(|value| result(value, 2))
        .map_err(|_| error("DELETE_READ_FAILED"))?;
    check(&thread["thread"]).map_err(|_| error("DELETE_SOURCE_UNAVAILABLE"))?;
    send(json!({"id":3,"method":"thread/delete","params":{"threadId":id}}))
        .map_err(|_| error("DELETE_OUTCOME_UNKNOWN"))?;
    let response = result(receive(3).map_err(|_| error("DELETE_OUTCOME_UNKNOWN"))?, 3)?;
    if !response.as_object().is_some_and(|v| v.is_empty()) {
        return Err(error("DELETE_OUTCOME_UNKNOWN"));
    }
    Ok(())
}
fn send_packet(
    input: &mut impl std::io::Write,
    value: &serde_json::Value,
    current: &dyn Fn() -> Result<(), SafeError>,
    delete_write_started: &mut bool,
    deadline: Instant,
) -> Result<(), SafeError> {
    current()?;
    // 复核可能耗尽助手剩余时限，超时后不得再发送任何协议帧。
    if Instant::now() >= deadline {
        return Err(error("DELETE_PREPARE_FAILED"));
    }
    // 验证失败尚未写入；一旦尝试序列化删除，即使写入报错也不能断言未发送。
    if value["method"] == "thread/delete" {
        *delete_write_started = true;
    }
    serde_json::to_writer(&mut *input, value).map_err(|_| error("DELETE_OUTCOME_UNKNOWN"))?;
    input
        .write_all(b"\n")
        .and_then(|_| input.flush())
        .map_err(|_| error("DELETE_OUTCOME_UNKNOWN"))
}
pub(crate) fn process(
    child: &mut std::process::Child,
    authority: &super::native_projection::registry::SessionAuthority,
    current: &dyn Fn() -> Result<(), SafeError>,
    delete_write_started: &mut bool,
) -> Result<(), SafeError> {
    use std::io::{BufRead, Read};
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| error("DELETE_UNAVAILABLE"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| error("DELETE_UNAVAILABLE"))?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    std::thread::Builder::new()
        .name("native-delete-read".into())
        .spawn(move || {
            let mut reader = std::io::BufReader::new(output);
            let mut total = 0usize;
            loop {
                let mut line = Vec::new();
                let read = reader.by_ref().take(65537).read_until(b'\n', &mut line);
                total = total.saturating_add(line.len());
                if !matches!(read, Ok(1..)) || line.len() > 65536 || total > 1048576 {
                    break;
                }
                let Ok(value) = serde_json::from_slice::<serde_json::Value>(&line) else {
                    break;
                };
                if sender.send(value).is_err() {
                    break;
                }
            }
        })
        .map_err(|_| error("DELETE_UNAVAILABLE"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    exchange(
        &authority.native_id,
        &mut |value| send_packet(&mut input, &value, current, delete_write_started, deadline),
        &mut |id| {
            for _ in 0..256 {
                let value = loop {
                    current()?;
                    let remaining = deadline
                        .checked_duration_since(Instant::now())
                        .ok_or_else(|| error("DELETE_OUTCOME_UNKNOWN"))?;
                    match receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
                        Ok(value) => break value,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return Err(error("DELETE_OUTCOME_UNKNOWN")),
                    }
                };
                if value.get("id").is_some() {
                    if value["id"] == id {
                        return Ok(value);
                    }
                    return Err(error("DELETE_OUTCOME_UNKNOWN"));
                }
                // 官方启动警告和账户路由仅作元数据跳过，不打印或返回其潜在敏感载荷。
                if matches!(
                    value["method"].as_str(),
                    Some("configWarning" | "account/updated" | "remoteControl/status/changed")
                ) && value["params"].is_object()
                {
                    continue;
                }
                // 此助手不加载会话；未经请求的工作通知不能充当执行许可。
                if id != 3
                    || value["method"] != "thread/deleted"
                    || value["params"]["threadId"]
                        .as_str()
                        .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
                {
                    return Err(error("DELETE_OUTCOME_UNKNOWN"));
                }
            }
            Err(error("DELETE_OUTCOME_UNKNOWN"))
        },
        &|thread| authority.check_delete_thread(thread),
        &|startup| authority.check_delete_startup(startup),
    )
}
#[cfg(test)]
#[path = "../tests/native_cli_delete.rs"]
mod tests;
