//! Exact native install evidence only; no endpoint grant or input lease.
use super::*;
use cc_desk_remote_record::projection::{Ack, Stamp, encode_ack};
use cc_desk_remote_terminal::TerminalEpochs;

use std::sync::atomic::AtomicU8;
const ISSUED: u8 = 0;
const SENDING: u8 = 1;
const DELIVERED: u8 = 2;
const REVOKED: u8 = 3;
pub(super) struct PeerScope {
    target: Target,
    device: [u8; 32],
    binding: [u8; 32],
    minor: u16,
    active: AtomicBool,
    closed: tokio::sync::watch::Sender<bool>,
}
impl PeerScope {
    pub(super) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub(super) fn close(&self) {
        self.active.store(false, Ordering::Release);
        self.closed.send_replace(true);
    }
}
pub(super) struct PeerControl {
    pub(super) scope: Arc<PeerScope>,
}
impl PeerControl {
    pub(super) fn new(
        target: Target,
        device: [u8; 32],
        binding: [u8; 32],
        minor: u16,
    ) -> Result<Arc<Self>, Reject> {
        if minor > 1 {
            return Err(Reject::Bounds);
        }
        let (closed, _) = tokio::sync::watch::channel(false);
        Ok(Arc::new(Self {
            scope: Arc::new(PeerScope {
                target,
                device,
                binding,
                minor,
                active: AtomicBool::new(true),
                closed,
            }),
        }))
    }
    pub(super) fn close(&self) {
        self.scope.close();
    }
}
impl Drop for PeerControl {
    fn drop(&mut self) {
        self.close();
    }
}
pub(super) struct IssuedView {
    scope: Arc<PeerScope>,
    stamp: Stamp,
    epochs: TerminalEpochs,
    phase: AtomicU8,
}
#[derive(Clone)]
pub(super) struct SyncedView {
    record: Arc<IssuedView>,
}
impl SyncedView {
    pub(super) fn digest(
        &self,
        key: &DigestKey,
        operation: &OperationKey,
        epoch: u64,
        bytes: &[u8],
    ) -> Result<KeyedDigest, Reject> {
        let context = encode_ack(
            &Ack {
                profile: 1,
                stamp: self.record.stamp.clone(),
            },
            self.record.scope.minor,
        )
        .map_err(|_| Reject::Bounds)?;
        key.input_in_context(operation, epoch, bytes, self.record.scope.minor, &context)
            .map_err(|_| Reject::Bounds)
    }
}
pub(super) struct ViewStart {
    pub(super) terminal: CanonicalStart,
    pub(super) binding: [u8; 32],
    pub(super) minor: u16,
}
pub(super) struct PeerViewHandle {
    input: LocalInputHandle,
    projection: ProjectionHandle,
    control: Arc<PeerControl>,
}
pub(super) struct PendingViewAck {
    reply: mpsc::Receiver<Result<SyncedView, Reject>>,
    _credit: Arc<Credit>,
}
pub(super) struct ViewAckRequest {
    pub(super) scope: Arc<PeerScope>,
    pub(super) ack: Ack,
    pub(super) reply: mpsc::SyncSender<Result<SyncedView, Reject>>,
    _credit: Arc<Credit>,
}
impl PendingViewAck {
    pub(super) fn wait(&self, budget: Duration) -> Result<Result<SyncedView, Reject>, WaitError> {
        self.reply.recv_timeout(budget).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
impl PeerViewHandle {
    pub(super) fn capture(&self) -> Result<PendingProjection, Reject> {
        if !self.control.scope.is_active() {
            return Err(Reject::Sync);
        }
        self.projection
            .capture_inner(Some(self.control.scope.clone()))
    }
    pub(super) fn acknowledge(&self, ack: Ack) -> Result<PendingViewAck, Reject> {
        if self.input.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        if !self.control.scope.is_active() {
            return Err(Reject::Sync);
        }
        let credit = self.input.credits.reserve()?;
        let bytes = encode_ack(&ack, self.control.scope.minor).map_err(|_| Reject::Bounds)?;
        let compact =
            cc_desk_remote_record::projection::decode_ack(&bytes, self.control.scope.minor)
                .map_err(|_| Reject::Bounds)?;
        let (tx, rx) = mpsc::sync_channel(1);
        let pending = PendingViewAck {
            reply: rx,
            _credit: credit.clone(),
        };
        self.input
            .tx
            .try_send(Command::ViewAck(ViewAckRequest {
                scope: self.control.scope.clone(),
                ack: compact,
                reply: tx,
                _credit: credit,
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                mpsc::TrySendError::Disconnected(_) => Reject::Closed,
            })?;
        Ok(pending)
    }
    pub(super) fn submit(
        &self,
        view: &SyncedView,
        key: OperationKey,
        epoch: u64,
        bytes: Vec<u8>,
    ) -> Result<PendingInput, Reject> {
        self.input
            .submit_inner(key, epoch, bytes, Some(view.clone()), || {})
    }
    pub(super) fn close(&self) {
        self.control.close();
    }
}
impl LocalWriter {
    pub(super) fn start_view_guarded(
        journal: Journal,
        stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
        start: ViewStart,
    ) -> Result<(Self, HostOutputHandle, PeerViewHandle), Reject> {
        let control = PeerControl::new(target.clone(), device, start.binding, start.minor)?;
        let budget = start.terminal.budget.clone();
        let context = Arc::new(());
        let mut writer = Self::start_with(
            journal,
            stdin,
            target,
            device,
            epoch,
            key,
            WriterConfig {
                replies: Some(ReplyState::canonical(context.clone(), start.terminal)),
                view_gate: Some(ViewGate::new(control.scope.clone())),
                #[cfg(test)]
                after_write: None,
            },
        )?;
        let projection = ProjectionHandle::new(&writer.handle, context.clone(), budget);
        let peer = PeerViewHandle {
            input: writer.handle(),
            projection: projection.clone(),
            control,
        };
        writer.projection = Some(projection);
        let output = HostOutputHandle::new(&writer.handle, context);
        Ok((writer, output, peer))
    }
}
pub(super) struct ViewGate {
    scope: Arc<PeerScope>,
    next_sync: u64,
    current: Option<Arc<IssuedView>>,
    accepted: bool,
}
impl ViewGate {
    pub(super) fn effect_if_current(
        &self,
        request: &Request,
        replies: Option<&ReplyState>,
        effect: impl FnOnce() -> Outcome,
    ) -> Outcome {
        if self.check_request(request, replies).is_err() {
            Outcome::Rejected
        } else {
            effect()
        }
    }
    pub(super) fn scope(&self) -> &Arc<PeerScope> {
        &self.scope
    }
    pub(super) fn check_request(
        &self,
        request: &Request,
        replies: Option<&ReplyState>,
    ) -> Result<(), Reject> {
        if request
            .source_scope
            .as_ref()
            .is_none_or(|source| !Arc::ptr_eq(source, &self.scope))
        {
            return Err(Reject::Sync);
        }
        self.check(
            request.view.as_ref().ok_or(Reject::Sync)?,
            &request.key.device,
            replies.and_then(ReplyState::epochs).ok_or(Reject::Sync)?,
        )
    }
    pub(super) fn new(scope: Arc<PeerScope>) -> Self {
        Self {
            scope,
            next_sync: 0,
            current: None,
            accepted: false,
        }
    }
    pub(super) fn issue(
        &mut self,
        mut frame: projection::CapturedProjection,
        scope: &Arc<PeerScope>,
    ) -> Result<projection::CapturedProjection, Reject> {
        if !Arc::ptr_eq(scope, &self.scope)
            || !scope.is_active()
            || frame.target() != &scope.target
            || frame.issued.is_some()
        {
            return Err(Reject::Sync);
        }
        let next = self.next_sync.checked_add(1).ok_or_else(|| {
            scope.close();
            Reject::Bounds
        })?;
        let mut barrier = [0; 16];
        barrier[..8].copy_from_slice(&frame.writer_epoch().to_be_bytes());
        barrier[8..].copy_from_slice(&next.to_be_bytes());
        let stamp = frame.stamp(&projection::ProjectionContext {
            binding: scope.binding,
            barrier,
            sync_epoch: next,
        });
        encode_ack(
            &Ack {
                profile: 1,
                stamp: stamp.clone(),
            },
            scope.minor,
        )
        .map_err(|_| Reject::Bounds)?;
        if let Some(old) = &self.current {
            old.phase.store(REVOKED, Ordering::Release);
        }
        let record = Arc::new(IssuedView {
            scope: scope.clone(),
            stamp,
            epochs: frame.view().epochs(),
            phase: AtomicU8::new(ISSUED),
        });
        self.next_sync = next;
        self.accepted = false;
        self.current = Some(record.clone());
        frame.issued = Some(record);
        Ok(frame)
    }
    pub(super) fn acknowledge(
        &mut self,
        scope: &Arc<PeerScope>,
        ack: &Ack,
        epochs: TerminalEpochs,
    ) -> Result<SyncedView, Reject> {
        let record = self.current.as_ref().ok_or(Reject::Sync)?;
        if !Arc::ptr_eq(scope, &self.scope)
            || !scope.is_active()
            || record.phase.load(Ordering::Acquire) != DELIVERED
            || record.epochs != epochs
            || ack.profile != 1
            || ack.stamp != record.stamp
        {
            return Err(Reject::Sync);
        }
        self.accepted = true;
        Ok(SyncedView {
            record: record.clone(),
        })
    }
    pub(super) fn check(
        &self,
        view: &SyncedView,
        device: &[u8; 32],
        epochs: TerminalEpochs,
    ) -> Result<(), Reject> {
        let current = self.current.as_ref().ok_or(Reject::Sync)?;
        if !self.accepted
            || !Arc::ptr_eq(&view.record, current)
            || !self.scope.is_active()
            || device != &self.scope.device
            || current.phase.load(Ordering::Acquire) != DELIVERED
            || current.epochs != epochs
        {
            return Err(Reject::Sync);
        }
        Ok(())
    }
}
impl Drop for ViewGate {
    fn drop(&mut self) {
        self.scope.close();
    }
}
#[path = "local_writer_view_delivery.rs"]
mod delivery;
#[cfg(all(test, windows))]
#[path = "local_writer_view_gate_tests.rs"]
pub(super) mod tests;
