use super::reply::decode_hex;
use super::*;
use crate::bound_test_support as support;
use cc_desk_remote_record::{control_io as wire, input, projection as projection_wire};
use std::sync::atomic::AtomicU64;

#[tokio::test]
async fn input_cursor_wire_recovers_pending_without_minting_input_permission() {
    for seed in [
        None,
        Some(Outcome::HostWritten),
        Some(Outcome::Rejected),
        Some(Outcome::Partial),
        Some(Outcome::Unknown),
    ] {
        let now = Arc::new(AtomicU64::new(1000));
        let (c, h, _gate, _cancel, _grant) = support::channels(now.clone()).await;
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
        let dir = tempfile::tempdir().unwrap();
        let mut journal = Journal::open(&dir.path().join("cursor.sqlite")).unwrap();
        let key = OperationKey {
            host: target.host,
            session: target.session,
            run: target.run.clone(),
            generation: 3,
            incarnation: 11,
            device: support::device(),
            operation: [9; 16],
            sequence: 1,
        };
        if let Some(outcome) = seed {
            journal
                .run_once(&key, &KeyedDigest([7; 32]), || outcome)
                .unwrap();
        }
        let started = owner
            .start_authorized(
                journal,
                std::io::sink(),
                SessionStart {
                    target: target.clone(),
                    device: support::device(),
                    writer_epoch: 9,
                    key: DigestKey::from_backend_bytes([0x5a; 32]),
                    rows: 3,
                    columns: 10,
                },
                h.authority(),
                [8; 16],
                4,
            )
            .unwrap();
        let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
        let reply = exchange(
            &mut conn,
            &c,
            &h,
            br#"{"message":"InputCursor","profile":1,"request":"1"}"#,
        )
        .await;
        let wire::Message::InputCursorState {
            request,
            next_sequence,
            pending,
            ..
        } = reply
        else {
            panic!("cursor state");
        };
        assert_eq!(request, "1");
        assert!(conn.view.is_none());
        assert!(!conn.busy);
        if let Some(outcome) = seed {
            assert!(next_sequence.is_none());
            let p = pending.unwrap();
            assert_eq!(p.operation, "09".repeat(16));
            assert_eq!(p.sequence, "1");
            let expected = match outcome {
                Outcome::HostWritten => wire::Outcome::HostWritten,
                Outcome::Rejected => wire::Outcome::Rejected,
                Outcome::Partial => wire::Outcome::Partial,
                _ => wire::Outcome::Unknown,
            };
            assert_eq!(p.outcome, expected);
            if let Some(proof) = p.proof {
                let ack = wire::encode(&wire::Message::ReceiptAck {
                    profile: 1,
                    request: "2".into(),
                    operation: p.operation,
                    sequence: p.sequence,
                    outcome: p.outcome,
                    proof,
                })
                .unwrap();
                assert!(matches!(
                    exchange(&mut conn, &c, &h, &ack).await,
                    wire::Message::ReceiptSettled {
                        disposition: wire::Disposition::Advanced,
                        ..
                    }
                ));
                assert!(
                    matches!(exchange(&mut conn,&c,&h,br#"{"message":"InputCursor","profile":1,"request":"3"}"#).await,wire::Message::InputCursorState{next_sequence:Some(n),pending:None,..} if n=="2")
                );
            } else {
                assert!(matches!(outcome, Outcome::Partial | Outcome::Unknown));
            }
        } else {
            assert_eq!(next_sequence.as_deref(), Some("1"));
            assert!(pending.is_none());
        }
        let duplicate = received(
            &c,
            &h,
            4,
            br#"{"message":"InputCursor","profile":1,"request":"1"}"#,
        )
        .await;
        let expected = if seed.is_none() {
            now.store(11000, Ordering::Release);
            Reject::Policy
        } else if seed == Some(Outcome::Partial) {
            h.authority().request_close();
            Reject::Policy
        } else if seed == Some(Outcome::Rejected) {
            conn.fence = 5;
            Reject::Target
        } else {
            Reject::Epoch
        };
        assert_eq!(
            conn.dispatch_wire(&duplicate, &h, Duration::from_secs(1))
                .await,
            Err(expected)
        );
        drop(conn);
        owner.seal(&target).unwrap();
        let end = Instant::now() + Duration::from_secs(1);
        while !owner.try_reap(&target).unwrap() {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        h.close().await;
        c.close().await;
    }
}

async fn received(c: &RunningChannel, h: &RunningChannel, fence: u64, bytes: &[u8]) -> DataRecord {
    c.send_control(&support::scope(), fence, bytes, Duration::from_secs(1))
        .await
        .unwrap();
    h.receive(&support::scope(), fence, Duration::from_secs(1))
        .await
        .unwrap()
}
async fn exchange(
    conn: &mut Connection<'_>,
    c: &RunningChannel,
    h: &RunningChannel,
    bytes: &[u8],
) -> wire::Message {
    let r = received(c, h, 4, bytes).await;
    conn.dispatch_wire(&r, h, Duration::from_secs(1))
        .await
        .unwrap();
    let reply = c
        .receive(&support::scope(), 4, Duration::from_secs(1))
        .await
        .unwrap();
    wire::decode(reply.payload()).unwrap()
}
async fn synchronized(
    conn: &mut Connection<'_>,
    c: &RunningChannel,
    h: &RunningChannel,
) -> ([u8; 32], u64) {
    let request = received(
        c,
        h,
        4,
        br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
    )
    .await;
    conn.dispatch_wire(&request, h, Duration::from_secs(1))
        .await
        .unwrap();
    let mut stamp = None;
    let mut cells = 0usize;
    while cells < 30 {
        let record = c
            .receive(&support::scope(), 4, Duration::from_secs(1))
            .await
            .unwrap();
        let message = projection_wire::decode(record.payload(), 1).unwrap();
        let projection_wire::Content::SnapshotPart(part) = message.content else {
            panic!("snapshot expected")
        };
        assert_eq!(usize::from(part.start), cells);
        cells += part.cells.len();
        stamp = Some(part.stamp);
    }
    let stamp = stamp.unwrap();
    let ack = projection_wire::encode_ack(
        &projection_wire::Ack {
            profile: 1,
            stamp: stamp.clone(),
        },
        1,
    )
    .unwrap();
    assert_eq!(
        exchange(conn, c, h, &ack).await,
        wire::Message::ProjectionAcknowledged { profile: 1, stamp }
    );
    let reply = exchange(
        conn,
        c,
        h,
        br#"{"message":"Window","profile":1,"request":"2"}"#,
    )
    .await;
    let wire::Message::WindowGranted {
        request,
        window,
        not_after_host_ms,
        ..
    } = reply
    else {
        panic!("window expected")
    };
    assert_eq!(request, "2");
    (
        decode_hex(&window).unwrap(),
        not_after_host_ms.parse().unwrap(),
    )
}
#[tokio::test]
async fn phone_wire_unknown_and_expired_input_do_not_unlock_or_replay() {
    struct Fail(Arc<AtomicUsize>);
    impl Write for Fail {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Err(std::io::Error::other("TEST_ONLY_UNCERTAIN_WRITE"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for expired in [false, true] {
        let now = Arc::new(AtomicU64::new(1000));
        let (c, h, _gate, _cancel, _grant) = support::channels(now.clone()).await;
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
        let dir = tempfile::tempdir().unwrap();
        let writes = Arc::new(AtomicUsize::new(0));
        let started = owner
            .start_authorized(
                Journal::open(&dir.path().join("unknown.sqlite")).unwrap(),
                Fail(writes.clone()),
                SessionStart {
                    target: target.clone(),
                    device: support::device(),
                    writer_epoch: 9,
                    key: DigestKey::from_backend_bytes([0x5a; 32]),
                    rows: 3,
                    columns: 10,
                },
                h.authority(),
                [8; 16],
                4,
            )
            .unwrap();
        let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
        let (window, not_after_host_ms) = synchronized(&mut conn, &c, &h).await;
        let bytes = input::encode(&input::Input {
            operation: [2; 16],
            sequence: 1,
            window,
            not_after_host_ms,
            bytes: b"TEST_ONLY".to_vec(),
        })
        .unwrap();
        if expired {
            let held = received(&c, &h, 4, &bytes).await;
            now.store(11000, Ordering::Release);
            assert_eq!(
                conn.dispatch_wire(&held, &h, Duration::from_secs(1)).await,
                Err(Reject::Policy)
            );
            assert_eq!(writes.load(Ordering::Acquire), 0);
        } else {
            let held = received(&c, &h, 4, &bytes).await;
            // The actual native writer seals its peer on uncertain IO. Live
            // authority is gone, so even an Unknown reply must not be disclosed.
            assert_eq!(
                conn.dispatch_wire(&held, &h, Duration::from_secs(1)).await,
                Err(Reject::Policy)
            );
            assert_eq!(
                conn.dispatch_wire(&held, &h, Duration::from_secs(1)).await,
                Err(Reject::Policy)
            );
            let forged = format!(
                r#"{{"message":"ReceiptAck","profile":1,"request":"4","operation":"{}","sequence":"1","outcome":"Unknown","proof":"{}"}}"#,
                "02".repeat(16),
                "03".repeat(32)
            );
            assert!(wire::decode(forged.as_bytes()).is_err());
            assert_eq!(writes.load(Ordering::Acquire), 1);
        }
        assert!(h.authority().check().is_err());
        drop(conn);
        owner.seal(&target).unwrap();
        let end = Instant::now() + Duration::from_secs(1);
        while !owner.try_reap(&target).unwrap() {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        h.close().await;
        c.close().await;
    }
}
#[tokio::test]
async fn phone_wire_cancel_during_input_closes_owner_without_replaying_write() {
    struct Block(Arc<AtomicUsize>, mpsc::SyncSender<()>, mpsc::Receiver<()>);
    impl Write for Block {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.fetch_add(1, Ordering::AcqRel);
            self.1.send(()).unwrap();
            self.2.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (c, h, _gate, _cancel, _grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
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
    let dir = tempfile::tempdir().unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let (entered_tx, entered) = mpsc::sync_channel(1);
    let (release, release_rx) = mpsc::sync_channel(1);
    let started = owner
        .start_authorized(
            Journal::open(&dir.path().join("cancel.sqlite")).unwrap(),
            Block(writes.clone(), entered_tx, release_rx),
            SessionStart {
                target: target.clone(),
                device: support::device(),
                writer_epoch: 9,
                key: DigestKey::from_backend_bytes([0x5a; 32]),
                rows: 3,
                columns: 10,
            },
            h.authority(),
            [8; 16],
            4,
        )
        .unwrap();
    let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
    let (window, not_after_host_ms) = synchronized(&mut conn, &c, &h).await;
    let bytes = input::encode(&input::Input {
        operation: [2; 16],
        sequence: 1,
        window,
        not_after_host_ms,
        bytes: b"TEST_ONLY".to_vec(),
    })
    .unwrap();
    let r = received(&c, &h, 4, &bytes).await;
    let mut pending = Box::pin(conn.dispatch_wire(&r, &h, Duration::from_secs(1)));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(pending.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(pending);
    assert!(h.authority().check().is_err());
    release.send(()).unwrap();
    drop(conn);
    owner.seal(&target).unwrap();
    let end = Instant::now() + Duration::from_secs(1);
    while !owner.try_reap(&target).unwrap() {
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    assert_eq!(writes.load(Ordering::Acquire), 1);
    h.close().await;
    c.close().await;
}
#[tokio::test]
async fn phone_wire_roundtrip_uses_exact_native_receipt_and_durable_settlement() {
    struct Count(Arc<AtomicUsize>);
    impl Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let now = Arc::new(AtomicU64::new(1000));
    let (c, h, _gate, _cancel, _grant) = support::channels(now.clone()).await;
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
    let dir = tempfile::tempdir().unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let started = owner
        .start_authorized(
            Journal::open(&dir.path().join("phone-wire.sqlite")).unwrap(),
            Count(writes.clone()),
            SessionStart {
                target: target.clone(),
                device: support::device(),
                writer_epoch: 9,
                key: DigestKey::from_backend_bytes([0x5a; 32]),
                rows: 3,
                columns: 10,
            },
            h.authority(),
            [8; 16],
            4,
        )
        .unwrap();
    let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
    let request = received(
        &c,
        &h,
        4,
        br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
    )
    .await;
    conn.dispatch_wire(&request, &h, Duration::from_secs(1))
        .await
        .unwrap();
    let mut stamp = None;
    let mut cells = 0usize;
    while cells < 30 {
        let record = c
            .receive(&support::scope(), 4, Duration::from_secs(1))
            .await
            .unwrap();
        let message = projection_wire::decode(record.payload(), 1).unwrap();
        let projection_wire::Content::SnapshotPart(part) = message.content else {
            panic!("snapshot expected")
        };
        assert_eq!(usize::from(part.start), cells);
        cells += part.cells.len();
        stamp = Some(part.stamp);
    }
    let stamp = stamp.unwrap();
    let ack = projection_wire::encode_ack(
        &projection_wire::Ack {
            profile: 1,
            stamp: stamp.clone(),
        },
        1,
    )
    .unwrap();
    assert_eq!(
        exchange(&mut conn, &c, &h, &ack).await,
        wire::Message::ProjectionAcknowledged { profile: 1, stamp }
    );
    let reply = exchange(
        &mut conn,
        &c,
        &h,
        br#"{"message":"Window","profile":1,"request":"2"}"#,
    )
    .await;
    let wire::Message::WindowGranted {
        request,
        window,
        not_after_host_ms,
        ..
    } = reply
    else {
        panic!("window expected")
    };
    assert_eq!(request, "2");
    let token = decode_hex::<32>(&window).unwrap();
    let bytes = input::encode(&input::Input {
        operation: [2; 16],
        sequence: 1,
        window: token,
        not_after_host_ms: not_after_host_ms.parse().unwrap(),
        bytes: b"TEST_ONLY".to_vec(),
    })
    .unwrap();
    let reply = exchange(&mut conn, &c, &h, &bytes).await;
    let wire::Message::InputReceipt {
        profile,
        operation,
        sequence,
        outcome,
        proof: Some(proof),
    } = reply
    else {
        panic!("native receipt expected")
    };
    assert_eq!(outcome, wire::Outcome::HostWritten);
    assert_eq!(writes.load(Ordering::Acquire), 1);
    let query = wire::encode(&wire::Message::ReceiptQuery {
        profile: 1,
        request: "3".into(),
        operation: operation.clone(),
        sequence: sequence.clone(),
    })
    .unwrap();
    let status = exchange(&mut conn, &c, &h, &query).await;
    assert_eq!(
        status,
        wire::Message::ReceiptStatus {
            profile,
            request: "3".into(),
            operation: operation.clone(),
            sequence: sequence.clone(),
            status: wire::Status::Known,
            outcome: Some(outcome),
            proof: Some(proof.clone())
        }
    );
    for (request, submitted, expected) in [
        ("4", "ff".repeat(32), wire::Disposition::Rejected),
        ("5", proof.clone(), wire::Disposition::Advanced),
        ("6", proof.clone(), wire::Disposition::AlreadySettled),
    ] {
        let ack = wire::encode(&wire::Message::ReceiptAck {
            profile: 1,
            request: request.into(),
            operation: operation.clone(),
            sequence: sequence.clone(),
            outcome,
            proof: submitted,
        })
        .unwrap();
        assert_eq!(
            exchange(&mut conn, &c, &h, &ack).await,
            wire::Message::ReceiptSettled {
                profile: 1,
                request: request.into(),
                operation: operation.clone(),
                sequence: sequence.clone(),
                outcome,
                disposition: expected
            }
        );
    }
    assert_eq!(
        writes.load(Ordering::Acquire),
        1,
        "query/settlement must not replay IO"
    );
    let stale = received(
        &c,
        &h,
        3,
        br#"{"message":"Window","profile":1,"request":"7"}"#,
    )
    .await;
    assert_eq!(
        conn.dispatch_wire(&stale, &h, Duration::from_secs(1)).await,
        Err(Reject::Target)
    );
    assert!(
        h.authority().check().is_err(),
        "failed dispatch closes only the original data owner"
    );
    drop(conn);
    owner.seal(&target).unwrap();
    let end = Instant::now() + Duration::from_secs(1);
    while !owner.try_reap(&target).unwrap() {
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    h.close().await;
    c.close().await;
}

#[path = "local_writer_control_lease_tests.rs"]
mod lease;

#[tokio::test]
async fn snapshot_pull_original_dispatch_captures_full_grid_and_counter_two() {
    use cc_desk_remote_record::snapshot_pull as pull;
    let (c, h, _gate, _cancel) = support::pull_channels(Arc::new(AtomicU64::new(1000))).await;
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
    let dir = tempfile::tempdir().unwrap();
    let started = owner
        .start_authorized(
            Journal::open(&dir.path().join("pull.sqlite")).unwrap(),
            std::io::sink(),
            SessionStart {
                target,
                device: support::device(),
                writer_epoch: 9,
                key: DigestKey::from_backend_bytes([0x5a; 32]),
                rows: 24,
                columns: 80,
            },
            h.authority(),
            [8; 16],
            4,
        )
        .unwrap();
    let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
    let begin = pull::encode(&pull::Message::SnapshotBegin {
        profile: 1,
        request: "1".into(),
    })
    .unwrap();
    let request = received(&c, &h, 4, &begin).await;
    let client = async {
        let mut cells = 0;
        let mut index = 0;
        let mut stamp = None;
        loop {
            let r = c
                .receive(&support::scope(), 4, Duration::from_secs(1))
                .await
                .unwrap();
            let (batch, part) = pull::decode_validated_batch(r.payload()).unwrap();
            assert_eq!(batch.request, 1);
            assert_eq!(batch.batch, index);
            assert_eq!(usize::from(part.start), cells);
            cells += part.cells.len();
            if let Some(s) = &stamp {
                assert!(*s == part.stamp);
            } else {
                stamp = Some(part.stamp.clone());
            }
            if batch.last {
                break;
            }
            index += 1;
            let next = pull::encode(&pull::Message::SnapshotNext {
                profile: 1,
                request: "1".into(),
                transfer: batch.transfer.iter().map(|b| format!("{b:02x}")).collect(),
                batch: index.to_string(),
                through: cells.to_string(),
            })
            .unwrap();
            drop(part);
            drop(r);
            c.send_control(&support::scope(), 4, &next, Duration::from_secs(1))
                .await
                .unwrap();
        }
        assert_eq!(cells, 1920);
        assert!(index > 0);
        stamp.unwrap()
    };
    let (result, stamp) = tokio::join!(
        conn.dispatch_wire(&request, &h, Duration::from_secs(1)),
        client
    );
    result.unwrap();
    assert!(!conn.busy);
    assert_eq!(conn.last, 1);
    let ack = projection_wire::encode_ack(&projection_wire::Ack { profile: 1, stamp }, 1).unwrap();
    exchange(&mut conn, &c, &h, &ack).await;
    let result = exchange(
        &mut conn,
        &c,
        &h,
        br#"{"message":"Window","profile":1,"request":"2"}"#,
    )
    .await;
    assert!(matches!(result,wire::Message::WindowGranted{request,..} if request=="2"));
    c.close().await;
    h.close().await;
    assert!(c.wait_closed_checked().await.is_ok());
    assert!(h.wait_closed_checked().await.is_ok());
}

#[tokio::test]
async fn snapshot_pull_original_dispatch_rejects_bad_next_revoke_and_deadline() {
    for mode in 0..4 {
        use cc_desk_remote_record::snapshot_pull as pull;
        let (c, h, mut gate, _cancel) =
            support::pull_channels(Arc::new(AtomicU64::new(1000))).await;
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
        let dir = tempfile::tempdir().unwrap();
        let started = owner
            .start_authorized(
                Journal::open(&dir.path().join("pull.sqlite")).unwrap(),
                std::io::sink(),
                SessionStart {
                    target,
                    device: support::device(),
                    writer_epoch: 9,
                    key: DigestKey::from_backend_bytes([0x5a; 32]),
                    rows: 24,
                    columns: 80,
                },
                h.authority(),
                [8; 16],
                4,
            )
            .unwrap();
        let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
        let begin = pull::encode(&pull::Message::SnapshotBegin {
            profile: 1,
            request: "1".into(),
        })
        .unwrap();
        let request = received(&c, &h, 4, &begin).await;
        let client = async {
            let r = c
                .receive(&support::scope(), 4, Duration::from_secs(1))
                .await
                .unwrap();
            let (batch, _) = pull::decode_validated_batch(r.payload()).unwrap();
            assert!(!batch.last);
            let next = pull::encode(&pull::Message::SnapshotNext {
                profile: 1,
                request: "1".into(),
                transfer: batch.transfer.iter().map(|b| format!("{b:02x}")).collect(),
                batch: "1".into(),
                through: (batch.through + 1).to_string(),
            })
            .unwrap();
            drop(r);
            match mode {
                0 => {
                    c.send_control(&support::scope(), 4, &next, Duration::from_secs(1))
                        .await
                        .unwrap();
                }
                1 => h.authority().request_close(),
                3 => gate.revoke(),
                _ => tokio::time::sleep(Duration::from_millis(150)).await,
            }
        };
        let (result, ()) = tokio::join!(
            conn.dispatch_wire(&request, &h, Duration::from_millis(100)),
            client
        );
        assert!(result.is_err());
        assert_eq!(owner.budget.test_used(), 0);
        assert!(h.authority().check().is_err());
        c.close().await;
        h.close().await;
        assert!(c.wait_closed_checked().await.is_ok());
        assert!(h.wait_closed_checked().await.is_ok());
    }
}

#[tokio::test]
async fn snapshot_pull_production_client_and_host_dispatch() {
    let (c, h, _gate, _cancel) =
        support::native_pull_channels(Arc::new(AtomicU64::new(1000))).await;
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
    let dir = tempfile::tempdir().unwrap();
    let started = owner
        .start_authorized(
            Journal::open(&dir.path().join("pull.sqlite")).unwrap(),
            std::io::sink(),
            SessionStart {
                target,
                device: support::device(),
                writer_epoch: 9,
                key: DigestKey::from_backend_bytes([0x5a; 32]),
                rows: 24,
                columns: 80,
            },
            h.authority(),
            [8; 16],
            4,
        )
        .unwrap();
    let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
    let server = async {
        let request = h
            .receive(&support::scope(), 4, Duration::from_secs(1))
            .await
            .unwrap();
        conn.dispatch_wire(&request, &h, Duration::from_secs(1))
            .await
    };
    let (result, sent) = tokio::join!(c.snapshot_pull(1, Duration::from_secs(1)), server);
    sent.unwrap();
    let batch = result.unwrap();
    assert!(batch.parts.len() > 1);
    let mut cells = 0;
    for raw in &batch.parts {
        let projection_wire::Content::SnapshotPart(p) =
            projection_wire::decode(raw, 1).unwrap().content
        else {
            panic!()
        };
        assert_eq!(usize::from(p.start), cells);
        cells += p.cells.len();
    }
    assert_eq!(cells, 1920);
    assert_eq!(conn.last, 1);
    assert!(c.retire().await.is_ok());
    h.close().await;
    assert!(h.wait_closed_checked().await.is_ok());
}

#[tokio::test]
async fn snapshot_pull_cancel_polled_host_future_releases_frame_credit() {
    use cc_desk_remote_record::snapshot_pull as pull;
    let (c, h, _gate, _cancel) = support::pull_channels(Arc::new(AtomicU64::new(1000))).await;
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
    let dir = tempfile::tempdir().unwrap();
    let started = owner
        .start_authorized(
            Journal::open(&dir.path().join("pull.sqlite")).unwrap(),
            std::io::sink(),
            SessionStart {
                target,
                device: support::device(),
                writer_epoch: 9,
                key: DigestKey::from_backend_bytes([0x5a; 32]),
                rows: 24,
                columns: 80,
            },
            h.authority(),
            [8; 16],
            4,
        )
        .unwrap();
    let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
    let begin = pull::encode(&pull::Message::SnapshotBegin {
        profile: 1,
        request: "1".into(),
    })
    .unwrap();
    let request = received(&c, &h, 4, &begin).await;
    {
        let pending = conn.dispatch_wire(&request, &h, Duration::from_secs(1));
        tokio::pin!(pending);
        let scope = support::scope();
        let first = c.receive(&scope, 4, Duration::from_secs(1));
        tokio::pin!(first);
        tokio::select! {
            result=&mut pending => panic!("dispatch must wait Next: {:?}", result),
            r=&mut first => { let r=r.unwrap(); assert!(pull::decode_validated_batch(r.payload()).is_ok()); }
        }
        assert_eq!(owner.budget.test_used(), 1);
    }
    assert_eq!(owner.budget.test_used(), 0);
    assert!(h.authority().check().is_err());
    c.close().await;
    h.close().await;
    assert!(c.wait_closed_checked().await.is_ok());
    assert!(h.wait_closed_checked().await.is_ok());
}

#[tokio::test]
async fn snapshot_pull_next_preserves_original_host_deadline() {
    use cc_desk_remote_record::snapshot_pull as pull;
    let (c, h, _gate, _cancel) = support::pull_channels(Arc::new(AtomicU64::new(1000))).await;
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
    let dir = tempfile::tempdir().unwrap();
    let started = owner
        .start_authorized(
            Journal::open(&dir.path().join("pull.sqlite")).unwrap(),
            std::io::sink(),
            SessionStart {
                target,
                device: support::device(),
                writer_epoch: 9,
                key: DigestKey::from_backend_bytes([0x5a; 32]),
                rows: 24,
                columns: 80,
            },
            h.authority(),
            [8; 16],
            4,
        )
        .unwrap();
    let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
    let begin = pull::encode(&pull::Message::SnapshotBegin {
        profile: 1,
        request: "1".into(),
    })
    .unwrap();
    let request = received(&c, &h, 4, &begin).await;
    let started_at = tokio::time::Instant::now();
    let client = async {
        let r = c
            .receive(&support::scope(), 4, Duration::from_secs(1))
            .await
            .unwrap();
        let (batch, _) = pull::decode_validated_batch(r.payload()).unwrap();
        let next = pull::encode(&pull::Message::SnapshotNext {
            profile: 1,
            request: "1".into(),
            transfer: batch.transfer.iter().map(|b| format!("{b:02x}")).collect(),
            batch: "1".into(),
            through: batch.through.to_string(),
        })
        .unwrap();
        drop(r);
        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(60)).await;
        c.send_control(&support::scope(), 4, &next, Duration::from_secs(1))
            .await
            .unwrap();
        let r = c
            .receive(&support::scope(), 4, Duration::from_secs(1))
            .await
            .unwrap();
        let (batch, _) = pull::decode_validated_batch(r.payload()).unwrap();
        assert_eq!(batch.batch, 1);
        drop(r);
    };
    let (result, ()) = tokio::join!(
        conn.dispatch_wire(&request, &h, Duration::from_millis(100)),
        client
    );
    assert!(result.is_err());
    let elapsed = started_at.elapsed();
    assert!(
        elapsed >= Duration::from_millis(100) && elapsed < Duration::from_millis(150),
        "{elapsed:?}"
    );
    assert_eq!(owner.budget.test_used(), 0);
    c.close().await;
    h.close().await;
    assert!(c.wait_closed_checked().await.is_ok());
    assert!(h.wait_closed_checked().await.is_ok());
}

#[tokio::test]
async fn snapshot_pull_production_maximum_grid_budget() {
    let (c, h, _gate, _cancel) =
        support::native_pull_channels(Arc::new(AtomicU64::new(1000))).await;
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
    let dir = tempfile::tempdir().unwrap();
    let started = owner
        .start_authorized(
            Journal::open(&dir.path().join("pull.sqlite")).unwrap(),
            std::io::sink(),
            SessionStart {
                target,
                device: support::device(),
                writer_epoch: 9,
                key: DigestKey::from_backend_bytes([0x5a; 32]),
                rows: 100,
                columns: 240,
            },
            h.authority(),
            [8; 16],
            4,
        )
        .unwrap();
    let mut conn = Connection::new(&started.peer, h.authority(), 4).unwrap();
    let server = async {
        let request = h
            .receive(&support::scope(), 4, Duration::from_secs(1))
            .await
            .unwrap();
        conn.dispatch_wire(&request, &h, Duration::from_secs(1))
            .await
    };
    let (result, sent) = tokio::join!(c.snapshot_pull(1, Duration::from_secs(1)), server);
    sent.unwrap();
    let batch = result.unwrap();
    assert!(batch.parts.len() > 1);
    let raw_bytes = batch.parts.iter().map(Vec::len).sum::<usize>();
    assert!(raw_bytes <= 4 * 1024 * 1024);
    println!(
        "maximum grid raw bytes={raw_bytes}, parts={}",
        batch.parts.len()
    );
    let mut cells = 0;
    for raw in &batch.parts {
        let projection_wire::Content::SnapshotPart(p) =
            projection_wire::decode(raw, 1).unwrap().content
        else {
            panic!()
        };
        assert_eq!(usize::from(p.start), cells);
        cells += p.cells.len();
    }
    assert_eq!(cells, 24000);
    assert_eq!(conn.last, 1);
    assert_eq!(owner.budget.test_used(), 0);
    assert!(c.retire().await.is_ok());
    h.close().await;
    assert!(h.wait_closed_checked().await.is_ok());
}
