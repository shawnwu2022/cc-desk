//! Native live-channel proof; neither a wire DTO nor a user/issuer enrollment API.
use crate::{CancelToken, DataError, GrantClock, GrantError, Role};
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
    grant: crate::data::ChannelGrant,
    active: Arc<AtomicBool>,
    cancel: CancelToken,
    stop: watch::Sender<bool>,
    identity: Arc<ChannelIdentity>,
}
#[derive(Clone)]
struct ChannelIdentity {
    binding: [u8; 32],
    role: Role,
    control_lease: Option<crate::bind::NegotiatedControlLease>,
    data_keepalive: Option<crate::bind::NegotiatedDataKeepalive>,
    snapshot_pull: Option<crate::bind::NegotiatedSnapshotPull>,
}
impl std::fmt::Debug for BoundDataAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundDataAuthority(<native-live-channel>)")
    }
}
impl BoundDataAuthority {
    pub(crate) fn new(
        grant: crate::data::ChannelGrant,
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
            identity: Arc::new(ChannelIdentity {
                binding,
                role,
                control_lease: None,
                data_keepalive: None,
                snapshot_pull: None,
            }),
        }
    }
    pub(crate) fn with_control_lease(
        mut self,
        fact: Option<crate::bind::NegotiatedControlLease>,
    ) -> Self {
        Arc::make_mut(&mut self.identity).control_lease = fact;
        self
    }
    pub(crate) fn with_data_keepalive(
        mut self,
        fact: Option<crate::bind::NegotiatedDataKeepalive>,
    ) -> Self {
        Arc::make_mut(&mut self.identity).data_keepalive = fact;
        self
    }
    pub(crate) fn with_snapshot_pull(
        mut self,
        fact: Option<crate::bind::NegotiatedSnapshotPull>,
    ) -> Self {
        Arc::make_mut(&mut self.identity).snapshot_pull = fact;
        self
    }
    // 只描述原 channel 的协商事实，不代替 grant/Installed/截止校验。
    pub fn snapshot_pull_negotiated(&self) -> bool {
        self.identity
            .snapshot_pull
            .is_some_and(|fact| fact.matches(self.identity.binding, self.identity.role))
    }
    pub fn data_keepalive_negotiated(&self) -> bool {
        self.identity.data_keepalive.is_some()
    }
    pub fn control_lease_renew_enabled(&self) -> bool {
        self.identity.control_lease.is_some()
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
            && self.identity.binding == other.identity.binding
            && self.identity.role == other.identity.role
    }
    pub fn local_role(&self) -> Role {
        self.identity.role
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
        self.identity.binding
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
