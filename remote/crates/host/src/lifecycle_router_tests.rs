use super::*;
use crate::bound_test_support as support;
use std::sync::{Arc, Mutex, atomic::AtomicU64};
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Vec<Kind>>>);
impl Fake {
    fn call(&mut self, c: &Context<'_>, r: &Request) -> Status {
        assert!(c.proof.check().is_ok());
        assert_eq!(c.state, r.state);
        self.0.lock().unwrap().push(r.kind);
        Status::Accepted
    }
}
impl Host for Fake {
    fn create(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.call(c, r)
    }
    fn attach(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.call(c, r)
    }
    fn resize(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.call(c, r)
    }
    fn resume(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.call(c, r)
    }
    fn close(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.call(c, r)
    }
}
fn expected(h: &cc_desk_remote_security::RunningChannel) -> Expected {
    Expected {
        context: h.authority().context().clone(),
        principal: [4; 32],
        scope: support::scope(),
        state: state(),
    }
}
fn state() -> State {
    State {
        generation: 3,
        fence: 4,
        mode: 0,
        dimension: 0,
        sync: 1,
    }
}
#[tokio::test]
async fn lifecycle_backend_panic_keeps_unknown_operation_pending() {
    struct Panics(Arc<Mutex<usize>>);
    impl Host for Panics {
        fn create(&mut self, _c: &Context<'_>, _r: &Request) -> Status {
            let mut n = self.0.lock().unwrap();
            *n += 1;
            let first = *n == 1;
            drop(n);
            if first {
                panic!("TEST_ONLY_AFTER_FAKE_QUEUE")
            }
            Status::Accepted
        }
    }
    let (c, h, _gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let calls = Arc::new(Mutex::new(0));
    let mut router = Router::new(
        h.authority(),
        expected(&h),
        Rights::test_all(),
        Panics(calls.clone()),
    )
    .unwrap();
    let r = request(Kind::Create);
    let rec = incoming(&c, &h, &r).await;
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| router.dispatch(&rec))).is_err()
    );
    assert_eq!(router.dispatch(&rec).unwrap().status, Status::Unknown);
    let mut next = r;
    next.sequence = 2;
    next.operation = [9; 16];
    let next_rec = incoming(&c, &h, &next).await;
    assert_eq!(
        router.dispatch(&next_rec).unwrap().status,
        Status::Rejected(Code::Pending)
    );
    assert_eq!(*calls.lock().unwrap(), 1);
    h.close().await;
    c.close().await;
}
fn request(kind: Kind) -> Request {
    Request {
        kind,
        operation: [6; 16],
        sequence: 1,
        state: state(),
        size: if kind == Kind::Resize {
            Some((24, 80))
        } else {
            None
        },
    }
}
async fn incoming(
    c: &cc_desk_remote_security::RunningChannel,
    h: &cc_desk_remote_security::RunningChannel,
    r: &Request,
) -> DataRecord {
    let b = encode(r).unwrap();
    c.send_control(&support::scope(), 4, &b, std::time::Duration::from_secs(1))
        .await
        .unwrap();
    h.receive(&support::scope(), 4, std::time::Duration::from_secs(1))
        .await
        .unwrap()
}
#[tokio::test]
async fn lifecycle_rejects_wrong_state_binding_revoke_and_conflicting_replay() {
    let (c, h, mut gate, _cancel, grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let backend = Fake::default();
    let calls = backend.0.clone();
    assert!(matches!(
        Router::new(
            h.authority(),
            {
                let mut e = expected(&h);
                e.principal = [5; 32];
                e
            },
            Rights::test_all(),
            backend.clone()
        ),
        Err(Code::Unauthorized)
    ));
    assert!(matches!(
        Router::new(
            c.authority(),
            expected(&h),
            Rights::test_all(),
            backend.clone()
        ),
        Err(Code::Unauthorized)
    ));
    for field in 0..5 {
        let mut e = expected(&h);
        match field {
            0 => e.scope.session = "03".repeat(16),
            1 => e.scope.run.push('x'),
            2 => e.context.client_endpoint_id = "03".repeat(32),
            3 => e.context.activation = "44".repeat(32),
            _ => e.context.policy_epoch = "14".into(),
        };
        assert!(matches!(
            Router::new(h.authority(), e, Rights::test_all(), backend.clone()),
            Err(Code::Unauthorized)
        ));
    }
    let mut denied = Router::new(
        h.authority(),
        expected(&h),
        Rights::default(),
        backend.clone(),
    )
    .unwrap();
    let denied_rec = incoming(&c, &h, &request(Kind::Create)).await;
    assert_eq!(
        denied.dispatch(&denied_rec).unwrap().status,
        Status::Rejected(Code::Unauthorized)
    );
    drop(denied_rec);
    let mut router = Router::new(h.authority(), expected(&h), Rights::test_all(), backend).unwrap();
    for field in 0..5 {
        let mut r = request(Kind::Close);
        match field {
            0 => r.state.generation += 1,
            1 => r.state.fence += 1,
            2 => r.state.mode += 1,
            3 => r.state.dimension += 1,
            _ => r.state.sync += 1,
        };
        let rec = incoming(&c, &h, &r).await;
        let reply = router.dispatch(&rec).unwrap();
        assert_eq!(reply.status, Status::Rejected(Code::Stale));
        assert_eq!(reply.request, r);
    }
    assert!(calls.lock().unwrap().is_empty());
    let first = request(Kind::Create);
    let rec = incoming(&c, &h, &first).await;
    let accepted = router.dispatch(&rec).unwrap();
    assert_eq!(router.dispatch(&rec).unwrap(), accepted);
    let mut changed = first.clone();
    changed.kind = Kind::Close;
    let changed_rec = incoming(&c, &h, &changed).await;
    assert_eq!(
        router.dispatch(&changed_rec).unwrap().status,
        Status::Rejected(Code::Conflict)
    );
    drop(changed_rec);
    let mut next = first.clone();
    next.sequence = 2;
    next.operation = [7; 16];
    let next_rec = incoming(&c, &h, &next).await;
    assert_eq!(
        router.dispatch(&next_rec).unwrap().status,
        Status::Rejected(Code::Pending)
    );
    drop(next_rec);
    let (mut c2, mut h2, _cancel2) = support::pair().await;
    c2.install(&grant).unwrap();
    h2.install(&grant).unwrap();
    let c2 = c2.start().unwrap();
    let h2 = h2.start().unwrap();
    let other = incoming(&c2, &h2, &first).await;
    assert_eq!(router.dispatch(&other), Err(Code::Unauthorized));
    gate.revoke();
    assert_eq!(router.dispatch(&rec), Err(Code::Unauthorized));
    assert_eq!(calls.lock().unwrap().len(), 1);
    h.close().await;
    c.close().await;
    h2.close().await;
    c2.close().await;
}
#[tokio::test]
async fn lifecycle_unknown_and_unsupported_replies_never_become_completion() {
    struct Unknown;
    impl Host for Unknown {
        fn close(&mut self, _c: &Context<'_>, _r: &Request) -> Status {
            Status::Unknown
        }
    }
    let (c, h, _gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let mut router = Router::new(h.authority(), expected(&h), Rights::test_all(), Unknown).unwrap();
    let r = request(Kind::Close);
    let rec = incoming(&c, &h, &r).await;
    let reply = router.dispatch(&rec).unwrap();
    assert_eq!(reply.status, Status::Unknown);
    assert_eq!(router.dispatch(&rec).unwrap(), reply);
    let mut next = request(Kind::Resume);
    next.sequence = 2;
    next.operation = [8; 16];
    let next_rec = incoming(&c, &h, &next).await;
    assert_eq!(
        router.dispatch(&next_rec).unwrap().status,
        Status::Rejected(Code::Pending)
    );
    struct Missing;
    impl Host for Missing {}
    let mut missing =
        Router::new(h.authority(), expected(&h), Rights::test_all(), Missing).unwrap();
    assert_eq!(
        missing.dispatch(&rec).unwrap().status,
        Status::Rejected(Code::Unsupported)
    );
    h.close().await;
    c.close().await;
}
#[tokio::test]
async fn lifecycle_routes_preserve_identity_and_do_not_repeat_backend() {
    for kind in [
        Kind::Create,
        Kind::Attach,
        Kind::Resize,
        Kind::Resume,
        Kind::Close,
    ] {
        let (c, h, _gate, _cancel, _grant) =
            support::channels(Arc::new(AtomicU64::new(1000))).await;
        let backend = Fake::default();
        let calls = backend.0.clone();
        let mut denied = Router::new(
            h.authority(),
            expected(&h),
            Rights::default(),
            backend.clone(),
        )
        .unwrap();
        let denied_rec = incoming(&c, &h, &request(Kind::Create)).await;
        assert_eq!(
            denied.dispatch(&denied_rec).unwrap().status,
            Status::Rejected(Code::Unauthorized)
        );
        drop(denied_rec);
        let mut rights = Rights::default();
        rights.allow(kind);
        let mut router = Router::new(h.authority(), expected(&h), rights, backend).unwrap();
        let r = request(kind);
        let record = incoming(&c, &h, &r).await;
        let reply = router.dispatch(&record).unwrap();
        assert_eq!(reply.status, Status::Accepted);
        assert_eq!(reply.request, r);
        assert_eq!(decode_reply(&encode_reply(&reply).unwrap()).unwrap(), reply);
        assert_eq!(router.dispatch(&record).unwrap(), reply);
        for other in [
            Kind::Create,
            Kind::Attach,
            Kind::Resize,
            Kind::Resume,
            Kind::Close,
        ] {
            if other != kind {
                let denied = incoming(&c, &h, &request(other)).await;
                assert_eq!(
                    router.dispatch(&denied).unwrap().status,
                    Status::Rejected(Code::Unauthorized)
                );
            }
        }
        assert_eq!(*calls.lock().unwrap(), vec![kind]);
        h.close().await;
        c.close().await;
    }
}
