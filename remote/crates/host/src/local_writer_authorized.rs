//! Native bound-channel admission only. No enrollment, endpoint listener or launch.
use super::*;
use cc_desk_remote_record::{grant::DataRole, projection::decode_ack};
use cc_desk_remote_security::{BoundDataAuthority, DataRecord, GrantClockDomain, Role};
use view_gate::control::{ControlAdmission, GrantContext, HostClock};
pub(super) struct VerifiedClock {
    domain: GrantClockDomain,
}
impl HostClock for VerifiedClock {
    fn now_ms(&self) -> Result<u64, Reject> {
        self.domain.now_ms().map_err(|_| Reject::Policy)
    }
}
pub(in crate::local_writer) struct AuthorizedWriter {
    pub(in crate::local_writer) output: HostOutputHandle,
    pub(in crate::local_writer) peer: AuthorizedPeer,
}
pub(in crate::local_writer) struct AuthorizedPeer {
    inner: view_gate::PeerViewHandle,
    authority: BoundDataAuthority,
    target: Target,
    fence: u64,
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|n| format!("{n:02x}")).collect()
}
pub(in crate::local_writer) fn same_host(a: &BoundDataAuthority, b: &BoundDataAuthority) -> bool {
    let (a, b) = (a.context(), b.context());
    a.host_endpoint_id == b.host_endpoint_id
        && a.host_key_epoch == b.host_key_epoch
        && a.activation == b.activation
        && a.incarnation == b.incarnation
        && a.policy_epoch == b.policy_epoch
        && a.issuer_epoch == b.issuer_epoch
        && a.origin == b.origin
}
impl WriterHostOwner {
    pub(in crate::local_writer) fn new_authorized(
        host: [u8; 32],
        incarnation: u64,
        principal: [u8; 32],
    ) -> Result<Self, OwnerError> {
        if principal == [0; 32] {
            return Err(OwnerError::Target);
        }
        let mut owner = Self::new(host, incarnation)?;
        owner.principal = Some(principal);
        Ok(owner)
    }
    pub(super) fn authorized_admission(
        &mut self,
        target: &Target,
        device: [u8; 32],
        authority: &BoundDataAuthority,
        lease: [u8; 16],
        fence: u64,
    ) -> Result<ControlAdmission, OwnerError> {
        let reject = || OwnerError::Writer(Reject::Policy);
        authority.check().map_err(|_| reject())?;
        let c = authority.context();
        let scope = authority.scope();
        if authority.local_role() != Role::Host
            || authority.data_role() != DataRole::Controller
            || self.principal != Some(authority.principal_id())
            || c.host_endpoint_id != hex(&self.host)
            || c.client_endpoint_id != hex(&device)
            || target.host != self.host
            || target.incarnation != self.incarnation
            || c.incarnation != target.incarnation.to_string()
            || scope.session != hex(&target.session)
            || scope.run != target.run
            || scope.generation != target.generation.to_string()
            || lease == [0; 16]
            || fence == 0
        {
            return Err(reject());
        }
        if self
            .remote_policy
            .as_ref()
            .is_some_and(|old| !same_host(old, authority))
        {
            return Err(reject());
        }
        let domain = authority.clock_domain();
        if self
            .remote_clock
            .as_ref()
            .is_some_and(|old| !old.domain.same_origin(&domain))
        {
            return Err(reject());
        }
        let clock = self
            .remote_clock
            .get_or_insert_with(|| Arc::new(VerifiedClock { domain }))
            .clone();
        let policy = c.policy_epoch.parse().map_err(|_| reject())?;
        let grant = GrantContext {
            challenge_ms: authority.created_host_ms(),
            expires_host_ms: authority.expires_host_ms(),
            grant: authority.grant_id(),
            lease,
            fence,
            policy,
        };
        self.remote_policy.get_or_insert_with(|| authority.clone());
        Ok(ControlAdmission {
            authority: Some(authority.clone()),
            binding: authority.binding_id(),
            minor: 1,
            clock,
            grant,
        })
    }
    pub(in crate::local_writer) fn start_authorized(
        &mut self,
        journal: Journal,
        stdin: impl Write + Send + 'static,
        start: SessionStart,
        authority: BoundDataAuthority,
        lease: [u8; 16],
        fence: u64,
    ) -> Result<AuthorizedWriter, OwnerError> {
        let admission =
            self.authorized_admission(&start.target, start.device, &authority, lease, fence)?;
        let target = start.target.clone();
        let mut native = self.start_controlled(journal, stdin, start, admission)?;
        Ok(AuthorizedWriter {
            output: native.output,
            peer: AuthorizedPeer {
                inner: native.peer.take().ok_or(OwnerError::Closed)?,
                authority,
                target,
                fence,
            },
        })
    }
    pub(in crate::local_writer) fn rebind_authorized(
        &mut self,
        target: &Target,
        device: [u8; 32],
        expected: u64,
        authority: BoundDataAuthority,
        lease: [u8; 16],
    ) -> Result<AuthorizedPendingRebind, OwnerError> {
        let next = expected
            .checked_add(1)
            .ok_or(OwnerError::Writer(Reject::Epoch))?;
        let admission = self.authorized_admission(target, device, &authority, lease, next)?;
        let pending = self.rebind(target, device, expected, admission)?;
        Ok(AuthorizedPendingRebind {
            pending,
            authority,
            target: target.clone(),
            fence: next,
        })
    }
}
pub(in crate::local_writer) struct AuthorizedPendingRebind {
    pending: view_gate::rebind::PendingRebind,
    authority: BoundDataAuthority,
    target: Target,
    fence: u64,
}
impl AuthorizedPendingRebind {
    pub(in crate::local_writer) fn from_native(
        pending: view_gate::rebind::PendingRebind,
        authority: BoundDataAuthority,
        target: Target,
    ) -> Self {
        Self {
            pending,
            authority,
            target,
            fence: 1,
        }
    }
    pub(in crate::local_writer) fn wait(
        &self,
        budget: Duration,
    ) -> Result<Result<AuthorizedPeer, Reject>, WaitError> {
        self.pending.wait(budget).map(|p| {
            p.map(|inner| AuthorizedPeer {
                inner,
                authority: self.authority.clone(),
                target: self.target.clone(),
                fence: self.fence,
            })
        })
    }
}
impl AuthorizedPeer {
    fn operation(&self, operation: [u8; 16], sequence: u64) -> OperationKey {
        OperationKey {
            host: self.target.host,
            session: self.target.session,
            run: self.target.run.clone(),
            generation: self.target.generation,
            incarnation: self.target.incarnation,
            device: self.inner.device(),
            operation,
            sequence,
        }
    }
    pub(in crate::local_writer) fn pending_receipt(
        &self,
        operation: [u8; 16],
        sequence: u64,
    ) -> Result<view_gate::control::PendingValue<ReceiptLookup>, Reject> {
        self.authority.check().map_err(|_| Reject::Policy)?;
        self.inner
            .pending_receipt(self.operation(operation, sequence))
    }
    pub(in crate::local_writer) fn settle_receipt(
        &self,
        proof: ReceiptProof,
    ) -> Result<view_gate::control::PendingValue<AckDecision>, Reject> {
        self.authority.check().map_err(|_| Reject::Policy)?;
        if proof.key.target() != self.target || proof.key.device != self.inner.device() {
            return Err(Reject::Target);
        }
        self.inner.settle_receipt(proof)
    }
    fn record(&self, record: &DataRecord) -> Result<(), Reject> {
        self.authority.check().map_err(|_| Reject::Policy)?;
        if !record.source().same_channel(&self.authority)
            || record.scope() != self.authority.scope()
            || record.fence() != self.fence
        {
            return Err(Reject::Target);
        }
        record.source().check().map_err(|_| Reject::Policy)
    }
    pub(in crate::local_writer) fn capture(&self) -> Result<PendingProjection, Reject> {
        self.authority.check().map_err(|_| Reject::Policy)?;
        self.inner.capture()
    }
    pub(in crate::local_writer) fn acknowledge_received(
        &self,
        record: &DataRecord,
    ) -> Result<view_gate::PendingViewAck, Reject> {
        self.record(record)?;
        let ack = decode_ack(record.payload(), 1).map_err(|_| Reject::Bounds)?;
        self.inner.acknowledge(ack)
    }
    pub(in crate::local_writer) fn input_window(
        &self,
        view: &view_gate::SyncedView,
    ) -> Result<view_gate::PendingViewAck, Reject> {
        self.authority.check().map_err(|_| Reject::Policy)?;
        self.inner.input_window(view)
    }
    pub(in crate::local_writer) fn window_token(
        &self,
        window: &view_gate::SyncedView,
    ) -> Result<([u8; 32], u64), Reject> {
        self.authority.check().map_err(|_| Reject::Policy)?;
        self.inner.wire_window(window)
    }
    pub(in crate::local_writer) fn submit_received(
        &self,
        record: &DataRecord,
        window: &view_gate::SyncedView,
    ) -> Result<PendingInput, Reject> {
        use subtle::ConstantTimeEq;
        self.record(record)?;
        let command =
            cc_desk_remote_record::input::decode(record.payload()).map_err(|_| Reject::Bounds)?;
        let (token, deadline) = self.inner.wire_window(window)?;
        if !bool::from(command.window.ct_eq(&token)) || command.not_after_host_ms != deadline {
            return Err(Reject::Policy);
        }
        let key = self.operation(command.operation, command.sequence);
        self.inner.submit_bound(window, key, command.bytes)
    }
}
