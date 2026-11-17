#![allow(non_snake_case)]
use super::*;
use std::sync::Barrier;
use std::thread;

fn binding() -> Binding {
    Binding {
        installation: [1; 32],
        device: [2; 32],
        key_fingerprints: [[3; 32]; 4],
        full_record_digest: [7; 32],
        enrollment_revision: 1,
        issuer: [4; 32],
        principal: [5; 32],
        host_epoch: 1,
        client_epoch: 1,
        policy_epoch: 1,
        issuer_epoch: 1,
        run: [6; 32],
        origin: "https://fixture.invalid".into(),
        rp: "fixture.invalid".into(),
        relay_endpoint: "wss://fixture.invalid/relay".into(),
        route: [7; 16],
        role: Role::Control,
    }
}
fn sample() -> Sample {
    Sample {
        wall_ms: 1_000_000,
        monotonic: Instant::now(),
    }
}
fn later(now: Sample, ms: u64) -> Sample {
    Sample {
        wall_ms: now.wall_ms + ms,
        monotonic: now.monotonic + Duration::from_millis(ms),
    }
}
fn consent(b: &Binding, now: Sample, duration: u64) -> NativeConsent {
    NativeConsent {
        binding: b.clone(),
        revision: 7,
        cutoff_ms: now.wall_ms + duration,
    }
}
fn enabled(duration: u64) -> (Owner, Binding, Sample) {
    let o = Owner::disabled();
    let b = binding();
    let n = sample();
    o.enable(consent(&b, n, duration), n).unwrap();
    (o, b, n)
}
fn proof(t: &Attempt) -> RetirementProof {
    RetirementProof {
        owner: t.owner.clone(),
        sequence: t.sequence,
    }
}

#[test]
fn Unattended_DisabledAndFiniteConsent_001() {
    let o = Owner::disabled();
    let b = binding();
    let n = sample();
    assert!(matches!(o.begin(&b, n), Err(Denied::Locked)));
    assert_eq!(o.enable(consent(&b, n, 0), n), Err(Denied::Bounds));
    assert_eq!(
        o.enable(consent(&b, n, MAX_DELEGATION_MS + 1), n),
        Err(Denied::Bounds)
    );
    o.enable(consent(&b, n, MAX_DELEGATION_MS), n).unwrap();
    assert_eq!(o.enable(consent(&b, n, 1000), n), Err(Denied::Locked));
}
#[test]
fn Unattended_RequiresRetirementAndFreshSequence_002() {
    let (o, b, n) = enabled(10_000);
    let first = o.begin(&b, n).unwrap();
    o.check_attempt(&first, &b, n, true).unwrap();
    assert!(matches!(o.begin(&b, n), Err(Denied::Busy)));
    assert_eq!(o.finish_retirement(proof(&first)), Err(Denied::Locked));
    o.disconnect(&first).unwrap();
    assert!(o.check_attempt(&first, &b, n, false).is_err());
    assert!(matches!(o.begin(&b, n), Err(Denied::Busy)));
    o.finish_retirement(proof(&first)).unwrap();
    let next = o.begin(&b, later(n, 1)).unwrap();
    assert_eq!(next.sequence, first.sequence + 1);
    assert_eq!(
        o.check_attempt(&first, &b, later(n, 1), true),
        Err(Denied::Stale)
    );
    assert_eq!(o.finish_retirement(proof(&first)), Err(Denied::Stale));
    o.check_attempt(&next, &b, later(n, 1), true).unwrap();
}
#[test]
fn Unattended_RevokeInvalidatesTicketsButKeepsCustody_003() {
    let (o, b, n) = enabled(10_000);
    let t = o.begin(&b, n).unwrap();
    o.revoke().unwrap();
    assert!(o.check_attempt(&t, &b, n, true).is_err());
    assert!(o.lock().unwrap().live.is_some());
    o.finish_retirement(proof(&t)).unwrap();
    assert!(o.begin(&b, n).is_err());
    assert_eq!(o.enable(consent(&b, n, 20_000), n), Err(Denied::Locked));
}
#[test]
fn Unattended_UnknownNeverTurnsIntoRetry_004() {
    let (o, b, n) = enabled(10_000);
    let t = o.begin(&b, n).unwrap();
    o.retirement_unknown(&t).unwrap();
    o.disconnect(&t).unwrap();
    assert!(o.finish_retirement(proof(&t)).is_err());
    assert!(o.begin(&b, later(n, 5_000)).is_err());
    o.revoke().unwrap();
    assert!(o.finish_retirement(proof(&t)).is_err());
}
#[test]
fn Unattended_WallRollbackIsSticky_005() {
    let (o, b, n) = enabled(10_000);
    let t = o.begin(&b, n).unwrap();
    o.check_attempt(&t, &b, later(n, 100), false).unwrap();
    assert!(o
        .check_attempt(
            &t,
            &b,
            Sample {
                wall_ms: n.wall_ms + 99,
                monotonic: later(n, 101).monotonic
            },
            true
        )
        .is_err());
    assert_eq!(o.lock().unwrap().gate, Gate::Revoked);
    assert!(o.check_attempt(&t, &b, later(n, 102), true).is_err());
}
#[test]
fn Unattended_BothDeadlinesAreFixed_006() {
    for wall_only in [true, false] {
        let (o, b, n) = enabled(1000);
        let t = o.begin(&b, n).unwrap();
        let end = Sample {
            wall_ms: n.wall_ms + if wall_only { 1000 } else { 1 },
            monotonic: n.monotonic + Duration::from_millis(if wall_only { 1 } else { 1000 }),
        };
        assert!(o.check_attempt(&t, &b, end, true).is_err());
        assert_eq!(o.lock().unwrap().gate, Gate::Expired);
    }
    let (o, b, n) = enabled(1000);
    let t = o.begin(&b, n).unwrap();
    o.disconnect(&t).unwrap();
    o.finish_retirement(proof(&t)).unwrap();
    let t = o.begin(&b, later(n, 900)).unwrap();
    assert!(o.check_attempt(&t, &b, later(n, 1000), true).is_err());
}
#[test]
fn Unattended_AttemptIsShortAndCannotRepublish_007() {
    let (o, b, n) = enabled(MAX_DELEGATION_MS);
    let t = o.begin(&b, n).unwrap();
    o.check_attempt(&t, &b, n, true).unwrap();
    assert!(o.check_attempt(&t, &b, n, true).is_err());
    assert!(o
        .check_attempt(&t, &b, later(n, ATTEMPT_MS), false)
        .is_err());
    assert!(matches!(
        o.begin(&b, later(n, ATTEMPT_MS)),
        Err(Denied::Busy)
    ));
}
#[test]
fn Unattended_BindingChangeRevokesNotRebinds_008() {
    let (o, b, n) = enabled(1000);
    let t = o.begin(&b, n).unwrap();
    let mut other = b.clone();
    other.run[0] ^= 1;
    assert!(o.check_attempt(&t, &other, n, true).is_err());
    assert!(o.check_attempt(&t, &b, n, true).is_err());
    for change in 0..8 {
        let (o, b, n) = enabled(1000);
        let mut changed = b.clone();
        match change {
            0 => changed.device[0] ^= 1,
            1 => changed.enrollment_revision += 1,
            2 => changed.host_epoch += 1,
            3 => changed.client_epoch += 1,
            4 => changed.policy_epoch += 1,
            5 => changed.issuer_epoch += 1,
            6 => changed.origin.push('/'),
            _ => changed.role = Role::View,
        }
        assert!(o.begin(&changed, n).is_err());
        assert!(o.begin(&b, n).is_err());
    }
}
#[test]
fn Unattended_OtherOwnerAndRevisionAreFenced_009() {
    let (o, b, n) = enabled(1000);
    let t = o.begin(&b, n).unwrap();
    let other = Owner::disabled();
    other.enable(consent(&b, n, 1000), n).unwrap();
    let fresh = other.begin(&b, n).unwrap();
    assert_eq!(other.check_attempt(&t, &b, n, true), Err(Denied::Stale));
    other.disconnect(&fresh).unwrap();
    assert_eq!(other.finish_retirement(proof(&t)), Err(Denied::Stale));
    let forged = Attempt {
        owner: t.owner.clone(),
        revision: t.revision + 1,
        epoch: t.epoch,
        sequence: t.sequence,
    };
    assert_eq!(o.check_attempt(&forged, &b, n, true), Err(Denied::Stale));
    let forged = Attempt {
        owner: t.owner.clone(),
        revision: t.revision,
        epoch: t.epoch + 1,
        sequence: t.sequence,
    };
    assert_eq!(o.check_attempt(&forged, &b, n, true), Err(Denied::Stale));
}
#[test]
fn Unattended_ConcurrentBeginHasSingleOwner_010() {
    let (o, b, n) = enabled(1000);
    let o = Arc::new(o);
    let barrier = Arc::new(Barrier::new(3));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let (o, b, barrier) = (o.clone(), b.clone(), barrier.clone());
        threads.push(thread::spawn(move || {
            barrier.wait();
            o.begin(&b, n)
        }));
    }
    barrier.wait();
    let results: Vec<_> = threads.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Denied::Busy)))
            .count(),
        1
    );
}
#[test]
fn Unattended_RevokeRacingPublishNeverLeavesValidTicket_011() {
    for _ in 0..32 {
        let (o, b, n) = enabled(1000);
        let o = Arc::new(o);
        let t = Arc::new(o.begin(&b, n).unwrap());
        let barrier = Arc::new(Barrier::new(2));
        let (o2, b2, t2, barrier2) = (o.clone(), b.clone(), t.clone(), barrier.clone());
        let worker = thread::spawn(move || {
            barrier2.wait();
            o2.check_attempt(&t2, &b2, n, true)
        });
        barrier.wait();
        o.revoke().unwrap();
        let _ = worker.join().unwrap();
        assert!(o.check_attempt(&t, &b, n, false).is_err());
        assert!(o.begin(&b, n).is_err());
        o.finish_retirement(proof(&t)).unwrap();
    }
}
#[test]
fn Unattended_PoisonAndSequenceOverflowFailClosed_012() {
    let (o, b, n) = enabled(1000);
    o.lock().unwrap().sequence = u64::MAX;
    assert!(matches!(o.begin(&b, n), Err(Denied::Bounds)));
    assert_eq!(o.lock().unwrap().gate, Gate::Revoked);
    let (o, b, n) = enabled(1000);
    let o = Arc::new(o);
    let o2 = o.clone();
    assert!(thread::spawn(move || {
        let _s = o2.state.lock().unwrap();
        panic!("fixture poison")
    })
    .join()
    .is_err());
    assert!(matches!(o.begin(&b, n), Err(Denied::Unknown)));
    assert_eq!(o.revoke(), Err(Denied::Unknown));
}
#[test]
fn Unattended_NewProcessOwnerStartsDisabled_013() {
    let (o, b, n) = enabled(1000);
    let t = o.begin(&b, n).unwrap();
    drop(o);
    let new = Owner::disabled();
    assert!(new.check_attempt(&t, &b, n, true).is_err());
    assert!(new.begin(&b, n).is_err());
}
