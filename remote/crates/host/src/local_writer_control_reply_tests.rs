use super::reply::decode_hex;
use super::*;
use crate::bound_test_support as support;
use cc_desk_remote_record::{control_io as wire, input, projection as projection_wire};
use std::sync::atomic::AtomicU64;

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
