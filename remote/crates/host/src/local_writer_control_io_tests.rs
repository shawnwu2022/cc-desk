use super::*;
#[allow(non_snake_case)]
#[test]
fn OwnerControl_RealPipe_001() {
    if !supervision::run("control_tests::OwnerControl_RealPipe_001") {
        return;
    }
    real_pipe(None, 901000);
}
#[allow(non_snake_case)]
#[test]
fn OwnerControl_RecoveredPending_002() {
    if !supervision::run("control_tests::OwnerControl_RecoveredPending_002") {
        return;
    }
    real_pipe(Some(Outcome::HostWritten), 901000);
}
#[allow(non_snake_case)]
#[test]
fn OwnerControl_RecoveredUnknown_003() {
    if !supervision::run("control_tests::OwnerControl_RecoveredUnknown_003") {
        return;
    }
    real_pipe(Some(Outcome::Unknown), 901000);
}
#[allow(non_snake_case)]
#[test]
fn OwnerControl_ShortGrant_004() {
    if !supervision::run("control_tests::OwnerControl_ShortGrant_004") {
        return;
    }
    real_pipe(None, 4000);
}
fn real_pipe(seed: Option<Outcome>, expires_host_ms: u64) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            use crate::local_writer::owner::{SessionStart, WriterHostOwner};
            use crate::local_writer::view_gate::control::{ControlAdmission, GrantContext};
            use cc_desk_remote_record::projection::Ack;
            use cc_desk_remote_transport::records::RecordWriter;
            use std::sync::atomic::AtomicU64;
            let dir = tempfile::tempdir().unwrap();
            let marker = dir.path().join("owned-control-effects");
            let mut run = fixture(&marker);
            let now = Arc::new(AtomicU64::new(1000));
            let clock = now.clone();
            let journal_path = dir.path().join("journal.sqlite");
            let mut journal = Journal::open(&journal_path).unwrap();
            if let Some(outcome) = seed {
                // Public synthetic prior row: not evidence of a historical CLI effect.
                assert_eq!(
                    journal
                        .run_once(&key(), &KeyedDigest([0x71; 32]), || outcome)
                        .unwrap(),
                    outcome
                );
                drop(journal);
                journal = Journal::open(&journal_path).unwrap();
            }
            let mut owner = WriterHostOwner::new([1; 32], 4).unwrap();
            let mut started = owner
                .start_controlled(
                    journal,
                    run.take_stdin().unwrap(),
                    SessionStart {
                        target: key().target(),
                        device: key().device,
                        writer_epoch: 9,
                        key: DigestKey::from_backend_bytes([0x5a; 32]),
                        rows: 3,
                        columns: 10,
                    },
                    ControlAdmission {
                        authority: None,
                        binding: [0x33; 32],
                        minor: 1,
                        clock: Arc::new(move || Ok(clock.load(Ordering::Acquire))),
                        grant: GrantContext {
                            challenge_ms: 1000,
                            expires_host_ms,
                            grant: [7; 32],
                            lease: [8; 16],
                            fence: 4,
                            policy: 5,
                        },
                    },
                )
                .unwrap();
            let peer = started.peer.take().unwrap();
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
            let view = peer
                .acknowledge(ack)
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            let window = peer
                .input_window(&view)
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            if let Some(outcome) = seed {
                assert_eq!(
                    peer.submit(&window, next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                        .unwrap()
                        .wait(Duration::from_secs(2))
                        .unwrap(),
                    WriteResult::Rejected(Reject::Pending)
                );
                assert!(!marker.exists());
                let ReceiptLookup::Known(old) = peer
                    .pending_receipt(key())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap()
                    .unwrap()
                else {
                    panic!("prior receipt");
                };
                assert_eq!(old.outcome, outcome);
                if outcome == Outcome::HostWritten {
                    assert_eq!(
                        peer.settle_receipt(old.into_proof().unwrap())
                            .unwrap()
                            .wait(Duration::from_secs(2))
                            .unwrap(),
                        Ok(AckDecision::Advanced(1))
                    );
                    assert!(matches!(
                        peer.submit(&window, next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                            .unwrap()
                            .wait(Duration::from_secs(2))
                            .unwrap(),
                        WriteResult::Receipt(ReceiptEnvelope {
                            outcome: Outcome::HostWritten,
                            ..
                        })
                    ));
                    effects(&marker, 1);
                } else {
                    assert!(old.into_proof().is_none());
                    assert!(!marker.exists());
                }
                owner.seal(&key().target()).unwrap();
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
                return;
            }
            for _ in 0..2 {
                assert!(matches!(
                    peer.submit(&window, key(), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                        .unwrap()
                        .wait(Duration::from_secs(2))
                        .unwrap(),
                    WriteResult::Receipt(ReceiptEnvelope {
                        outcome: Outcome::HostWritten,
                        ..
                    })
                ));
            }
            effects(&marker, 1);
            assert_eq!(
                peer.submit(&window, next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Rejected(Reject::Pending)
            );
            now.store(
                if expires_host_ms == 4000 { 4000 } else { 6000 },
                Ordering::Release,
            );
            assert_eq!(
                peer.submit(&window, next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Rejected(Reject::Policy)
            );
            if expires_host_ms == 4000 {
                assert_eq!(
                    peer.renew_lease([8; 16], 4, 1)
                        .unwrap()
                        .wait(Duration::from_secs(2))
                        .unwrap(),
                    Err(Reject::Policy)
                );
                assert!(matches!(
                    peer.input_window(&view)
                        .unwrap()
                        .wait(Duration::from_secs(2))
                        .unwrap(),
                    Err(Reject::Policy)
                ));
                assert!(
                    peer.pending_receipt(key())
                        .unwrap()
                        .wait(Duration::from_secs(2))
                        .unwrap()
                        .is_err()
                );
                owner.seal(&key().target()).unwrap();
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
                assert_eq!(std::fs::read(&marker).unwrap().len(), 1);
                let reopened = Journal::open(&journal_path).unwrap();
                assert!(matches!(
                    reopened.query(&key()).unwrap(),
                    ReceiptLookup::Known(ReceiptEnvelope {
                        outcome: Outcome::HostWritten,
                        ..
                    })
                ));
                return;
            }
            assert_eq!(
                peer.renew_lease([8; 16], 4, 1)
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                Ok(21000)
            );
            assert_eq!(
                peer.submit(&window, next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Rejected(Reject::Policy)
            );
            assert_eq!(std::fs::read(&marker).unwrap().len(), 1);
            let ReceiptLookup::Known(receipt) = peer
                .pending_receipt(key())
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap()
            else {
                panic!("durable receipt");
            };
            assert_eq!(receipt.outcome, Outcome::HostWritten);
            let proof = receipt.into_proof().unwrap();
            let mut wrong = proof.clone();
            wrong.key.device = [9; 32];
            assert!(
                peer.settle_receipt(wrong)
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap()
                    .is_err()
            );
            assert_eq!(
                peer.submit(&view, next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Rejected(Reject::Policy)
            );
            // A proof ACK may be durably applied even when its response is lost.
            drop(peer.settle_receipt(proof.clone()).unwrap());
            assert_eq!(
                peer.settle_receipt(proof.clone())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                Ok(AckDecision::AlreadySettled(1))
            );
            let current = peer
                .input_window(&view)
                .unwrap()
                .wait(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            assert_eq!(
                peer.submit(&current, next(3), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Journal(JournalError::Sequence)
            );
            assert!(matches!(
                peer.submit(&current, next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Receipt(ReceiptEnvelope {
                    outcome: Outcome::HostWritten,
                    ..
                })
            ));
            effects(&marker, 2);
            assert!(
                peer.settle_receipt(proof)
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap()
                    .is_err()
            ); // old ACK cannot clear new pending
            assert_eq!(
                peer.submit(&current, next(3), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
                    .unwrap()
                    .wait(Duration::from_secs(2))
                    .unwrap(),
                WriteResult::Rejected(Reject::Pending)
            );
            owner.seal(&key().target()).unwrap();
            let end = Instant::now() + Duration::from_secs(2);
            while !owner.try_reap(&key().target()).unwrap() {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            let exit = run.stop(Instant::now() + Duration::from_secs(2)).unwrap();
            assert_ne!(exit.exit_code, 90);
            assert_eq!(std::fs::read(&marker).unwrap().len(), 2);
        });
}
