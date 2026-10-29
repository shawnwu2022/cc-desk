use super::*;
use cc_desk_remote_record::projection::{Ack, Content, decode};
use cc_desk_remote_transport::records::{RecordReader, RecordWriter};
use std::io;
pub(in crate::local_writer) fn submit_observed(
    peer: &PeerViewHandle,
    view: &SyncedView,
    key: OperationKey,
    epoch: u64,
    bytes: Vec<u8>,
    after_copy: impl FnOnce(),
) -> Result<PendingInput, Reject> {
    peer.input
        .submit_inner(key, epoch, bytes, Some(view.clone()), after_copy)
}
fn captured(dir: &tempfile::TempDir) -> (LocalWriter, projection::CapturedProjection) {
    captured_size(dir, 3, 10)
}
fn captured_size(
    dir: &tempfile::TempDir,
    rows: usize,
    columns: usize,
) -> (LocalWriter, projection::CapturedProjection) {
    let (writer, _) = LocalWriter::start_canonical_terminal(
        Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
        io::sink(),
        target(),
        [5; 32],
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        CanonicalStart::new(rows, columns, &HostViewBudget::new()).unwrap(),
    )
    .unwrap();
    let frame = writer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    (writer, frame)
}
fn finish(native: &mut LocalWriter) {
    native.seal();
    let end = std::time::Instant::now() + Duration::from_secs(2);
    while !native.try_join().unwrap() {
        assert!(std::time::Instant::now() < end);
        std::thread::yield_now();
    }
}
#[tokio::test]
async fn exact_ack_matrix_cannot_confirm_another_peer_target_or_future_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let (mut native, frame) = captured(&dir);
    let peer = PeerControl::new(target(), [5; 32], [0x33; 32], 1).unwrap();
    let mut gate = ViewGate::new(peer.scope.clone());
    let frame = gate.issue(frame, &peer.scope).unwrap();
    let ack = Ack {
        profile: 1,
        stamp: frame.issued_stamp().unwrap().clone(),
    };
    let epochs = frame.view().epochs();
    let (host, client) = tokio::io::duplex(32768);
    let _stream = frame
        .deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(1))
        .await
        .unwrap();
    drop(client);
    for field in [
        "target",
        "binding",
        "barrier",
        "revision",
        "dimension",
        "mode",
        "structure",
        "sync",
    ] {
        let mut bad = ack.clone();
        match field {
            "target" => bad.stamp.target.generation = "99".into(),
            "binding" => bad.stamp.binding = "aa".repeat(32),
            "barrier" => bad.stamp.barrier = "aa".repeat(16),
            "revision" => bad.stamp.revision = "99".into(),
            "dimension" => bad.stamp.dimension_epoch = "99".into(),
            "mode" => bad.stamp.mode_epoch = "99".into(),
            "structure" => bad.stamp.structure_epoch = "99".into(),
            _ => bad.stamp.sync_epoch = "99".into(),
        }
        assert!(
            matches!(
                gate.acknowledge(&peer.scope, &bad, epochs),
                Err(Reject::Sync)
            ),
            "{field}"
        );
    }
    let other = PeerControl::new(target(), [5; 32], [0x33; 32], 1).unwrap(); // same wire IDs, different native channel activation
    assert!(matches!(
        gate.acknowledge(&other.scope, &ack, epochs),
        Err(Reject::Sync)
    ));
    let proof = gate.acknowledge(&peer.scope, &ack, epochs).unwrap();
    assert_eq!(gate.check(&proof, &[5; 32], epochs), Ok(()));
    assert_eq!(gate.check(&proof, &[6; 32], epochs), Err(Reject::Sync));
    let changed = cc_desk_remote_terminal::TerminalEpochs {
        mode_epoch: epochs.mode_epoch + 2,
        ..epochs
    };
    assert!(matches!(
        gate.acknowledge(&peer.scope, &ack, changed),
        Err(Reject::Sync)
    ));
    assert_eq!(gate.check(&proof, &[5; 32], changed), Err(Reject::Sync));
    finish(&mut native);
}
#[tokio::test(start_paused = true)]
async fn one_absolute_deadline_covers_all_snapshot_parts_not_each_individual_write() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::AsyncWrite;
    struct DelayedFlush {
        io: tokio::io::DuplexStream,
        wait: Option<Pin<Box<tokio::time::Sleep>>>,
    }
    impl AsyncWrite for DelayedFlush {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            b: &[u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.io).poll_write(cx, b)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            let wait = self
                .wait
                .get_or_insert_with(|| Box::pin(tokio::time::sleep(Duration::from_millis(20))));
            if wait.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.wait = None;
            Pin::new(&mut self.io).poll_flush(cx)
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.io).poll_shutdown(cx)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut native, frame) = captured_size(&dir, 3, 80);
    let peer = PeerControl::new(target(), [5; 32], [0x33; 32], 1).unwrap();
    let mut gate = ViewGate::new(peer.scope.clone());
    let frame = gate.issue(frame, &peer.scope).unwrap();
    let ack = Ack {
        profile: 1,
        stamp: frame.issued_stamp().unwrap().clone(),
    };
    let epochs = frame.view().epochs();
    let (host, client) = tokio::io::duplex(65536);
    let start = tokio::time::Instant::now();
    let result = frame
        .deliver(
            RecordWriter::new(
                DelayedFlush {
                    io: host,
                    wait: None,
                },
                1,
            )
            .unwrap(),
            Duration::from_millis(30),
        )
        .await;
    assert!(matches!(result, Err(Reject::Continuity)));
    assert_eq!(start.elapsed(), Duration::from_millis(30));
    assert!(!peer.scope.is_active());
    assert!(matches!(
        gate.acknowledge(&peer.scope, &ack, epochs),
        Err(Reject::Sync)
    ));
    let mut reader = RecordReader::new(client, 1).unwrap();
    assert!(reader.read(Duration::from_secs(1)).await.unwrap().is_some());
    finish(&mut native); // virtual deadline fixture, not an OS latency measurement
}
#[tokio::test]
async fn cancelling_a_partially_written_delivery_retires_scope_and_cannot_be_acknowledged() {
    use std::future::Future;
    use tokio::io::AsyncReadExt;
    let dir = tempfile::tempdir().unwrap();
    let (mut native, frame) = captured(&dir);
    let peer = PeerControl::new(target(), [5; 32], [0x33; 32], 1).unwrap();
    let mut gate = ViewGate::new(peer.scope.clone());
    let frame = gate.issue(frame, &peer.scope).unwrap();
    let ack = Ack {
        profile: 1,
        stamp: frame.issued_stamp().unwrap().clone(),
    };
    let epochs = frame.view().epochs();
    let (host, mut client) = tokio::io::duplex(1);
    let mut sending =
        Box::pin(frame.deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(1)));
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(sending);
    assert!(!peer.scope.is_active());
    assert!(matches!(
        gate.acknowledge(&peer.scope, &ack, epochs),
        Err(Reject::Sync)
    ));
    let mut bytes = Vec::new();
    client.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes.len(), 1);
    finish(&mut native);
}
#[tokio::test]
async fn closing_native_peer_wakes_a_blocked_sender_without_waiting_for_its_deadline() {
    use std::future::Future;
    let dir = tempfile::tempdir().unwrap();
    let (mut native, frame) = captured(&dir);
    let peer = PeerControl::new(target(), [5; 32], [0x33; 32], 1).unwrap();
    let mut gate = ViewGate::new(peer.scope.clone());
    let frame = gate.issue(frame, &peer.scope).unwrap();
    let (host, _client) = tokio::io::duplex(1);
    let mut sending =
        Box::pin(frame.deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(30)));
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    peer.close();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), sending)
            .await
            .unwrap(),
        Err(Reject::Sync)
    ));
    finish(&mut native);
}
#[tokio::test]
async fn revocation_after_real_durable_intent_commits_a_no_effect_rejection_proof() {
    let source = tempfile::tempdir().unwrap();
    let (mut native, frame) = captured(&source);
    let peer = PeerControl::new(target(), [5; 32], [0x33; 32], 1).unwrap();
    let mut gate = ViewGate::new(peer.scope.clone());
    let frame = gate.issue(frame, &peer.scope).unwrap();
    let ack = Ack {
        profile: 1,
        stamp: frame.issued_stamp().unwrap().clone(),
    };
    let epochs = frame.view().epochs();
    let (host, _client) = tokio::io::duplex(32768);
    let _stream = frame
        .deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(1))
        .await
        .unwrap();
    let proof = gate.acknowledge(&peer.scope, &ack, epochs).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.sqlite");
    let marker = dir.path().join("must-not-write");
    let mut journal = Journal::open(&path).unwrap();
    let operation = OperationKey {
        host: target().host,
        session: target().session,
        run: target().run,
        generation: 3,
        incarnation: 4,
        device: [5; 32],
        operation: [6; 16],
        sequence: 1,
    };
    let digest = proof
        .digest(
            &DigestKey::from_backend_bytes([0x5a; 32]),
            &operation,
            9,
            b"PUBLIC_TEST_ONLY",
        )
        .unwrap();
    let PrepareDecision::Prepared(prepared) = journal.prepare(&operation, &digest).unwrap() else {
        panic!("intent")
    };
    let credits = Arc::new(Credits::default());
    let (feedback, _) = mpsc::sync_channel(1);
    let request = Request {
        key: operation.clone(),
        epoch: 9,
        bytes: b"PUBLIC_TEST_ONLY".as_slice().into(),
        view: Some(proof),
        source_scope: Some(peer.scope.clone()),
        reply: feedback,
        _credit: credits.reserve().unwrap(),
    };
    let state = ReplyState::canonical(
        Arc::new(()),
        CanonicalStart::new(3, 10, &HostViewBudget::new()).unwrap(),
    );
    peer.close();
    let outcome = gate.effect_if_current(&request, Some(&state), || {
        std::fs::write(&marker, b"PUBLIC_TEST_ONLY").unwrap();
        Outcome::HostWritten
    });
    assert_eq!(
        journal.finalize(prepared, outcome).unwrap(),
        Outcome::Rejected
    );
    assert!(!marker.exists());
    assert!(matches!(
        journal.receipt(&operation).unwrap(),
        ReceiptLookup::Known(ReceiptEnvelope {
            outcome: Outcome::Rejected,
            receipt_proof: Some(_),
            ..
        })
    ));
    finish(&mut native);
}
fn target() -> Target {
    Target {
        host: [1; 32],
        session: [2; 16],
        run: "PUBLIC_TEST_ONLY_VIEW_GATE".into(),
        generation: 3,
        incarnation: 4,
    }
}
#[tokio::test]
async fn only_real_complete_native_delivery_and_exact_ack_create_install_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let (mut native, frame) = captured(&dir);
    let peer = PeerControl::new(target(), [5; 32], [0x33; 32], 1).unwrap();
    let mut gate = ViewGate::new(peer.scope.clone());
    let frame = gate.issue(frame, &peer.scope).unwrap();
    let ack = Ack {
        profile: 1,
        stamp: frame.issued_stamp().unwrap().clone(),
    };
    let epochs = frame.view().epochs();
    assert!(matches!(
        gate.acknowledge(&peer.scope, &ack, epochs),
        Err(Reject::Sync)
    ));
    let (host, client) = tokio::io::duplex(32768);
    let mut reader = RecordReader::new(client, 1).unwrap();
    let receiving = tokio::spawn(async move {
        let raw = reader.read(Duration::from_secs(1)).await.unwrap().unwrap();
        let Content::SnapshotPart(p) = decode(&raw.payload, 1).unwrap().content else {
            panic!("snapshot")
        };
        assert_eq!(p.cells.len(), 30);
        p.stamp
    });
    let writer = frame
        .deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(1))
        .await
        .unwrap();
    assert!(receiving.await.unwrap() == ack.stamp);
    assert!(gate.acknowledge(&peer.scope, &ack, epochs).is_ok());
    drop(writer);
    let mut bad = ack.clone();
    bad.stamp.barrier = "44".repeat(16);
    assert!(matches!(
        gate.acknowledge(&peer.scope, &bad, epochs),
        Err(Reject::Sync)
    ));
    peer.close();
    assert!(matches!(
        gate.acknowledge(&peer.scope, &ack, epochs),
        Err(Reject::Sync)
    ));
    native.seal();
    let end = std::time::Instant::now() + Duration::from_secs(2);
    while !native.try_join().unwrap() {
        assert!(std::time::Instant::now() < end);
        std::thread::yield_now();
    }
}
#[tokio::test]
async fn actual_writer_queue_requires_native_delivered_ack_before_committing_input() {
    #[derive(Clone, Default)]
    struct Sink(Arc<std::sync::Mutex<Vec<u8>>>);
    impl io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let sink = Sink::default();
    let observed = sink.clone();
    let (mut native, _output, peer) = LocalWriter::start_view_guarded(
        Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
        sink,
        target(),
        [5; 32],
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        ViewStart {
            terminal: CanonicalStart::new(3, 10, &HostViewBudget::new()).unwrap(),
            binding: [0x33; 32],
            minor: 1,
        },
    )
    .unwrap();
    let operation = OperationKey {
        host: target().host,
        session: target().session,
        run: target().run,
        generation: 3,
        incarnation: 4,
        device: [5; 32],
        operation: [6; 16],
        sequence: 1,
    };
    assert!(matches!(
        native.handle().submit(operation.clone(), 9, b"x".to_vec()),
        Err(Reject::Sync)
    ));
    let frame = peer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let ack = Ack {
        profile: 1,
        stamp: frame.issued_stamp().unwrap().clone(),
    };
    assert!(matches!(
        peer.acknowledge(ack.clone())
            .unwrap()
            .wait(Duration::from_secs(2))
            .unwrap(),
        Err(Reject::Sync)
    ));
    let (host, client) = tokio::io::duplex(32768);
    let mut reader = RecordReader::new(client, 1).unwrap();
    let receiving =
        tokio::spawn(async move { reader.read(Duration::from_secs(1)).await.unwrap().unwrap() });
    let stream = frame
        .deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(1))
        .await
        .unwrap();
    receiving.await.unwrap();
    let view = peer
        .acknowledge(ack)
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let mut unbound = native.handle();
    unbound.view_scope = None;
    assert_eq!(
        unbound
            .submit_inner(
                operation.clone(),
                9,
                b"x".to_vec(),
                Some(view.clone()),
                || {}
            )
            .unwrap()
            .wait(Duration::from_secs(2))
            .unwrap(),
        WriteResult::Rejected(Reject::Sync)
    );
    for _ in 0..2 {
        assert!(matches!(
            peer.submit(&view, operation.clone(), 9, b"x".to_vec())
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap(),
            WriteResult::Receipt(ReceiptEnvelope {
                outcome: Outcome::HostWritten,
                ..
            })
        ));
    }
    assert_eq!(&*observed.0.lock().unwrap(), b"x");
    let replacement = peer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let replacement_ack = Ack {
        profile: 1,
        stamp: replacement.issued_stamp().unwrap().clone(),
    };
    let (replacement_host, replacement_client) = tokio::io::duplex(32768);
    let reading = tokio::spawn(async move {
        RecordReader::new(replacement_client, 1)
            .unwrap()
            .read(Duration::from_secs(1))
            .await
            .unwrap()
            .unwrap()
    });
    let replacement_stream = replacement
        .deliver(
            RecordWriter::new(replacement_host, 1).unwrap(),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    reading.await.unwrap();
    let replacement_view = peer
        .acknowledge(replacement_ack)
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(
        peer.submit(&replacement_view, operation.clone(), 9, b"x".to_vec())
            .unwrap()
            .wait(Duration::from_secs(2))
            .unwrap(),
        WriteResult::Journal(JournalError::Conflict)
    );
    drop(replacement_stream);
    peer.close();
    let mut late = operation;
    late.operation = [7; 16];
    late.sequence = 2;
    let refusal = peer.submit(&view, late, 9, b"late".to_vec());
    match refusal {
        Err(Reject::Sync) => {}
        Ok(p) => assert_eq!(
            p.wait(Duration::from_secs(2)).unwrap(),
            WriteResult::Rejected(Reject::Sync)
        ),
        _ => panic!("closed view"),
    }
    assert_eq!(&*observed.0.lock().unwrap(), b"x");
    drop(stream);
    native.seal();
    let end = std::time::Instant::now() + Duration::from_secs(2);
    while !native.try_join().unwrap() {
        assert!(std::time::Instant::now() < end);
        std::thread::yield_now();
    }
}
