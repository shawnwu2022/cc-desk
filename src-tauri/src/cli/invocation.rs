//! Native invocation contract. Behavioral implementation follows observed RED.
#![allow(dead_code)]

use super::environment::EnvMap;
use super::profiles::{error, Launcher};
use super::snapshot::{CallerIdentity, LaunchSnapshot};
use super::types::{LaunchRequest, RunPublicIdentity, SafeError};
use std::ffi::OsString;
use std::path::Path;

/// Borrow the one authoritative snapshot; never serialize or log its values.
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

    /// Complete frozen environment. D10 must clear inherited environment first.
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

    pub(crate) fn requested_session_id(&self) -> Option<&str> {
        None
    }

    pub(crate) fn initial_identity(&self) -> RunPublicIdentity {
        panic!("INVOCATION_NOT_IMPLEMENTED")
    }
}

pub(crate) fn build_invocation<'a>(
    _request: &LaunchRequest,
    _snapshot: &'a LaunchSnapshot,
) -> Result<CliInvocation<'a>, SafeError> {
    Err(error("INVOCATION_NOT_IMPLEMENTED"))
}
