use super::*;
use cc_desk_remote_record::{
    Kind,
    grant::{DATA_PREFIX, DataRole, SessionScope},
    projection as wire,
};
use cc_desk_remote_security::{DataError, ObserverProjectionSender};
use cc_desk_remote_terminal::SnapshotEncoder;
use ring::rand::SecureRandom;
use tokio::time::{Instant, timeout_at};

const MAX_OBSERVER_CAPTURE_WAIT: Duration = Duration::from_secs(1);

/// Holds the originating producer's canonical continuity and the exact grant.
/// This path never takes ownership of the producer, input sink, or peer custody.
#[derive(Clone)]
struct ObserverCaptureBinding {
    authority: BoundDataAuthority,
    scope: SessionScope,
    complete: Arc<AtomicBool>,
    sealed: Arc<AtomicBool>,
}

impl ObserverCaptureBinding {
    fn check(&self, authority: &BoundDataAuthority) -> Result<(), Reject> {
        if !self.complete.load(Ordering::Acquire)
            || self.sealed.load(Ordering::Acquire)
            || authority.local_role() != Role::Host
            || authority.data_role() != DataRole::Observer
            || authority.scope() != &self.scope
            || authority.control_lease_renew_enabled()
            || authority.data_keepalive_negotiated()
            || !self.authority.same_channel(authority)
            || self.authority.check().is_err()
            || authority.check().is_err()
        {
            return Err(Reject::Policy);
        }
        Ok(())
    }
}

/// One request in the existing bounded canonical projection queue.
pub struct PendingObserverCapture {
    binding: ObserverCaptureBinding,
    pending: PendingProjection,
    _queued: Arc<()>,
}

impl PendingObserverCapture {
    /// Synchronous waiter. A caller must not hold run locks or block an async
    /// executor while waiting; this does not install a peer or send frames.
    pub fn wait(self, budget: Duration) -> Result<ObserverCapture, Reject> {
        self.binding.check(&self.binding.authority)?;
        if budget.is_zero() || budget > MAX_OBSERVER_CAPTURE_WAIT {
            return Err(Reject::Policy);
        }
        let captured = self
            .pending
            .wait(budget)
            .map_err(|failure| match failure {
                WaitError::Uncertain => Reject::Pending,
                WaitError::Closed => Reject::Closed,
            })??;
        self.binding.check(&self.binding.authority)?;
        if captured.target().run != self.binding.scope.run
            || captured.target().generation.to_string() != self.binding.scope.generation
        {
            return Err(Reject::Target);
        }
        Ok(ObserverCapture {
            binding: self.binding,
            captured,
        })
    }
}

/// Holds the existing view credit until dropped. Its trusted scope comes
/// from the producer's real native session, not its raw projection sentinel.
pub struct ObserverCapture {
    binding: ObserverCaptureBinding,
    captured: projection::CapturedProjection,
}

impl ObserverCapture {
    pub fn scope(&self) -> &SessionScope {
        &self.binding.scope
    }

    /// Recheck using the original channel's authority. Borrowed output is
    /// not revocable; recheck again at the eventual frame emission boundary.
    pub fn view(
        &self,
        authority: &BoundDataAuthority,
    ) -> Result<&cc_desk_remote_terminal::SemanticSnapshot, Reject> {
        self.binding.check(authority)?;
        Ok(self.captured.view())
    }
}

impl NativeRunProducer {
    pub fn request_observer_capture(
        &self,
        authority: BoundDataAuthority,
    ) -> Result<PendingObserverCapture, Reject> {
        let binding = ObserverCaptureBinding {
            authority,
            scope: self.native_scope(),
            complete: self.complete.clone(),
            sealed: self.writer.handle.sealed.clone(),
        };
        binding.check(&binding.authority)?;
        let queued = Arc::new(());
        let admission = binding.clone();
        let live = Arc::downgrade(&queued);
        let pending = self
            .writer
            .projection
            .as_ref()
            .ok_or(Reject::Unsupported)?
            .capture_observer(Box::new(move || {
                live.upgrade().is_some() && admission.check(&admission.authority).is_ok()
            }))?;
        binding.check(&binding.authority)?;
        Ok(PendingObserverCapture {
            binding,
            pending,
            _queued: queued,
        })
    }
}

struct CloseObserver(Option<BoundDataAuthority>);

impl Drop for CloseObserver {
    fn drop(&mut self) {
        if let Some(authority) = &self.0 {
            authority.request_close();
        }
    }
}

/// Already installed output-only peer on the original authenticated channel.
/// No controller actor or writer capability is acquired. The caller owns the
/// channel and must still close it on original run retirement.
pub struct ObserverSnapshotTransfer<'a> {
    capture: ObserverCapture,
    authority: BoundDataAuthority,
    sender: ObserverProjectionSender<'a>,
    stamp: wire::Stamp,
}

impl ObserverCapture {
    /// Complete the real peer request only after validating this native capture.
    /// The fence and view barrier are generated here; callers cannot supply them.
    pub async fn install_snapshot(
        self,
        channel: &RunningChannel,
        budget: Duration,
    ) -> Result<ObserverSnapshotTransfer<'_>, DataError> {
        let authority = channel.authority();
        self.view(&authority).map_err(|_| DataError::Context)?;

        if budget.is_zero() || budget > Duration::from_secs(60) {
            return Err(DataError::Bounds);
        }
        let end = Instant::now()
            .checked_add(budget)
            .ok_or(DataError::Bounds)?;
        let mut nonce = [0; 24];
        ring::rand::SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| DataError::Context)?;
        let fence = u64::from_be_bytes(nonce[..8].try_into().expect("fixed fence bytes"));
        if fence == 0 || nonce[8..] == [0; 16] {
            return Err(DataError::Fence);
        }
        let context = authority.context();
        let scope = self.scope();
        let epochs = self
            .view(&authority)
            .map_err(|_| DataError::Context)?
            .epochs();
        let hex = |data: &[u8]| data.iter().map(|b| format!("{b:02x}")).collect();
        let stamp = wire::Stamp {
            target: wire::Target {
                host: context.host_endpoint_id.clone(),
                session: scope.session.clone(),
                run: scope.run.clone(),
                generation: scope.generation.clone(),
                incarnation: context.incarnation.clone(),
            },
            binding: hex(&authority.binding_id()),
            barrier: hex(&nonce[8..]),
            revision: self.captured.revision().to_string(),
            dimension_epoch: epochs.dimension_epoch.to_string(),
            mode_epoch: epochs.mode_epoch.to_string(),
            structure_epoch: epochs.structure_epoch.to_string(),
            sync_epoch: "1".into(),
        };
        // Preflight the full bounded encoding before completing the transport install.
        // The peer receipt is transport-only, not proof a client rendered the view.
        for part in self.encoder(&authority, stamp.clone())? {
            part.map_err(|_| DataError::Codec)?;
            if Instant::now() >= end {
                return Err(DataError::Deadline);
            }
        }
        let request = timeout_at(end, channel.host_install_request())
            .await
            .map_err(|_| DataError::Deadline)??;
        let mut close = CloseObserver(Some(authority.clone()));
        self.view(&authority).map_err(|_| DataError::Context)?;
        timeout_at(end, channel.complete_host_install(request, fence))
            .await
            .map_err(|_| DataError::Deadline)??;
        self.view(&authority).map_err(|_| DataError::Context)?;
        let source = self.binding.clone();
        let sender = channel.observer_projection_sender_until(end, move || {
            source.check(&source.authority).is_ok()
        })?;
        close.0 = None;
        Ok(ObserverSnapshotTransfer {
            capture: self,
            authority,
            sender,
            stamp,
        })
    }

    fn encoder(
        &self,
        authority: &BoundDataAuthority,
        stamp: wire::Stamp,
    ) -> Result<SnapshotEncoder<'_>, DataError> {
        SnapshotEncoder::with_payload_limit(
            self.view(authority).map_err(|_| DataError::Context)?,
            stamp,
            1,
            Kind::Projection.limit() - DATA_PREFIX,
        )
        .map_err(|_| DataError::Codec)
    }
}

/// Emission finished, not a browser display receipt. Only retirement is exposed.
pub struct ObserverSnapshotSent<'a>(ObserverProjectionSender<'a>);

impl ObserverSnapshotSent<'_> {
    pub async fn retire(self) -> Result<DataError, DataError> {
        self.0.retire().await
    }
}

impl<'a> ObserverSnapshotTransfer<'a> {
    /// Consume the transfer so no frame can be replayed. Return the same
    /// transport owner for checked retirement or a future typed receipt path.
    pub async fn send_snapshot(self) -> Result<ObserverSnapshotSent<'a>, DataError> {
        let Self {
            capture,
            authority,
            mut sender,
            stamp,
        } = self;
        for part in capture.encoder(&authority, stamp)? {
            capture.view(&authority).map_err(|_| DataError::Context)?;
            let part = part.map_err(|_| DataError::Codec)?;
            let message = wire::decode(&part, 1).map_err(|_| DataError::Codec)?;
            sender.send_snapshot_part(message).await?;
            capture.view(&authority).map_err(|_| DataError::Context)?;
        }
        Ok(ObserverSnapshotSent(sender))
    }
}
