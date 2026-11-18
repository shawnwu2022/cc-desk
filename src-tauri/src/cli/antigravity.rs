//! Antigravity 官方 agy TUI：只映射已核实的启动动作，不注入权限或模型参数。
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
            Ok(vec!["--conversation".into(), native_session_id.into()])
        }
        // TUI 的 /resume 不是启动参数，不能用 --continue 冒充选择器。
        _ => Err(error("UNSUPPORTED_ACTION")),
    }
}
