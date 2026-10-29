use super::*;
fn canonical(
    dir: &tempfile::TempDir,
    sink: impl io::Write + Send + 'static,
) -> (LocalWriter, HostOutputHandle) {
    LocalWriter::start_canonical_terminal(
        Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
        sink,
        operation().target(),
        operation().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        CanonicalStart::new(3, 10, &HostViewBudget::new()).unwrap(),
    )
    .unwrap()
}

#[test]
fn projection_capture_is_ordered_with_actual_output_and_exact_target() {
    let dir = tempfile::tempdir().unwrap();
    let (mut writer, mut cap) = canonical(&dir, Sink::default());
    assert_eq!(
        feed(&mut cap, 0, b"ab"),
        OutputResult::Processed { replies: 0 }
    );
    let before = writer.capture().unwrap();
    assert_eq!(
        feed(&mut cap, 2, b"Z"),
        OutputResult::Processed { replies: 0 }
    );
    let before = before.wait(Duration::from_secs(2)).unwrap().unwrap();
    assert_eq!(before.through_offset(), 2);
    assert_eq!(before.revision(), 1);
    assert_eq!(before.target(), &operation().target());
    assert_eq!(before.view().cell(0, 2).unwrap().scalar, ' ');
    let after = writer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!((after.through_offset(), after.revision()), (3, 2));
    assert_eq!(after.view().cell(0, 2).unwrap().scalar, 'Z');
    finish(&mut writer);
}

#[test]
fn projection_encoder_uses_the_captured_target_revision_and_core_epochs() {
    use cc_desk_remote_record::projection::{Content, decode};
    let dir = tempfile::tempdir().unwrap();
    let (mut writer, mut cap) = canonical(&dir, Sink::default());
    assert_eq!(
        feed(&mut cap, 0, b"ok\x1b[?1004h"),
        OutputResult::Processed { replies: 0 }
    );
    let frame = writer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let context = projection::ProjectionContext {
        binding: [0x33; 32],
        barrier: [0x44; 16],
        sync_epoch: 7,
    };
    let encoded = frame.encoder(&context, 1).unwrap().next().unwrap().unwrap();
    let message = decode(&encoded, 1).unwrap();
    let Content::SnapshotPart(p) = message.content else {
        panic!("snapshot")
    };
    assert_eq!(p.stamp.target.host, "01".repeat(32));
    assert_eq!(p.stamp.target.run, operation().run);
    assert_eq!(p.stamp.revision, "1");
    assert_eq!(p.stamp.mode_epoch, "1");
    assert_eq!(p.stamp.sync_epoch, "7");
    assert_eq!(p.cells[0].scalar, 'o');
    finish(&mut writer);
}

#[test]
fn projection_capture_credits_cover_pending_and_consumed_but_retained_frames() {
    let dir = tempfile::tempdir().unwrap();
    let (mut writer, _) = canonical(&dir, Sink::default());
    let a = writer.capture().unwrap();
    let b = writer.capture().unwrap();
    assert!(matches!(writer.capture(), Err(Reject::Full)));
    let frame_a = a.wait(Duration::from_secs(2)).unwrap().unwrap();
    let frame_b = b.wait(Duration::from_secs(2)).unwrap().unwrap();
    drop(a);
    drop(b);
    assert!(matches!(writer.capture(), Err(Reject::Full)));
    drop(frame_a);
    let fresh = writer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(fresh.revision(), 0);
    drop(frame_b);
    drop(fresh);
    finish(&mut writer);
}

#[test]
fn projection_capture_refuses_static_and_sealed_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let (mut static_writer, _) = start(Sink::default(), &dir);
    assert!(matches!(static_writer.capture(), Err(Reject::Unsupported)));
    finish(&mut static_writer);
    let dir = tempfile::tempdir().unwrap();
    let (mut writer, mut cap) = canonical(&dir, Sink::default());
    assert_eq!(
        feed(&mut cap, 0, b"\x1b]52;c;x\x07"),
        OutputResult::Rejected(Reject::Unsupported)
    );
    assert!(matches!(writer.capture(), Err(Reject::Sealed)));
    finish(&mut writer);
}

#[test]
fn projection_capture_shared_host_budget_covers_different_session_writers() {
    let budget = HostViewBudget::new();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let make = |dir: &tempfile::TempDir, target: Target| {
        LocalWriter::start_canonical_terminal(
            Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
            Sink::default(),
            target,
            operation().device,
            9,
            DigestKey::from_backend_bytes([0x5a; 32]),
            CanonicalStart::new(3, 10, &budget).unwrap(),
        )
        .unwrap()
    };
    let (mut first, _) = make(&a, operation().target());
    let mut other = operation().target();
    other.session = [7; 16];
    let (mut second, _) = make(&b, other.clone());
    let one = first
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let two = second
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(two.target(), &other);
    assert!(matches!(first.capture(), Err(Reject::Full)));
    assert!(matches!(second.capture(), Err(Reject::Full)));
    drop(one);
    let again = second
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    drop(two);
    drop(again);
    finish(&mut first);
    finish(&mut second);
}

#[test]
fn projection_capture_timeout_is_uncertain_and_queued_capture_refuses_after_seal() {
    struct Block {
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl io::Write for Block {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (entered, observed) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    let (mut writer, mut cap) = canonical(
        &dir,
        Block {
            entered,
            release: blocked,
        },
    );
    let output = cap.feed_at(0, b"\x1b[c".to_vec()).unwrap();
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    let capture = writer.capture().unwrap();
    assert!(matches!(
        capture.wait(Duration::from_millis(5)),
        Err(WaitError::Uncertain)
    ));
    writer.seal();
    release.send(()).unwrap();
    assert_eq!(
        output.wait(Duration::from_secs(2)).unwrap(),
        OutputResult::Processed { replies: 1 }
    );
    assert!(matches!(
        capture.wait(Duration::from_secs(2)),
        Ok(Err(Reject::Sealed))
    ));
    finish(&mut writer);
}

#[test]
fn projection_capture_stale_context_and_lost_feedback_cannot_steal_or_leak_frames() {
    let dir = tempfile::tempdir().unwrap();
    let (mut writer, mut cap) = canonical(&dir, Sink::default());
    let stale = ProjectionHandle::new(&writer.handle, Arc::new(()), HostViewBudget::new());
    assert!(matches!(
        stale.capture().unwrap().wait(Duration::from_secs(2)),
        Ok(Err(Reject::Epoch))
    ));
    drop(writer.capture().unwrap());
    let pending = writer.capture().unwrap();
    let frame = pending.wait(Duration::from_secs(2)).unwrap().unwrap();
    drop(pending);
    let second = writer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(frame.through_offset(), 0);
    drop(frame);
    drop(second);
    assert_eq!(
        feed(&mut cap, 0, b"ab"),
        OutputResult::Processed { replies: 0 }
    );
    let fresh = writer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(fresh.through_offset(), 2);
    finish(&mut writer);
}
