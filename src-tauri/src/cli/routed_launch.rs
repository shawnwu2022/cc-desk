//! D11 owned-route interface. Behavioral scaffold; not connected to live IPC.
#![allow(dead_code)]

use super::launch::LaunchCoordinator;
use super::profiles::error;
use super::run_registry::LaunchStatus;
use super::snapshot::{CallerIdentity, LaunchSnapshot};
use super::types::{LaunchRequest, SafeError};
use crate::platform::owned_pty::OwnedPty;
use std::io::{self, Read};
use std::sync::Arc;

pub(crate) struct RoutedResource<P, L> {
    pub(crate) process: P,
    route: Option<Arc<L>>,
}

impl<P, L> std::fmt::Debug for RoutedResource<P, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RoutedResource(<redacted>)")
    }
}

impl<P, L> LaunchCoordinator<RoutedResource<P, L>> {
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
        self.start(
            caller,
            request,
            prepare,
            |status| connect(status).map(drop),
            |snapshot| {
                spawn(snapshot).map(|process| RoutedResource {
                    process,
                    route: None,
                })
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
        self.start_routed(caller, request, prepare, connect, |_| {
            Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
        })
    }
}

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
        Err(error("OWNED_PTY_NOT_IMPLEMENTED"))
    }
}
