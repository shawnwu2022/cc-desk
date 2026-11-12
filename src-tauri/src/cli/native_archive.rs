//! Codex 归档只交给官方 thread/archive；不移动历史文件或写 Desk 归档标记。
use super::native_projection::wire::SourceRef;
use super::types::SafeError;
use crate::version_history::codex_archive_admission::CodexArchiveOwners;
use serde::{Deserialize, Serialize};
#[path = "native_archive_relations.rs"]
mod relations;

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ArchiveRequest {
    pub(crate) operation_id: String,
    pub(crate) source: SourceRef,
    pub(crate) session_key: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArchiveReceipt {
    pub(crate) operation_id: String,
    pub(crate) source: SourceRef,
    pub(crate) session_key: String,
    pub(crate) state: &'static str,
    pub(crate) root_archived: bool,
    pub(crate) descendants: &'static str,
}
impl ArchiveRequest {
    pub(crate) fn target_id(&self) -> Result<uuid::Uuid, SafeError> {
        let parts: Vec<String> = serde_json::from_str(&self.session_key)
            .map_err(|_| super::profiles::error("SOURCE_INVALID"))?;
        if parts.len() != 4
            || parts[0] != "local"
            || parts[1] != "codex"
            || parts[2] != self.source.source_root_key
        {
            return Err(super::profiles::error("SOURCE_INVALID"));
        }
        uuid::Uuid::parse_str(&parts[3]).map_err(|_| super::profiles::error("SOURCE_INVALID"))
    }
    pub(crate) fn validate(&self) -> Result<(), SafeError> {
        if self.operation_id.is_empty()
            || self.operation_id.len() > 128
            || self.operation_id.chars().any(char::is_control)
        {
            return Err(SafeError::invalid("operationId"));
        }
        if self.session_key.is_empty() || self.session_key.len() > 16384 {
            return Err(SafeError::invalid("sessionKey"));
        }
        if self.source.cli != super::types::CliKind::Codex
            || self.source.basis != super::native_projection::wire::SourceBasis::ConfiguredProfile
            || !matches!(
                self.source.target,
                super::native_projection::wire::ScopeTarget::Profile { .. }
            )
        {
            return Err(super::profiles::error("NATIVE_ARCHIVE_UNSUPPORTED"));
        }
        self.source.target.validate()
    }
    pub(crate) fn receipt(&self) -> ArchiveReceipt {
        ArchiveReceipt {
            operation_id: self.operation_id.clone(),
            source: self.source.clone(),
            session_key: self.session_key.clone(),
            state: "archived",
            root_archived: true,
            descendants: "unverified",
        }
    }
}
struct OperationEntry {
    caller: super::snapshot::CallerIdentity,
    request: ArchiveRequest,
    physical: String,
    result: Option<Result<(), SafeError>>,
}
fn uncertain(result: &Option<Result<(), SafeError>>) -> bool {
    result.as_ref().is_none_or(|result| {
        result.as_ref().is_err_and(|failure| {
            matches!(
                failure.code.as_str(),
                "ARCHIVE_OUTCOME_UNKNOWN" | "ARCHIVE_VERIFICATION_FAILED"
            )
        })
    })
}
#[derive(Default)]
pub(crate) struct Operations(std::sync::Mutex<Vec<OperationEntry>>);
impl Operations {
    pub(crate) fn has_unknown(&self, physical: Option<&str>) -> Result<bool, SafeError> {
        let entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("ARCHIVE_FAILED"))?;
        Ok(entries.iter().any(|entry| {
            physical.is_none_or(|key| key == entry.physical) && uncertain(&entry.result)
        }))
    }
    pub(crate) fn unknown_session_keys(&self, root: &str) -> Result<Vec<String>, SafeError> {
        let entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("ARCHIVE_FAILED"))?;
        Ok(entries
            .iter()
            .filter(|entry| {
                entry.request.source.source_root_key == root && uncertain(&entry.result)
            })
            .map(|entry| entry.request.session_key.clone())
            .collect())
    }
    pub(crate) fn validate_request(
        &self,
        caller: &super::snapshot::CallerIdentity,
        request: &ArchiveRequest,
        physical: &str,
    ) -> Result<(), SafeError> {
        let entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("ARCHIVE_FAILED"))?;
        if entries.iter().any(|entry| {
            &entry.caller == caller
                && entry.request.operation_id == request.operation_id
                && (entry.request != *request || entry.physical != physical)
        }) {
            return Err(super::profiles::error("ARCHIVE_OPERATION_CONFLICT"));
        }
        Ok(())
    }
    pub(crate) fn release_revoked(
        &self,
        authorized: impl Fn(&super::snapshot::CallerIdentity) -> bool,
    ) -> Result<(), SafeError> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("ARCHIVE_FAILED"))?;
        // 物理目标的不确定写入跨文档保留；关闭页面不等于原生写入没有发生。
        entries.retain(|entry| uncertain(&entry.result) || authorized(&entry.caller));
        Ok(())
    }
    pub(crate) fn reconcile_archived(&self, physical: &str) -> Result<(), SafeError> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("ARCHIVE_FAILED"))?;
        if entries
            .iter()
            .any(|entry| entry.physical == physical && entry.result.is_none())
        {
            return Err(super::profiles::error("ARCHIVE_OUTCOME_UNKNOWN"));
        }
        for entry in entries
            .iter_mut()
            .filter(|entry| entry.physical == physical)
        {
            // 调用者已证明后台无 pending owner 且同源根位于原生归档容器。
            if entry.result.is_some() {
                entry.result = Some(Ok(()));
            }
        }
        Ok(())
    }
    pub(crate) fn begin(
        &self,
        caller: &super::snapshot::CallerIdentity,
        request: &ArchiveRequest,
        physical: &str,
    ) -> Result<Option<Result<(), SafeError>>, SafeError> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("ARCHIVE_FAILED"))?;
        if let Some(entry) = entries.iter().find(|entry| {
            &entry.caller == caller && entry.request.operation_id == request.operation_id
        }) {
            if entry.request != *request || entry.physical != physical {
                return Err(super::profiles::error("ARCHIVE_OPERATION_CONFLICT"));
            }
            return Ok(Some(entry.result.clone().unwrap_or_else(|| {
                Err(super::profiles::error("ARCHIVE_OUTCOME_UNKNOWN"))
            })));
        }
        if entries
            .iter()
            .any(|entry| entry.physical == physical && uncertain(&entry.result))
        {
            return Err(super::profiles::error("ARCHIVE_OUTCOME_UNKNOWN"));
        }
        if entries.len() >= 4096 {
            return Err(super::profiles::error("SOURCE_BUSY"));
        }
        entries.push(OperationEntry {
            caller: caller.clone(),
            request: request.clone(),
            physical: physical.into(),
            result: None,
        });
        Ok(None)
    }
    pub(crate) fn finish(
        &self,
        caller: &super::snapshot::CallerIdentity,
        request: &ArchiveRequest,
        result: Result<(), SafeError>,
    ) {
        if let Ok(mut entries) = self.0.lock() {
            if let Some(entry) = entries
                .iter_mut()
                .find(|entry| &entry.caller == caller && entry.request == *request)
            {
                entry.result = Some(result);
            }
        }
    }
}

pub(crate) fn process(
    child: &mut std::process::Child,
    authority: &super::native_projection::registry::SessionAuthority,
    owners: &CodexArchiveOwners,
    current: &dyn Fn() -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    use super::profiles::error;
    use std::io::{BufRead, Read};
    let input = child.stdin.take().ok_or_else(|| error("ARCHIVE_FAILED"))?;
    let output = child.stdout.take().ok_or_else(|| error("ARCHIVE_FAILED"))?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    std::thread::Builder::new()
        .name("native-archive-read".into())
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
        .map_err(|_| error("ARCHIVE_FAILED"))?;
    let write_sender = input_writer(input)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let write_committed = std::cell::Cell::new(false);
    exchange_scoped(
        &authority.native_id,
        owners.sessions(),
        &mut |message| {
            let classify = |failure| {
                if write_committed.get() {
                    SendFailure::Attempted(failure)
                } else {
                    SendFailure::NotSent(failure)
                }
            };
            current().map_err(classify)?;
            let mut bytes =
                serde_json::to_vec(&message).map_err(|_| classify(error("ARCHIVE_FAILED")))?;
            if bytes.len() >= 2048 {
                return Err(classify(error("ARCHIVE_FAILED")));
            }
            bytes.push(b'\n');
            if message["method"] == "thread/archive" {
                // 门禁锁内只提交阶段；队列和管道 I/O 都在锁外，失败保守视为可能已发送。
                owners
                    .with_current(|| write_committed.set(true))
                    .map_err(classify)?;
            }
            current().map_err(classify)?;
            send_bytes(&write_sender, bytes, deadline, current).map_err(classify)
        },
        &mut |id| {
            for _ in 0..256 {
                let value = loop {
                    current()?;
                    let remaining = deadline
                        .checked_duration_since(std::time::Instant::now())
                        .ok_or_else(|| error("ARCHIVE_FAILED"))?;
                    match receiver
                        .recv_timeout(remaining.min(std::time::Duration::from_millis(100)))
                    {
                        Ok(value) => break value,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return Err(error("ARCHIVE_FAILED")),
                    }
                };
                if value.get("id").is_some() {
                    if value["id"] == id {
                        return Ok(value);
                    }
                    return Err(error("ARCHIVE_FAILED"));
                }
            }
            Err(error("ARCHIVE_FAILED"))
        },
        &|startup| authority.check_delete_startup(startup),
        &|thread, after| {
            if after {
                authority.check_archived_thread(thread)
            } else {
                authority.check_thread(thread)
            }
        },
    )
}

type ArchiveInput = std::sync::mpsc::SyncSender<(Vec<u8>, std::sync::mpsc::SyncSender<bool>)>;
fn input_writer(
    mut input: impl std::io::Write + Send + 'static,
) -> Result<ArchiveInput, SafeError> {
    let (sender, receiver) =
        std::sync::mpsc::sync_channel::<(Vec<u8>, std::sync::mpsc::SyncSender<bool>)>(1);
    std::thread::Builder::new()
        .name("native-archive-write".into())
        .spawn(move || {
            while let Ok((bytes, acknowledgment)) = receiver.recv() {
                let written = input.write_all(&bytes).and_then(|_| input.flush()).is_ok();
                let _ = acknowledgment.try_send(written);
                if !written {
                    break;
                }
            }
        })
        .map_err(|_| super::profiles::error("ARCHIVE_FAILED"))?;
    Ok(sender)
}
fn send_bytes(
    sender: &ArchiveInput,
    bytes: Vec<u8>,
    deadline: std::time::Instant,
    current: &dyn Fn() -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    use super::profiles::error;
    let (acknowledge, acknowledged) = std::sync::mpsc::sync_channel(1);
    sender
        .try_send((bytes, acknowledge))
        .map_err(|_| error("ARCHIVE_FAILED"))?;
    loop {
        current()?;
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(|| error("ARCHIVE_FAILED"))?;
        match acknowledged.recv_timeout(remaining.min(std::time::Duration::from_millis(100))) {
            Ok(true) => return Ok(()),
            Ok(false) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(error("ARCHIVE_FAILED"))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

enum SendFailure {
    NotSent(SafeError),
    Attempted(SafeError),
}
impl SendFailure {
    fn before(self) -> SafeError {
        match self {
            Self::NotSent(e) | Self::Attempted(e) => e,
        }
    }
    fn archive(self) -> SafeError {
        match self {
            Self::NotSent(e) => e,
            Self::Attempted(_) => super::profiles::error("ARCHIVE_OUTCOME_UNKNOWN"),
        }
    }
}
#[cfg(test)]
pub(crate) fn exchange(
    id: &str,
    send: &mut dyn FnMut(serde_json::Value) -> Result<(), SafeError>,
    receive: &mut dyn FnMut(u64) -> Result<serde_json::Value, SafeError>,
    startup: &dyn Fn(&serde_json::Value) -> Result<(), SafeError>,
    check: &dyn Fn(&serde_json::Value, bool) -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    exchange_scoped(
        id,
        &Default::default(),
        &mut |frame| send(frame).map_err(SendFailure::Attempted),
        receive,
        startup,
        check,
    )
}
fn exchange_scoped(
    id: &str,
    owners: &std::collections::BTreeSet<uuid::Uuid>,
    send: &mut dyn FnMut(serde_json::Value) -> Result<(), SendFailure>,
    receive: &mut dyn FnMut(u64) -> Result<serde_json::Value, SafeError>,
    startup: &dyn Fn(&serde_json::Value) -> Result<(), SafeError>,
    check: &dyn Fn(&serde_json::Value, bool) -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    use super::profiles::error;
    use serde_json::json;
    fn result(value: serde_json::Value, id: u64) -> Result<serde_json::Value, SafeError> {
        if value["id"] != id || value.get("error").is_some() {
            return Err(error("ARCHIVE_FAILED"));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| error("ARCHIVE_FAILED"))
    }
    send(json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"cc_desk","version":env!("CARGO_PKG_VERSION")},"capabilities":{"explicitGatewayOauth":true}}})).map_err(SendFailure::before)?;
    startup(&result(receive(1)?, 1)?)?;
    send(json!({"method":"initialized"})).map_err(SendFailure::before)?;
    send(json!({"id":2,"method":"thread/read","params":{"threadId":id,"includeTurns":false}}))
        .map_err(SendFailure::before)?;
    let before = result(receive(2)?, 2)?;
    check(&before["thread"], false)?;
    if !owners.is_empty() {
        let target =
            uuid::Uuid::parse_str(id).map_err(|_| error("ARCHIVE_RELATION_UNAVAILABLE"))?;
        let mut request_id = 100u64;
        relations::check(target, &before["thread"], owners, &mut |owner| {
            let current_id = request_id;
            request_id += 1;
            send(json!({"id":current_id,"method":"thread/read","params":{"threadId":owner.to_string(),"includeTurns":false}})).map_err(SendFailure::before)?;
            Ok(result(receive(current_id)?, current_id)?["thread"].clone())
        })?;
    }
    // 归档帧只发一次。只有确定一个字节都尚未发送的失败才可返回普通拒绝。
    send(json!({"id":3,"method":"thread/archive","params":{"threadId":id}}))
        .map_err(SendFailure::archive)?;
    result(receive(3).map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?, 3)
        .map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?;
    send(json!({"id":4,"method":"thread/read","params":{"threadId":id,"includeTurns":false}}))
        .map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?;
    let after = result(receive(4).map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?, 4)
        .map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?;
    check(&after["thread"], true).map_err(|_| error("ARCHIVE_VERIFICATION_FAILED"))?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/native_cli_archive.rs"]
mod tests;
