use super::*;
impl ControlState {
    pub(in crate::local_writer::view_gate) fn replacement(
        &self,
        scope: &PeerScope,
        device: [u8; 32],
        expected_fence: u64,
        admission: &ControlAdmission,
        journal: &Journal,
    ) -> Result<Self, Reject> {
        if *scope.writer_closed.borrow() {
            return Err(Reject::Sealed);
        }
        if expected_fence != self.identity.fence {
            return Err(Reject::Epoch);
        }
        let next = self.identity.fence.checked_add(1).ok_or_else(|| {
            scope.close_writer();
            Reject::Epoch
        })?;
        if admission.grant.fence != next
            || admission.grant.policy != self.identity.policy
            || admission.binding == scope.binding
            || admission.grant.lease == self.identity.lease
            || !Arc::ptr_eq(&admission.clock, &self.clock)
        {
            return Err(Reject::Policy);
        }
        if device != scope.device && self.pending.is_some() {
            return Err(Reject::Pending);
        }
        if admission.grant.grant == self.identity.grant
            && (device != scope.device
                || admission.grant.challenge_ms.checked_add(900_000) != Some(self.grant_deadline))
        {
            return Err(Reject::Policy);
        }
        let previous = self.sample(scope)?;
        let mut replacement = Self::at(
            admission.clock.clone(),
            admission.grant,
            previous,
            admission.grant.grant != self.identity.grant,
        )?;
        if let Err(error) = replacement.restore_pending(journal, &scope.target, &device) {
            if error == Reject::Closed {
                scope.close_writer();
            }
            return Err(error);
        }
        Ok(replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    #[test]
    fn rebind_uses_accepted_grant_without_restarting_its_deadline() {
        let now = Arc::new(AtomicU64::new(1000));
        let source = now.clone();
        let clock: Arc<dyn HostClock> = Arc::new(move || Ok(source.load(Ordering::Acquire)));
        let grant = GrantContext {
            challenge_ms: 1000,
            grant: [7; 32],
            lease: [8; 16],
            fence: 4,
            policy: 5,
        };
        let old = ControlState::new(clock.clone(), grant).unwrap();
        let target = Target {
            host: [1; 32],
            session: [2; 16],
            run: "TEST_ONLY_REBIND".into(),
            generation: 3,
            incarnation: 4,
        };
        let peer = PeerControl::new(target, [5; 32], [0x33; 32], 1).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::open(&dir.path().join("journal.sqlite")).unwrap();
        now.store(40000, Ordering::Release);
        let next = ControlAdmission {
            binding: [0x44; 32],
            minor: 1,
            clock: clock.clone(),
            grant: GrantContext {
                lease: [9; 16],
                fence: 5,
                ..grant
            },
        };
        let replacement = old
            .replacement(&peer.scope, [5; 32], 4, &next, &journal)
            .unwrap();
        assert_eq!(replacement.grant_deadline, 901000);
        assert_eq!(replacement.lease_deadline, 55000);
        now.store(901000, Ordering::Release);
        assert!(matches!(
            old.replacement(&peer.scope, [5; 32], 4, &next, &journal),
            Err(Reject::Policy)
        ));
    }
    #[test]
    fn transition_rejects_identity_epoch_and_clock_namespace_substitution() {
        let clock: Arc<dyn HostClock> = Arc::new(|| Ok(1000));
        let grant = GrantContext {
            challenge_ms: 1000,
            grant: [7; 32],
            lease: [8; 16],
            fence: 4,
            policy: 5,
        };
        let old = ControlState::new(clock.clone(), grant).unwrap();
        let peer = PeerControl::new(
            Target {
                host: [1; 32],
                session: [2; 16],
                run: "TEST_ONLY_REBIND".into(),
                generation: 3,
                incarnation: 4,
            },
            [5; 32],
            [0x33; 32],
            1,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::open(&dir.path().join("journal.sqlite")).unwrap();
        for case in 0..8 {
            let mut next = ControlAdmission {
                binding: [0x44; 32],
                minor: 1,
                clock: clock.clone(),
                grant: GrantContext {
                    lease: [9; 16],
                    fence: 5,
                    ..grant
                },
            };
            let mut expected = 4;
            let mut device = [5; 32];
            match case {
                0 => expected = 3,
                1 => next.grant.fence = 6,
                2 => next.binding = [0x33; 32],
                3 => next.grant.lease = [8; 16],
                4 => next.grant.policy = 4,
                5 => next.clock = Arc::new(|| Ok(1000)),
                6 => next.grant.challenge_ms = 2000,
                _ => device = [6; 32],
            }
            assert!(
                old.replacement(&peer.scope, device, expected, &next, &journal)
                    .is_err(),
                "{case}"
            );
            assert!(peer.scope.is_active());
        }
    }
    #[test]
    fn clock_rollback_and_fence_overflow_freeze_the_writer_namespace() {
        for overflow in [false, true] {
            let now = Arc::new(AtomicU64::new(1000));
            let sampled = now.clone();
            let clock: Arc<dyn HostClock> = Arc::new(move || Ok(sampled.load(Ordering::Acquire)));
            let grant = GrantContext {
                challenge_ms: 1000,
                grant: [7; 32],
                lease: [8; 16],
                fence: if overflow { u64::MAX } else { 4 },
                policy: 5,
            };
            let old = ControlState::new(clock.clone(), grant).unwrap();
            let target = Target {
                host: [1; 32],
                session: [2; 16],
                run: "TEST_ONLY_FREEZE".into(),
                generation: 3,
                incarnation: 4,
            };
            let peer = PeerControl::new(target.clone(), [5; 32], [0x33; 32], 1).unwrap();
            let alias = PeerControl::on_writer(
                target,
                [5; 32],
                [0x77; 32],
                1,
                peer.scope.writer_closed.clone(),
            )
            .unwrap();
            let dir = tempfile::tempdir().unwrap();
            let journal = Journal::open(&dir.path().join("journal.sqlite")).unwrap();
            let next = ControlAdmission {
                binding: [0x44; 32],
                minor: 1,
                clock,
                grant: GrantContext {
                    fence: 5,
                    lease: [9; 16],
                    ..grant
                },
            };
            if !overflow {
                now.store(999, Ordering::Release);
            }
            assert!(
                old.replacement(&peer.scope, [5; 32], grant.fence, &next, &journal)
                    .is_err()
            );
            assert!(!peer.scope.is_active());
            assert!(!alias.scope.is_active());
            now.store(1001, Ordering::Release);
            assert!(matches!(
                old.replacement(&peer.scope, [5; 32], grant.fence, &next, &journal),
                Err(Reject::Sealed)
            ));
        }
    }
}
