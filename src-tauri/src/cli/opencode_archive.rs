//! OpenCode 可逆原生归档。只调用嵌入的官方 Session 元数据子集，不删除会话。
use super::{
    native_projection::wire::SourceRef,
    profiles::error,
    types::{CliKind, SafeError},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) operation_id: String,
    pub(crate) source: SourceRef,
    pub(crate) session_key: String,
    pub(crate) archived: bool,
    pub(crate) acknowledge_recoverable: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Receipt {
    operation_id: String,
    source: SourceRef,
    session_key: String,
    archived: bool,
    state: &'static str,
}
impl Request {
    pub(crate) fn validate(&self) -> Result<(), SafeError> {
        if uuid::Uuid::parse_str(&self.operation_id).is_err()
            || !self.acknowledge_recoverable
            || self.session_key.is_empty()
            || self.session_key.len() > 16384
        {
            return Err(SafeError::invalid("archive"));
        }
        if self.source.cli != CliKind::Opencode {
            return Err(error("OPENCODE_ARCHIVE_UNSUPPORTED"));
        }
        self.source.target.validate()
    }
    pub(crate) fn receipt(&self) -> Receipt {
        Receipt {
            operation_id: self.operation_id.clone(),
            source: self.source.clone(),
            session_key: self.session_key.clone(),
            archived: self.archived,
            state: "archived",
        }
    }
}
type OperationEntry = (
    super::snapshot::CallerIdentity,
    Request,
    Option<Result<(), SafeError>>,
);
#[derive(Default)]
pub(crate) struct Operations(std::sync::Mutex<Vec<OperationEntry>>);
impl Operations {
    pub(crate) fn release_revoked(
        &self,
        authorized: impl Fn(&super::snapshot::CallerIdentity) -> bool,
    ) -> Result<(), SafeError> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("ARCHIVE_SOURCE_UNAVAILABLE"))?;
        // 活跃文档的确定/未知结果仍是去重凭据；在途记录不能因失权提前释放。
        entries.retain(|(owner, _, result)| result.is_none() || authorized(owner));
        Ok(())
    }
    pub(crate) fn begin(
        &self,
        caller: &super::snapshot::CallerIdentity,
        request: &Request,
    ) -> Result<Option<Result<(), SafeError>>, SafeError> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| super::profiles::error("ARCHIVE_SOURCE_UNAVAILABLE"))?;
        if let Some((_, old, result)) = entries
            .iter()
            .find(|(owner, old, _)| owner == caller && old.operation_id == request.operation_id)
        {
            if old != request {
                return Err(super::profiles::error("ARCHIVE_OPERATION_CONFLICT"));
            }
            return Ok(Some(result.clone().unwrap_or_else(|| {
                Err(super::profiles::error("ARCHIVE_OUTCOME_UNKNOWN"))
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
        request: &Request,
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

#[cfg(windows)]
#[path = "opencode_archive_windows.rs"]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{PinnedSource, Prepared};

#[cfg(windows)]
pub(crate) fn before_deadline(
    expires: std::time::Instant,
    check: impl FnOnce() -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    if std::time::Instant::now() >= expires {
        return Err(error("ARCHIVE_SOURCE_UNAVAILABLE"));
    }
    check()?;
    if std::time::Instant::now() >= expires {
        return Err(error("ARCHIVE_SOURCE_UNAVAILABLE"));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn owned(
    timeout: std::time::Duration,
    work: impl FnOnce() -> Result<(), SafeError> + Send + 'static,
) -> Result<(), SafeError> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("opencode-archive-owner".into())
        .spawn(move || {
            let result = work();
            let _ = sender.send(result);
        })
        .map_err(|_| error("SDK_HELPER_UNAVAILABLE"))?;
    receiver
        .recv_timeout(timeout)
        .map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?
}

#[cfg(all(test, windows))]
#[path = "../tests/native_cli_opencode_archive.rs"]
mod tests;
