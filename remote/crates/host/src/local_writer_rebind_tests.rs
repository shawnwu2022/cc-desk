use super::*;
use crate::local_writer::owner::{SessionStart, WriterHostOwner};
use crate::local_writer::view_gate::control::{ControlAdmission, GrantContext, HostClock};
use cc_desk_remote_record::projection::Ack;
use cc_desk_remote_transport::records::RecordWriter;

fn admission(clock: Arc<dyn HostClock>, fence: u64, binding: u8, grant: u8) -> ControlAdmission {
    ControlAdmission {
        binding: [binding; 32],
        minor: 1,
        clock,
        grant: GrantContext {
            challenge_ms: 1000,
            grant: [grant; 32],
            lease: [binding; 16],
            fence,
            policy: 5,
        },
    }
}
async fn synced(peer: &view_gate::PeerViewHandle) -> view_gate::SyncedView {
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
    let (host, _client) = tokio::io::duplex(32768);
    let _stream = frame
        .deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(1))
        .await
        .unwrap();
    peer.acknowledge(ack)
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap()
}
#[allow(non_snake_case)]
#[test]
fn OwnerRebind_RealPipe_001() {
    if !supervision::run("rebind_tests::OwnerRebind_RealPipe_001") {
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let marker = dir.path().join("rebind-effects");
            let mut run = fixture(&marker);
            let clock: Arc<dyn HostClock> = Arc::new(|| Ok(1000));
            let mut owner = WriterHostOwner::new([1; 32], 4).unwrap();
            let mut started = owner
                .start_controlled(
                    Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
                    run.take_stdin().unwrap(),
                    SessionStart {
                        target: key().target(),
                        device: key().device,
                        writer_epoch: 9,
                        key: DigestKey::from_backend_bytes([0x5a; 32]),
                        rows: 3,
                        columns: 10,
                    },
                    admission(clock.clone(), 4, 0x33, 7),
                )
                .unwrap();
            let old = started.peer.take().unwrap();
            let view = synced(&old).await;
            let window = old
                .input_window(&view)
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            let WriteResult::Receipt(receipt) = old
                .submit(&window, key(), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
            else {
                panic!("receipt");
            };
            effects(&marker, 1);
            assert!(matches!(
                owner
                    .rebind(
                        &key().target(),
                        [6; 32],
                        4,
                        admission(clock.clone(), 5, 0x44, 10)
                    )
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                Err(Reject::Pending)
            ));
            assert_eq!(
                old.settle_receipt(receipt.into_proof().unwrap())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                Ok(AckDecision::Advanced(1))
            );
            let peer = owner
                .rebind(
                    &key().target(),
                    [6; 32],
                    4,
                    admission(clock.clone(), 5, 0x44, 10),
                )
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            assert!(matches!(
                old.submit(&window, next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec()),
                Err(Reject::Sync)
            ));
            let mut second = key();
            second.device = [6; 32];
            assert_eq!(
                peer.submit(&window, second.clone(), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Rejected(Reject::Sync)
            );
            let fresh = synced(&peer).await;
            let fresh_window = peer
                .input_window(&fresh)
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            assert!(matches!(
                peer.submit(
                    &fresh_window,
                    second.clone(),
                    9,
                    b"PUBLIC_TEST_ONLY\n".to_vec()
                )
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap(),
                WriteResult::Receipt(ReceiptEnvelope {
                    outcome: Outcome::HostWritten,
                    ..
                })
            ));
            effects(&marker, 2);
            let recovered = owner
                .rebind(
                    &key().target(),
                    [6; 32],
                    5,
                    admission(clock.clone(), 6, 0x55, 10),
                )
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            let ReceiptLookup::Known(receipt) = recovered
                .pending_receipt(second.clone())
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap()
            else {
                panic!("recover pending");
            };
            assert_eq!(
                recovered
                    .settle_receipt(receipt.into_proof().unwrap())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                Ok(AckDecision::Advanced(1))
            );
            let fresh = synced(&recovered).await;
            let current = recovered
                .input_window(&fresh)
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            second.sequence = 2;
            second.operation = [9; 16];
            assert!(matches!(
                recovered
                    .submit(&current, second, 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Receipt(ReceiptEnvelope {
                    outcome: Outcome::HostWritten,
                    ..
                })
            ));
            effects(&marker, 3);
            let frame = recovered
                .capture()
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            let (host, _client) = tokio::io::duplex(1);
            let blocked = tokio::spawn(
                frame.deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(10)),
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
            owner.seal(&key().target()).unwrap();
            assert!(matches!(
                tokio::time::timeout(Duration::from_millis(200), blocked)
                    .await
                    .unwrap()
                    .unwrap(),
                Err(Reject::Sync)
            ));
            let end = Instant::now() + Duration::from_secs(2);
            while !owner.try_reap(&key().target()).unwrap() {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            assert_ne!(
                run.stop(Instant::now() + Duration::from_secs(2))
                    .unwrap()
                    .exit_code,
                90
            );
            assert_eq!(std::fs::read(&marker).unwrap().len(), 3);
        });
}

#[allow(non_snake_case)]
#[test]
fn OwnerRebind_BlockedAndUncertain_002() {
    if !supervision::run("rebind_tests::OwnerRebind_BlockedAndUncertain_002") {
        return;
    }
    struct BlockingSink {
        first: bool,
        entered: mpsc::SyncSender<()>,
        go: mpsc::Receiver<()>,
        effects: Arc<AtomicUsize>,
    }
    impl std::io::Write for BlockingSink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.first {
                self.first = false;
                self.entered
                    .try_send(())
                    .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
                self.go
                    .recv_timeout(Duration::from_secs(3))
                    .map_err(|_| std::io::ErrorKind::TimedOut)?;
            }
            self.effects.fetch_add(1, Ordering::AcqRel);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let clock: Arc<dyn HostClock> = Arc::new(|| Ok(1000));
            let count = Arc::new(AtomicUsize::new(0));
            let (entered_tx, entered_rx) = mpsc::sync_channel(1);
            let (go_tx, go_rx) = mpsc::sync_channel(1);
            let mut owner = WriterHostOwner::new([1; 32], 4).unwrap();
            let mut started = owner
                .start_controlled(
                    Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
                    BlockingSink {
                        first: true,
                        entered: entered_tx,
                        go: go_rx,
                        effects: count.clone(),
                    },
                    SessionStart {
                        target: key().target(),
                        device: key().device,
                        writer_epoch: 9,
                        key: DigestKey::from_backend_bytes([0x5a; 32]),
                        rows: 3,
                        columns: 10,
                    },
                    admission(clock.clone(), 4, 0x33, 7),
                )
                .unwrap();
            let old = started.peer.take().unwrap();
            let view = synced(&old).await;
            let window = old
                .input_window(&view)
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            let pending = old
                .submit(&window, key(), 9, b"TEST_ONLY".to_vec())
                .unwrap();
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(matches!(
                pending.wait(Duration::from_millis(20)),
                Err(WaitError::Uncertain)
            ));
            drop(pending); // loss is not cancellation
            let cross = owner
                .rebind(
                    &key().target(),
                    [6; 32],
                    4,
                    admission(clock.clone(), 5, 0x44, 10),
                )
                .unwrap();
            assert!(matches!(
                cross.wait(Duration::from_millis(20)),
                Err(WaitError::Uncertain)
            ));
            let same = owner
                .rebind(
                    &key().target(),
                    [5; 32],
                    4,
                    admission(clock.clone(), 5, 0x44, 7),
                )
                .unwrap();
            assert!(matches!(
                same.wait(Duration::from_millis(20)),
                Err(WaitError::Uncertain)
            ));
            drop(same); // don't resend an uncertain transition
            let status = owner.control_status(&key().target()).unwrap();
            assert!(matches!(
                status.wait(Duration::from_millis(20)),
                Err(WaitError::Uncertain)
            ));
            assert_eq!(count.load(Ordering::Acquire), 0);
            // A blocked actor must not hold a Host-wide database/IO lock.
            let other_dir = tempfile::tempdir().unwrap();
            let mut other_target = key().target();
            other_target.session = [3; 16];
            let mut other = owner
                .start_controlled(
                    Journal::open(&other_dir.path().join("journal.sqlite")).unwrap(),
                    std::io::sink(),
                    SessionStart {
                        target: other_target.clone(),
                        device: [9; 32],
                        writer_epoch: 9,
                        key: DigestKey::from_backend_bytes([0x5a; 32]),
                        rows: 3,
                        columns: 10,
                    },
                    admission(clock.clone(), 1, 0x66, 11),
                )
                .unwrap();
            let other_peer = other.peer.take().unwrap();
            let other_view = synced(&other_peer).await;
            let other_window = other_peer
                .input_window(&other_view)
                .unwrap()
                .wait(Duration::from_secs(1))
                .unwrap()
                .unwrap();
            let other_key = OperationKey {
                host: other_target.host,
                session: other_target.session,
                run: other_target.run.clone(),
                generation: other_target.generation,
                incarnation: other_target.incarnation,
                device: [9; 32],
                operation: [22; 16],
                sequence: 1,
            };
            assert!(matches!(
                other_peer
                    .submit(&other_window, other_key, 9, b"TEST_ONLY".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(1))
                    .unwrap(),
                WriteResult::Receipt(ReceiptEnvelope {
                    outcome: Outcome::HostWritten,
                    ..
                })
            ));
            assert_eq!(count.load(Ordering::Acquire), 0);
            owner.seal(&other_target).unwrap();
            let other_end = Instant::now() + Duration::from_secs(2);
            while !owner.try_reap(&other_target).unwrap() {
                assert!(Instant::now() < other_end);
                std::thread::yield_now();
            }
            go_tx.try_send(()).unwrap();
            assert!(matches!(
                cross.wait(Duration::from_secs(2)).unwrap(),
                Err(Reject::Pending)
            ));
            let status = status.wait(Duration::from_secs(2)).unwrap().unwrap();
            assert_eq!(status.fence, 5);
            assert_eq!(status.device, [5; 32]);
            assert!(!status.active);
            assert_eq!(status.pending.as_ref(), Some(&key()));
            assert_eq!(count.load(Ordering::Acquire), 1);
            let peer = owner
                .rebind(
                    &key().target(),
                    [5; 32],
                    5,
                    admission(clock.clone(), 6, 0x55, 7),
                )
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            let ReceiptLookup::Known(receipt) = peer
                .pending_receipt(key())
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap()
            else {
                panic!("receipt");
            };
            assert_eq!(
                peer.settle_receipt(receipt.into_proof().unwrap())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                Ok(AckDecision::Advanced(1))
            );
            let view = synced(&peer).await;
            let window = peer
                .input_window(&view)
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            assert!(matches!(
                peer.submit(&window, next(2), 9, b"TEST_ONLY".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Receipt(ReceiptEnvelope {
                    outcome: Outcome::HostWritten,
                    ..
                })
            ));
            assert_eq!(count.load(Ordering::Acquire), 2);
            owner.seal(&key().target()).unwrap();
            let end = Instant::now() + Duration::from_secs(2);
            while !owner.try_reap(&key().target()).unwrap() {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
        });
}
