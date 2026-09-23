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
        LaunchAction::Raw { .. } => Err(error("UNSUPPORTED_ACTION")),
    }
}
