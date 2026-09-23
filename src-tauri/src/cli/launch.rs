//! Single-execution coordination around the authoritative snapshot and owned resources.
#![allow(dead_code)] // IPC awaits actual document-lifetime binding and stream integration.

use super::profiles::error;
use super::run_registry::{LaunchFailure, LaunchStatus, Reservation, RunRegistry};
use super::snapshot::{CallerIdentity, LaunchSnapshot};
use super::types::{LaunchRequest, SafeError};
use std::sync::Arc;

pub(crate) struct LaunchCoordinator<R> {
    registry: Arc<RunRegistry<R>>,
}

impl<R> LaunchCoordinator<R> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            registry: Arc::new(RunRegistry::new(capacity)),
        }
    }

    pub(crate) fn registry(&self) -> &Arc<RunRegistry<R>> {
        &self.registry
    }

    /// P prepares immutable inputs only; C registers the result/output route;
    /// S returns owned resources or an error with no surviving unowned process.
    /// No callback runs while the registry mutex is held. A panic after begin is
    /// indeterminate, never permission to replay the operation automatically.
    pub(crate) fn start<P, C, S>(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
        prepare: P,
        connect: C,
        spawn: S,
    ) -> Result<LaunchStatus, SafeError>
    where
        P: FnOnce() -> Result<LaunchSnapshot, SafeError>,
        C: FnOnce(&LaunchStatus) -> Result<(), SafeError>,
        S: FnOnce(&LaunchSnapshot) -> Result<R, SafeError>,
    {
        // A retry must not need a profile that may have been edited or deleted.
        if let Some(status) = self.registry.existing(caller, request)? {
            return Ok(status);
        }
        request.validate()?;
        let snapshot = match prepare() {
            Ok(snapshot) => snapshot,
            Err(failure) => {
                // Another caller may have won while this caller was preparing.
                return match self.registry.existing(caller, request)? {
                    Some(status) => Ok(status),
                    None => Err(failure),
                };
            }
        };
        if snapshot.request() != request || snapshot.owner() != caller {
            return Err(error("REQUEST_SNAPSHOT_MISMATCH"));
        }
        let mut ticket = match self.registry.reserve(caller, request)? {
            Reservation::Existing(status) => return Ok(status),
            Reservation::New(ticket) => ticket,
        };
        if connect(&ticket.status()).is_err() {
            return Ok(ticket.fail(LaunchFailure::RouteUnavailable));
        }
        if !ticket.begin() {
            return Ok(ticket.status());
        }
        match spawn(&snapshot) {
            Ok(resource) => Ok(ticket.complete(resource)),
            Err(_) => Ok(ticket.fail(LaunchFailure::ProcessStartFailed)),
        }
    }
}
