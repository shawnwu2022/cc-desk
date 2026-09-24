//! Pure native argv construction from one authoritative launch snapshot.
#![allow(dead_code)] // The production launch route is switched in D10/D11.

use super::environment::EnvMap;
use super::profiles::{error, Launcher};
use super::snapshot::{CallerIdentity, LaunchSnapshot};
use super::types::{CliKind, LaunchAction, LaunchRequest, RunPublicIdentity, SafeError};
use std::ffi::OsString;
use std::path::Path;

/// Borrow frozen inputs instead of re-reading profiles, PATH or the environment.
/// This is not a process handle or proof that the native CLI has started.
pub(crate) struct CliInvocation<'a> {
    snapshot: &'a LaunchSnapshot,
    args: Vec<OsString>,
}

impl std::fmt::Debug for CliInvocation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CliInvocation(<redacted>)")
    }
}

impl CliInvocation<'_> {
    pub(crate) fn program(&self) -> &Path {
        self.snapshot.program()
    }

    pub(crate) fn args(&self) -> &[OsString] {
        &self.args
    }

    pub(crate) fn cwd(&self) -> &Path {
        Path::new(&self.snapshot.request().launch_cwd)
    }

    /// Complete frozen environment, including removals already applied by D08.
    /// D10 MUST clear inheritance before applying this map, even when it is empty.
    pub(crate) fn environment(&self) -> &EnvMap {
        self.snapshot.environment()
    }

    pub(crate) fn launcher(&self) -> &Launcher {
        self.snapshot.launcher()
    }

    pub(crate) fn runner(&self) -> Option<&Path> {
        self.snapshot.runner()
    }

    pub(crate) fn owner(&self) -> &CallerIdentity {
        self.snapshot.owner()
    }

    /// A requested locator, not verified identity. Never infer it from raw argv.
    pub(crate) fn requested_session_id(&self) -> Option<&str> {
        match &self.snapshot.request().action {
            LaunchAction::ResumeId { native_session_id } => Some(native_session_id),
            _ => None,
        }
    }

    pub(crate) fn initial_identity(&self) -> RunPublicIdentity {
        RunPublicIdentity::unverified_launch(self.snapshot.request())
    }
}

/// Only the structured locator field is constrained. User-supplied raw argv is
/// not a blacklist target; new CLI syntax remains available through raw/picker.
pub(super) fn validate_locator(value: &str) -> Result<(), SafeError> {
    if value.starts_with('-') || value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(SafeError::invalid("action.nativeSessionId"));
    }
    Ok(())
}

pub(crate) fn build_invocation<'a>(
    request: &LaunchRequest,
    snapshot: &'a LaunchSnapshot,
) -> Result<CliInvocation<'a>, SafeError> {
    // Compare all fields, including dimensions and generation. Never combine
    // new user intent with an older profile/environment/owner snapshot.
    if request != snapshot.request() {
        return Err(error("REQUEST_SNAPSHOT_MISMATCH"));
    }
    request.validate()?;
    if let Some(args) = snapshot.raw_args() {
        return Ok(CliInvocation {
            snapshot,
            args: args.to_vec(),
        });
    }
    if snapshot
        .legacy_default_args()
        .is_some_and(|text| !text.is_empty())
    {
        return Err(error("LEGACY_ARGUMENTS_REQUIRE_MIGRATION"));
    }
    let mut args = match request.cli {
        CliKind::Claude => super::claude::action_args(snapshot)?,
        CliKind::Codex => super::codex::action_args(snapshot)?,
        CliKind::Shell => super::shell::action_args(snapshot)?,
    };
    // No quoting, parsing or deduplication. Explicit caller flags and positionals
    // retain native parser semantics; raw provides exact full-command ordering.
    args.extend_from_slice(snapshot.default_args());
    if let Some(plugin) = snapshot.observer_plugin() {
        args.push("--plugin-dir".into());
        args.push(plugin.as_os_str().to_owned());
    }
    args.extend_from_slice(snapshot.extra_args());
    Ok(CliInvocation { snapshot, args })
}
