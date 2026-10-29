use super::*;
use crate::bound_test_support as support;
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
#[derive(Clone)]
struct Fake(Arc<AtomicUsize>, bool);
impl Host for Fake {
    fn create(&mut self, _c: &Context<'_>, _r: &Request) -> Status {
        self.0.fetch_add(1, Ordering::AcqRel);
        assert!(!self.1, "TEST_ONLY_AFTER_FAKE_EFFECT");
        Status::Accepted
    }
}
fn request() -> Request {
    Request {
        kind: Kind::Create,
        operation: [6; 16],
        sequence: 1,
        state: State {
            generation: 3,
            fence: 4,
            mode: 0,
            dimension: 0,
            sync: 1,
        },
        size: None,
    }
}
fn key() -> DigestKey {
    DigestKey::from_backend_bytes([0x5a; 32])
}
#[tokio::test]
async fn lifecycle_durable_missing_identity_and_revocation_refuse_backend() {
    let (c, h, mut gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let proof = h.authority();
    let ctx = Context {
        proof: &proof,
        state: request().state,
    };
    let d = tempfile::tempdir().unwrap();
    let n = Arc::new(AtomicUsize::new(0));
    let mut a = Durable::new(
        Store::open(&d.path().join("lifecycle.sqlite")).unwrap(),
        key(),
        Fake(n.clone(), false),
    );
    assert_eq!(
        a.create(&ctx, &request()),
        Status::Rejected(Code::Unauthorized)
    );
    drop(a);
    let mut a = Durable::new(
        Store::open(&d.path().join("lifecycle.sqlite")).unwrap(),
        key(),
        bound_fake(n.clone(), false, &proof),
    );
    a.before_backend = Some(Box::new(move || gate.revoke()));
    assert_eq!(
        a.create(&ctx, &request()),
        Status::Rejected(Code::Unauthorized)
    );
    assert_eq!(n.load(Ordering::Acquire), 0);
    drop(a);
    h.close().await;
    c.close().await;
}
struct SqlFake {
    base: BoundFake,
    path: std::path::PathBuf,
}
impl Host for SqlFake {
    fn resource_identity(&self) -> Option<&crate::lifecycle_router::ResourceIdentity> {
        self.base.resource_identity()
    }
    fn create(&mut self, c: &Context<'_>, r: &Request) -> Status {
        let db = rusqlite::Connection::open(&self.path).unwrap();
        db.busy_timeout(std::time::Duration::ZERO).unwrap();
        db.execute_batch("BEGIN IMMEDIATE;ROLLBACK;").unwrap();
        self.base.create(c, r)
    }
}
#[tokio::test]
async fn lifecycle_durable_backend_queue_call_holds_no_database_transaction() {
    let (c, h, _gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let proof = h.authority();
    let ctx = Context {
        proof: &proof,
        state: request().state,
    };
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("lifecycle.sqlite");
    let n = Arc::new(AtomicUsize::new(0));
    let mut a = Durable::new(
        Store::open(&path).unwrap(),
        key(),
        SqlFake {
            base: bound_fake(n.clone(), false, &proof),
            path,
        },
    );
    assert_eq!(a.create(&ctx, &request()), Status::Accepted);
    assert_eq!(n.load(Ordering::Acquire), 1);
    drop(a);
    h.close().await;
    c.close().await;
}
#[derive(Clone)]
struct BoundFake {
    fake: Fake,
    identity: crate::lifecycle_router::ResourceIdentity,
}
fn bound_fake(
    n: Arc<AtomicUsize>,
    panic: bool,
    proof: &cc_desk_remote_security::BoundDataAuthority,
) -> BoundFake {
    BoundFake {
        fake: Fake(n, panic),
        identity: crate::lifecycle_router::ResourceIdentity::from_proof(proof),
    }
}
impl Host for BoundFake {
    fn resource_identity(&self) -> Option<&crate::lifecycle_router::ResourceIdentity> {
        Some(&self.identity)
    }
    fn create(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.fake.create(c, r)
    }
}
#[tokio::test]
async fn lifecycle_durable_foreign_backend_identity_refuses_before_effect() {
    use crate::lifecycle_router::ResourceIdentity;
    let (c, h, _gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let proof = h.authority();
    let ctx = Context {
        proof: &proof,
        state: request().state,
    };
    for field in 0..6 {
        let mut identity = ResourceIdentity::from_proof(&proof);
        match field {
            0 => identity.host.push('x'),
            1 => identity.session.push('x'),
            2 => identity.run.push('x'),
            3 => identity.generation = "4".into(),
            4 => identity.incarnation = "12".into(),
            _ => identity.principal = [9; 32],
        };
        let d = tempfile::tempdir().unwrap();
        let n = Arc::new(AtomicUsize::new(0));
        let mut a = Durable::new(
            Store::open(&d.path().join("lifecycle.sqlite")).unwrap(),
            key(),
            BoundFake {
                fake: Fake(n.clone(), false),
                identity,
            },
        );
        assert_eq!(
            a.create(&ctx, &request()),
            Status::Rejected(Code::Unauthorized)
        );
        assert_eq!(n.load(Ordering::Acquire), 0);
    }
    h.close().await;
    c.close().await;
}
#[tokio::test]
async fn lifecycle_durable_changed_key_never_resets_deduplication() {
    let (c, h, _gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let proof = h.authority();
    let ctx = Context {
        proof: &proof,
        state: request().state,
    };
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("lifecycle.sqlite");
    let n = Arc::new(AtomicUsize::new(0));
    let mut a = Durable::new(
        Store::open(&p).unwrap(),
        key(),
        bound_fake(n.clone(), false, &proof),
    );
    assert_eq!(a.create(&ctx, &request()), Status::Accepted);
    drop(a);
    let mut a = Durable::new(
        Store::open(&p).unwrap(),
        DigestKey::from_backend_bytes([0x5b; 32]),
        bound_fake(n.clone(), false, &proof),
    );
    assert_eq!(a.create(&ctx, &request()), Status::Unknown);
    assert_eq!(n.load(Ordering::Acquire), 1);
    drop(a);
    h.close().await;
    c.close().await;
}
#[tokio::test]
async fn lifecycle_durable_expiry_during_flush_refuses_backend() {
    let now = Arc::new(AtomicU64::new(1000));
    let (c, h, _gate, _cancel, _grant) = support::channels(now.clone()).await;
    let proof = h.authority();
    let ctx = Context {
        proof: &proof,
        state: request().state,
    };
    let d = tempfile::tempdir().unwrap();
    let n = Arc::new(AtomicUsize::new(0));
    let mut a = Durable::new(
        Store::open(&d.path().join("lifecycle.sqlite")).unwrap(),
        key(),
        bound_fake(n.clone(), false, &proof),
    );
    a.before_backend = Some(Box::new(move || now.store(11000, Ordering::Release)));
    assert_eq!(
        a.create(&ctx, &request()),
        Status::Rejected(Code::Unauthorized)
    );
    assert_eq!(n.load(Ordering::Acquire), 0);
    drop(a);
    h.close().await;
    c.close().await;
}
#[tokio::test]
async fn lifecycle_durable_router_recreation_does_not_duplicate_queue() {
    use crate::lifecycle_router::{Expected, Rights, Router};
    let (c, h, _gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let effects = Arc::new(AtomicUsize::new(0));
    let r = request();
    let wire = encode(&r).unwrap();
    c.send_control(
        &support::scope(),
        4,
        &wire,
        std::time::Duration::from_secs(1),
    )
    .await
    .unwrap();
    let rec = h
        .receive(&support::scope(), 4, std::time::Duration::from_secs(1))
        .await
        .unwrap();
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("lifecycle.sqlite");
    for _ in 0..2 {
        let mut rights = Rights::default();
        rights.allow(Kind::Create);
        let expected = Expected {
            context: h.authority().context().clone(),
            principal: [4; 32],
            scope: support::scope(),
            state: r.state,
        };
        let mut router = Router::durable(
            h.authority(),
            expected,
            rights,
            Store::open(&p).unwrap(),
            key(),
            bound_fake(effects.clone(), false, &h.authority()),
        )
        .unwrap();
        router.dispatch(&rec).unwrap();
    }
    assert_eq!(effects.load(Ordering::Acquire), 1);
    h.close().await;
    c.close().await;
}
#[tokio::test]
async fn lifecycle_durable_adapter_reopen_does_not_requeue_accepted_or_panicked_effect() {
    for panic_after in [false, true] {
        let (c, h, _gate, _cancel, _grant) =
            support::channels(Arc::new(AtomicU64::new(1000))).await;
        let proof = h.authority();
        let ctx = Context {
            proof: &proof,
            state: request().state,
        };
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("lifecycle.sqlite");
        let effects = Arc::new(AtomicUsize::new(0));
        let fake = bound_fake(effects.clone(), panic_after, &proof);
        let mut a = Durable::new(Store::open(&p).unwrap(), key(), fake.clone());
        if panic_after {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || a.create(&ctx, &request())
                ))
                .is_err()
            )
        } else {
            assert_eq!(a.create(&ctx, &request()), Status::Accepted)
        }
        drop(a);
        let mut a = Durable::new(
            Store::open(&p).unwrap(),
            key(),
            bound_fake(effects.clone(), false, &h.authority()),
        );
        assert_eq!(a.create(&ctx, &request()), Status::Unknown);
        let mut altered = request();
        altered.state.fence = 5;
        let altered_ctx = Context {
            proof: &proof,
            state: altered.state,
        };
        assert_eq!(
            a.create(&altered_ctx, &altered),
            Status::Rejected(Code::Conflict)
        );
        let mut next = request();
        next.sequence = 2;
        next.operation = [7; 16];
        assert_eq!(a.create(&ctx, &next), Status::Rejected(Code::Pending));
        assert_eq!(effects.load(Ordering::Acquire), 1);
        drop(a);
        h.close().await;
        c.close().await;
    }
}
#[tokio::test]
async fn lifecycle_durable_fake_completion_is_recorded_without_claiming_wire_completion() {
    let (c, h, _gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let proof = h.authority();
    let ctx = Context {
        proof: &proof,
        state: request().state,
    };
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("lifecycle.sqlite");
    let n = Arc::new(AtomicUsize::new(0));
    let mut a = Durable::new(
        Store::open(&p).unwrap(),
        key(),
        bound_fake(n.clone(), false, &proof),
    );
    assert_eq!(a.create(&ctx, &request()), Status::Accepted);
    a.complete_fixture().unwrap();
    drop(a);
    let mut a = Durable::new(
        Store::open(&p).unwrap(),
        key(),
        bound_fake(n.clone(), false, &proof),
    );
    assert_eq!(a.create(&ctx, &request()), Status::Accepted);
    assert_eq!(n.load(Ordering::Acquire), 1);
    let mut next = request();
    next.sequence = 2;
    next.operation = [8; 16];
    assert_eq!(a.create(&ctx, &next), Status::Accepted);
    assert_eq!(n.load(Ordering::Acquire), 2);
    drop(a);
    h.close().await;
    c.close().await;
}
