//! opencode action 映射。不注入 model/agent/TUI 默认值；picker 无 CLI flag（spec §2）。
use super::invocation::validate_locator;
use super::profiles::error;
use super::snapshot::LaunchSnapshot;
use super::types::{LaunchAction, SafeError};
use std::ffi::OsString;

pub(super) fn action_args(snapshot: &LaunchSnapshot) -> Result<Vec<OsString>, SafeError> {
    match &snapshot.request().action {
        LaunchAction::New => Ok(Vec::new()),
        LaunchAction::ResumeId { native_session_id } => {
            validate_locator(native_session_id)?;
            // opencode resume 忽略 launch cwd（实测改用会话记录目录）；
            // argv 只表达意图，cwd 语义由 effective_cwd 保持 Unknown 承接。
            Ok(vec!["-s".into(), native_session_id.into()])
        }
        // ResumePicker 无 picker flag；无参启动是"新建"，冒充 picker 即撒谎。
        LaunchAction::ResumePicker { .. } => Err(error("UNSUPPORTED_ACTION")),
        // opencode 无 CLI 归档概念；Unarchive 只属于 Codex。
        LaunchAction::Unarchive { .. } => Err(error("UNSUPPORTED_ACTION")),
        // Raw 由公共 builder 在分发前整体接管（与 claude.rs 同约定）。
        LaunchAction::Raw { .. } => Err(error("UNSUPPORTED_ACTION")),
    }
}
