use super::*;
use std::{io, sync::Mutex, time::Instant};

#[test]
fn output_chunk_submission_requires_an_exclusive_producer_borrow() {
    let _single_producer: fn(
        &mut HostOutputHandle,
        Vec<u8>,
    ) -> Result<replies::PendingOutput, Reject> = HostOutputHandle::feed;
}

#[cfg(feature = "experimental-canonical-replies")]
#[test]
fn canonical_prefix_replies_use_the_same_writer_and_no_user_journal_operation() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Sink::default();
    let observed = sink.clone();
    let (mut writer, mut cap) = LocalWriter::start_canonical_terminal(
        Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
        sink,
        operation().target(),
        operation().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        CanonicalStart::new(3, 10).unwrap(),
    )
    .unwrap();
    assert_eq!(
        feed(&mut cap, b"abc\x1b[6n\x1b[2;5HZ\x1b[6n"),
        OutputResult::Processed { replies: 2 }
    );
    finish(&mut writer);
    assert_eq!(&*observed.0.lock().unwrap(), b"\x1b[1;4R\x1b[2;6R");
    assert_eq!(
        Journal::open(&dir.path().join("journal.sqlite"))
            .unwrap()
            .query(&operation())
            .unwrap(),
        ReceiptLookup::Missing
    );
    assert!(matches!(CanonicalStart::new(0, 10), Err(Reject::Bounds)));
}

#[cfg(feature = "experimental-canonical-replies")]
#[test]
fn canonical_bad_tail_and_rate_overflow_do_not_emit_unvalidated_replies() {
    for flood in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let sink = Sink::default();
        let observed = sink.clone();
        let (mut writer, mut cap) = LocalWriter::start_canonical_terminal(
            Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
            sink,
            operation().target(),
            operation().device,
            9,
            DigestKey::from_backend_bytes([0x5a; 32]),
            CanonicalStart::new(3, 10).unwrap(),
        )
        .unwrap();
        if flood {
            assert_eq!(
                feed(&mut cap, &b"\x1b[6n".repeat(32)),
                OutputResult::Processed { replies: 32 }
            );
            assert_eq!(
                feed(&mut cap, b"\x1b[6n"),
                OutputResult::Rejected(Reject::Rate)
            );
            assert_eq!(observed.0.lock().unwrap().len(), 32 * 6);
        } else {
            assert_eq!(
                feed(&mut cap, b"abc\x1b[6n\x1b]52;c;x\x07"),
                OutputResult::Rejected(Reject::Unsupported)
            );
            assert!(observed.0.lock().unwrap().is_empty());
        }
        assert!(matches!(cap.feed(b"\x1b[6n".to_vec()), Err(Reject::Sealed)));
        finish(&mut writer);
    }
}

#[cfg(feature = "experimental-canonical-replies")]
#[test]
fn canonical_stale_activation_and_stop_between_queries_never_reopen_the_writer() {
    struct Stop {
        sealed: Arc<Mutex<Option<Arc<AtomicBool>>>>,
        sink: Sink,
    }
    impl io::Write for Stop {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let n = self.sink.write(bytes)?;
            self.sealed
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .store(true, Ordering::Release);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let sink = Sink::default();
    let observed = sink.clone();
    let sealed = Arc::new(Mutex::new(None));
    let (mut writer, mut cap) = LocalWriter::start_canonical_terminal(
        Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
        Stop {
            sealed: sealed.clone(),
            sink,
        },
        operation().target(),
        operation().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        CanonicalStart::new(3, 10).unwrap(),
    )
    .unwrap();
    *sealed.lock().unwrap() = Some(writer.handle.sealed.clone());
    let actual = cap.context.clone();
    cap.context = Arc::new(());
    assert_eq!(
        feed(&mut cap, b"abc\x1b[6n"),
        OutputResult::Rejected(Reject::Epoch)
    );
    assert!(observed.0.lock().unwrap().is_empty());
    cap.context = actual;
    assert_eq!(feed(&mut cap, b"ab\x1b[6nZ\x1b[6n"), OutputResult::Partial);
    assert_eq!(&*observed.0.lock().unwrap(), b"\x1b[1;3R");
    assert!(matches!(cap.feed(b"\x1b[6n".to_vec()), Err(Reject::Sealed)));
    finish(&mut writer);
}

#[cfg(feature = "experimental-canonical-replies")]
#[test]
fn canonical_partial_or_unknown_io_remains_sealed_without_user_receipts() {
    struct Fail {
        progress: bool,
        bytes: Vec<u8>,
    }
    impl io::Write for Fail {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.progress {
                self.progress = false;
                self.bytes.push(bytes[0]);
                Ok(1)
            } else {
                Err(io::Error::other("PUBLIC_TEST_ONLY_CANONICAL_FAILURE"))
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    for progress in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut writer, mut cap) = LocalWriter::start_canonical_terminal(
            Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
            Fail {
                progress,
                bytes: Vec::new(),
            },
            operation().target(),
            operation().device,
            9,
            DigestKey::from_backend_bytes([0x5a; 32]),
            CanonicalStart::new(3, 10).unwrap(),
        )
        .unwrap();
        assert_eq!(
            feed(&mut cap, b"abc\x1b[6n"),
            if progress {
                OutputResult::Partial
            } else {
                OutputResult::Unknown
            }
        );
        assert!(matches!(cap.feed(b"\x1b[6n".to_vec()), Err(Reject::Sealed)));
        finish(&mut writer);
        assert_eq!(
            Journal::open(&dir.path().join("journal.sqlite"))
                .unwrap()
                .query(&operation())
                .unwrap(),
            ReceiptLookup::Missing
        );
    }
}

#[test]
fn interrupted_io_has_a_finite_retry_budget_and_never_replays_partial_bytes() {
    struct Interrupt {
        calls: usize,
        progress: bool,
    }
    impl io::Write for Interrupt {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.progress && self.calls == 1 {
                return Ok(1);
            }
            if self.calls <= 33 {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            } else {
                Err(io::Error::other("PUBLIC_TEST_ONLY_FINITE_RED_GUARD"))
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("no flush after exhausted retry budget")
        }
    }
    for progress in [false, true] {
        let mut sink = Interrupt { calls: 0, progress };
        assert_eq!(
            write_once(&mut sink, b"PUBLIC_TEST_ONLY"),
            if progress {
                Outcome::Partial
            } else {
                Outcome::Unknown
            }
        );
        assert!(
            sink.calls <= if progress { 18 } else { 17 },
            "unbounded Interrupted retries: {}",
            sink.calls
        );
    }
}
fn operation() -> OperationKey {
    OperationKey {
        host: [1; 32],
        session: [2; 16],
        run: "PUBLIC_TEST_ONLY_REPLIES".into(),
        generation: 3,
        incarnation: 4,
        device: [5; 32],
        operation: [6; 16],
        sequence: 1,
    }
}
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);
impl io::Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn finish(writer: &mut LocalWriter) {
    writer.seal();
    let end = Instant::now() + Duration::from_secs(2);
    while !writer.try_join().unwrap() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn start(
    sink: impl io::Write + Send + 'static,
    dir: &tempfile::TempDir,
) -> (LocalWriter, HostOutputHandle) {
    LocalWriter::start_terminal(
        Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
        sink,
        operation().target(),
        operation().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
    )
    .unwrap()
}
fn feed(cap: &mut HostOutputHandle, bytes: &[u8]) -> OutputResult {
    cap.feed(bytes.to_vec())
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
}

#[test]
fn internal_da_is_same_writer_io_without_consuming_user_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Sink::default();
    let observed = sink.clone();
    let (mut writer, mut cap) = start(sink, &dir);
    assert_eq!(
        feed(&mut cap, b"\x1b["),
        OutputResult::Processed { replies: 0 }
    );
    assert_eq!(feed(&mut cap, b"c"), OutputResult::Processed { replies: 1 });
    assert!(matches!(
        writer
            .handle()
            .submit(operation(), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
            .unwrap()
            .wait(Duration::from_secs(2))
            .unwrap(),
        WriteResult::Receipt(ReceiptEnvelope {
            outcome: Outcome::HostWritten,
            ..
        })
    ));
    finish(&mut writer);
    assert_eq!(&*observed.0.lock().unwrap(), b"\x1b[?6cPUBLIC_TEST_ONLY\n");
    assert!(matches!(
        Journal::open(&dir.path().join("journal.sqlite"))
            .unwrap()
            .query(&operation())
            .unwrap(),
        ReceiptLookup::Known(_)
    ));
}

#[test]
fn wrong_parser_activation_and_user_raw_reply_cannot_write() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Sink::default();
    let observed = sink.clone();
    let (mut writer, mut cap) = start(sink, &dir);
    let real = cap.context.clone();
    cap.context = Arc::new(());
    assert_eq!(
        feed(&mut cap, b"\x1b[c"),
        OutputResult::Rejected(Reject::Epoch)
    );
    assert_eq!(
        writer
            .handle()
            .terminal_reply(&operation().target(), 9, b"EVIL"),
        Err(Reject::Unsupported)
    );
    assert!(observed.0.lock().unwrap().is_empty());
    cap.context = real;
    assert_eq!(
        feed(&mut cap, b"\x1b[c"),
        OutputResult::Processed { replies: 1 }
    );
    finish(&mut writer);
    assert_eq!(
        Journal::open(&dir.path().join("journal.sqlite"))
            .unwrap()
            .query(&operation())
            .unwrap(),
        ReceiptLookup::Missing
    );
}

#[test]
fn query_flood_and_rate_exhaustion_seal_without_partial_batch_replies() {
    for flood in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let sink = Sink::default();
        let observed = sink.clone();
        let (mut writer, mut cap) = start(sink, &dir);
        if flood {
            assert_eq!(
                feed(&mut cap, &b"\x1b[c".repeat(33)),
                OutputResult::Rejected(Reject::Unsupported)
            );
            assert!(observed.0.lock().unwrap().is_empty());
        } else {
            assert_eq!(
                feed(&mut cap, &b"\x1b[c".repeat(32)),
                OutputResult::Processed { replies: 32 }
            );
            assert_eq!(
                feed(&mut cap, b"\x1b[c"),
                OutputResult::Rejected(Reject::Rate)
            );
            assert_eq!(observed.0.lock().unwrap().len(), 160);
        }
        assert!(matches!(cap.feed(b"\x1b[c".to_vec()), Err(Reject::Sealed)));
        finish(&mut writer);
    }
}

#[test]
fn pending_output_feedback_shares_the_input_credit_budget() {
    let dir = tempfile::tempdir().unwrap();
    let (mut writer, mut cap) = start(Sink::default(), &dir);
    let mut pending = Vec::new();
    for _ in 0..MAX_CREDITS {
        let p = cap.feed(vec![b'x']).unwrap();
        assert_eq!(
            p.wait(Duration::from_secs(2)).unwrap(),
            OutputResult::Processed { replies: 0 }
        );
        pending.push(p);
    }
    assert!(matches!(cap.feed(vec![b'x']), Err(Reject::Full)));
    assert!(matches!(
        writer.handle().submit(operation(), 9, vec![b'x']),
        Err(Reject::Full)
    ));
    drop(pending);
    assert_eq!(feed(&mut cap, b"x"), OutputResult::Processed { replies: 0 });
    finish(&mut writer);
}

#[test]
fn stop_rejects_queued_reply_while_an_already_committed_write_is_blocked() {
    struct Block {
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
        sink: Sink,
    }
    impl io::Write for Block {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(2)).unwrap();
            self.sink.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let sink = Sink::default();
    let observed = sink.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let (mut writer, mut cap) = start(
        Block {
            entered: tx,
            release: wait,
            sink,
        },
        &dir,
    );
    let committed = cap.feed(b"\x1b[c".to_vec()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let queued = cap.feed(b"\x1b[c".to_vec()).unwrap();
    writer.seal();
    assert!(matches!(cap.feed(b"\x1b[c".to_vec()), Err(Reject::Sealed)));
    release.send(()).unwrap();
    assert_eq!(
        committed.wait(Duration::from_secs(2)).unwrap(),
        OutputResult::Processed { replies: 1 }
    );
    assert_eq!(
        queued.wait(Duration::from_secs(2)).unwrap(),
        OutputResult::Rejected(Reject::Sealed)
    );
    finish(&mut writer);
    assert_eq!(&*observed.0.lock().unwrap(), b"\x1b[?6c");
}

#[test]
fn partial_or_unknown_reply_io_seals_and_does_not_create_a_user_receipt() {
    struct Fail(bool);
    impl io::Write for Fail {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            if self.0 {
                self.0 = false;
                Ok(b.len().min(2))
            } else {
                Err(io::Error::other("PUBLIC_TEST_ONLY"))
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    for partial in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut writer, mut cap) = start(Fail(partial), &dir);
        assert_eq!(
            feed(&mut cap, b"\x1b[c"),
            if partial {
                OutputResult::Partial
            } else {
                OutputResult::Unknown
            }
        );
        assert!(matches!(cap.feed(b"\x1b[c".to_vec()), Err(Reject::Sealed)));
        finish(&mut writer);
        assert_eq!(
            Journal::open(&dir.path().join("journal.sqlite"))
                .unwrap()
                .query(&operation())
                .unwrap(),
            ReceiptLookup::Missing
        );
    }
}

#[test]
fn stop_between_replies_reports_partial_instead_of_no_effect_rejection() {
    struct StopAfterFirst {
        sealed: Arc<Mutex<Option<Arc<AtomicBool>>>>,
        sink: Sink,
    }
    impl io::Write for StopAfterFirst {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let n = self.sink.write(bytes)?;
            self.sealed
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .store(true, Ordering::Release);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let sink = Sink::default();
    let observed = sink.clone();
    let sealed = Arc::new(Mutex::new(None));
    let (mut writer, mut cap) = start(
        StopAfterFirst {
            sealed: sealed.clone(),
            sink,
        },
        &dir,
    );
    *sealed.lock().unwrap() = Some(writer.handle.sealed.clone());
    assert_eq!(feed(&mut cap, b"\x1b[c\x1b[c"), OutputResult::Partial);
    finish(&mut writer);
    assert_eq!(&*observed.0.lock().unwrap(), b"\x1b[?6c");
}
