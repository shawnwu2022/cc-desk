//! Complete PTY resource interface for D11. Behavioral scaffold.
#![allow(dead_code)]

use super::launch::ProcessLaunchSpec;
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use portable_pty::{CommandBuilder, ExitStatus, PtyPair, PtySize};
use std::io::{self, Read, Write};

pub(crate) struct OwnedPty;

impl std::fmt::Debug for OwnedPty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwnedPty(<redacted>)")
    }
}

impl OwnedPty {
    pub(crate) fn spawn(_spec: &ProcessLaunchSpec, _size: PtySize) -> Result<Self, SafeError> {
        Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn attach_and_spawn(
        _pair: PtyPair,
        _command: CommandBuilder,
    ) -> Result<Self, SafeError> {
        Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn take_reader(&self) -> Result<Box<dyn Read + Send>, SafeError> {
        Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn with_writer<T>(
        &self,
        _operation: impl FnOnce(&mut (dyn Write + Send)) -> io::Result<T>,
    ) -> io::Result<T> {
        Err(io::Error::other("OWNED_PTY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn try_wait(&self) -> Result<Option<ExitStatus>, SafeError> {
        Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn wait(&self) -> Result<ExitStatus, SafeError> {
        Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn terminate_root(&self) -> Result<(), SafeError> {
        Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn resize(&self, _size: PtySize) -> Result<(), SafeError> {
        Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
    }
}
