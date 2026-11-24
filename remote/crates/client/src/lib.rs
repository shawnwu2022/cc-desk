//! Native client data-owner bridge. Construct only from the original granted
//! client channel, never a JS identity, connection label or authorization flag.
//! No IPC registration, listener, enrollment or automatic retry is installed.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod bootstrap;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub mod browser;
use cc_desk_remote_record::{command, control_io as wire, grant::SessionScope, projection};
pub use cc_desk_remote_record::{
    grant::DataRole as ClientRole, projection::Target as ClientTarget,
};
pub use cc_desk_remote_security::DataError;
use cc_desk_remote_security::owner_clock::Instant;
use cc_desk_remote_security::{BoundDataAuthority, Role, RunningChannel};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::sync::Mutex;
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
pub struct NativeClient {
    channel: RunningChannel,
    scope: SessionScope,
    fence: u64,
    pending: Mutex<()>,
    observer_view_taken: AtomicBool,
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    mux: Option<cc_desk_remote_transport::OwnedTunnel>,
    lifetime_cancel: Option<cc_desk_remote_security::CancelHandle>,
}
/// Safe display metadata from one installed live authority; never connection inputs.
pub struct ClientDescriptor {
    pub target: ClientTarget,
    pub binding: String,
    pub role: ClientRole,
}
/// Bounded opaque wire parts. The UI reducer must still install the complete
/// semantic frame; returning these bytes never authorizes input or an ACK.
pub struct SnapshotBatch {
    pub stamp: projection::Stamp,
    pub parts: Vec<Vec<u8>>,
}
/// Local receipt for one complete, authenticated Observer grid. This is not
/// a wire acknowledgement, renderer installation, or permission to send input.
/// The browser must still validate semantic cells and invalidate on retirement.
pub struct ObserverViewReceipt {
    authority: BoundDataAuthority,
    batch: SnapshotBatch,
}
impl ObserverViewReceipt {
    pub fn into_snapshot(self) -> Result<SnapshotBatch, DataError> {
        self.authority.check()?;
        Ok(self.batch)
    }
}
struct CancelOnDrop(Option<BoundDataAuthority>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(authority) = &self.0 {
            authority.request_close();
        }
    }
}
enum Expected {
    LeaseState(String),
    LeaseRenew(String, String, String, String),
    Cursor(String),
    Window(String),
    Ack(Box<projection::Stamp>),
    Input(String, String),
    Query(String, String, String),
    Settle(String, String, String, wire::Outcome),
}
fn expected(bytes: &[u8]) -> Result<Expected, DataError> {
    let hex = |b: &[u8]| b.iter().map(|n| format!("{n:02x}")).collect::<String>();
    match command::decode(bytes) {
        Ok(command::Command::Window(request)) => Ok(Expected::Window(request.to_string())),
        Ok(command::Command::Ack(ack)) => Ok(Expected::Ack(Box::new(ack.stamp))),
        Ok(command::Command::Input(input)) => Ok(Expected::Input(
            hex(&input.operation),
            input.sequence.to_string(),
        )),
        Ok(_) => Err(DataError::Codec),
        Err(_) => match wire::decode(bytes).map_err(|_| DataError::Codec)? {
            wire::Message::LeaseState { request, .. } => Ok(Expected::LeaseState(request)),
            wire::Message::LeaseRenew {
                request,
                lease,
                fence,
                renewal_sequence,
                ..
            } => Ok(Expected::LeaseRenew(
                request,
                lease,
                fence,
                renewal_sequence,
            )),
            wire::Message::InputCursor { request, .. } => Ok(Expected::Cursor(request)),
            wire::Message::ReceiptQuery {
                request,
                operation,
                sequence,
                ..
            } => Ok(Expected::Query(request, operation, sequence)),
            wire::Message::ReceiptAck {
                request,
                operation,
                sequence,
                outcome,
                ..
            } => Ok(Expected::Settle(request, operation, sequence, outcome)),
            _ => Err(DataError::Codec),
        },
    }
}
impl Expected {
    fn matches(&self, reply: &wire::Message) -> bool {
        match (self, reply) {
            (Self::LeaseState(a), wire::Message::LeaseStateReply { request, .. }) => a == request,
            (
                Self::LeaseRenew(a, b, c, d),
                wire::Message::LeaseRenewed {
                    request,
                    lease,
                    fence,
                    renewal_sequence,
                    ..
                },
            ) => a == request && b == lease && c == fence && d == renewal_sequence,
            (Self::Cursor(a), wire::Message::InputCursorState { request, .. }) => a == request,
            (Self::Window(a), wire::Message::WindowGranted { request, .. }) => a == request,
            (Self::Ack(a), wire::Message::ProjectionAcknowledged { stamp, .. }) => {
                a.as_ref() == stamp
            }
            (
                Self::Input(a, b),
                wire::Message::InputReceipt {
                    operation,
                    sequence,
                    ..
                }
                | wire::Message::InputNotAccepted {
                    operation,
                    sequence,
                    ..
                },
            ) => a == operation && b == sequence,
            (
                Self::Query(a, b, c),
                wire::Message::ReceiptStatus {
                    request,
                    operation,
                    sequence,
                    ..
                },
            ) => a == request && b == operation && c == sequence,
            (
                Self::Settle(a, b, c, d),
                wire::Message::ReceiptSettled {
                    request,
                    operation,
                    sequence,
                    outcome,
                    ..
                },
            ) => a == request && b == operation && c == sequence && d == outcome,
            _ => false,
        }
    }
}
fn deadline(budget: Duration) -> Result<Instant, DataError> {
    if budget.is_zero() || budget > Duration::from_secs(1) {
        return Err(DataError::Bounds);
    }
    Instant::now().checked_add(budget).ok_or(DataError::Bounds)
}
fn remaining(end: Instant) -> Result<Duration, DataError> {
    let left = end.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(DataError::Deadline)
    } else {
        Ok(left)
    }
}
impl NativeClient {
    pub fn new(channel: RunningChannel, fence: u64) -> Result<Self, DataError> {
        if channel.peer_install_enabled() {
            return Err(DataError::Context);
        }
        Self::construct(channel, fence)
    }
    pub fn from_installed(
        channel: RunningChannel,
        proof: cc_desk_remote_security::ClientInstalledPeer,
    ) -> Result<Self, DataError> {
        Self::from_installed_retained(channel, proof).map_err(|original| original.0)
    }
    fn from_installed_retained(
        channel: RunningChannel,
        proof: cc_desk_remote_security::ClientInstalledPeer,
    ) -> Result<Self, Box<(DataError, RunningChannel)>> {
        let fence = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            proof.consume_for(&channel)
        })) {
            Ok(Ok(fence)) => fence,
            Ok(Err(e)) => return Err(Box::new((e, channel))),
            Err(_) => return Err(Box::new((DataError::Uncertain, channel))),
        };
        Self::construct_retained(channel, fence)
    }
    fn construct(channel: RunningChannel, fence: u64) -> Result<Self, DataError> {
        Self::construct_retained(channel, fence).map_err(|original| original.0)
    }
    fn construct_retained(
        channel: RunningChannel,
        fence: u64,
    ) -> Result<Self, Box<(DataError, RunningChannel)>> {
        // Validate while borrowing the original owner; native clock/provider
        // panic cannot unwind a moved RunningChannel out of the cleanup slot.
        let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let authority = channel.authority();
            if authority.local_role() != Role::Client {
                return Err(DataError::Role);
            }
            authority.check()?;
            if fence == 0 {
                return Err(DataError::Fence);
            }
            Ok(authority.scope().clone())
        }));
        let scope = match checked {
            Ok(Ok(scope)) => scope,
            Ok(Err(e)) => return Err(Box::new((e, channel))),
            Err(_) => return Err(Box::new((DataError::Uncertain, channel))),
        };
        Ok(Self {
            channel,
            scope,
            fence,
            pending: Mutex::new(()),
            observer_view_taken: AtomicBool::new(false),
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            mux: None,
            lifetime_cancel: None,
        })
    }
    pub fn binding_id(&self) -> [u8; 32] {
        self.channel.binding_id()
    }
    pub fn descriptor(&self) -> Result<ClientDescriptor, DataError> {
        let authority = self.channel.authority();
        authority.check()?;
        if !self.channel.peer_install_enabled() || self.channel.installed_fence()? != self.fence {
            return Err(DataError::Context);
        }
        let descriptor = ClientDescriptor {
            target: ClientTarget {
                host: authority.context().host_endpoint_id.clone(),
                session: self.scope.session.clone(),
                run: self.scope.run.clone(),
                generation: self.scope.generation.clone(),
                incarnation: authority.context().incarnation.clone(),
            },
            binding: authority
                .binding_id()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            role: authority.data_role(),
        };
        authority.check()?;
        Ok(descriptor)
    }
    fn record(&self, record: &cc_desk_remote_security::DataRecord) -> Result<(), DataError> {
        let authority = self.channel.authority();
        authority.check()?;
        if !record.source().same_channel(&authority)
            || record.scope() != &self.scope
            || record.fence() != self.fence
        {
            return Err(DataError::Context);
        }
        Ok(())
    }
    /// No queue: a concurrent call is refused before it can write. Cancellation
    /// after admission closes this exact owner; it never proves non-execution.
    pub async fn exchange(&self, payload: &[u8], budget: Duration) -> Result<Vec<u8>, DataError> {
        let expected = expected(payload)?;
        let lease = matches!(
            &expected,
            Expected::LeaseState(_) | Expected::LeaseRenew(..)
        );
        if lease {
            let authority = self.channel.authority();
            authority.check()?;
            if !authority.control_lease_renew_enabled()
                || authority.data_role() != ClientRole::Controller
                || self.channel.installed_fence()? != self.fence
            {
                return Err(DataError::Role);
            }
            if matches!(&expected,Expected::LeaseRenew(_,_,fence,_) if fence!=&self.fence.to_string())
            {
                return Err(DataError::Fence);
            }
        }
        let end = deadline(budget)?;
        let _pending = self.pending.try_lock().map_err(|_| DataError::Full)?;
        let mut close = CancelOnDrop(Some(self.channel.authority()));
        self.channel
            .send_control(&self.scope, self.fence, payload, remaining(end)?)
            .await?;
        let record = self
            .channel
            .receive(&self.scope, self.fence, remaining(end)?)
            .await?;
        self.record(&record)?;
        let reply = wire::decode(record.payload()).map_err(|_| DataError::Codec)?;
        if lease {
            let authority = self.channel.authority();
            let until = match &reply {
                wire::Message::LeaseStateReply {
                    not_after_host_ms, ..
                }
                | wire::Message::LeaseRenewed {
                    not_after_host_ms, ..
                } => not_after_host_ms
                    .parse::<u64>()
                    .map_err(|_| DataError::Codec)?,
                _ => return Err(DataError::Context),
            };
            if until <= authority.created_host_ms() || until > authority.expires_host_ms() {
                return Err(DataError::Context);
            }
        }
        if lease
            && matches!(&reply,wire::Message::LeaseStateReply{fence,..}|wire::Message::LeaseRenewed{fence,..} if fence!=&self.fence.to_string())
        {
            return Err(DataError::Fence);
        }
        if !expected.matches(&reply) {
            return Err(DataError::Context);
        }
        remaining(end)?;
        self.channel.authority().check()?;
        let bytes = record.payload().to_vec();
        close.0 = None;
        Ok(bytes)
    }
    /// Resolves only on contiguous full-grid coverage under one exact stamp and
    /// state. No ACK is generated here. The existing semantic reducer remains
    /// responsible for cell roles, continuity and renderer installation.
    pub async fn snapshot(
        &self,
        payload: &[u8],
        budget: Duration,
    ) -> Result<SnapshotBatch, DataError> {
        self.snapshot_inner(Some(payload), budget, None).await
    }
    /// Explicit opt-in; never falls back to the legacy full-push snapshot.
    pub async fn snapshot_pull(
        &self,
        request: u64,
        budget: Duration,
    ) -> Result<SnapshotBatch, DataError> {
        if !self.channel.authority().snapshot_pull_negotiated() {
            return Err(DataError::Context);
        }
        let begin = cc_desk_remote_record::snapshot_pull::Message::SnapshotBegin {
            profile: 1,
            request: request.to_string(),
        };
        let bytes =
            cc_desk_remote_record::snapshot_pull::encode(&begin).map_err(|_| DataError::Codec)?;
        self.snapshot_inner(Some(&bytes), budget, Some(request))
            .await
    }
    /// Observer view is one-shot, starts only from an authentic installed
    /// Client channel and issues no command, ACK or general control message.
    pub async fn receive_observer_view(
        &self,
        budget: Duration,
    ) -> Result<ObserverViewReceipt, DataError> {
        let authority = self.channel.authority();
        authority.check()?;
        if authority.data_role() != ClientRole::Observer
            || authority.local_role() != Role::Client
            || authority.control_lease_renew_enabled()
            || authority.data_keepalive_negotiated()
            || authority.snapshot_pull_negotiated()
        {
            return Err(DataError::Role);
        }
        if !self.channel.peer_install_enabled() || self.channel.installed_fence()? != self.fence {
            return Err(DataError::Context);
        }
        let batch = self.snapshot_inner(None, budget, None).await?;
        authority.check()?;
        Ok(ObserverViewReceipt { authority, batch })
    }

    async fn snapshot_inner(
        &self,
        payload: Option<&[u8]>,
        budget: Duration,
        pull_request: Option<u64>,
    ) -> Result<SnapshotBatch, DataError> {
        if let Some(payload) = payload
            && pull_request.is_none()
            && !matches!(command::decode(payload), Ok(command::Command::Snapshot(_)))
        {
            return Err(DataError::Codec);
        }
        let end = deadline(budget)?;
        let _pending = self.pending.try_lock().map_err(|_| DataError::Full)?;
        if payload.is_none() && self.observer_view_taken.swap(true, Ordering::AcqRel) {
            return Err(DataError::Full);
        }
        let mut close = CancelOnDrop(Some(self.channel.authority()));
        if let Some(payload) = payload {
            self.channel
                .send_control(&self.scope, self.fence, payload, remaining(end)?)
                .await?;
        }
        let authority = self.channel.authority();
        let mut current: Option<(projection::Stamp, projection::State)> = None;
        let mut parts = Vec::new();
        let mut next = 0usize;
        let mut total_bytes = 0usize;
        let mut transfer = None;
        let mut batch_index = 0u32;
        loop {
            let record = self
                .channel
                .receive(&self.scope, self.fence, remaining(end)?)
                .await?;
            self.record(&record)?;
            let (part_bytes, part) = if let Some(request) = pull_request {
                let (batch, part) =
                    cc_desk_remote_record::snapshot_pull::decode_validated_batch(record.payload())
                        .map_err(|_| DataError::Codec)?;
                if batch.request != request
                    || batch.batch != batch_index
                    || transfer.is_some_and(|t| t != batch.transfer)
                {
                    return Err(DataError::Context);
                }
                transfer = Some(batch.transfer);
                (batch.part, part)
            } else {
                let message =
                    projection::decode(record.payload(), 1).map_err(|_| DataError::Codec)?;
                let projection::Content::SnapshotPart(part) = message.content else {
                    return Err(DataError::Codec);
                };
                (record.payload(), part)
            };
            total_bytes = total_bytes
                .checked_add(part_bytes.len())
                .ok_or(DataError::Bounds)?;
            if total_bytes > MAX_SNAPSHOT_BYTES {
                return Err(DataError::Bounds);
            }
            let t = &part.stamp.target;
            let binding = authority
                .binding_id()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            if t.host != authority.context().host_endpoint_id
                || t.session != self.scope.session
                || t.run != self.scope.run
                || t.generation != self.scope.generation
                || t.incarnation != authority.context().incarnation
                || part.stamp.binding != binding
            {
                return Err(DataError::Context);
            }
            if usize::from(part.start) != next || part.cells.is_empty() {
                return Err(DataError::Context);
            }
            if let Some((stamp, state)) = &current {
                if stamp != &part.stamp || state != &part.state {
                    return Err(DataError::Context);
                }
            } else {
                current = Some((part.stamp.clone(), part.state.clone()));
            }
            let cells = usize::from(part.state.rows) * usize::from(part.state.columns);
            next = next
                .checked_add(part.cells.len())
                .ok_or(DataError::Bounds)?;
            if next > cells {
                return Err(DataError::Bounds);
            }
            parts.push(part_bytes.to_vec());
            remaining(end)?;
            if next == cells {
                self.channel.authority().check()?;
                close.0 = None;
                return Ok(SnapshotBatch {
                    stamp: current.unwrap().0,
                    parts,
                });
            }
            drop(part);
            drop(record);
            if let Some(request) = pull_request {
                batch_index = batch_index.checked_add(1).ok_or(DataError::Bounds)?;
                let next = cc_desk_remote_record::snapshot_pull::Message::SnapshotNext {
                    profile: 1,
                    request: request.to_string(),
                    transfer: transfer
                        .ok_or(DataError::Context)?
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect(),
                    batch: batch_index.to_string(),
                    through: next.to_string(),
                };
                let bytes = cc_desk_remote_record::snapshot_pull::encode(&next)
                    .map_err(|_| DataError::Codec)?;
                self.channel
                    .send_control(&self.scope, self.fence, &bytes, remaining(end)?)
                    .await?;
            }
        }
    }
    /// Invalidates this exact channel synchronously; close() still joins it.
    pub fn request_close(&self) {
        self.channel.authority().request_close();
        if let Some(cancel) = &self.lifetime_cancel {
            cancel.cancel();
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        if let Some(mux) = &self.mux {
            mux.request_close();
        }
    }
    /// Signal all owners before awaiting any; a cancelled observer leaves every
    /// original JoinHandle retained in its owner slot.
    pub async fn retire(&self) -> Result<DataError, DataError> {
        self.request_close();
        let channel = self.channel.wait_closed_checked().await;
        // Join every original owner even when a prior owner panicked/aborted.
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let mux = if let Some(mux) = &self.mux {
            mux.wait_closed().await.map_err(|_| DataError::Uncertain)
        } else {
            Ok(())
        };
        let reason = channel?;
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        mux?;
        Ok(reason)
    }
    pub async fn close(&self) -> DataError {
        self.retire().await.unwrap_or(DataError::Uncertain)
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod peer_proof_tests;
