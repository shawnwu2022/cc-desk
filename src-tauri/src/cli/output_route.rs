//! Per-document callback leases; dispatch acceptance is not renderer consumption.
#![allow(dead_code)] // Product streaming and parser acknowledgments are later integration.

use super::profiles::error;
use super::types::SafeError;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
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

trait Revoke: Send + Sync {
    fn revoke(&self);
}

enum Entry {
    Reserved,
    Bound(Weak<dyn Revoke>),
}

#[derive(Default)]
struct RouteTable {
    revoked: bool,
    entries: HashMap<u32, Entry>,
}

pub(crate) struct OutputRoutes {
    capacity: usize,
    table: Arc<Mutex<RouteTable>>,
}

struct CallbackLease {
    id: u32,
    table: Arc<Mutex<RouteTable>>,
}
impl Drop for CallbackLease {
    fn drop(&mut self) {
        self.table.lock().entries.remove(&self.id);
    }
}

struct Active<T> {
    channel: Channel<T>,
    authorize: AuthorityCheck,
}
struct RouteCore<T> {
    active: Mutex<Option<Active<T>>>,
    revoked: AtomicBool,
    // Must outlive Active and every temporary revocation reference. Reusing an
    // ID before the native Channel destructor runs can end the next callback.
    _lease: CallbackLease,
}

/// No raw Channel escape hatch. A reader retains the lease, not native owners
/// after revocation. Only D14/D15 may decide when the run itself can retire.
pub(crate) struct OutputRoute<T> {
    core: Arc<RouteCore<T>>,
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
            table: Arc::new(Mutex::new(RouteTable::default())),
        }
    }

    pub(crate) fn bind<T: Send + Sync + 'static>(
        &self,
        id: u32,
        authorize: AuthorityCheck,
        create: impl FnOnce() -> Result<Channel<T>, SafeError>,
    ) -> Result<OutputRoute<T>, SafeError> {
        authorize()?;
        let lease = {
            let mut table = self.table.lock();
            if table.revoked {
                return Err(error("FORBIDDEN"));
            }
            if table.entries.contains_key(&id) {
                return Err(error("OUTPUT_CHANNEL_BUSY"));
            }
            if table.entries.len() >= self.capacity {
                return Err(error("OUTPUT_ROUTE_CAPACITY"));
            }
            table.entries.insert(id, Entry::Reserved);
            CallbackLease {
                id,
                table: self.table.clone(),
            }
        };
        // Reserve before construction; rejecting a duplicate must not construct
        // then drop a Channel which would end the original renderer callback.
        let channel = create()?;
        authorize()?;
        let core = Arc::new(RouteCore {
            active: Mutex::new(Some(Active { channel, authorize })),
            revoked: AtomicBool::new(false),
            _lease: lease,
        });
        let erased: Arc<dyn Revoke> = core.clone();
        {
            let mut table = self.table.lock();
            if table.revoked {
                return Err(error("FORBIDDEN"));
            }
            table
                .entries
                .insert(id, Entry::Bound(Arc::downgrade(&erased)));
        }
        Ok(OutputRoute { core })
    }

    pub(crate) fn revoke(&self) {
        let routes: Vec<_> = {
            let mut table = self.table.lock();
            table.revoked = true;
            table
                .entries
                .values()
                .filter_map(|entry| match entry {
                    Entry::Reserved => None,
                    Entry::Bound(route) => route.upgrade(),
                })
                .collect()
        };
        // Never run native destructors or wait for sends under the table lock.
        for route in routes {
            route.revoke();
        }
    }
}

impl<T: Send + Sync> Revoke for RouteCore<T> {
    fn revoke(&self) {
        self.revoked.store(true, Ordering::SeqCst);
        // The native UI may revoke while a sender awaits a UI URL query.
        // Waiting here would deadlock that UI. The sender also checks after
        // unlocking, closing the restore-vs-revoke race without waiting here.
        let retired = self.active.try_lock().and_then(|mut slot| slot.take());
        drop(retired);
    }
}

impl<T: IpcResponse + Send + Sync> OutputRoute<T> {
    pub(crate) fn send(&self, value: T) -> Result<(), SafeError> {
        let mut pending = None; // On unwind the guard must drop before Active.
        let mut slot = self.core.active.lock();
        if self.core.revoked.load(Ordering::SeqCst) {
            return Err(error("FORBIDDEN"));
        }
        std::mem::swap(&mut *slot, &mut pending);
        let active = pending
            .as_ref()
            .ok_or_else(|| error("OUTPUT_ROUTE_CLOSED"))?;
        let result = (active.authorize)().and_then(|()| {
            active
                .channel
                .send(value)
                .map_err(|_| error("OUTPUT_ROUTE_LOST"))
        });
        if result.is_ok() && !self.core.revoked.load(Ordering::SeqCst) {
            *slot = pending.take();
        }
        drop(slot);
        if self.core.revoked.load(Ordering::SeqCst) {
            self.core.revoke();
        }
        // Both native Channel and admission captures retire outside locks on
        // failure/unwind. Already accepted frames cannot be recalled.
        result
    }
}

#[cfg(test)]
mod cleanup_race {
    use super::*;
    use serde_json::Value;

    // 撤权未取得忙锁时，下一次发送的拒绝分支也必须清理宿主引用。
    #[test]
    fn d11_lifetime_revoked_before_send_drops_native_owners_006() {
        let routes = OutputRoutes::new(1);
        let host = Arc::new(());
        let weak = Arc::downgrade(&host);
        let route = routes
            .bind(
                1,
                Box::new(move || {
                    let _ = &host;
                    Ok(())
                }),
                || Ok(Channel::<Value>::new(|_| panic!("revoked dispatch"))),
            )
            .unwrap();
        let busy = route.core.active.lock();
        routes.revoke();
        drop(busy);
        assert_eq!(route.send(Value::Null).unwrap_err().code, "FORBIDDEN");
        assert!(
            weak.upgrade().is_none(),
            "early denial retained native owner"
        );
    }
}
