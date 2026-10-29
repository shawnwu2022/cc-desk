//! Native live-channel proof; neither a wire DTO nor a user/issuer enrollment API.
use crate::{CancelToken, DataError, GrantClock, GrantError, Role, VerifiedGrant};
use cc_desk_remote_record::{
    appbind::Context,
    grant::{DataRole, SessionScope},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::watch;
#[derive(Clone)]
pub struct GrantClockDomain(Arc<dyn GrantClock>);
impl GrantClockDomain {
    pub(crate) fn new(clock: Arc<dyn GrantClock>) -> Self {
        Self(clock)
    }
    pub fn now_ms(&self) -> Result<u64, GrantError> {
        self.0.now_ms()
    }
    pub fn same_origin(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
#[derive(Clone)]
pub struct BoundDataAuthority {
    grant: VerifiedGrant,
    active: Arc<AtomicBool>,
    cancel: CancelToken,
    stop: watch::Sender<bool>,
    binding: [u8; 32],
    role: Role,
}
impl std::fmt::Debug for BoundDataAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundDataAuthority(<native-live-channel>)")
    }
}
impl BoundDataAuthority {
    pub(crate) fn new(
        grant: VerifiedGrant,
        active: Arc<AtomicBool>,
        cancel: CancelToken,
        stop: watch::Sender<bool>,
        binding: [u8; 32],
        role: Role,
    ) -> Self {
        Self {
            grant,
            active,
            cancel,
            stop,
            binding,
            role,
        }
    }
    pub fn check(&self) -> Result<(), DataError> {
        if self.cancel.is_cancelled() {
            return Err(DataError::Cancelled);
        }
        if !self.active.load(Ordering::Acquire) || *self.stop.borrow() {
            return Err(DataError::Closed);
        }
        self.grant.check().map_err(DataError::Grant)?;
        Ok(())
    }
    /// Invalidates this native owner immediately; callers still join via wait_closed.
    pub fn request_close(&self) {
        self.stop.send_replace(true);
    }
    pub fn same_channel(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.active, &other.active)
            && self.binding == other.binding
            && self.role == other.role
    }
    pub fn local_role(&self) -> Role {
        self.role
    }
    pub fn data_role(&self) -> DataRole {
        self.grant.role()
    }
    pub fn context(&self) -> &Context {
        self.grant.context()
    }
    pub fn scope(&self) -> &SessionScope {
        self.grant.scope()
    }
    pub fn principal_id(&self) -> [u8; 32] {
        self.grant.principal_id()
    }
    pub fn binding_id(&self) -> [u8; 32] {
        self.binding
    }
    pub fn grant_id(&self) -> [u8; 32] {
        self.grant.id()
    }
    pub fn created_host_ms(&self) -> u64 {
        self.grant.created_host_ms()
    }
    pub fn expires_host_ms(&self) -> u64 {
        self.grant.expires_host_ms()
    }
    pub fn clock_domain(&self) -> GrantClockDomain {
        self.grant.clock_domain()
    }
}
