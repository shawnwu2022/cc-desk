//! Application-owned launch composition. No wire caller may install a supervisor.
#![allow(dead_code)]
use super::environment::EnvMap;
use super::launch::LaunchCoordinator;
use super::output_route::OutputRoute;
use super::profiles::error;
use super::routed_launch::RoutedResource;
use super::run_registry::{LaunchStatus, RunKey, RunRegistry};
use super::snapshot::{CallerIdentity, LaunchSnapshot};
use super::storage::WorkspaceRepository;
use super::types::{LaunchRequest, SafeError};
use crate::platform::owned_pty::OwnedPty;
use portable_pty::PtySize;
use serde_json::Value;
use std::io::{self, Write};
use std::sync::Arc;

pub(crate) struct FrozenPty {
    pub(crate) pty: OwnedPty,
    pub(crate) snapshot: Arc<LaunchSnapshot>,
}
pub(crate) type NativeRun = RoutedResource<FrozenPty, OutputRoute<Value>>;

/// D14/D15 supply a backend consumer. It must retain/reap its owned run and
/// implement bounded output/drain policy. This interface is never deserialized.
pub(crate) trait RunSupervisor: Send + Sync {
    fn adopt(&self, run: &RunKey, resource: Arc<NativeRun>) -> Result<(), SafeError>;
}

pub(crate) struct LaunchService {
    coordinator: LaunchCoordinator<NativeRun>,
    repository: WorkspaceRepository,
    inherited: Option<EnvMap>,
    supervisor: Option<Arc<dyn RunSupervisor>>,
}
impl LaunchService {
    pub(crate) fn new(
        repository: WorkspaceRepository,
        inherited: Option<EnvMap>,
        supervisor: Option<Arc<dyn RunSupervisor>>,
    ) -> Self {
        Self {
            coordinator: LaunchCoordinator::new(4096),
            repository,
            inherited,
            supervisor,
        }
    }
    pub(crate) fn registry(&self) -> &Arc<RunRegistry<NativeRun>> {
        self.coordinator.registry()
    }
    pub(crate) fn start(
        &self,
        _caller: &CallerIdentity,
        _request: &LaunchRequest,
        _connect: impl FnOnce(&LaunchStatus) -> Result<OutputRoute<Value>, SafeError>,
    ) -> Result<LaunchStatus, SafeError> {
        Err(error("LAUNCH_SERVICE_NOT_IMPLEMENTED"))
    }
    pub(crate) fn access(
        self: &Arc<Self>,
        _caller: &CallerIdentity,
        _run: &RunKey,
    ) -> Result<RunAccess, SafeError> {
        Err(error("RUN_ACCESS_NOT_IMPLEMENTED"))
    }
}

pub(crate) struct RunAccess;
impl std::fmt::Debug for RunAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunAccess(<redacted>)")
    }
}
impl RunAccess {
    pub(crate) fn with_writer<T>(
        &self,
        _operation: impl FnOnce(&mut (dyn Write + Send)) -> io::Result<T>,
    ) -> Result<T, SafeError> {
        Err(error("RUN_ACCESS_NOT_IMPLEMENTED"))
    }
    pub(crate) fn resize(&self, _size: PtySize) -> Result<(), SafeError> {
        Err(error("RUN_ACCESS_NOT_IMPLEMENTED"))
    }
    pub(crate) fn terminate_root(&self) -> Result<(), SafeError> {
        Err(error("RUN_ACCESS_NOT_IMPLEMENTED"))
    }
    pub(crate) fn snapshot(&self) -> Result<Arc<LaunchSnapshot>, SafeError> {
        Err(error("RUN_ACCESS_NOT_IMPLEMENTED"))
    }
}
