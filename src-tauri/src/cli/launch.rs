//! Launch coordination contract. No IPC or old PTY route is exposed by this scaffold.
#![allow(dead_code)]

use super::profiles::error;
use super::run_registry::{LaunchStatus, RunRegistry};
use super::snapshot::{CallerIdentity, LaunchSnapshot};
use super::types::{LaunchRequest, SafeError};
use std::sync::Arc;

pub(crate) struct LaunchCoordinator<R> {
    registry: Arc<RunRegistry<R>>,
}

impl<R> LaunchCoordinator<R> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self { registry: Arc::new(RunRegistry::new(capacity)) }
    }

    pub(crate) fn registry(&self) -> &Arc<RunRegistry<R>> {
        &self.registry
    }

    pub(crate) fn start<P, C, S>(
        &self,
        _caller: &CallerIdentity,
        _request: &LaunchRequest,
        _prepare: P,
        _connect: C,
        _spawn: S,
    ) -> Result<LaunchStatus, SafeError>
    where
        P: FnOnce() -> Result<LaunchSnapshot, SafeError>,
        C: FnOnce(&LaunchStatus) -> Result<(), SafeError>,
        S: FnOnce(&LaunchSnapshot) -> Result<R, SafeError>,
    {
        Err(error("REGISTRY_NOT_IMPLEMENTED"))
    }
}
