use super::*;
use crate::{
    bound_test_support as support,
    lifecycle_journal::Store,
    lifecycle_router::{Expected, Rights, Router},
};
use cc_desk_remote_record::lifecycle::{Kind, Request, encode};
use std::sync::atomic::AtomicU64;
#[path = "local_writer_attach_connection_tests.rs"]
mod connection_tests;
#[test]
fn native_attach_memory_default_denies_history() {
    struct Unopted;
    impl Host for Unopted {}
    assert!(!Unopted.permits_attach_recovery());
}
struct TestIo {
    writes: Arc<AtomicUsize>,
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
    block: bool,
}
impl Write for TestIo {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.writes.fetch_add(1, Ordering::AcqRel);
        if self.block {
            self.block = false;
            self.entered.send(()).unwrap();
            self.release
                .recv_timeout(Duration::from_secs(2))
                .map_err(std::io::Error::other)?;
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[tokio::test]
async fn native_attach_memory_installs_peer_without_snapshot_ack() {
    for drift in 0..=18 {
        let now = Arc::new(AtomicU64::new(1000));
        let writes = Arc::new(AtomicUsize::new(0));
        let (c, h, mut gate, _cancel, grant) = support::channels(now.clone()).await;
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let mut owner =
            WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
        owner.principal = Some([4; 32]);
        let target = Target {
            host: support::host(),
            session: [2; 16],
            run: support::scope().run,
            generation: 3,
            incarnation: 11,
        };
        let inputdir = tempfile::tempdir().unwrap();
        let mut started = owner
            .start_authorized(
                Journal::open(&inputdir.path().join("input.sqlite")).unwrap(),
                TestIo {
                    writes: writes.clone(),
                    entered: entered_tx,
                    release: release_rx,
                    block: drift == 3 || drift == 4 || (13..=15).contains(&drift),
                },
                SessionStart {
                    target: target.clone(),
                    device: support::device(),
                    writer_epoch: 9,
                    key: DigestKey::from_backend_bytes([0x5a; 32]),
                    rows: if (9..=12).contains(&drift) { 64 } else { 3 },
                    columns: if (9..=12).contains(&drift) { 160 } else { 10 },
                },
                h.authority(),
                [8; 16],
                4,
            )
            .unwrap();
        let old = started.peer;
        let frame = old
            .capture()
            .unwrap()
            .wait(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let stamp = frame.issued_stamp().unwrap();
        let stale = stamp.clone();
        let mut baseline = State {
            generation: 3,
            fence: 4,
            mode: stamp.mode_epoch.parse().unwrap(),
            dimension: stamp.dimension_epoch.parse().unwrap(),
            sync: stamp.sync_epoch.parse().unwrap(),
        };
        drop(frame);
        if drift == 5 {
            baseline.fence = 3;
        }
        let (mut c2, mut h2, _cancel2) = support::pair().await;
        c2.install(&grant).unwrap();
        h2.install(&grant).unwrap();
        let c2 = c2.start().unwrap();
        let h2 = h2.start().unwrap();
        let owner = Arc::new(Mutex::new(owner));
        let backend = AttachBackend::new(
            owner.clone(),
            target.clone(),
            support::device(),
            [9; 16],
            baseline,
            [0x61; 32],
            Duration::from_secs(1),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut rights = Rights::default();
        rights.allow(Kind::Attach);
        let mut router = Router::durable(
            h2.authority(),
            Expected {
                context: h2.authority().context().clone(),
                principal: [4; 32],
                scope: support::scope(),
                state: baseline,
            },
            rights,
            Store::open(&dir.path().join("attach.sqlite")).unwrap(),
            DigestKey::from_backend_bytes([0x5a; 32]),
            backend,
        )
        .unwrap();
        let r = Request {
            kind: Kind::Attach,
            operation: [7; 16],
            sequence: 1,
            state: baseline,
            size: None,
        };
        c2.send_control(
            &support::scope(),
            baseline.fence,
            &encode(&r).unwrap(),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        let rec = h2
            .receive(&support::scope(), baseline.fence, Duration::from_secs(1))
            .await
            .unwrap();
        if drift == 1 {
            drop(
                old.capture()
                    .unwrap()
                    .wait(Duration::from_secs(1))
                    .unwrap()
                    .unwrap(),
            );
        }
        if drift == 2 {
            assert!(matches!(
                started
                    .output
                    .feed_at(0, b"\x1b[?1004h".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(1))
                    .unwrap(),
                replies::OutputResult::Processed { .. }
            ));
        }
        let blocked = if drift == 3 || drift == 4 {
            let p = started.output.feed_at(0, b"\x1b[5n".to_vec()).unwrap();
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            Some(p)
        } else {
            None
        };
        if drift == 6 {
            now.store(11000, Ordering::Release);
            assert_eq!(router.dispatch(&rec), Err(Code::Unauthorized));
        } else if drift == 7 {
            {
                let _held = owner.lock().unwrap();
                assert_eq!(
                    router.dispatch(&rec).unwrap().status,
                    Status::Rejected(Code::Pending)
                );
            }
            assert_eq!(
                owner
                    .lock()
                    .unwrap()
                    .control_status(&target)
                    .unwrap()
                    .wait(Duration::from_secs(1))
                    .unwrap()
                    .unwrap()
                    .fence,
                4
            );
        } else {
            assert_eq!(router.dispatch(&rec).unwrap().status, Status::Accepted);
            if drift == 8 {
                router.inspect_backend_mut().expire_before_publish = true;
            }
            assert!(!router.inspect_backend().permits_attach_recovery());
            assert!(matches!(
                router.inspect_backend_mut().connection(),
                Err(Reject::Pending)
            ));
            let mut next = r.clone();
            next.sequence = 2;
            next.operation = [8; 16];
            c2.send_control(
                &support::scope(),
                baseline.fence,
                &encode(&next).unwrap(),
                Duration::from_secs(1),
            )
            .await
            .unwrap();
            let next_record = h2
                .receive(&support::scope(), baseline.fence, Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(
                router.dispatch(&next_record).unwrap().status,
                Status::Rejected(Code::Pending)
            );
            if drift == 3 {
                router.inspect_backend_mut().pending.as_mut().unwrap().end = Instant::now();
                assert_eq!(router.complete_attach(), Err(Code::Backend));
                assert!(router.inspect_backend().pending.is_some());
            }
            if drift == 4 {
                gate.revoke();
            }
            if let Some(p) = blocked {
                release_tx.send(()).unwrap();
                assert!(matches!(
                    p.wait(Duration::from_secs(1)).unwrap(),
                    replies::OutputResult::Processed { .. }
                ));
            }
            let end = Instant::now() + Duration::from_secs(1);
            if (1..=8).contains(&drift) {
                loop {
                    match router.complete_attach() {
                        Err(Code::Backend) => break,
                        Ok(false) => {
                            assert!(Instant::now() < end);
                            tokio::task::yield_now().await;
                        }
                        v => panic!("stale native epoch accepted: {v:?}"),
                    }
                }
                if drift == 4 {
                    assert_eq!(router.attached(), Err(Code::Unauthorized));
                } else {
                    assert_eq!(router.attached().unwrap(), None);
                    assert_eq!(router.dispatch(&rec).unwrap().status, Status::Accepted);
                }
                let status = owner
                    .lock()
                    .unwrap()
                    .control_status(&target)
                    .unwrap()
                    .wait(Duration::from_secs(1))
                    .unwrap()
                    .unwrap();
                assert_eq!(status.fence, if drift == 4 || drift == 5 { 4 } else { 5 });
            } else {
                if drift == 18 {
                    router.interrupt_publication_for_test();
                    let end = Instant::now() + Duration::from_secs(1);
                    loop {
                        match router.complete_attach() {
                            Ok(false) => {
                                assert!(Instant::now() < end);
                                tokio::task::yield_now().await;
                            }
                            Err(Code::Backend) => break,
                            v => panic!("publication cut: {v:?}"),
                        }
                    }
                    assert!(matches!(
                        router.inspect_backend_mut().connection(),
                        Err(Reject::Pending)
                    ));
                }
                while !router.complete_attach().unwrap() {
                    assert!(Instant::now() < end);
                    tokio::task::yield_now().await;
                }
                assert_eq!(router.attached().unwrap().unwrap().after.fence, 5);
                assert!(old.capture().is_err());
                assert!(h.authority().check().is_err());
                let peer = router.inspect_backend().peer.as_ref().unwrap();
                let frame = peer
                    .capture()
                    .unwrap()
                    .wait(Duration::from_secs(1))
                    .unwrap()
                    .unwrap();
                let ack = cc_desk_remote_record::projection::encode_ack(
                    &cc_desk_remote_record::projection::Ack {
                        profile: 1,
                        stamp: frame.issued_stamp().unwrap().clone(),
                    },
                    1,
                )
                .unwrap();
                c2.send_control(&support::scope(), 5, &ack, Duration::from_secs(1))
                    .await
                    .unwrap();
                let record = h2
                    .receive(&support::scope(), 5, Duration::from_secs(1))
                    .await
                    .unwrap();
                assert!(
                    peer.acknowledge_received(&record)
                        .unwrap()
                        .wait(Duration::from_secs(1))
                        .unwrap()
                        .is_err()
                );
                drop(frame);
                drop(record);
                drop(rec);
                drop(next_record);
                let mut connection = router.inspect_backend_mut().connection().unwrap();
                connection_tests::connection_flow(
                    &mut connection,
                    connection_tests::Flow {
                        c: &c2,
                        h: &h2,
                        writes: &writes,
                        now: &now,
                        gate: &mut gate,
                        output: &mut started.output,
                        entered: &entered_rx,
                        release: &release_tx,
                        grant: &grant,
                        stale: &stale,
                    },
                    drift,
                )
                .await;
                drop(connection);
                assert!(matches!(
                    router.inspect_backend_mut().connection(),
                    Err(Reject::Pending)
                ));
            }
        }
        drop(router);
        owner.lock().unwrap().seal(&target).unwrap();
        let end = Instant::now() + Duration::from_secs(1);
        while !owner.lock().unwrap().try_reap(&target).unwrap() {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        drop(started.output);
        h2.close().await;
        c2.close().await;
        h.close().await;
        c.close().await;
    }
}
