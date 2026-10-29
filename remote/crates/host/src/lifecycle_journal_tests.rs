use super::*;
#[test]
fn lifecycle_durable_sequence_collision_is_explicit_conflict_after_reopen() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("lifecycle.sqlite");
    let mut s = Store::open(&p).unwrap();
    let t = ticket(&mut s);
    drop(t);
    drop(s);
    let mut s = Store::open(&p).unwrap();
    assert!(matches!(
        s.prepare([1; 32], [9; 16], 1, [3; 32]),
        Err(Error::Conflict)
    ));
}
#[test]
fn lifecycle_durable_foreign_schema_never_migrates_and_directories_stay_exclusive() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("input.sqlite");
    let (db, guard) = crate::journal_storage::open(&p).unwrap();
    assert!(matches!(
        Store::open(&d.path().join("lifecycle.sqlite")),
        Err(Error::OwnerBusy)
    ));
    let separate = d.path().join("lifecycle");
    std::fs::create_dir(&separate).unwrap();
    let s = Store::open(&separate.join("metadata.sqlite")).unwrap();
    drop(s);
    drop(db);
    drop(guard);
    let before = std::fs::read(&p).unwrap();
    assert!(matches!(Store::open(&p), Err(Error::Schema)));
    assert_eq!(std::fs::read(&p).unwrap(), before);
    let p = separate.join("metadata.sqlite");
    let before = std::fs::read(&p).unwrap();
    assert!(matches!(
        crate::journal_storage::open(&p),
        Err(Error::Schema)
    ));
    assert_eq!(std::fs::read(&p).unwrap(), before);
}
#[test]
fn lifecycle_durable_concurrent_conflicting_payloads_issue_one_ticket() {
    let d = tempfile::tempdir().unwrap();
    let s = std::sync::Arc::new(std::sync::Mutex::new(
        Store::open(&d.path().join("lifecycle.sqlite")).unwrap(),
    ));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for digest in [[3; 32], [4; 32]] {
        let s = s.clone();
        let b = barrier.clone();
        threads.push(std::thread::spawn(move || {
            b.wait();
            s.lock().unwrap().prepare([1; 32], [2; 16], 1, digest)
        }));
    }
    barrier.wait();
    let mut winners = Vec::new();
    let mut conflicts = 0;
    for h in threads {
        match h.join().unwrap() {
            Ok(Prepare::New(t)) => winners.push(t),
            Err(Error::Conflict) => conflicts += 1,
            _ => panic!("unexpected concurrent admission"),
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(conflicts, 1);
}
fn ticket(s: &mut Store) -> Ticket {
    match s.prepare([1; 32], [2; 16], 1, [3; 32]).unwrap() {
        Prepare::New(t) => t,
        _ => panic!("new"),
    }
}
#[test]
fn lifecycle_durable_all_recovery_phases_are_monotone() {
    for stop in [
        Phase::Prepared,
        Phase::InFlight,
        Phase::Accepted,
        Phase::Completed,
        Phase::Uncertain,
        Phase::Rejected(5),
    ] {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("lifecycle.sqlite");
        let mut s = Store::open(&p).unwrap();
        let t = ticket(&mut s);
        if stop != Phase::Prepared {
            s.transition(&t, Phase::InFlight).unwrap();
        }
        if matches!(stop, Phase::Accepted | Phase::Completed) {
            s.transition(&t, Phase::Accepted).unwrap();
        }
        if matches!(
            stop,
            Phase::Completed | Phase::Uncertain | Phase::Rejected(_)
        ) {
            s.transition(&t, stop).unwrap();
        }
        drop(t);
        drop(s);
        let mut s = Store::open(&p).unwrap();
        let want = if matches!(stop, Phase::Rejected(_)) {
            stop
        } else {
            Phase::Uncertain
        };
        assert!(
            matches!(s.prepare([1;32],[2;16],1,[3;32]).unwrap(),Prepare::Recorded(p)if p==want)
        );
        if want == Phase::Uncertain {
            assert!(matches!(
                s.prepare([1; 32], [4; 16], 2, [3; 32]),
                Err(Error::Sequence)
            ));
        }
    }
}
#[test]
fn lifecycle_durable_commit_cuts_never_issue_a_second_execution() {
    for cut in [Cut::BeforePrepare, Cut::AfterPrepare] {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("lifecycle.sqlite");
        let mut s = Store::open(&p).unwrap();
        s.fault = Some(cut);
        assert!(s.prepare([1; 32], [2; 16], 1, [3; 32]).is_err());
        assert!(s.prepare([1; 32], [2; 16], 1, [3; 32]).is_err());
        drop(s);
        let mut s = Store::open(&p).unwrap();
        match (cut, s.prepare([1; 32], [2; 16], 1, [3; 32]).unwrap()) {
            (Cut::BeforePrepare, Prepare::New(_)) => (),
            (Cut::AfterPrepare, Prepare::Recorded(Phase::Uncertain)) => (),
            _ => panic!("wrong cut classification"),
        }
    }
    for cut in [Cut::BeforeTransition, Cut::AfterTransition] {
        for phase in [
            Phase::InFlight,
            Phase::Accepted,
            Phase::Completed,
            Phase::Rejected(5),
            Phase::Uncertain,
        ] {
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join("lifecycle.sqlite");
            let mut s = Store::open(&p).unwrap();
            let t = ticket(&mut s);
            if phase != Phase::InFlight {
                s.transition(&t, Phase::InFlight).unwrap();
            }
            if phase == Phase::Completed {
                s.transition(&t, Phase::Accepted).unwrap();
            }
            s.fault = Some(cut);
            assert!(s.transition(&t, phase).is_err());
            drop(t);
            drop(s);
            let mut s = Store::open(&p).unwrap();
            let want = if cut == Cut::AfterTransition && matches!(phase, Phase::Rejected(_)) {
                phase
            } else {
                Phase::Uncertain
            };
            assert!(
                matches!(s.prepare([1;32],[2;16],1,[3;32]).unwrap(),Prepare::Recorded(p)if p==want)
            );
        }
    }
}
#[test]
fn lifecycle_durable_tickets_pin_owner_and_conflicting_concurrent_requests() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("lifecycle.sqlite");
    let s = std::sync::Arc::new(std::sync::Mutex::new(Store::open(&p).unwrap()));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for digest in [[3; 32], [3; 32]] {
        let s = s.clone();
        let b = barrier.clone();
        threads.push(std::thread::spawn(move || {
            b.wait();
            s.lock()
                .unwrap()
                .prepare([1; 32], [2; 16], 1, digest)
                .unwrap()
        }));
    }
    barrier.wait();
    let mut winners = Vec::new();
    for h in threads {
        if let Prepare::New(t) = h.join().unwrap() {
            winners.push(t)
        }
    }
    assert_eq!(winners.len(), 1);
    assert!(matches!(
        s.lock().unwrap().prepare([1; 32], [2; 16], 1, [4; 32]),
        Err(Error::Conflict)
    ));
    drop(s);
    assert!(matches!(Store::open(&p), Err(Error::OwnerBusy)));
    drop(winners);
    assert!(Store::open(&p).is_ok());
}
#[test]
fn lifecycle_durable_transition_rejects_changed_digest_after_update() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("lifecycle.sqlite");
    let mut s = Store::open(&p).unwrap();
    let t = ticket(&mut s);
    s.db.execute_batch("CREATE TEMP TRIGGER corrupt AFTER UPDATE ON ops BEGIN UPDATE ops SET digest=zeroblob(32) WHERE scope=NEW.scope AND op=NEW.op; END;").unwrap();
    assert!(s.transition(&t, Phase::InFlight).is_err());
}
#[test]
fn lifecycle_durable_intent_reopen_never_reissues_a_ticket() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("lifecycle.sqlite");
    let mut s = Store::open(&p).unwrap();
    let Prepare::New(ticket) = s.prepare([1; 32], [2; 16], 1, [3; 32]).unwrap() else {
        panic!("new intent")
    };
    drop(ticket);
    drop(s);
    let mut s = Store::open(&p).unwrap();
    assert!(matches!(
        s.prepare([1; 32], [2; 16], 1, [3; 32]).unwrap(),
        Prepare::Recorded(Phase::Uncertain)
    ));
    assert!(matches!(
        s.prepare([1; 32], [2; 16], 1, [4; 32]),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        s.prepare([1; 32], [4; 16], 2, [3; 32]),
        Err(Error::Sequence)
    ));
}
