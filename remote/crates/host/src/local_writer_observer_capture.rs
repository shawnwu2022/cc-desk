use super::*;
use cc_desk_remote_record::grant::{DataRole, SessionScope};

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
