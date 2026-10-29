use super::*;
#[tokio::test]
async fn attach_completion_rebuilt_router_recovers_exact_result_without_execution() {
    for interrupted in [false, true] {
        for corrupt in [false, true] {
            let (c, h, mut gate, _cancel, _grant) =
                support::channels(Arc::new(AtomicU64::new(1000))).await;
            let identity = ResourceIdentity::from_proof(&h.authority());
            let sender = Arc::new(Mutex::new(None));
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("attach.sqlite");
            let r = req();
            let build = || {
                let mut rights = Rights::default();
                rights.allow(Kind::Attach);
                Router::durable(
                    h.authority(),
                    Expected {
                        context: h.authority().context().clone(),
                        principal: [4; 32],
                        scope: support::scope(),
                        state: r.state,
                    },
                    rights,
                    Store::open(&path).unwrap(),
                    DigestKey::from_backend_bytes([0x5a; 32]),
                    Fake {
                        identity: identity.clone(),
                        sender: sender.clone(),
                        early: true,
                        unknown: false,
                    },
                )
                .unwrap()
            };
            let mut router = build();
            c.send_control(
                &support::scope(),
                4,
                &encode(&r).unwrap(),
                std::time::Duration::from_secs(1),
            )
            .await
            .unwrap();
            let record = h
                .receive(&support::scope(), 4, std::time::Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(router.dispatch(&record).unwrap().status, Status::Accepted);
            router.interrupt_completion = interrupted;
            assert_eq!(
                router.complete_attach(),
                if interrupted {
                    Err(Code::Backend)
                } else {
                    Ok(true)
                }
            );
            let old = sender.lock().unwrap().take().unwrap();
            drop(router);
            if corrupt {
                let db = rusqlite::Connection::open(&path).unwrap();
                db.execute("UPDATE results SET seal=zeroblob(32)", [])
                    .unwrap();
            }
            let mut rebuilt = build();
            if corrupt {
                assert_eq!(rebuilt.dispatch(&record), Err(Code::Conflict));
                let mut next = r.clone();
                next.operation = [8; 16];
                next.sequence = 2;
                c.send_control(
                    &support::scope(),
                    4,
                    &encode(&next).unwrap(),
                    std::time::Duration::from_secs(1),
                )
                .await
                .unwrap();
                let rec = h
                    .receive(&support::scope(), 4, std::time::Duration::from_secs(1))
                    .await
                    .unwrap();
                assert_eq!(rebuilt.dispatch(&rec), Err(Code::Conflict));
                assert!(sender.lock().unwrap().is_none());
                drop(rebuilt);
                h.close().await;
                c.close().await;
                continue;
            }
            assert_eq!(rebuilt.dispatch(&record).unwrap().status, Status::Accepted);
            assert!(sender.lock().unwrap().is_none());
            assert_eq!(rebuilt.complete_attach(), Ok(false));
            assert_eq!(
                rebuilt.attached().unwrap(),
                Some(Applied {
                    request: r.clone(),
                    after: after(&r)
                })
            );
            assert!(!rebuilt.pending);
            assert_eq!(rebuilt.dispatch(&record).unwrap().status, Status::Accepted);
            assert!(sender.lock().unwrap().is_none());
            let mut next = r.clone();
            next.operation = [8; 16];
            next.sequence = 2;
            next.state = after(&r);
            c.send_control(
                &support::scope(),
                5,
                &encode(&next).unwrap(),
                std::time::Duration::from_secs(1),
            )
            .await
            .unwrap();
            let rec = h
                .receive(&support::scope(), 5, std::time::Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(rebuilt.dispatch(&rec).unwrap().status, Status::Accepted);
            assert_eq!(rebuilt.complete_attach(), Ok(true));
            assert_eq!(
                rebuilt.attached().unwrap(),
                Some(Applied {
                    request: next.clone(),
                    after: after(&next)
                })
            );
            gate.revoke();
            assert_eq!(rebuilt.attached(), Err(Code::Unauthorized));
            assert_eq!(rebuilt.dispatch(&record), Err(Code::Unauthorized));
            assert_eq!(old.complete(&r, &identity, after(&r)), Err(Code::Stale));
            drop(rebuilt);
            h.close().await;
            c.close().await;
        }
    }
}
use crate::{
    bound_test_support as support,
    journal_digest::DigestKey,
    lifecycle_completion::{Applied, Sender},
    lifecycle_journal::Store,
};
use std::sync::{Arc, Mutex, atomic::AtomicU64};
struct Fake {
    identity: ResourceIdentity,
    sender: Arc<Mutex<Option<Sender>>>,
    early: bool,
    unknown: bool,
}
impl Host for Fake {
    fn completion_backend_id(&self) -> Option<[u8; 32]> {
        Some([0x61; 32])
    }
    fn resource_identity(&self) -> Option<&ResourceIdentity> {
        Some(&self.identity)
    }
    fn attach_with_completion(&mut self, _c: &Context<'_>, r: &Request, s: Sender) -> Status {
        if self.early {
            s.complete(r, &self.identity, after(r)).unwrap();
        }
        *self.sender.lock().unwrap() = Some(s);
        if self.unknown {
            Status::Unknown
        } else {
            Status::Accepted
        }
    }
}
fn req() -> Request {
    Request {
        kind: Kind::Attach,
        operation: [7; 16],
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
#[tokio::test]
async fn attach_completion_fault_cache_cut_revocation_and_reopen() {
    for fault in [false, true] {
        let (c, h, mut gate, _cancel, _grant) =
            support::channels(Arc::new(AtomicU64::new(1000))).await;
        let proof = h.authority();
        let identity = ResourceIdentity::from_proof(&proof);
        let sender = Arc::new(Mutex::new(None));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attach.sqlite");
        let r = req();
        let mut rights = Rights::default();
        rights.allow(Kind::Attach);
        let mut router = Router::durable(
            proof,
            Expected {
                context: h.authority().context().clone(),
                principal: [4; 32],
                scope: support::scope(),
                state: r.state,
            },
            rights,
            Store::open(&path).unwrap(),
            DigestKey::from_backend_bytes([0x5a; 32]),
            Fake {
                identity: identity.clone(),
                sender: sender.clone(),
                early: true,
                unknown: false,
            },
        )
        .unwrap();
        c.send_control(
            &support::scope(),
            4,
            &encode(&r).unwrap(),
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap();
        let record = h
            .receive(&support::scope(), 4, std::time::Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(router.dispatch(&record).unwrap().status, Status::Accepted);
        if fault {
            router.backend.fail_completion_for_test()
        } else {
            router.interrupt_completion = true
        }
        assert_eq!(router.complete_attach(), Err(Code::Backend));
        assert!(router.pending);
        assert_eq!(router.attached().unwrap(), None);
        if !fault {
            gate.revoke();
            assert_eq!(router.complete_attach(), Ok(true));
            assert_eq!(router.attached(), Err(Code::Unauthorized));
        }
        let old = sender.lock().unwrap().take().unwrap();
        drop(router);
        assert_eq!(old.complete(&r, &identity, after(&r)), Err(Code::Stale));
        let reopened = Store::open(&path).unwrap();
        let db = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let phase: i64 = db
            .query_row("SELECT state FROM ops", [], |r| r.get(0))
            .unwrap();
        assert_eq!(phase, if fault { 4 } else { 3 });
        drop(db);
        drop(reopened);
        if fault {
            // Recovered uncertainty cannot acquire a new completion capability.
            let mut rights = Rights::default();
            rights.allow(Kind::Attach);
            let mut rebuilt = Router::durable(
                h.authority(),
                Expected {
                    context: h.authority().context().clone(),
                    principal: [4; 32],
                    scope: support::scope(),
                    state: r.state,
                },
                rights,
                Store::open(&path).unwrap(),
                DigestKey::from_backend_bytes([0x5a; 32]),
                Fake {
                    identity: identity.clone(),
                    sender: Arc::new(Mutex::new(None)),
                    early: false,
                    unknown: false,
                },
            )
            .unwrap();
            assert_eq!(rebuilt.dispatch(&record).unwrap().status, Status::Unknown);
            assert_eq!(rebuilt.complete_attach(), Ok(false));
            assert!(rebuilt.pending);
            assert_eq!(rebuilt.attached().unwrap(), None);
        }
        h.close().await;
        c.close().await;
    }
}
fn after(r: &Request) -> State {
    let mut a = r.state;
    a.fence += 1;
    a.sync += 1;
    a
}
#[tokio::test]
async fn attach_completion_router_early_late_and_uncertain() {
    for (early, unknown) in [(true, false), (false, false), (true, true)] {
        let (c, h, _gate, _cancel, _grant) =
            support::channels(Arc::new(AtomicU64::new(1000))).await;
        let proof = h.authority();
        let identity = ResourceIdentity::from_proof(&proof);
        let sender = Arc::new(Mutex::new(None));
        let dir = tempfile::tempdir().unwrap();
        let r = req();
        let mut rights = Rights::default();
        rights.allow(Kind::Attach);
        let mut router = Router::durable(
            proof,
            Expected {
                context: h.authority().context().clone(),
                principal: [4; 32],
                scope: support::scope(),
                state: r.state,
            },
            rights,
            Store::open(&dir.path().join("attach.sqlite")).unwrap(),
            DigestKey::from_backend_bytes([0x5a; 32]),
            Fake {
                identity: identity.clone(),
                sender: sender.clone(),
                early,
                unknown,
            },
        )
        .unwrap();
        c.send_control(
            &support::scope(),
            4,
            &encode(&r).unwrap(),
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap();
        let record = h
            .receive(&support::scope(), 4, std::time::Duration::from_secs(1))
            .await
            .unwrap();
        let reply = router.dispatch(&record).unwrap();
        assert_eq!(
            reply.status,
            if unknown {
                Status::Unknown
            } else {
                Status::Accepted
            }
        );
        assert_eq!(router.attached().unwrap(), None);
        sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .complete(&r, &identity, after(&r))
            .unwrap();
        if unknown {
            assert!(router.complete_attach().is_err());
            assert!(router.pending)
        } else {
            assert_eq!(router.complete_attach(), Ok(true));
            assert_eq!(router.complete_attach(), Ok(false));
            assert!(!router.pending);
            assert_eq!(router.state, after(&r));
            assert_eq!(
                router.attached().unwrap(),
                Some(Applied {
                    request: r.clone(),
                    after: router.state
                })
            );
            if !early {
                let old = sender.lock().unwrap().as_ref().unwrap().clone();
                let mut next = r.clone();
                next.state = router.state;
                next.operation = [8; 16];
                next.sequence = 2;
                c.send_control(
                    &support::scope(),
                    5,
                    &encode(&next).unwrap(),
                    std::time::Duration::from_secs(1),
                )
                .await
                .unwrap();
                let rec = h
                    .receive(&support::scope(), 5, std::time::Duration::from_secs(1))
                    .await
                    .unwrap();
                assert_eq!(router.dispatch(&rec).unwrap().status, Status::Accepted);
                assert!(router.pending);
                assert_eq!(router.attached().unwrap(), None);
                assert_eq!(old.complete(&r, &identity, after(&r)), Err(Code::Stale));
                assert_eq!(router.complete_attach(), Ok(false));
                sender
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .complete(&next, &identity, after(&next))
                    .unwrap();
                assert_eq!(router.complete_attach(), Ok(true));
            }
        }
        drop(router);
        h.close().await;
        c.close().await;
    }
}
