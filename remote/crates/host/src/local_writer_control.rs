use super::*;
use std::sync::Mutex;
#[path = "local_writer_control_receipts.rs"]
mod receipts;
#[path = "local_writer_control_transition.rs"]
mod transition;

// This native-only input is supplied by a future verified data-authority factory,
// never directly deserialized from a peer. It does not authenticate a grant.
pub(in crate::local_writer) trait HostClock: Send + Sync {
    fn now_ms(&self) -> Result<u64, Reject>;
}
impl<F: Fn() -> Result<u64, Reject> + Send + Sync> HostClock for F {
    fn now_ms(&self) -> Result<u64, Reject> {
        self()
    }
}
#[derive(Clone, Copy)]
pub(in crate::local_writer) struct GrantContext {
    pub(in crate::local_writer) challenge_ms: u64,
    pub(in crate::local_writer) grant: [u8; 32],
    pub(in crate::local_writer) lease: [u8; 16],
    pub(in crate::local_writer) fence: u64,
    pub(in crate::local_writer) policy: u64,
}
pub(in crate::local_writer) struct ControlAdmission {
    pub(in crate::local_writer) binding: [u8; 32],
    pub(in crate::local_writer) minor: u16,
    pub(in crate::local_writer) clock: Arc<dyn HostClock>,
    pub(in crate::local_writer) grant: GrantContext,
}
pub(in crate::local_writer) struct ControlStart {
    pub(super) view: ViewStart,
    pub(super) policy: Option<(Arc<dyn HostClock>, GrantContext)>,
}
impl ControlStart {
    pub(in crate::local_writer) fn new(
        view: ViewStart,
        clock: Arc<dyn HostClock>,
        grant: GrantContext,
    ) -> Self {
        Self {
            view,
            policy: Some((clock, grant)),
        }
    }
}

/// Runtime-owned origin; a new origin is a new namespace, not grant renewal.
pub(super) struct MonotonicClock(std::time::Instant);
impl MonotonicClock {
    pub(super) fn new() -> Self {
        Self(std::time::Instant::now())
    }
}
impl HostClock for MonotonicClock {
    fn now_ms(&self) -> Result<u64, Reject> {
        self.0
            .elapsed()
            .as_millis()
            .try_into()
            .map_err(|_| Reject::Policy)
    }
}
struct Identity {
    grant: [u8; 32],
    lease: [u8; 16],
    fence: u64,
    policy: u64,
}
pub(super) struct InputWindow {
    identity: Arc<Identity>,
    view: Arc<IssuedView>,
    id: [u8; 16],
    issued_ms: u64,
    deadline_ms: u64,
}
impl InputWindow {
    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(b"\0CC-DESK-CONTROL-WINDOW-v1\0");
        bytes.extend_from_slice(&self.identity.grant);
        bytes.extend_from_slice(&self.identity.lease);
        bytes.extend_from_slice(&self.identity.fence.to_be_bytes());
        bytes.extend_from_slice(&self.identity.policy.to_be_bytes());
        bytes.extend_from_slice(&self.id);
        bytes.extend_from_slice(&self.issued_ms.to_be_bytes());
        bytes.extend_from_slice(&self.deadline_ms.to_be_bytes());
    }
}
pub(super) struct ControlState {
    clock: Arc<dyn HostClock>,
    last_ms: Mutex<u64>,
    identity: Arc<Identity>,
    grant_deadline: u64,
    lease_deadline: u64,
    renew_sequence: u64,
    next_window: u64,
    windows: Vec<Arc<InputWindow>>,
    pending: Option<(OperationKey, [u8; 32])>,
}
pub(in crate::local_writer) enum AdmissionError {
    Pending,
    Conflict,
}
impl AdmissionError {
    pub(in crate::local_writer) fn feedback(self) -> WriteResult {
        match self {
            Self::Pending => WriteResult::Rejected(Reject::Pending),
            Self::Conflict => WriteResult::Journal(JournalError::Conflict),
        }
    }
}
impl ControlState {
    pub(super) fn new(clock: Arc<dyn HostClock>, grant: GrantContext) -> Result<Self, Reject> {
        let now = clock.now_ms()?;
        Self::at(clock, grant, now, true)
    }
    fn at(
        clock: Arc<dyn HostClock>,
        grant: GrantContext,
        now: u64,
        require_response: bool,
    ) -> Result<Self, Reject> {
        let response_deadline = grant
            .challenge_ms
            .checked_add(30_000)
            .ok_or(Reject::Policy)?;
        let grant_deadline = grant
            .challenge_ms
            .checked_add(900_000)
            .ok_or(Reject::Policy)?;
        if now < grant.challenge_ms
            || (require_response && now >= response_deadline)
            || now >= grant_deadline
            || grant.fence == 0
            || grant.policy == 0
            || grant.grant == [0; 32]
            || grant.lease == [0; 16]
        {
            return Err(Reject::Policy);
        }
        let lease_deadline = now
            .checked_add(15_000)
            .ok_or(Reject::Policy)?
            .min(grant_deadline);
        Ok(Self {
            clock,
            last_ms: Mutex::new(now),
            identity: Arc::new(Identity {
                grant: grant.grant,
                lease: grant.lease,
                fence: grant.fence,
                policy: grant.policy,
            }),
            grant_deadline,
            lease_deadline,
            renew_sequence: 0,
            next_window: 0,
            windows: Vec::with_capacity(2),
            pending: None,
        })
    }
    pub(super) fn can_admit(
        &self,
        key: &OperationKey,
        digest: &KeyedDigest,
    ) -> Result<(), AdmissionError> {
        use subtle::ConstantTimeEq;
        if let Some((old, old_digest)) = &self.pending {
            if old != key {
                return Err(AdmissionError::Pending);
            }
            if !bool::from(old_digest.ct_eq(&digest.0)) {
                return Err(AdmissionError::Conflict);
            }
        }
        Ok(())
    }
    pub(super) fn note_admitted(&mut self, key: &OperationKey, digest: &KeyedDigest) {
        if self.pending.is_none() {
            self.pending = Some((key.clone(), digest.0));
        }
    }
    pub(super) fn status(&self, scope: &PeerScope) -> rebind::ControlStatus {
        rebind::ControlStatus {
            fence: self.identity.fence,
            device: scope.device,
            active: scope.is_active(),
            pending: self.pending.as_ref().map(|(key, _)| key.clone()),
        }
    }
    pub(super) fn restore_pending(
        &mut self,
        journal: &Journal,
        target: &Target,
        device: &[u8; 32],
    ) -> Result<(), Reject> {
        self.pending = journal
            .single_unsettled(target, device)
            .map_err(|error| match error {
                JournalError::Sequence => Reject::Pending,
                _ => Reject::Closed,
            })?
            .map(|(key, digest)| (key, digest.0));
        Ok(())
    }
    fn sample(&self, scope: &PeerScope) -> Result<u64, Reject> {
        let result = (|| {
            let now = self.clock.now_ms()?;
            let mut last = self.last_ms.lock().map_err(|_| Reject::Policy)?;
            if now < *last {
                return Err(Reject::Policy);
            }
            *last = now;
            Ok(now)
        })();
        if result.is_err() {
            scope.close_writer();
        }
        result
    }
    fn eligible(&self, scope: &PeerScope) -> Result<u64, Reject> {
        let now = self.sample(scope)?;
        if !scope.is_active() || now >= self.lease_deadline || now >= self.grant_deadline {
            return Err(Reject::Policy);
        }
        Ok(now)
    }
    pub(super) fn issue(
        &mut self,
        scope: &PeerScope,
        view: &SyncedView,
    ) -> Result<SyncedView, Reject> {
        let now = self.eligible(scope)?;
        // Only expired eligibility is removed. Never delete receipt/floor data.
        // The actor cannot issue while an old write is still executing.
        self.windows.retain(|w| now < w.deadline_ms);
        if self.windows.len() == 2 {
            return Err(Reject::Full);
        }
        let next = self.next_window.checked_add(1).ok_or_else(|| {
            scope.close();
            Reject::Policy
        })?;
        let deadline = now
            .checked_add(5000)
            .ok_or(Reject::Policy)?
            .min(self.lease_deadline)
            .min(self.grant_deadline);
        if now >= deadline {
            return Err(Reject::Policy);
        }
        let mut id = [0; 16];
        id[..8].copy_from_slice(&self.identity.fence.to_be_bytes());
        id[8..].copy_from_slice(&next.to_be_bytes());
        let window = Arc::new(InputWindow {
            identity: self.identity.clone(),
            view: view.record.clone(),
            id,
            issued_ms: now,
            deadline_ms: deadline,
        });
        self.next_window = next;
        self.windows.push(window.clone());
        Ok(SyncedView {
            record: view.record.clone(),
            window: Some(window),
        })
    }
    pub(super) fn check(&self, scope: &PeerScope, view: &SyncedView) -> Result<(), Reject> {
        let now = self.eligible(scope)?;
        let window = view.window.as_ref().ok_or(Reject::Policy)?;
        if !Arc::ptr_eq(&window.identity, &self.identity)
            || !Arc::ptr_eq(&window.view, &view.record)
            || !self.windows.iter().any(|w| Arc::ptr_eq(w, window))
            || now < window.issued_ms
            || now >= window.deadline_ms
        {
            return Err(Reject::Policy);
        }
        Ok(())
    }
    pub(super) fn renew(
        &mut self,
        scope: &PeerScope,
        lease: [u8; 16],
        fence: u64,
        sequence: u64,
    ) -> Result<u64, Reject> {
        let now = self.eligible(scope)?;
        if lease != self.identity.lease || fence != self.identity.fence || sequence == 0 {
            return Err(Reject::Policy);
        }
        if sequence == self.renew_sequence {
            return Ok(self.lease_deadline);
        }
        if Some(sequence) != self.renew_sequence.checked_add(1) {
            return Err(Reject::Policy);
        }
        self.lease_deadline = now
            .checked_add(15_000)
            .ok_or(Reject::Policy)?
            .min(self.grant_deadline);
        self.renew_sequence = sequence;
        Ok(self.lease_deadline)
    }
}

pub(super) enum ControlAction {
    Window {
        view: SyncedView,
        reply: mpsc::SyncSender<Result<SyncedView, Reject>>,
    },
    Renew {
        lease: [u8; 16],
        fence: u64,
        sequence: u64,
        reply: mpsc::SyncSender<Result<u64, Reject>>,
    },
    Query {
        key: OperationKey,
        reply: mpsc::SyncSender<Result<ReceiptLookup, receipts::ControlError>>,
    },
    Settle {
        proof: ReceiptProof,
        reply: mpsc::SyncSender<Result<AckDecision, receipts::ControlError>>,
    },
}
pub(in crate::local_writer) struct ControlRequest {
    pub(super) scope: Arc<PeerScope>,
    pub(super) action: ControlAction,
    pub(super) _credit: Arc<Credit>,
}
impl ControlRequest {
    pub(in crate::local_writer) fn reject(self, error: Reject) {
        match self.action {
            ControlAction::Window { reply, .. } => {
                let _ = reply.try_send(Err(error));
            }
            ControlAction::Renew { reply, .. } => {
                let _ = reply.try_send(Err(error));
            }
            ControlAction::Query { reply, .. } => {
                let _ = reply.try_send(Err(receipts::ControlError::Rejected(error)));
            }
            ControlAction::Settle { reply, .. } => {
                let _ = reply.try_send(Err(receipts::ControlError::Rejected(error)));
            }
        }
    }
    pub(in crate::local_writer) fn process(
        self,
        gate: &mut ViewGate,
        epochs: TerminalEpochs,
        journal: &mut Journal,
    ) {
        if !Arc::ptr_eq(&self.scope, &gate.scope) || !self.scope.is_active() {
            self.reject(Reject::Policy);
            return;
        }
        match self.action {
            ControlAction::Query { key, reply } => {
                let result = gate
                    .control
                    .as_ref()
                    .ok_or(receipts::ControlError::Rejected(Reject::Unsupported))
                    .and_then(|c| c.query_receipt(&self.scope, &key, journal));
                if matches!(
                    result,
                    Err(receipts::ControlError::Journal(
                        JournalError::Storage | JournalError::Schema
                    ))
                ) {
                    self.scope.close();
                }
                let _ = reply.try_send(result);
            }
            ControlAction::Settle { proof, reply } => {
                let result = gate
                    .control
                    .as_mut()
                    .ok_or(receipts::ControlError::Rejected(Reject::Unsupported))
                    .and_then(|c| c.settle(&self.scope, proof, journal));
                if matches!(
                    result,
                    Err(receipts::ControlError::Journal(
                        JournalError::Storage | JournalError::Schema
                    ))
                ) {
                    self.scope.close();
                }
                let _ = reply.try_send(result);
            }
            ControlAction::Window { view, reply } => {
                let result = gate.check(&view, &self.scope.device, epochs).and_then(|_| {
                    gate.control
                        .as_mut()
                        .ok_or(Reject::Unsupported)?
                        .issue(&self.scope, &view)
                });
                let _ = reply.try_send(result);
            }
            ControlAction::Renew {
                lease,
                fence,
                sequence,
                reply,
            } => {
                let result = gate
                    .control
                    .as_mut()
                    .ok_or(Reject::Unsupported)
                    .and_then(|c| c.renew(&self.scope, lease, fence, sequence));
                let _ = reply.try_send(result);
            }
        }
    }
}
pub(in crate::local_writer) struct PendingLease {
    reply: mpsc::Receiver<Result<u64, Reject>>,
    _credit: Arc<Credit>,
}
impl PendingLease {
    pub(in crate::local_writer) fn wait(
        &self,
        budget: Duration,
    ) -> Result<Result<u64, Reject>, WaitError> {
        self.reply.recv_timeout(budget).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
impl PeerViewHandle {
    pub(in crate::local_writer) fn input_window(
        &self,
        view: &SyncedView,
    ) -> Result<PendingViewAck, Reject> {
        let credit = self.input.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.send_control(ControlRequest {
            scope: self.control.scope.clone(),
            action: ControlAction::Window {
                view: view.clone(),
                reply: tx,
            },
            _credit: credit.clone(),
        })?;
        Ok(PendingViewAck {
            reply: rx,
            _credit: credit,
        })
    }
    pub(in crate::local_writer) fn renew_lease(
        &self,
        lease: [u8; 16],
        fence: u64,
        sequence: u64,
    ) -> Result<PendingLease, Reject> {
        let credit = self.input.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.send_control(ControlRequest {
            scope: self.control.scope.clone(),
            action: ControlAction::Renew {
                lease,
                fence,
                sequence,
                reply: tx,
            },
            _credit: credit.clone(),
        })?;
        Ok(PendingLease {
            reply: rx,
            _credit: credit,
        })
    }
    fn send_control(&self, request: ControlRequest) -> Result<(), Reject> {
        if self.input.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        if !self.control.scope.is_active() {
            return Err(Reject::Policy);
        }
        self.input
            .tx
            .try_send(Command::Control(request))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                mpsc::TrySendError::Disconnected(_) => Reject::Closed,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    fn grant(challenge_ms: u64) -> GrantContext {
        GrantContext {
            challenge_ms,
            grant: [7; 32],
            lease: [8; 16],
            fence: 4,
            policy: 5,
        }
    }
    #[test]
    fn challenge_bounds_and_grant_cap_use_host_creation_time() {
        for (challenge, now) in [
            (1000, 999),
            (1000, 31000),
            (u64::MAX - 30_000, u64::MAX - 30_000),
        ] {
            assert!(matches!(
                ControlState::new(Arc::new(move || Ok(now)), grant(challenge)),
                Err(Reject::Policy)
            ));
        }
        let now = Arc::new(AtomicU64::new(29_999));
        let sampled = now.clone();
        let mut state = ControlState::new(
            Arc::new(move || Ok(sampled.load(Ordering::Acquire))),
            grant(0),
        )
        .unwrap();
        let target = Target {
            host: [1; 32],
            session: [2; 16],
            run: "TEST_ONLY_POLICY".into(),
            generation: 3,
            incarnation: 4,
        };
        let peer = PeerControl::new(target, [5; 32], [0x33; 32], 1).unwrap();
        let mut sequence = 1;
        while now.load(Ordering::Acquire) < 890_000 {
            now.fetch_add(14_000, Ordering::AcqRel);
            assert!(state.renew(&peer.scope, [8; 16], 4, sequence).unwrap() <= 900_000);
            sequence += 1;
        }
        now.store(899_999, Ordering::Release);
        assert_eq!(state.renew(&peer.scope, [8; 16], 4, sequence), Ok(900_000));
        now.store(900_000, Ordering::Release);
        assert_eq!(
            state.renew(&peer.scope, [8; 16], 4, sequence + 1),
            Err(Reject::Policy)
        );
    }
    #[test]
    fn clock_failure_and_invalid_native_grant_fail_closed() {
        let clock = MonotonicClock::new();
        let first = clock.now_ms().unwrap();
        assert!(clock.now_ms().unwrap() >= first);
        assert!(matches!(
            ControlState::new(Arc::new(|| Err(Reject::Policy)), grant(0)),
            Err(Reject::Policy)
        ));
        for field in 0..4 {
            let mut invalid = grant(0);
            match field {
                0 => invalid.grant = [0; 32],
                1 => invalid.lease = [0; 16],
                2 => invalid.fence = 0,
                _ => invalid.policy = 0,
            }
            assert!(matches!(
                ControlState::new(Arc::new(|| Ok(0)), invalid),
                Err(Reject::Policy)
            ));
        }
    }
    #[cfg(windows)]
    #[test]
    fn startup_refuses_multiple_or_missing_unsettled_metadata() {
        for missing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("journal.sqlite");
            let mut journal = Journal::open(&path).unwrap();
            let target = Target {
                host: [1; 32],
                session: [2; 16],
                run: "TEST_ONLY_RESTORE".into(),
                generation: 3,
                incarnation: 4,
            };
            for sequence in 1..=if missing { 1 } else { 2 } {
                let key = OperationKey {
                    host: target.host,
                    session: target.session,
                    run: target.run.clone(),
                    generation: 3,
                    incarnation: 4,
                    device: [5; 32],
                    operation: [sequence as u8; 16],
                    sequence,
                };
                assert_eq!(
                    journal
                        .run_once(&key, &KeyedDigest([0x71; 32]), || Outcome::HostWritten)
                        .unwrap(),
                    Outcome::HostWritten
                );
            }
            if missing {
                let fault = rusqlite::Connection::open(&path).unwrap();
                fault.execute("DELETE FROM operations", []).unwrap();
            }
            let result = LocalWriter::start_control_guarded(
                journal,
                std::io::sink(),
                target,
                [5; 32],
                9,
                DigestKey::from_backend_bytes([0x5a; 32]),
                ControlStart::new(
                    ViewStart {
                        terminal: CanonicalStart::new(3, 10, &HostViewBudget::new()).unwrap(),
                        binding: [0x33; 32],
                        minor: 1,
                    },
                    Arc::new(|| Ok(0)),
                    grant(0),
                ),
            );
            match result {
                Err(error) => assert_eq!(
                    error,
                    if missing {
                        Reject::Closed
                    } else {
                        Reject::Pending
                    }
                ),
                Ok((mut writer, _, _)) => {
                    writer.seal();
                    while !writer.try_join().unwrap() {
                        std::thread::yield_now();
                    }
                    panic!("unsettled state was reset");
                }
            }
        }
    }
}
