use super::*;
use control::ControlAdmission;
pub(in crate::local_writer) struct ControlStatus {
    pub(in crate::local_writer) fence: u64,
    pub(in crate::local_writer) device: [u8; 32],
    // Native scope lifetime only; not grant/lease/data authorization evidence.
    pub(in crate::local_writer) active: bool,
    pub(in crate::local_writer) pending: Option<OperationKey>,
}
pub(in crate::local_writer) struct PendingStatus {
    reply: mpsc::Receiver<Result<ControlStatus, Reject>>,
    _credit: Arc<Credit>,
}
impl PendingStatus {
    pub(in crate::local_writer) fn wait(
        &self,
        budget: Duration,
    ) -> Result<Result<ControlStatus, Reject>, WaitError> {
        self.reply.recv_timeout(budget).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
pub(in crate::local_writer) struct PendingRebind {
    reply: mpsc::Receiver<Result<PeerViewHandle, Reject>>,
    _credit: Arc<Credit>,
}
impl PendingRebind {
    pub(in crate::local_writer) fn wait(
        &self,
        budget: Duration,
    ) -> Result<Result<PeerViewHandle, Reject>, WaitError> {
        self.reply.recv_timeout(budget).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
impl LocalWriter {
    pub(in crate::local_writer) fn control_status(&self) -> Result<PendingStatus, Reject> {
        if self.handle.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        let credit = self.handle.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.handle
            .tx
            .try_send(Command::ControlStatus(StatusRequest {
                reply: tx,
                _credit: credit.clone(),
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                mpsc::TrySendError::Disconnected(_) => Reject::Closed,
            })?;
        Ok(PendingStatus {
            reply: rx,
            _credit: credit,
        })
    }
    pub(in crate::local_writer) fn rebind(
        &self,
        device: [u8; 32],
        expected_fence: u64,
        admission: ControlAdmission,
    ) -> Result<PendingRebind, Reject> {
        if self.handle.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        let projection = self.projection.as_ref().ok_or(Reject::Unsupported)?.clone();
        let credit = self.handle.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.handle
            .tx
            .try_send(Command::Rebind(Box::new(RebindRequest {
                device,
                expected_fence,
                admission,
                input: self.handle.clone(),
                projection,
                reply: tx,
                _credit: credit.clone(),
            })))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                mpsc::TrySendError::Disconnected(_) => Reject::Closed,
            })?;
        Ok(PendingRebind {
            reply: rx,
            _credit: credit,
        })
    }
}
pub(in crate::local_writer) struct StatusRequest {
    reply: mpsc::SyncSender<Result<ControlStatus, Reject>>,
    _credit: Arc<Credit>,
}
impl StatusRequest {
    pub(in crate::local_writer) fn reject(self, error: Reject) {
        let _ = self.reply.try_send(Err(error));
    }
    pub(in crate::local_writer) fn process(self, gate: &ViewGate) {
        let result = gate
            .control
            .as_ref()
            .ok_or(Reject::Unsupported)
            .map(|c| c.status(&gate.scope));
        let _ = self.reply.try_send(result);
    }
}
pub(in crate::local_writer) struct RebindRequest {
    device: [u8; 32],
    expected_fence: u64,
    admission: ControlAdmission,
    input: LocalInputHandle,
    projection: ProjectionHandle,
    reply: mpsc::SyncSender<Result<PeerViewHandle, Reject>>,
    _credit: Arc<Credit>,
}
impl RebindRequest {
    pub(in crate::local_writer) fn reject(self, error: Reject) {
        let _ = self.reply.try_send(Err(error));
    }
    pub(in crate::local_writer) fn process(self, gate: &mut ViewGate, journal: &Journal) {
        let result = (|| {
            gate.resize_ready()?;
            if self.input.target.as_ref() != &gate.scope.target {
                return Err(Reject::Target);
            }
            let replacement = gate
                .control
                .as_ref()
                .ok_or(Reject::Unsupported)?
                .replacement(
                    &gate.scope,
                    self.device,
                    self.expected_fence,
                    &self.admission,
                    journal,
                )?;
            let control = PeerControl::on_writer(
                gate.scope.target.clone(),
                self.device,
                self.admission.binding,
                self.admission.minor,
                gate.scope.writer_closed.clone(),
            )?;
            if let Some(current) = &gate.current {
                // Install the successor's proof before publishing it; old
                // scope retirement cancels only the old TLS owner.
                current.phase.store(REVOKED, Ordering::Release);
            }
            if let Some(authority) = &replacement.authority {
                control
                    .scope
                    .authority
                    .set(authority.clone())
                    .map_err(|_| Reject::Policy)?;
            }
            gate.scope.close();
            gate.scope = control.scope.clone();
            gate.control = Some(replacement);
            gate.current = None;
            gate.accepted = false;
            let mut input = self.input;
            input.device = self.device;
            input.view_scope = Some(control.scope.clone());
            Ok(PeerViewHandle {
                input,
                projection: self.projection,
                control,
            })
        })();
        let _ = self.reply.try_send(result);
    }
}
