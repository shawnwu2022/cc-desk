//! Pure native argv construction from one authoritative launch snapshot.
#![allow(dead_code)] // The production launch route is switched in D10/D11.

use super::environment::EnvMap;
use super::launch_recipe::LaunchRecipe;
use super::profiles::{error, Launcher};
use super::snapshot::{CallerIdentity, LaunchSnapshot};
use super::types::{LaunchAction, LaunchRequest, RunPublicIdentity, SafeError};
use std::ffi::OsString;
use std::path::Path;

/// Borrow frozen inputs instead of re-reading profiles, PATH or the environment.
/// This is not a process handle or proof that the native CLI has started.
pub(crate) struct CliInvocation<'a> {
    snapshot: &'a LaunchSnapshot,
    args: Vec<OsString>,
    recipe: LaunchRecipe<'a>,
}

impl std::fmt::Debug for CliInvocation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CliInvocation(<redacted>)")
    }
}

impl CliInvocation<'_> {
    pub(crate) fn program(&self) -> &Path {
        self.recipe.program()
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
        self.recipe.runner()
    }

    pub(crate) fn recipe(&self) -> &LaunchRecipe<'_> {
        &self.recipe
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
    let recipe = LaunchRecipe::from_snapshot(snapshot)?;
    let args = recipe.arguments(request)?;
    Ok(CliInvocation {
        snapshot,
        args,
        recipe,
    })
}
