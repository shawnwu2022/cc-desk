use super::*;
#[test]
fn attach_completion_dropped_owner_rejects_old_capability() {
    let (r, i, a) = fixture();
    let (s, p) = channel(r.clone(), i.clone(), [9; 32], Arc::new(())).unwrap();
    drop(p);
    assert_eq!(s.complete(&r, &i, a), Err(Code::Stale));
}
fn fixture() -> (Request, ResourceIdentity, State) {
    let r = Request {
        kind: Kind::Attach,
        operation: [1; 16],
        sequence: 1,
        state: State {
            generation: 3,
            fence: 4,
            mode: 0,
            dimension: 0,
            sync: 1,
        },
        size: None,
    };
    let i = ResourceIdentity {
        host: "host".into(),
        session: "session".into(),
        run: "run".into(),
        generation: "3".into(),
        incarnation: "11".into(),
        principal: [4; 32],
    };
    let mut after = r.state;
    after.fence += 1;
    after.sync += 1;
    (r, i, after)
}
#[test]
fn attach_completion_exact_bounded_evidence_and_duplicates() {
    let (r, i, a) = fixture();
    let b = Arc::new(());
    let (s, p) = channel(r.clone(), i.clone(), [9; 32], b.clone()).unwrap();
    assert_eq!(p.evidence([9; 32], &b).unwrap(), None);
    assert_eq!(s.complete(&r, &i, a), Ok(()));
    assert_eq!(s.complete(&r, &i, a), Ok(()));
    assert_eq!(
        p.evidence([9; 32], &b).unwrap(),
        Some(Applied {
            request: r.clone(),
            after: a
        })
    );
    assert_eq!(p.evidence([8; 32], &b), Err(Code::Conflict));
    assert_eq!(p.evidence([9; 32], &Arc::new(())), Err(Code::Conflict));
}
#[test]
fn attach_completion_rejects_every_changed_binding() {
    let (r, i, a) = fixture();
    let b = Arc::new(());
    let (s, p) = channel(r.clone(), i.clone(), [9; 32], b.clone()).unwrap();
    for n in 0..6 {
        let mut bad = i.clone();
        match n {
            0 => bad.host.push('x'),
            1 => bad.session.push('x'),
            2 => bad.run.push('x'),
            3 => bad.generation.push('x'),
            4 => bad.incarnation.push('x'),
            _ => bad.principal = [8; 32],
        };
        assert_eq!(s.complete(&r, &bad, a), Err(Code::Conflict));
    }
    for n in 0..5 {
        let mut bad = a;
        match n {
            0 => bad.generation += 1,
            1 => bad.fence += 1,
            2 => bad.mode += 1,
            3 => bad.dimension += 1,
            _ => bad.sync += 1,
        };
        assert_eq!(s.complete(&r, &i, bad), Err(Code::Conflict));
    }
    for n in 0..10 {
        let mut q = r.clone();
        match n {
            0 => q.operation = [2; 16],
            1 => q.sequence += 1,
            2 => q.kind = Kind::Resume,
            3 => q.state.generation += 1,
            4 => q.state.fence += 1,
            5 => q.state.mode += 1,
            6 => q.state.dimension += 1,
            7 => q.state.sync += 1,
            _ => q.size = Some((1, 1)),
        };
        assert_eq!(s.complete(&q, &i, a), Err(Code::Conflict));
    }
    let mut wrong = i.clone();
    wrong.principal = [7; 32];
    assert_eq!(s.complete(&r, &wrong, a), Err(Code::Conflict));
    let mut wrong = a;
    wrong.fence += 1;
    assert_eq!(s.complete(&r, &i, wrong), Err(Code::Conflict));
    assert_eq!(p.evidence([9; 32], &b).unwrap(), None);
    s.complete(&r, &i, a).unwrap();
    assert_eq!(s.complete(&r, &i, wrong), Err(Code::Conflict));
}
