use super::*;
use crate::bound_test_support as support;
use cc_desk_remote_record::lifecycle::{Kind, State};
use cc_desk_remote_security::{CancelHandle, GrantGate, RunningChannel};
use cc_desk_remote_terminal::{TerminalCore, TerminalEpochs};
use std::sync::atomic::{AtomicU64, Ordering};
const B: Duration = Duration::from_secs(1);
struct Fixture {
    owner: Owner,
    _dir: tempfile::TempDir,
    c: RunningChannel,
    h: RunningChannel,
    gate: GrantGate,
    _cancel: CancelHandle,
    now: Arc<AtomicU64>,
}
impl Fixture {
    async fn new() -> Self {
        let now = Arc::new(AtomicU64::new(1000));
        let (c, h, gate, cancel, _) = support::channels(now.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let baseline = State {
            generation: 3,
            fence: 4,
            mode: 0,
            dimension: 0,
            sync: 1,
        };
        let current = Fact {
            request: Request {
                kind: Kind::Resize,
                operation: [99; 16],
                sequence: 1,
                state: baseline,
                size: Some((3, 10)),
            },
            resource: ResourceIdentity::from_proof(&h.authority()),
            backend: [6; 32],
            digest: [7; 32],
            before_size: (3, 10),
            after_size: (3, 10),
            before_terminal: TerminalEpochs::default(),
            after_terminal: TerminalEpochs::default(),
            after: baseline,
            outcome: Outcome::Applied,
        };
        let owner = Owner::fake(
            Store::open_resize_candidate(&dir.path().join("resize.sqlite")).unwrap(),
            DigestKey::from_backend_bytes([0x5a; 32]),
            h.authority(),
            current,
            true,
        )
        .unwrap();
        Self {
            owner,
            _dir: dir,
            c,
            h,
            gate,
            _cancel: cancel,
            now,
        }
    }
    fn request(&self) -> Request {
        Request {
            kind: Kind::Resize,
            operation: [1; 16],
            sequence: 1,
            state: self.owner.current.after,
            size: Some((4, 12)),
        }
    }
    fn execute(&self, r: &Request, applied: bool) -> Fact {
        // Actual synthetic emulator supplies dimensions/epochs; the mailbox template is not the oracle.
        let mut core = TerminalCore::new(3, 10).unwrap();
        let before_terminal = core.epochs();
        if applied {
            core.resize(4, 12).unwrap();
        }
        let mut after = r.state;
        after.dimension = core.epochs().dimension_epoch;
        Fact {
            request: r.clone(),
            resource: self.owner.resource.clone(),
            backend: self.owner.backend,
            digest: self.owner.metadata(r).unwrap().1,
            before_size: (3, 10),
            after_size: (core.dimensions().0 as u16, core.dimensions().1 as u16),
            before_terminal,
            after_terminal: core.epochs(),
            after,
            outcome: if applied {
                Outcome::Applied
            } else {
                Outcome::NotApplied
            },
        }
    }
    async fn close(self) {
        self.h.close().await;
        self.c.close().await;
    }
}
#[tokio::test]
async fn lifecycle_resize_durable_recovery_and_no_reexecution() {
    for applied in [false, true] {
        let mut f = Fixture::new().await;
        let r = f.request();
        let sender = f.owner.begin(r.clone(), B).unwrap().unwrap();
        // Independent connection observes Accepted; no transaction spans fake execution.
        let p = f._dir.path().join("resize.sqlite");
        let db =
            rusqlite::Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        assert_eq!(
            db.query_row("SELECT state FROM ops", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        drop(db);
        assert!(matches!(f.owner.result(), Err(Code::Pending)));
        let fact = f.execute(&r, applied);
        sender.complete(fact.clone()).unwrap();
        sender.complete(fact.clone()).unwrap();
        let mut wrong = fact.clone();
        wrong.backend = [8; 32];
        assert!(sender.complete(wrong).is_err());
        assert!(f.owner.finish().unwrap());
        assert_eq!(f.owner.result().unwrap(), Some(fact.clone()));
        assert!(f.owner.begin(r.clone(), B).unwrap().is_none());
        assert!(f.owner.result().is_err());
        assert_eq!(f.owner.history(&r).unwrap(), Some(fact.clone()));
        let mut baseline = f.owner.current.clone();
        // Simulate a later trusted mode state; historical lookup must not roll it back.
        baseline.request.state.mode += 1;
        baseline.after.mode += 1;
        baseline.before_terminal.mode_epoch += 1;
        baseline.after_terminal.mode_epoch += 1;
        let old = f.owner;
        drop(old);
        assert!(Store::open(&p).is_err());
        let mut recovered = Owner::fake(
            Store::open_resize_candidate(&p).unwrap(),
            DigestKey::from_backend_bytes([0x5a; 32]),
            f.h.authority(),
            baseline.clone(),
            true,
        )
        .unwrap();
        assert_eq!(recovered.history(&r).unwrap(), Some(fact.clone()));
        assert!(recovered.begin(r.clone(), B).unwrap().is_none());
        assert_eq!(recovered.current, baseline);
        assert!(recovered.result().is_err());
        let mut next = r.clone();
        next.sequence = 2;
        next.operation = [2; 16];
        next.state = baseline.after;
        assert!(matches!(recovered.begin(next, B), Err(Code::Unsupported)));
        assert!(sender.complete(fact).is_err());
        drop(recovered);
        f.h.close().await;
        f.c.close().await;
    }
}
#[tokio::test]
async fn lifecycle_resize_unknown_cancel_and_late_callback() {
    for timeout in [false, true] {
        let mut f = Fixture::new().await;
        let r = f.request();
        let s = f
            .owner
            .begin(
                r.clone(),
                if timeout {
                    Duration::from_millis(50)
                } else {
                    B
                },
            )
            .unwrap()
            .unwrap();
        let fact = f.execute(&r, true);
        if timeout {
            std::thread::sleep(Duration::from_millis(60));
            assert!(f.owner.finish().is_err());
        } else {
            f.owner.cancel();
        }
        assert!(s.complete(fact).is_err());
        assert!(f.owner.finish().is_err());
        assert!(f.owner.begin(r.clone(), B).is_err());
        assert_eq!(f.owner.history(&r).unwrap(), None);
        f.close().await;
    }
}
#[tokio::test]
async fn lifecycle_resize_commit_cuts_expiry_and_revocation() {
    for mode in 0..6 {
        let mut f = Fixture::new().await;
        let r = f.request();
        let s = f.owner.begin(r.clone(), B).unwrap().unwrap();
        let fact = f.execute(&r, true);
        s.complete(fact.clone()).unwrap();
        match mode {
            0 => f.owner.store.fault = Some(crate::lifecycle_journal::Cut::BeforeTransition),
            1 => f.owner.store.fault = Some(crate::lifecycle_journal::Cut::AfterTransition),
            2 => f.owner.cut = Some(Cut::AfterDurable),
            3 => f.owner.cut = Some(Cut::BeforePublish),
            4 => f.now.store(11000, Ordering::Release),
            _ => f.gate.revoke(),
        }
        assert!(f.owner.finish().is_err());
        assert!(f.owner.result().is_err());
        if mode >= 4 {
            assert!(f.owner.history(&r).is_err());
        }
        assert!(f.owner.begin(r.clone(), B).is_err());
        let p = f._dir.path().join("resize.sqlite");
        let baseline = f.owner.current.clone();
        drop(f.owner);
        if mode < 4 {
            let recovered = Owner::fake(
                Store::open_resize_candidate(&p).unwrap(),
                DigestKey::from_backend_bytes([0x5a; 32]),
                f.h.authority(),
                baseline,
                true,
            )
            .unwrap();
            assert_eq!(
                recovered.history(&r).unwrap(),
                if mode == 0 { None } else { Some(fact) }
            );
        }
        f.h.close().await;
        f.c.close().await;
    }
}
#[tokio::test]
async fn lifecycle_resize_held_sender_drop_and_incomplete_restart() {
    let mut f = Fixture::new().await;
    let r = f.request();
    let s = f.owner.begin(r.clone(), B).unwrap().unwrap();
    let fact = f.execute(&r, true);
    let baseline = f.owner.current.clone();
    let p = f._dir.path().join("resize.sqlite");
    drop(f.owner);
    assert!(s.complete(fact).is_err());
    let mut recovered = Owner::fake(
        Store::open_resize_candidate(&p).unwrap(),
        DigestKey::from_backend_bytes([0x5a; 32]),
        f.h.authority(),
        baseline.clone(),
        true,
    )
    .unwrap();
    assert_eq!(recovered.history(&r).unwrap(), None);
    assert!(recovered.begin(r, B).is_err());
    assert_eq!(recovered.current, baseline);
    drop(recovered);
    f.h.close().await;
    f.c.close().await;
}
#[tokio::test]
async fn lifecycle_resize_wrong_full_callback_and_history_request() {
    let mut f = Fixture::new().await;
    let r = f.request();
    let s = f.owner.begin(r.clone(), B).unwrap().unwrap();
    let fact = f.execute(&r, true);
    for mode in 0..6 {
        let mut bad = fact.clone();
        match mode {
            0 => {
                bad.request.size = Some((5, 12));
                bad.after_size = (5, 12);
            }
            1 => bad.request.operation = [8; 16],
            2 => bad.resource.principal = [8; 32],
            3 => bad.resource.incarnation = "12".into(),
            4 => bad.digest = [8; 32],
            _ => bad.backend = [8; 32],
        };
        assert!(s.complete(bad).is_err());
    }
    s.complete(fact).unwrap();
    f.owner.finish().unwrap();
    let mut wrong = r.clone();
    wrong.size = Some((5, 12));
    assert!(f.owner.history(&wrong).is_err());
    f.gate.revoke();
    assert!(f.owner.history(&r).is_err());
    assert!(f.owner.result().is_err());
    f.close().await;
}
#[tokio::test]
async fn lifecycle_resize_no_capability_before_durable_admission() {
    use crate::lifecycle_journal::Cut as JournalCut;
    for cut in [
        JournalCut::BeforePrepare,
        JournalCut::AfterPrepare,
        JournalCut::BeforeTransition,
        JournalCut::AfterTransition,
    ] {
        let mut f = Fixture::new().await;
        let r = f.request();
        let baseline = f.owner.current.clone();
        f.owner.store.fault = Some(cut);
        assert!(f.owner.begin(r.clone(), B).is_err());
        assert!(f.owner.pending.is_none());
        assert_eq!(f.owner.current, baseline);
        let p = f._dir.path().join("resize.sqlite");
        drop(f.owner);
        let mut recovered = Owner::fake(
            Store::open_resize_candidate(&p).unwrap(),
            DigestKey::from_backend_bytes([0x5a; 32]),
            f.h.authority(),
            baseline,
            true,
        )
        .unwrap();
        assert_eq!(recovered.history(&r).unwrap(), None);
        assert!(recovered.begin(r, B).is_err());
        drop(recovered);
        f.h.close().await;
        f.c.close().await;
    }
}
