//! 原生名称写入只通过 CLI 官方元数据协议；不写 Desk 别名。
use super::native_projection::wire::SourceRef;
use super::types::SafeError;
#[path = "claude_rename.rs"]
pub(crate) mod claude;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RenameRequest {
    pub(crate) operation_id: String,
    pub(crate) source: SourceRef,
    pub(crate) session_key: String,
    pub(crate) title: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameReceipt {
    pub(crate) operation_id: String,
    pub(crate) source: SourceRef,
    pub(crate) session_key: String,
    pub(crate) title: String,
    pub(crate) state: &'static str,
}
impl RenameRequest {
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
        if self.title.trim().is_empty()
            || self.title.len() > 1024
            || self.title.chars().any(char::is_control)
        {
            return Err(SafeError::invalid("title"));
        }
        self.source.target.validate()
    }
    pub(crate) fn receipt(&self) -> RenameReceipt {
        RenameReceipt {
            operation_id: self.operation_id.clone(),
            source: self.source.clone(),
            session_key: self.session_key.clone(),
            title: self.title.clone(),
            state: "renamed",
        }
    }
}
type OperationEntry = (
    super::snapshot::CallerIdentity,
    RenameRequest,
    Option<Result<(), SafeError>>,
);
#[derive(Default)]
pub(crate) struct Operations(std::sync::Mutex<Vec<OperationEntry>>);
impl Operations {
    pub(crate) fn begin(
        &self,
        caller: &super::snapshot::CallerIdentity,
        request: &RenameRequest,
    ) -> Result<Option<Result<(), SafeError>>, SafeError> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("RENAME_FAILED"))?;
        if let Some((_, old, result)) = entries
            .iter()
            .find(|(owner, old, _)| owner == caller && old.operation_id == request.operation_id)
        {
            if old != request {
                return Err(super::profiles::error("RENAME_OPERATION_CONFLICT"));
            }
            return Ok(Some(result.clone().unwrap_or_else(|| {
                Err(super::profiles::error("RENAME_OUTCOME_UNKNOWN"))
            })));
        }
        if entries.len() >= 4096 {
            return Err(super::profiles::error("SOURCE_BUSY"));
        }
        entries.push((caller.clone(), request.clone(), None));
        Ok(None)
    }
    pub(crate) fn finish(
        &self,
        caller: &super::snapshot::CallerIdentity,
        request: &RenameRequest,
        result: Result<(), SafeError>,
    ) {
        if let Ok(mut entries) = self.0.lock() {
            if let Some((_, _, stored)) = entries
                .iter_mut()
                .find(|(owner, old, _)| owner == caller && old == request)
            {
                *stored = Some(result);
            }
        }
    }
}

pub(crate) fn process(
    child: &mut std::process::Child,
    authority: &super::native_projection::registry::RenameAuthority,
    title: &str,
    current: &dyn Fn() -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    use super::profiles::error;
    use std::io::{BufRead, Read, Write};
    let mut input = child.stdin.take().ok_or_else(|| error("RENAME_FAILED"))?;
    let output = child.stdout.take().ok_or_else(|| error("RENAME_FAILED"))?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    std::thread::Builder::new()
        .name("native-rename-read".into())
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
        .map_err(|_| error("RENAME_FAILED"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    exchange(
        &authority.native_id,
        title,
        &mut |message| {
            current()?;
            serde_json::to_writer(&mut input, &message).map_err(|_| error("RENAME_FAILED"))?;
            input
                .write_all(b"\n")
                .and_then(|_| input.flush())
                .map_err(|_| error("RENAME_FAILED"))
        },
        &mut |id| {
            for _ in 0..256 {
                let value = loop {
                    current()?;
                    let remaining = deadline
                        .checked_duration_since(std::time::Instant::now())
                        .ok_or_else(|| error("RENAME_FAILED"))?;
                    match receiver
                        .recv_timeout(remaining.min(std::time::Duration::from_millis(100)))
                    {
                        Ok(value) => break value,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return Err(error("RENAME_FAILED")),
                    }
                };
                if value.get("id").is_some() {
                    if value["id"] == id {
                        return Ok(value);
                    }
                    return Err(error("RENAME_FAILED"));
                }
            }
            Err(error("RENAME_FAILED"))
        },
        &|thread| authority.check_thread(thread),
    )
}

pub(crate) fn exchange(
    id: &str,
    title: &str,
    send: &mut dyn FnMut(serde_json::Value) -> Result<(), SafeError>,
    receive: &mut dyn FnMut(u64) -> Result<serde_json::Value, SafeError>,
    check: &dyn Fn(&serde_json::Value) -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    use super::profiles::error;
    use serde_json::json;
    fn result(value: serde_json::Value, id: u64) -> Result<serde_json::Value, SafeError> {
        if value["id"] != id || value.get("error").is_some() {
            return Err(super::profiles::error("RENAME_FAILED"));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| super::profiles::error("RENAME_FAILED"))
    }
    send(
        json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"cc_desk","version":env!("CARGO_PKG_VERSION")}}}),
    )?;
    result(receive(1)?, 1)?;
    send(json!({"method":"initialized"}))?;
    send(json!({"id":2,"method":"thread/read","params":{"threadId":id,"includeTurns":false}}))?;
    let before = result(receive(2)?, 2)?;
    check(&before["thread"])?;
    // 从写请求发出起，任何传输失败都不能证明 CLI 没有发布名称。
    send(json!({"id":3,"method":"thread/name/set","params":{"threadId":id,"name":title}}))
        .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?;
    result(receive(3).map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?, 3)
        .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?;
    send(json!({"id":4,"method":"thread/read","params":{"threadId":id,"includeTurns":false}}))
        .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?;
    let after = result(receive(4).map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?, 4)
        .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?;
    check(&after["thread"]).map_err(|_| error("RENAME_VERIFICATION_FAILED"))?;
    if after["thread"]["name"].as_str() != Some(title) {
        return Err(error("RENAME_VERIFICATION_FAILED"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/native_cli_rename.rs"]
mod tests;
