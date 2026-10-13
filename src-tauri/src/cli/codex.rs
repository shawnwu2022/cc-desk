//! Codex action mapping. Do not inject model, sandbox, approval or TUI defaults.
#![allow(dead_code)] // Consumed by the staged D10 platform launcher.

use super::invocation::validate_locator;
use super::profiles::error;
use super::snapshot::LaunchSnapshot;
use super::types::{LaunchAction, ResumeScope, SafeError};
use std::ffi::OsString;

pub(super) fn action_args(snapshot: &LaunchSnapshot) -> Result<Vec<OsString>, SafeError> {
    match &snapshot.request().action {
        LaunchAction::New => Ok(Vec::new()),
        LaunchAction::ResumePicker {
            scope: ResumeScope::CurrentProject,
        } => Ok(vec!["resume".into()]),
        LaunchAction::ResumePicker {
            scope: ResumeScope::All,
        } => Ok(vec!["resume".into(), "--all".into()]),
        LaunchAction::ResumeId { native_session_id } => {
            validate_locator(native_session_id)?;
            Ok(vec!["resume".into(), native_session_id.into()])
        }
        // codex 拒绝 resume 自己归档的会话（要求先 unarchive），因此恢复 CLI 归档
        // 会话必须走 codex 自己的 unarchive 子命令，而不是搬动 ~/.codex 下的文件。
        LaunchAction::Unarchive { native_session_id } => {
            validate_locator(native_session_id)?;
            Ok(vec!["unarchive".into(), native_session_id.into()])
        }
        LaunchAction::Raw { .. } => Err(error("UNSUPPORTED_ACTION")),
    }
}
