//! Internal Attach evidence only; no wire authority or native backend.
use crate::lifecycle_router::ResourceIdentity;
use cc_desk_remote_record::lifecycle::*;
use std::sync::{Arc, Mutex};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Applied {
    pub request: Request,
    pub after: State,
}
struct Slot {
    active: bool,
    request: Request,
    identity: ResourceIdentity,
    digest: [u8; 32],
    backend: Arc<()>,
    result: Option<Applied>,
}
#[derive(Clone)]
pub(crate) struct Sender(Arc<Mutex<Slot>>);
pub(crate) struct Receiver(Arc<Mutex<Slot>>);
pub(crate) fn channel(
    request: Request,
    identity: ResourceIdentity,
    digest: [u8; 32],
    backend: Arc<()>,
) -> Result<(Sender, Receiver), Code> {
    successor(&request)?;
    let s = Arc::new(Mutex::new(Slot {
        active: true,
        request,
        identity,
        digest,
        backend,
        result: None,
    }));
    Ok((Sender(s.clone()), Receiver(s)))
}
fn successor(r: &Request) -> Result<State, Code> {
    if r.kind != Kind::Attach || r.size.is_some() {
        return Err(Code::Unsupported);
    }
    let mut after = r.state;
    after.fence = after.fence.checked_add(1).ok_or(Code::Stale)?;
    after.sync = after.sync.checked_add(1).ok_or(Code::Stale)?;
    Ok(after)
}
impl Sender {
    pub(crate) fn complete(
        &self,
        request: &Request,
        identity: &ResourceIdentity,
        after: State,
    ) -> Result<(), Code> {
        let mut s = self.0.lock().map_err(|_| Code::Backend)?;
        if !s.active {
            return Err(Code::Stale);
        }
        if request != &s.request || identity != &s.identity || after != successor(&s.request)? {
            return Err(Code::Conflict);
        }
        let event = Applied {
            request: request.clone(),
            after,
        };
        if s.result.as_ref().is_some_and(|old| old != &event) {
            return Err(Code::Conflict);
        }
        s.result = Some(event);
        Ok(())
    }
}
impl Receiver {
    pub(crate) fn check_identity(&self, identity: Option<&ResourceIdentity>) -> Result<(), Code> {
        let s = self.0.lock().map_err(|_| Code::Backend)?;
        if identity == Some(&s.identity) {
            Ok(())
        } else {
            Err(Code::Unauthorized)
        }
    }
    pub(crate) fn evidence(
        &self,
        digest: [u8; 32],
        backend: &Arc<()>,
    ) -> Result<Option<Applied>, Code> {
        let s = self.0.lock().map_err(|_| Code::Backend)?;
        if s.digest != digest || !Arc::ptr_eq(&s.backend, backend) {
            return Err(Code::Conflict);
        }
        Ok(s.result.clone())
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.lock() {
            s.active = false
        }
    }
}
#[cfg(test)]
#[path = "lifecycle_completion_tests.rs"]
mod tests;
