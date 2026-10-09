//! A shell has no Desk-owned native agent session or automatic startup flags.
#![allow(dead_code)] // Consumed by the staged D10 platform launcher.

use super::profiles::error;
use super::snapshot::LaunchSnapshot;
use super::types::{LaunchAction, SafeError};
use std::ffi::OsString;

pub(super) fn action_args(snapshot: &LaunchSnapshot) -> Result<Vec<OsString>, SafeError> {
    match snapshot.request().action {
        LaunchAction::New => Ok(Vec::new()),
        _ => Err(error("UNSUPPORTED_ACTION")),
    }
}
