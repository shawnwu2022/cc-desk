//! Single installed peer connection, independent of uncommitted router WIP.
use super::*;
use cc_desk_remote_record::projection::Stamp;
use cc_desk_remote_security::{BoundDataAuthority, DataRecord, RunningChannel};
#[cfg(test)]
#[path = "local_writer_control_reply_tests.rs"]
mod control_reply_tests;
#[path = "local_writer_connection_pending.rs"]
pub(in crate::local_writer::owner) mod custody;
#[path = "local_writer_control_reply.rs"]
mod reply;

enum PeerRef<'a> {
    Borrowed(&'a AuthorizedPeer),
    Owned(Arc<AuthorizedPeer>),
}
impl std::ops::Deref for PeerRef<'_> {
    type Target = AuthorizedPeer;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(peer) => peer,
            Self::Owned(peer) => peer.as_ref(),
        }
    }
}
pub(in crate::local_writer::owner) struct Connection<'a> {
    peer: PeerRef<'a>,
    custody: Option<Arc<custody::Owner>>,
    proof: BoundDataAuthority,
    fence: u64,
    last: u64,
    busy: bool,
    pending_input: Option<([u8; 16], u64)>,
    delivered: Option<Stamp>,
    view: Option<view_gate::SyncedView>,
}
impl<'a> Connection<'a> {
    pub(super) fn new(
        peer: &'a AuthorizedPeer,
        proof: BoundDataAuthority,
        fence: u64,
    ) -> Result<Self, Reject> {
        proof.check().map_err(|_| Reject::Policy)?;
        Ok(Self {
            peer: PeerRef::Borrowed(peer),
            custody: None,
            proof,
            fence,
            last: 0,
            busy: false,
            pending_input: None,
            delivered: None,
            view: None,
        })
    }
}

impl Connection<'static> {
    pub(in crate::local_writer::owner) fn new_owned(
        peer: Arc<AuthorizedPeer>,
        proof: BoundDataAuthority,
        fence: u64,
        custody: Arc<custody::Owner>,
    ) -> Result<Self, Reject> {
        proof.check().map_err(|_| Reject::Policy)?;
        Ok(Self {
            peer: PeerRef::Owned(peer),
            custody: Some(custody),
            proof,
            fence,
            last: 0,
            busy: false,
            pending_input: None,
            delivered: None,
            view: None,
        })
    }
}
fn reserve<T: Send + 'static>(
    owner: &Option<Arc<custody::Owner>>,
) -> Result<Option<custody::Reservation<T>>, Reject> {
    owner.as_ref().map(|owner| owner.reserve()).transpose()
}
// A normal function: bind BEFORE returning a discardable future.
fn retained_wait<T: Send + 'static>(
    deadline: tokio::time::Instant,
    reservation: Option<custody::Reservation<T>>,
    f: impl FnMut(Duration) -> Result<Result<T, Reject>, WaitError> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, Reject>> {
    enum Mode<T: Send + 'static, F> {
        Owned(custody::Wait<T>),
        Legacy(F),
    }
    let mode = match reservation {
        Some(reservation) => Mode::Owned(reservation.bind(f)),
        None => Mode::Legacy(f),
    };
    async move {
        match mode {
            Mode::Owned(waiter) => waiter.until(deadline).await,
            Mode::Legacy(f) => wait(deadline, f).await,
        }
    }
}

fn end(b: Duration) -> Result<tokio::time::Instant, Reject> {
    if b.is_zero() || b > Duration::from_secs(1) {
        return Err(Reject::Bounds);
    }
    Ok(tokio::time::Instant::now() + b)
}
struct Abort<T>(tokio::task::JoinHandle<T>);
impl Drop for Connection<'_> {
    fn drop(&mut self) {
        self.view = None;
        self.delivered = None;
        self.busy = true;
    }
}
impl<T> Drop for Abort<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn wait<T: Send + 'static>(
    end: tokio::time::Instant,
    f: impl FnOnce(Duration) -> Result<Result<T, Reject>, WaitError> + Send + 'static,
) -> Result<T, Reject> {
    let mut task = Abort(tokio::task::spawn_blocking(move || {
        let left = end.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return Err(WaitError::Uncertain);
        }
        f(left)
    }));
    let value = tokio::time::timeout_at(end, &mut task.0)
        .await
        .map_err(|_| Reject::Continuity)?
        .map_err(|_| Reject::Thread)?
        .map_err(|_| Reject::Continuity)?;
    if tokio::time::Instant::now() >= end {
        return Err(Reject::Continuity);
    }
    value
}
impl Connection<'_> {
    fn context(&self, r: &DataRecord) -> Result<(), Reject> {
        self.proof.check().map_err(|_| Reject::Policy)?;
        if !r.source().same_channel(&self.proof)
            || r.scope() != self.proof.scope()
            || r.fence() != self.fence
        {
            return Err(Reject::Target);
        }
        Ok(())
    }
    fn record(&self, r: &DataRecord) -> Result<(), Reject> {
        self.context(r)?;
        if self.busy {
            return Err(Reject::Pending);
        }
        Ok(())
    }
    fn counter(&mut self, n: u64) -> Result<(), Reject> {
        if n <= self.last {
            return Err(Reject::Epoch);
        }
        self.last = n;
        Ok(())
    }
    pub(super) async fn snapshot(
        &mut self,
        r: &DataRecord,
        host: &RunningChannel,
        b: Duration,
    ) -> Result<(), Reject> {
        let deadline = end(b)?;
        self.record(r)?;
        if !host.authority().same_channel(&self.proof) {
            return Err(Reject::Target);
        }
        let cc_desk_remote_record::command::Command::Snapshot(n) =
            cc_desk_remote_record::command::decode(r.payload()).map_err(|_| Reject::Bounds)?
        else {
            return Err(Reject::Bounds);
        };
        self.counter(n)?;
        self.busy = true;
        self.view = None;
        self.delivered = None;
        let reservation = reserve(&self.custody)?;
        let pending = self.peer.capture()?;
        let frame = retained_wait(deadline, reservation, move |left| pending.wait(left)).await?;
        let stamp = frame.issued_stamp()?.clone();
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return Err(Reject::Continuity);
        }
        frame.deliver_bound(host, self.fence, left).await?;
        self.proof.check().map_err(|_| Reject::Policy)?;
        if tokio::time::Instant::now() >= deadline {
            return Err(Reject::Continuity);
        }
        self.delivered = Some(stamp);
        self.busy = false;
        Ok(())
    }
    pub(super) async fn acknowledge(&mut self, r: &DataRecord, b: Duration) -> Result<(), Reject> {
        let deadline = end(b)?;
        self.record(r)?;
        let ack = cc_desk_remote_record::projection::decode_ack(r.payload(), 1)
            .map_err(|_| Reject::Bounds)?;
        if self.view.is_some() || self.delivered.as_ref() != Some(&ack.stamp) {
            return Err(Reject::Sync);
        }
        self.busy = true;
        let reservation = reserve(&self.custody)?;
        let pending = self.peer.acknowledge_received(r)?;
        let view = retained_wait(deadline, reservation, move |left| pending.wait(left)).await?;
        self.proof.check().map_err(|_| Reject::Policy)?;
        self.view = Some(view);
        self.busy = false;
        Ok(())
    }
    pub(super) async fn window(
        &mut self,
        r: &DataRecord,
        b: Duration,
    ) -> Result<([u8; 32], u64), Reject> {
        let deadline = end(b)?;
        self.record(r)?;
        let cc_desk_remote_record::command::Command::Window(n) =
            cc_desk_remote_record::command::decode(r.payload()).map_err(|_| Reject::Bounds)?
        else {
            return Err(Reject::Bounds);
        };
        self.counter(n)?;
        let view = self.view.as_ref().ok_or(Reject::Sync)?;
        self.busy = true;
        let reservation = reserve(&self.custody)?;
        let pending = self.peer.input_window(view)?;
        let next = retained_wait(deadline, reservation, move |left| pending.wait(left)).await?;
        let descriptor = self.peer.window_token(&next)?;
        self.view = Some(next);
        self.busy = false;
        Ok(descriptor)
    }
    pub(super) async fn input(
        &mut self,
        r: &DataRecord,
        b: Duration,
    ) -> Result<WriteResult, Reject> {
        let deadline = end(b)?;
        self.record(r)?;
        let view = self.view.as_ref().ok_or(Reject::Sync)?;
        self.busy = true;
        let command =
            cc_desk_remote_record::input::decode(r.payload()).map_err(|_| Reject::Bounds)?;
        let reservation = reserve(&self.custody)?;
        let pending = self.peer.submit_received(r, view)?;
        self.pending_input = Some((command.operation, command.sequence));
        let value = retained_wait(deadline, reservation, move |left| {
            pending.wait(left).map(Ok)
        })
        .await?;
        self.proof.check().map_err(|_| Reject::Policy)?;
        if matches!(value, WriteResult::Receipt(_) | WriteResult::Rejected(_)) {
            self.busy = false;
            self.pending_input = None;
        }
        Ok(value)
    }
}
