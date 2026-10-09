//! Claude action mapping only. The native CLI owns sessions and permissions.
#![allow(dead_code)] // Consumed by the staged D10 platform launcher.

use super::invocation::validate_locator;
use super::profiles::error;
use super::snapshot::LaunchSnapshot;
use super::types::{LaunchAction, ResumeScope, SafeError};
use std::ffi::OsString;

pub(super) fn action_args(snapshot: &LaunchSnapshot) -> Result<Vec<OsString>, SafeError> {
    let action_args = match &snapshot.request().action {
        LaunchAction::New => Vec::new(),
        LaunchAction::ResumePicker {
            scope: ResumeScope::CurrentProject,
        } => vec!["--resume".into()],
        LaunchAction::ResumeId { native_session_id } => {
            validate_locator(native_session_id)?;
            vec!["--resume".into(), native_session_id.into()]
        }
        // No verified all-project flag; never substitute Codex's --all.
        // Raw actions are handled before dispatch in the common builder.
        _ => return Err(error("UNSUPPORTED_ACTION")),
    };
    let mut args = Vec::new();
    if snapshot.skip_permissions() == Some(true) {
        args.push("--dangerously-skip-permissions".into());
    }
    args.extend(action_args);
    Ok(args)
}
