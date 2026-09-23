//! Per-document callback leases; dispatch acceptance is not renderer consumption.
#![allow(dead_code)] // Product streaming and parser acknowledgments are later integration.

use super::profiles::error;
use super::types::SafeError;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::http::HeaderMap;
use tauri::ipc::{Channel, IpcResponse};

pub(crate) const CHANNEL_HEADER: &str = "x-cc-desk-output-channel";
pub(crate) type AuthorityCheck = Box<dyn Fn() -> Result<(), SafeError> + Send + Sync>;

pub(crate) fn parse_channel(headers: &HeaderMap) -> Result<u32, SafeError> {
    let invalid = || SafeError::invalid("outputChannel");
    let mut values = headers.get_all(CHANNEL_HEADER).iter();
    let supplied = values.next().ok_or_else(invalid)?;
    if values.next().is_some() || supplied.as_bytes().len() > 22 {
        return Err(invalid());
    }
    let number = supplied
        .to_str()
        .map_err(|_| invalid())?
        .strip_prefix("__CHANNEL__:")
        .ok_or_else(invalid)?;
    if number.is_empty()
        || !number.bytes().all(|byte| byte.is_ascii_digit())
        || (number != "0" && number.starts_with('0'))
    {
        return Err(invalid());
    }
    number.parse().map_err(|_| invalid())
}

pub(crate) struct OutputRoutes {
    capacity: usize,
    active: Arc<Mutex<HashSet<u32>>>,
}

struct CallbackLease {
    id: u32,
    active: Arc<Mutex<HashSet<u32>>>,
}

impl Drop for CallbackLease {
    fn drop(&mut self) {
        self.active.lock().remove(&self.id);
    }
}

/// Never expose the underlying Channel: every dispatch must pass the same guard.
/// Keep the lease until the last route/reader owner is dropped, even after loss.
pub(crate) struct OutputRoute<T> {
    channel: Mutex<Option<Channel<T>>>,
    authorize: AuthorityCheck,
    _lease: CallbackLease,
}

impl<T> std::fmt::Debug for OutputRoute<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OutputRoute(<redacted>)")
    }
}

impl OutputRoutes {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub(crate) fn bind<T>(
        &self,
        id: u32,
        authorize: AuthorityCheck,
        create: impl FnOnce() -> Result<Channel<T>, SafeError>,
    ) -> Result<OutputRoute<T>, SafeError> {
        authorize()?;
        let lease = {
            let mut active = self.active.lock();
            if active.contains(&id) {
                return Err(error("OUTPUT_CHANNEL_BUSY"));
            }
            if active.len() >= self.capacity {
                return Err(error("OUTPUT_ROUTE_CAPACITY"));
            }
            active.insert(id);
            CallbackLease {
                id,
                active: self.active.clone(),
            }
        };
        // Construct only after winning the callback lease. Dropping a rejected
        // duplicate Tauri Channel would otherwise end the original JS callback.
        // Both fallible work and RAII rollback run outside the table lock.
        let channel = create()?;
        authorize()?;
        Ok(OutputRoute {
            channel: Mutex::new(Some(channel)),
            authorize,
            _lease: lease,
        })
    }
}

impl<T: IpcResponse> OutputRoute<T> {
    pub(crate) fn send(&self, value: T) -> Result<(), SafeError> {
        // Declare the pending owner before the lock guard so unwinding drops
        // the guard first. Take the Channel out before external code: a panic
        // leaves the route closed, rather than permitting a later frame.
        let mut pending = None;
        let mut slot = self.channel.lock();
        std::mem::swap(&mut *slot, &mut pending);
        let channel = pending
            .as_ref()
            .ok_or_else(|| error("OUTPUT_ROUTE_CLOSED"))?;
        let result = (self.authorize)()
            .and_then(|()| channel.send(value).map_err(|_| error("OUTPUT_ROUTE_LOST")));
        if result.is_ok() {
            *slot = pending.take();
        }
        drop(slot);
        // Failure drops the Channel here, outside both route and table locks.
        // Panic propagates unchanged; no retry or replacement is authorized.
        result
    }
}
