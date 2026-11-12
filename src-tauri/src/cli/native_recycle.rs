//! 可恢复移除是独立操作：只搬主转录原件，不清空恢复区，不调用永久删除。
#[cfg(windows)]
use super::native_projection::scoped_fs::Root;
use super::{
    native_projection::wire::SourceRef,
    profiles::error,
    types::{CliKind, SafeError},
};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub source: SourceRef,
    pub action: String,
    pub operation_id: Option<String>,
    pub session_key: Option<String>,
    pub acknowledge_recoverable: bool,
}
impl Request {
    pub(crate) fn validate(&self) -> Result<(), SafeError> {
        self.source.target.validate()?;
        if self.source.cli != CliKind::Claude {
            return Err(error("RECYCLE_UNSUPPORTED"));
        }
        match self.action.as_str() {
            "list"
                if self.operation_id.is_none()
                    && self.session_key.is_none()
                    && !self.acknowledge_recoverable =>
            {
                Ok(())
            }
            "remove" | "restore" if self.acknowledge_recoverable => {
                let id = self
                    .operation_id
                    .as_ref()
                    .ok_or_else(|| error("RECYCLE_INVALID_REQUEST"))?;
                if uuid::Uuid::parse_str(id).is_err() {
                    return Err(error("RECYCLE_INVALID_REQUEST"));
                }
                if self.action == "remove" {
                    if self
                        .session_key
                        .as_ref()
                        .is_none_or(|s| s.is_empty() || s.len() > 16384)
                    {
                        return Err(error("RECYCLE_INVALID_REQUEST"));
                    }
                } else if self.session_key.is_some() {
                    return Err(error("RECYCLE_INVALID_REQUEST"));
                }
                Ok(())
            }
            _ => Err(error("RECYCLE_INVALID_REQUEST")),
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Record {
    pub operation_id: String,
    pub native_session_id: String,
    pub project_path: String,
    pub state: &'static str,
    pub original_occupied: bool,
    pub scope: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Response {
    pub records: Vec<Record>,
    pub unavailable_count: u32,
}
#[cfg(windows)]
pub(crate) struct Scope {
    pub root: Root,
    pub project_paths: Vec<std::path::PathBuf>,
    pub check: Arc<dyn Fn() -> Result<(), SafeError> + Send + Sync>,
}
#[cfg(windows)]
impl Scope {
    pub(crate) fn current(&self) -> Result<(), SafeError> {
        (self.check)()?;
        self.root.source_current().map_err(error)
    }
    pub(crate) fn permits(&self, cwd: &str) -> bool {
        // 日志和 transcript 的 cwd 是不可信文本；只比较后台已验证的项目拼写，
        // 不能为判定归属先打开该路径（尤其是 UNC / 外部绝对路径）。
        self.project_paths
            .iter()
            .any(|path| path == std::path::Path::new(cwd))
    }
}
#[cfg(windows)]
#[path = "native_recycle_windows.rs"]
mod platform;
#[cfg(windows)]
pub(crate) use platform::Target;
#[cfg(windows)]
pub(crate) use platform::{list, prepare_target, remove, restore};
#[cfg(test)]
#[cfg(windows)]
#[path = "../tests/native_cli_recycle.rs"]
mod tests;
