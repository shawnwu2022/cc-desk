//! Generic process launch boundary. Behavioral implementation follows observed RED.
#![allow(dead_code)]

use crate::cli::environment::EnvMap;
use crate::cli::invocation::CliInvocation;
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) struct ProcessLaunchSpec {
    program: PathBuf,
    args: Vec<OsString>,
    cwd: PathBuf,
    environment: EnvMap,
}

impl std::fmt::Debug for ProcessLaunchSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProcessLaunchSpec(<redacted>)")
    }
}

impl ProcessLaunchSpec {
    pub(crate) fn command(&self) -> Result<CommandBuilder, SafeError> {
        Err(error("PLATFORM_LAUNCH_NOT_IMPLEMENTED"))
    }
}

pub(crate) struct SpawnedProcess {
    pub(crate) master: Box<dyn MasterPty + Send>,
    pub(crate) child: Box<dyn Child + Send + Sync>,
}

pub(crate) fn resolve_process(_invocation: &CliInvocation<'_>) -> Result<ProcessLaunchSpec, SafeError> {
    Err(error("PLATFORM_LAUNCH_NOT_IMPLEMENTED"))
}

pub(crate) fn spawn_process(_spec: &ProcessLaunchSpec, _size: PtySize) -> Result<SpawnedProcess, SafeError> {
    Err(error("PLATFORM_LAUNCH_NOT_IMPLEMENTED"))
}
