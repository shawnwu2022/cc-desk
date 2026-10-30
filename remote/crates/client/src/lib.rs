//! Native client data-owner bridge. Construct only from the original granted
//! client channel, never a JS identity, connection label or authorization flag.
//! No IPC registration, listener, enrollment or automatic retry is installed.
use cc_desk_remote_record::{command, control_io as wire, grant::SessionScope, projection};
pub use cc_desk_remote_security::DataError;
use cc_desk_remote_security::{BoundDataAuthority, Role, RunningChannel};
use std::time::Duration;
use tokio::{sync::Mutex, time::Instant};
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
pub struct NativeClient {
    channel: RunningChannel,
    scope: SessionScope,
    fence: u64,
    pending: Mutex<()>,
}
/// Bounded opaque wire parts. The UI reducer must still install the complete
/// semantic frame; returning these bytes never authorizes input or an ACK.
pub struct SnapshotBatch {
    pub stamp: projection::Stamp,
    pub parts: Vec<Vec<u8>>,
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
        let authority = channel.authority();
        if authority.local_role() != Role::Client {
            return Err(DataError::Role);
        }
        authority.check()?;
        if fence == 0 {
            return Err(DataError::Fence);
        }
        let scope = authority.scope().clone();
        Ok(Self {
            channel,
            scope,
            fence,
            pending: Mutex::new(()),
        })
    }
    pub fn binding_id(&self) -> [u8; 32] {
        self.channel.binding_id()
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
        if !matches!(command::decode(payload), Ok(command::Command::Snapshot(_))) {
            return Err(DataError::Codec);
        }
        let end = deadline(budget)?;
        let _pending = self.pending.try_lock().map_err(|_| DataError::Full)?;
        let mut close = CancelOnDrop(Some(self.channel.authority()));
        self.channel
            .send_control(&self.scope, self.fence, payload, remaining(end)?)
            .await?;
        let authority = self.channel.authority();
        let mut current: Option<(projection::Stamp, projection::State)> = None;
        let mut parts = Vec::new();
        let mut next = 0usize;
        let mut total_bytes = 0usize;
        loop {
            let record = self
                .channel
                .receive(&self.scope, self.fence, remaining(end)?)
                .await?;
            self.record(&record)?;
            total_bytes = total_bytes
                .checked_add(record.payload().len())
                .ok_or(DataError::Bounds)?;
            if total_bytes > MAX_SNAPSHOT_BYTES {
                return Err(DataError::Bounds);
            }
            let message = projection::decode(record.payload(), 1).map_err(|_| DataError::Codec)?;
            let projection::Content::SnapshotPart(part) = message.content else {
                return Err(DataError::Codec);
            };
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
            parts.push(record.payload().to_vec());
            remaining(end)?;
            if next == cells {
                self.channel.authority().check()?;
                close.0 = None;
                return Ok(SnapshotBatch {
                    stamp: current.unwrap().0,
                    parts,
                });
            }
        }
    }
    /// Invalidates this exact channel synchronously; close() still joins it.
    pub fn request_close(&self) {
        self.channel.authority().request_close();
    }
    pub async fn close(&self) -> DataError {
        self.channel.close().await
    }
}
#[cfg(test)]
mod tests;
