//! Immutable native data-output bound, not a signature verifier or a writer lease.
use super::*;
pub(in crate::local_writer::view_gate) struct ReadGrant {
    pub(in crate::local_writer::view_gate) fence: u64,
    pub(in crate::local_writer::view_gate) authority:
        Option<cc_desk_remote_security::BoundDataAuthority>,
    clock: Arc<dyn HostClock>,
    last_ms: Arc<Mutex<u64>>,
    deadline_ms: u64,
}
pub(super) fn sample(
    clock: &dyn HostClock,
    last_ms: &Mutex<u64>,
    scope: &PeerScope,
) -> Result<u64, Reject> {
    let result = (|| {
        let mut last = last_ms.lock().map_err(|_| Reject::Policy)?;
        let now = clock.now_ms()?;
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
impl ReadGrant {
    pub(in crate::local_writer::view_gate) fn remaining(
        &self,
        scope: &PeerScope,
    ) -> Result<Duration, Reject> {
        if self.authority.as_ref().is_some_and(|a| a.check().is_err()) {
            scope.close();
            return Err(Reject::Policy);
        }
        let now = sample(self.clock.as_ref(), &self.last_ms, scope)?;
        if !scope.is_active() || now >= self.deadline_ms {
            scope.close();
            return Err(Reject::Policy);
        }
        Ok(Duration::from_millis(self.deadline_ms - now))
    }
}
impl ControlState {
    pub(in crate::local_writer::view_gate) fn read_grant(
        &self,
        scope: &PeerScope,
    ) -> Result<ReadGrant, Reject> {
        let grant = ReadGrant {
            fence: self.identity.fence,
            authority: self.authority.clone(),
            clock: self.clock.clone(),
            last_ms: self.last_ms.clone(),
            deadline_ms: self.grant_deadline,
        };
        grant.remaining(scope)?;
        Ok(grant)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    #[test]
    fn rebind_retains_a_later_old_output_sample_before_scope_closes() {
        use std::sync::atomic::AtomicU64;
        let now = Arc::new(AtomicU64::new(1000));
        let source = now.clone();
        let clock: Arc<dyn HostClock> = Arc::new(move || Ok(source.load(Ordering::Acquire)));
        let target = Target {
            host: [1; 32],
            session: [2; 16],
            run: "TEST_ONLY_REBIND_CLOCK".into(),
            generation: 3,
            incarnation: 4,
        };
        let peer = PeerControl::new(target.clone(), [5; 32], [0x33; 32], 1).unwrap();
        let grant = GrantContext {
            challenge_ms: 1000,
            expires_host_ms: 50000,
            grant: [7; 32],
            lease: [8; 16],
            fence: 4,
            policy: 5,
        };
        let old = ControlState::new(clock.clone(), grant).unwrap();
        let old_output = old.read_grant(&peer.scope).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::open(&dir.path().join("journal.sqlite")).unwrap();
        let replacement = old
            .replacement(
                &peer.scope,
                [5; 32],
                4,
                &ControlAdmission {
                    authority: None,
                    binding: [0x44; 32],
                    minor: 1,
                    clock,
                    grant: GrantContext {
                        lease: [9; 16],
                        fence: 5,
                        ..grant
                    },
                },
                &journal,
            )
            .unwrap();
        // Exact schedule: old delivery samples after replacement construction,
        // before the old scope closes and the new state is published.
        now.store(2000, Ordering::Release);
        assert!(old_output.remaining(&peer.scope).is_ok());
        peer.close();
        let successor = PeerControl::on_writer(
            target,
            [5; 32],
            [0x44; 32],
            1,
            peer.scope.writer_closed.clone(),
        )
        .unwrap();
        now.store(1500, Ordering::Release);
        assert!(matches!(
            replacement.read_grant(&successor.scope),
            Err(Reject::Policy)
        ));
        assert!(*successor.scope.writer_closed.borrow());
        now.store(2001, Ordering::Release);
        assert!(replacement.read_grant(&successor.scope).is_err());
    }
    #[test]
    fn concurrent_clock_completion_is_not_a_false_rollback() {
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let release_rx = Mutex::new(release_rx);
        let calls = AtomicUsize::new(0);
        let clock: Arc<dyn HostClock> = Arc::new(move || {
            let call = calls.fetch_add(1, Ordering::AcqRel);
            if call == 0 {
                entered_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap();
            }
            Ok(1000 + call as u64)
        });
        let peer = PeerControl::new(
            Target {
                host: [1; 32],
                session: [2; 16],
                run: "TEST_ONLY_CLOCK_ORDER".into(),
                generation: 3,
                incarnation: 4,
            },
            [5; 32],
            [0x33; 32],
            1,
        )
        .unwrap();
        let last = Arc::new(Mutex::new(1000));
        let (a_clock, a_last, a_scope) = (clock.clone(), last.clone(), peer.scope.clone());
        let a = thread::spawn(move || sample(a_clock.as_ref(), &a_last, &a_scope));
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let (b_clock, b_last, b_scope) = (clock, last, peer.scope.clone());
        let b = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            done_tx
                .send(sample(b_clock.as_ref(), &b_last, &b_scope))
                .unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let early = done_rx.recv_timeout(Duration::from_millis(50));
        release_tx.send(()).unwrap();
        let a_result = a.join().unwrap();
        let b_result =
            early.unwrap_or_else(|_| done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        b.join().unwrap();
        assert_eq!(a_result, Ok(1000));
        assert_eq!(b_result, Ok(1001));
        assert!(peer.scope.is_active());
    }
}
