//! Uses existing public TEST_ONLY channel fixture and memory writer, never PTY.
use super::*;
use crate::bound_test_support as support;
use cc_desk_remote_record::{control_io as wire, input, projection as projection_wire};
use std::sync::atomic::{AtomicU64, AtomicUsize};
async fn receive(
    c: &RunningChannel,
    h: &RunningChannel,
    bytes: &[u8],
) -> cc_desk_remote_security::DataRecord {
    c.send_control(&support::scope(), 5, bytes, Duration::from_secs(1))
        .await
        .unwrap();
    h.receive(&support::scope(), 5, Duration::from_secs(1))
        .await
        .unwrap()
}
#[tokio::test]
async fn native_connection_existing_dispatcher_cancellation_retains_original_pending_001() {
    let (c, h, _gate, _cancel, grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
    let (mut c2, mut h2, _cancel2) = support::pair().await;
    c2.install(&grant).unwrap();
    h2.install(&grant).unwrap();
    let c2 = c2.start().unwrap();
    let h2 = Arc::new(h2.start().unwrap());
    struct Block(Arc<AtomicUsize>, mpsc::SyncSender<()>, mpsc::Receiver<()>);
    impl Write for Block {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.fetch_add(1, Ordering::AcqRel);
            self.1.send(()).unwrap();
            self.2
                .recv_timeout(Duration::from_secs(4))
                .map_err(std::io::Error::other)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let writes = Arc::new(AtomicUsize::new(0));
    let (entered_tx, entered) = mpsc::sync_channel(1);
    let (release, release_rx) = mpsc::sync_channel(1);
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
    let _started = owner
        .start_authorized(
            Journal::open(&dir.path().join("TEST_ONLY_owned_connection.sqlite")).unwrap(),
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
    let mut host = NativeHost::from_owner(owner);
    let token = host
        .begin_rebind(&target, support::device(), 4, h2.clone(), [9; 16])
        .unwrap();
    let end = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match host.observe_install(token) {
            InstallObservation::Installed { fence: 5 } => break,
            InstallObservation::Pending => (),
            other => panic!("install: {other:?}"),
        }
        assert!(tokio::time::Instant::now() < end);
        tokio::task::yield_now().await;
    }
    let mut connection = host.connection(token).unwrap();
    assert!(matches!(
        host.connection(token),
        Err(NativeHostError::Rejected)
    ));
    let snapshot = receive(
        &c2,
        &h2,
        br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
    )
    .await;
    connection
        .dispatch_received(&snapshot, Duration::from_secs(1))
        .await
        .unwrap();
    let mut cells = 0;
    let mut stamp = None;
    while cells < 30 {
        let record = c2
            .receive(&support::scope(), 5, Duration::from_secs(1))
            .await
            .unwrap();
        let projection_wire::Content::SnapshotPart(part) =
            projection_wire::decode(record.payload(), 1)
                .unwrap()
                .content
        else {
            panic!("snapshot")
        };
        assert_eq!(usize::from(part.start), cells);
        cells += part.cells.len();
        stamp = Some(part.stamp);
    }
    let stamp = stamp.unwrap();
    let ack = projection_wire::encode_ack(&projection_wire::Ack { profile: 1, stamp }, 1).unwrap();
    let record = receive(&c2, &h2, &ack).await;
    connection
        .dispatch_received(&record, Duration::from_secs(1))
        .await
        .unwrap();
    let _ack = c2
        .receive(&support::scope(), 5, Duration::from_secs(1))
        .await
        .unwrap();
    let record = receive(
        &c2,
        &h2,
        br#"{"message":"Window","profile":1,"request":"2"}"#,
    )
    .await;
    connection
        .dispatch_received(&record, Duration::from_secs(1))
        .await
        .unwrap();
    let record = c2
        .receive(&support::scope(), 5, Duration::from_secs(1))
        .await
        .unwrap();
    let wire::Message::WindowGranted {
        window,
        not_after_host_ms,
        ..
    } = wire::decode(record.payload()).unwrap()
    else {
        panic!("window")
    };
    let mut token_bytes = [0u8; 32];
    assert_eq!(window.len(), 64);
    for (i, part) in window.as_bytes().chunks_exact(2).enumerate() {
        token_bytes[i] = u8::from_str_radix(std::str::from_utf8(part).unwrap(), 16).unwrap();
    }
    let bytes = input::encode(&input::Input {
        operation: [2; 16],
        sequence: 1,
        window: token_bytes,
        not_after_host_ms: not_after_host_ms.parse().unwrap(),
        bytes: b"TEST_ONLY".to_vec(),
    })
    .unwrap();
    let record = receive(&c2, &h2, &bytes).await;
    let mut future = Box::pin(connection.dispatch_received(&record, Duration::from_secs(1)));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(
            std::future::Future::poll(future.as_mut(), cx).is_pending()
        ))
        .await
    );
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(future);
    assert!(h2.authority().check().is_err());
    assert_eq!(host.pending.observe(), custody::Observation::Pending);
    host.begin_shutdown();
    assert_eq!(host.observe_shutdown(), WriterRetirement::Pending);
    // Native actor remains blocked; dropping API waiter cannot consume custody.
    release.send(()).unwrap();
    let end = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match host.observe_shutdown() {
            WriterRetirement::Joined => break,
            WriterRetirement::Pending => (),
            other => panic!("retirement: {other:?}"),
        }
        assert!(tokio::time::Instant::now() < end);
        tokio::task::yield_now().await;
    }
    assert_eq!(writes.load(Ordering::Acquire), 1);
    assert!(
        connection
            .dispatch_received(&record, Duration::from_secs(1))
            .await
            .is_err()
    );
    assert_eq!(writes.load(Ordering::Acquire), 1);
    drop(connection);
    host.network_retirement().unwrap().wait().await;
    h.close().await;
    c.close().await;
    c2.close().await;
}

// Adapt the existing wire contract test to the actual owned wait mode.
mod owned_wire {
    use super::*;
    use cc_desk_remote_security::DataRecord;
    use std::time::Instant;
    fn decode_hex<const N: usize>(text: &str) -> Result<[u8; N], Reject> {
        if text.len() != N * 2 {
            return Err(Reject::Bounds);
        }
        let mut out = [0u8; N];
        for (i, pair) in text.as_bytes().chunks_exact(2).enumerate() {
            out[i] = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| Reject::Bounds)?, 16)
                .map_err(|_| Reject::Bounds)?;
        }
        Ok(out)
    }
    async fn received(
        c: &RunningChannel,
        h: &RunningChannel,
        fence: u64,
        bytes: &[u8],
    ) -> DataRecord {
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

    #[tokio::test]
    async fn native_connection_owned_receipts_roundtrip_and_wrong_fence_rejection_002() {
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
        let mut conn = Connection::new_owned(
            Arc::new(started.peer),
            h.authority(),
            4,
            Arc::new(custody::Owner::new()),
        )
        .unwrap();
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
}
