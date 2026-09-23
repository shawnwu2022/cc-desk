//! Owned output routes around the single-execution coordinator.
#![allow(dead_code)] // Live IPC still awaits document-lifetime authentication.

use super::invocation::build_invocation;
use super::launch::LaunchCoordinator;
use super::profiles::error;
use super::run_registry::LaunchStatus;
use super::snapshot::{CallerIdentity, LaunchSnapshot};
use super::types::{LaunchRequest, SafeError};
use crate::platform::launch::resolve_process;
use crate::platform::owned_pty::OwnedPty;
use portable_pty::PtySize;
use std::cell::RefCell;
use std::io::{self, Read};
use std::sync::Arc;

pub(crate) struct RoutedResource<P, L> {
    pub(crate) process: P,
    route: Arc<L>,
}

impl<P, L> RoutedResource<P, L> {
    /// Pin only the guarded route in a reader; do not keep its master PTY alive.
    pub(crate) fn route(&self) -> Arc<L> {
        self.route.clone()
    }
}

impl<P, L> std::fmt::Debug for RoutedResource<P, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RoutedResource(<redacted>)")
    }
}

impl<P, L> LaunchCoordinator<RoutedResource<P, L>> {
    /// A successful connection returns its rollback/ownership lease. Keep one
    /// reference in this outer scope until start returns or unwinds: the inner
    /// reservation must publish its terminal outcome before the lease is dropped.
    /// Connect must clean up its own partial work on Err/panic; spawn must return
    /// an owned process or clean up any process it created before returning Err.
    pub(crate) fn start_routed<F, C, S>(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
        prepare: F,
        connect: C,
        spawn: S,
    ) -> Result<LaunchStatus, SafeError>
    where
        F: FnOnce() -> Result<LaunchSnapshot, SafeError>,
        C: FnOnce(&LaunchStatus) -> Result<L, SafeError>,
        S: FnOnce(&LaunchSnapshot) -> Result<P, SafeError>,
    {
        let lease: RefCell<Option<Arc<L>>> = RefCell::new(None);
        self.start(
            caller,
            request,
            prepare,
            |status| {
                let connected = connect(status)?;
                *lease.borrow_mut() = Some(Arc::new(connected));
                Ok(())
            },
            |snapshot| {
                // Release the RefCell borrow before calling external code.
                let route = lease
                    .borrow()
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| error("ROUTE_NOT_READY"))?;
                let process = spawn(snapshot)?;
                Ok(RoutedResource { process, route })
            },
        )
    }
}

impl<L> LaunchCoordinator<RoutedResource<OwnedPty, L>> {
    pub(crate) fn start_pty<F, C>(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
        prepare: F,
        connect: C,
    ) -> Result<LaunchStatus, SafeError>
    where
        F: FnOnce() -> Result<LaunchSnapshot, SafeError>,
        C: FnOnce(&LaunchStatus) -> Result<L, SafeError>,
    {
        self.start_routed(caller, request, prepare, connect, |snapshot| {
            let invocation = build_invocation(snapshot.request(), snapshot)?;
            let spec = resolve_process(&invocation)?;
            OwnedPty::spawn(
                &spec,
                PtySize {
                    rows: snapshot.request().rows,
                    cols: snapshot.request().cols,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
        })
    }
}

/// The reader pins the route, not the resource/master PTY. Pinning the whole
/// resource here would keep ConPTY open while the reader waits for its EOF.
pub(crate) struct RoutedReader<L> {
    reader: Box<dyn Read + Send>,
    _route: Arc<L>,
}

impl<L> Read for RoutedReader<L> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buffer)
    }
}

impl<L> RoutedResource<OwnedPty, L> {
    pub(crate) fn take_reader(&self) -> Result<RoutedReader<L>, SafeError> {
        Ok(RoutedReader {
            reader: self.process.take_reader()?,
            _route: self.route.clone(),
        })
    }
}
