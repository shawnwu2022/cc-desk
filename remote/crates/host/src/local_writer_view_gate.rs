//! Exact native install evidence only; no endpoint grant or input lease.
use super::*;
use cc_desk_remote_record::projection::{Ack, Stamp, encode_ack};
use cc_desk_remote_terminal::TerminalEpochs;

use std::sync::atomic::AtomicU8;
#[path = "local_writer_control.rs"]
pub(super) mod control;
use control::ControlStart;
#[path = "local_writer_rebind.rs"]
pub(super) mod rebind;
const ISSUED: u8 = 0;
const SENDING: u8 = 1;
const DELIVERED: u8 = 2;
const REVOKED: u8 = 3;
pub(super) struct PeerScope {
    authority: std::sync::OnceLock<cc_desk_remote_security::BoundDataAuthority>,
    target: Target,
    device: [u8; 32],
    binding: [u8; 32],
    minor: u16,
    active: AtomicBool,
    publication_gate: std::sync::Mutex<()>,
    closed: tokio::sync::watch::Sender<bool>,
    writer_closed: Arc<tokio::sync::watch::Sender<bool>>,
}
impl PeerScope {
    pub(super) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire) && !*self.writer_closed.borrow()
    }
    pub(super) fn close(&self) {
        let _guard = self
            .publication_gate
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        self.close_locked();
    }
    fn close_locked(&self) {
        self.active.store(false, Ordering::Release);
        if let Some(authority) = self.authority.get() {
            authority.request_close();
        }
        self.closed.send_replace(true);
    }
    pub(super) fn close_writer(&self) {
        let _guard = self
            .publication_gate
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        self.writer_closed.send_replace(true);
        self.close_locked();
    }
    pub(super) fn with_publication<T>(
        &self,
        f: impl FnOnce() -> Result<T, Reject>,
    ) -> Result<T, Reject> {
        let _guard = self.publication_gate.lock().map_err(|_| Reject::Thread)?;
        if !self.is_active() {
            return Err(Reject::Closed);
        }
        f()
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
        let (writer_closed, _) = tokio::sync::watch::channel(false);
        Self::on_writer(target, device, binding, minor, Arc::new(writer_closed))
    }
    fn on_writer(
        target: Target,
        device: [u8; 32],
        binding: [u8; 32],
        minor: u16,
        writer_closed: Arc<tokio::sync::watch::Sender<bool>>,
    ) -> Result<Arc<Self>, Reject> {
        if minor > 1 {
            return Err(Reject::Bounds);
        }
        let (closed, _) = tokio::sync::watch::channel(false);
        Ok(Arc::new(Self {
            scope: Arc::new(PeerScope {
                authority: std::sync::OnceLock::new(),
                target,
                device,
                binding,
                minor,
                active: AtomicBool::new(true),
                publication_gate: std::sync::Mutex::new(()),
                closed,
                writer_closed,
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
    read_grant: Option<control::ReadGrant>,
}
#[derive(Clone)]
pub(super) struct SyncedView {
    record: Arc<IssuedView>,
    window: Option<Arc<control::InputWindow>>,
}
impl SyncedView {
    #[cfg(test)]
    pub(super) fn diagnostic_epochs(&self) -> TerminalEpochs {
        self.record.epochs
    }
    pub(super) fn digest(
        &self,
        key: &DigestKey,
        operation: &OperationKey,
        epoch: u64,
        bytes: &[u8],
    ) -> Result<KeyedDigest, Reject> {
        let mut context = encode_ack(
            &Ack {
                profile: 1,
                stamp: self.record.stamp.clone(),
            },
            self.record.scope.minor,
        )
        .map_err(|_| Reject::Bounds)?;
        if let Some(window) = &self.window {
            window.append_digest(&mut context);
        }
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
    pub(super) fn prepare_durable_resize(
        &self,
        request: super::resize::durable::Prepare,
    ) -> Result<super::resize::durable::Pending<super::resize::durable::Prepared>, Reject> {
        super::resize::durable::prepare(&self.input, self.control.scope.clone(), request)
    }
    pub(super) fn resize_candidate(
        &self,
        request: cc_desk_remote_record::lifecycle::Request,
        budget: Duration,
    ) -> Result<super::resize::Pending, Reject> {
        super::resize::enqueue(&self.input, self.control.scope.clone(), request, budget)
    }
    pub(super) fn device(&self) -> [u8; 32] {
        self.control.scope.device
    }
    pub(super) fn submit_bound(
        &self,
        view: &SyncedView,
        key: OperationKey,
        bytes: Vec<u8>,
    ) -> Result<PendingInput, Reject> {
        self.submit(view, key, self.input.epoch, bytes)
    }
    pub(super) fn wire_window(&self, view: &SyncedView) -> Result<([u8; 32], u64), Reject> {
        if !Arc::ptr_eq(&view.record.scope, &self.control.scope) || !self.control.scope.is_active()
        {
            return Err(Reject::Sync);
        }
        view.window
            .as_ref()
            .map(|w| (w.wire_token, w.deadline_ms))
            .ok_or(Reject::Policy)
    }
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
    pub(super) fn start_control_guarded(
        journal: Journal,
        stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
        start: ControlStart,
    ) -> Result<(Self, HostOutputHandle, PeerViewHandle), Reject> {
        Self::start_guarded_inner(journal, stdin, target, device, epoch, key, start)
    }
    pub(super) fn start_view_guarded(
        journal: Journal,
        stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
        start: ViewStart,
    ) -> Result<(Self, HostOutputHandle, PeerViewHandle), Reject> {
        Self::start_guarded_inner(
            journal,
            stdin,
            target,
            device,
            epoch,
            key,
            ControlStart {
                authority: None,
                view: start,
                policy: None,
            },
        )
    }
    fn start_guarded_inner(
        journal: Journal,
        stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
        start: ControlStart,
    ) -> Result<(Self, HostOutputHandle, PeerViewHandle), Reject> {
        Self::start_guarded_with(journal, stdin, target, device, epoch, key, (start, None))
    }
    fn start_guarded_with(
        journal: Journal,
        stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
        configured: (ControlStart, Option<super::resize::State>),
    ) -> Result<(Self, HostOutputHandle, PeerViewHandle), Reject> {
        let (start, resize) = configured;
        let mut policy = start
            .policy
            .map(|(clock, grant)| control::ControlState::new(clock, grant))
            .transpose()?;
        if let Some(policy) = &mut policy {
            policy.authority = start.authority;
            policy.restore_pending(&journal, &target, &device)?;
        }
        let start = start.view;
        let control = PeerControl::new(target.clone(), device, start.binding, start.minor)?;
        if let Some(authority) = policy.as_ref().and_then(|p| p.authority.as_ref()) {
            control
                .scope
                .authority
                .set(authority.clone())
                .map_err(|_| Reject::Policy)?;
        }
        if let Some(policy) = &policy {
            policy.authorized(&control.scope)?;
        }
        let mut gate = ViewGate::new(control.scope.clone());
        gate.control = policy;
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
                view_gate: Some(gate),
                resize,
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
    pub(super) durable_resize: Option<Arc<super::resize::durable::Reservation>>,
    scope: Arc<PeerScope>,
    next_sync: u64,
    current: Option<Arc<IssuedView>>,
    accepted: bool,
    control: Option<control::ControlState>,
    pub(super) resize_feedback: Option<(std::time::Instant, Arc<AtomicBool>)>,
}
impl ViewGate {
    pub(super) fn resize_expired(&self) -> bool {
        if self.durable_resize.as_ref().is_some_and(|r| r.expired()) {
            return true;
        }
        self.resize_feedback
            .as_ref()
            .is_some_and(|(end, observed)| {
                !observed.load(Ordering::Acquire) && std::time::Instant::now() >= *end
            })
    }
    pub(super) fn resize_ready(&self) -> Result<(), Reject> {
        if let Some(reservation) = &self.durable_resize {
            reservation.ready()?;
        }
        if self.resize_expired() {
            self.scope.close_writer();
            return Err(Reject::Sealed);
        }
        if self
            .resize_feedback
            .as_ref()
            .is_some_and(|(_, observed)| !observed.load(Ordering::Acquire))
        {
            return Err(Reject::Pending);
        }
        Ok(())
    }
    pub(super) fn resize_state(
        &self,
        scope: &Arc<PeerScope>,
        epochs: TerminalEpochs,
    ) -> Result<cc_desk_remote_record::lifecycle::State, Reject> {
        if !Arc::ptr_eq(scope, &self.scope) || !scope.is_active() {
            return Err(Reject::Policy);
        }
        let control = self.control.as_ref().ok_or(Reject::Unsupported)?;
        control.eligible(scope)?;
        Ok(cc_desk_remote_record::lifecycle::State {
            generation: self.scope.target.generation,
            fence: control.status(scope).fence,
            mode: epochs.mode_epoch,
            dimension: epochs.dimension_epoch,
            sync: self.next_sync,
        })
    }
    pub(super) fn resize_proof(
        &self,
        scope: &Arc<PeerScope>,
        proof: &cc_desk_remote_security::BoundDataAuthority,
    ) -> Result<(), Reject> {
        if !Arc::ptr_eq(scope, &self.scope) || !scope.is_active() {
            return Err(Reject::Policy);
        }
        proof.check().map_err(|_| Reject::Policy)?;
        let hex = |b: &[u8]| b.iter().map(|n| format!("{n:02x}")).collect::<String>();
        if proof.local_role() != cc_desk_remote_security::Role::Host
            || proof.data_role() != cc_desk_remote_record::grant::DataRole::Controller
            || proof.context().host_endpoint_id != hex(&scope.target.host)
            || proof.context().client_endpoint_id != hex(&scope.device)
            || proof.scope().session != hex(&scope.target.session)
            || proof.scope().run != scope.target.run
            || proof.scope().generation != scope.target.generation.to_string()
            || proof.context().incarnation != scope.target.incarnation.to_string()
        {
            return Err(Reject::Policy);
        }
        let authority = self
            .control
            .as_ref()
            .and_then(|c| c.authority.as_ref())
            .ok_or(Reject::Policy)?;
        if !authority.same_channel(proof) {
            return Err(Reject::Policy);
        }
        Ok(())
    }
    pub(super) fn invalidate_for_resize(&mut self) {
        if let Some(old) = self.current.take() {
            old.phase.store(REVOKED, Ordering::Release);
        }
        self.accepted = false;
        if let Some(control) = &mut self.control {
            control.revoke_windows_for_resize();
        }
    }
    pub(super) fn device(&self) -> &[u8; 32] {
        &self.scope.device
    }
    pub(super) fn can_admit(
        &self,
        key: &OperationKey,
        digest: &KeyedDigest,
    ) -> Result<(), control::AdmissionError> {
        self.control
            .as_ref()
            .map_or(Ok(()), |c| c.can_admit(key, digest))
    }
    pub(super) fn note_admitted(&mut self, key: &OperationKey, digest: &KeyedDigest) {
        if let Some(control) = &mut self.control {
            control.note_admitted(key, digest);
        }
    }
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
        self.resize_ready()?;
        if request
            .source_scope
            .as_ref()
            .is_none_or(|source| !Arc::ptr_eq(source, &self.scope))
        {
            return Err(Reject::Sync);
        }
        let view = request.view.as_ref().ok_or(Reject::Sync)?;
        self.check(
            view,
            &request.key.device,
            replies.and_then(ReplyState::epochs).ok_or(Reject::Sync)?,
        )?;
        if let Some(control) = &self.control {
            control.check(&self.scope, view)?;
        }
        Ok(())
    }
    pub(super) fn new(scope: Arc<PeerScope>) -> Self {
        Self {
            durable_resize: None,
            scope,
            next_sync: 0,
            current: None,
            accepted: false,
            control: None,
            resize_feedback: None,
        }
    }
    pub(super) fn issue(
        &mut self,
        mut frame: projection::CapturedProjection,
        scope: &Arc<PeerScope>,
    ) -> Result<projection::CapturedProjection, Reject> {
        self.resize_ready()?;
        if !Arc::ptr_eq(scope, &self.scope)
            || !scope.is_active()
            || frame.target() != &scope.target
            || frame.issued.is_some()
        {
            return Err(Reject::Sync);
        }
        let read_grant = self
            .control
            .as_ref()
            .map(|c| c.read_grant(scope))
            .transpose()?;
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
            read_grant,
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
        self.resize_ready()?;
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
        if let Some(grant) = &record.read_grant {
            grant.remaining(scope)?;
        }
        self.accepted = true;
        Ok(SyncedView {
            record: record.clone(),
            window: None,
        })
    }
    pub(super) fn check(
        &self,
        view: &SyncedView,
        device: &[u8; 32],
        epochs: TerminalEpochs,
    ) -> Result<(), Reject> {
        self.resize_ready()?;
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
        self.scope.close_writer();
    }
}
#[path = "local_writer_view_delivery.rs"]
mod delivery;
#[cfg(test)]
#[path = "local_writer_resize_durable_tests.rs"]
mod durable_resize_tests;
#[cfg(test)]
#[path = "local_writer_resize_tests.rs"]
mod resize_tests;
#[cfg(all(test, windows))]
#[path = "local_writer_view_gate_tests.rs"]
pub(super) mod tests;
