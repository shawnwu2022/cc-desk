use super::*;
use std::{
    pin::Pin,
    sync::atomic::AtomicU64,
    task::{Context, Poll},
};
use tokio::io::AsyncWrite;

#[derive(Clone, Default)]
struct CapturedWrites {
    calls: Arc<AtomicUsize>,
    expire_after_flush: Option<Arc<AtomicU64>>,
    expire_on_call: usize,
}
impl AsyncWrite for CapturedWrites {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(clock) = &self.expire_after_flush
            && self.calls.load(Ordering::Acquire) >= self.expire_on_call
        {
            clock.store(5000, Ordering::Release);
        }
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
fn controlled(
    dir: &tempfile::TempDir,
    now: Arc<AtomicU64>,
    expiry: u64,
) -> (LocalWriter, HostOutputHandle, PeerViewHandle) {
    controlled_size(dir, now, expiry, 80)
}
fn controlled_size(
    dir: &tempfile::TempDir,
    now: Arc<AtomicU64>,
    expiry: u64,
    columns: usize,
) -> (LocalWriter, HostOutputHandle, PeerViewHandle) {
    LocalWriter::start_control_guarded(
        Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
        io::sink(),
        target(),
        [5; 32],
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        ControlStart::new(
            ViewStart {
                terminal: CanonicalStart::new(3, columns, &HostViewBudget::new()).unwrap(),
                binding: [0x33; 32],
                minor: 1,
            },
            Arc::new(move || match now.load(Ordering::Acquire) {
                u64::MAX => Err(Reject::Policy),
                value => Ok(value),
            }),
            control::GrantContext {
                challenge_ms: 1000,
                expires_host_ms: expiry,
                grant: [7; 32],
                lease: [8; 16],
                fence: 4,
                policy: 5,
            },
        ),
    )
    .unwrap()
}
fn capture(peer: &PeerViewHandle) -> projection::CapturedProjection {
    peer.capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn expired_held_frame_cannot_emit_and_new_capture_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let now = Arc::new(AtomicU64::new(1000));
    let (mut writer, _output, peer) = controlled(&dir, now.clone(), 5000);
    let frame = capture(&peer);
    let sink = CapturedWrites::default();
    let calls = sink.calls.clone();
    now.store(5000, Ordering::Release);
    assert!(matches!(
        frame
            .deliver(RecordWriter::new(sink, 1).unwrap(), Duration::from_secs(1))
            .await,
        Err(Reject::Policy)
    ));
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(!peer.control.scope.is_active());
    assert!(peer.capture().is_err());
    finish(&mut writer);
}
#[tokio::test]
async fn expired_grant_cannot_issue_a_fresh_native_capture() {
    let dir = tempfile::tempdir().unwrap();
    let now = Arc::new(AtomicU64::new(1000));
    let (mut writer, _output, peer) = controlled(&dir, now.clone(), 5000);
    now.store(5000, Ordering::Release);
    assert!(matches!(
        peer.capture()
            .unwrap()
            .wait(Duration::from_secs(2))
            .unwrap(),
        Err(Reject::Policy)
    ));
    assert!(!peer.control.scope.is_active());
    finish(&mut writer);
}
#[tokio::test]
async fn expiry_after_the_last_record_cannot_publish_delivered_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let now = Arc::new(AtomicU64::new(1000));
    let (mut writer, _output, peer) = controlled(&dir, now.clone(), 5000);
    let frame = capture(&peer);
    let record = frame.issued.as_ref().unwrap().clone();
    let count =
        cc_desk_remote_terminal::SnapshotEncoder::new(frame.view(), record.stamp.clone(), 1)
            .unwrap()
            .count();
    let sink = CapturedWrites {
        expire_after_flush: Some(now),
        expire_on_call: count,
        ..Default::default()
    };
    let calls = sink.calls.clone();
    assert!(matches!(
        frame
            .deliver(RecordWriter::new(sink, 1).unwrap(), Duration::from_secs(1))
            .await,
        Err(Reject::Policy)
    ));
    assert_eq!(calls.load(Ordering::Acquire), count);
    assert_ne!(record.phase.load(Ordering::Acquire), DELIVERED);
    assert!(!peer.control.scope.is_active());
    finish(&mut writer);
}
#[tokio::test]
async fn live_grant_outlives_writer_lease_but_expired_projection_ack_rejects() {
    let dir = tempfile::tempdir().unwrap();
    let now = Arc::new(AtomicU64::new(1000));
    let (mut writer, _output, peer) = controlled(&dir, now.clone(), 50000);
    now.store(20000, Ordering::Release); // initial writer lease ended at16000.
    let frame = capture(&peer);
    let ack = Ack {
        profile: 1,
        stamp: frame.issued_stamp().unwrap().clone(),
    };
    let _io = frame
        .deliver(
            RecordWriter::new(CapturedWrites::default(), 1).unwrap(),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert!(
        peer.acknowledge(ack.clone())
            .unwrap()
            .wait(Duration::from_secs(2))
            .unwrap()
            .is_ok()
    );
    now.store(50000, Ordering::Release);
    assert!(matches!(
        peer.acknowledge(ack)
            .unwrap()
            .wait(Duration::from_secs(2))
            .unwrap(),
        Err(Reject::Policy)
    ));
    finish(&mut writer);
}
#[tokio::test]
async fn each_projection_part_checks_grant_before_the_next_record() {
    let dir = tempfile::tempdir().unwrap();
    let now = Arc::new(AtomicU64::new(1000));
    let (mut writer, _output, peer) = controlled(&dir, now.clone(), 5000);
    let frame = capture(&peer);
    let sink = CapturedWrites {
        expire_after_flush: Some(now),
        ..Default::default()
    };
    let calls = sink.calls.clone();
    assert!(matches!(
        frame
            .deliver(RecordWriter::new(sink, 1).unwrap(), Duration::from_secs(1))
            .await,
        Err(Reject::Policy)
    ));
    assert_eq!(calls.load(Ordering::Acquire), 1); // first complete record only.
    assert!(!peer.control.scope.is_active());
    finish(&mut writer);
}
#[tokio::test(start_paused = true)]
async fn blocked_projection_uses_the_original_short_grant_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let now = Arc::new(AtomicU64::new(1000));
    let (mut writer, _output, peer) = controlled(&dir, now, 1025);
    let frame = capture(&peer);
    let (host, _client) = tokio::io::duplex(1);
    let start = tokio::time::Instant::now();
    assert!(matches!(
        frame
            .deliver(RecordWriter::new(host, 1).unwrap(), Duration::from_secs(1))
            .await,
        Err(Reject::Continuity)
    ));
    assert_eq!(start.elapsed(), Duration::from_millis(25));
    assert!(!peer.control.scope.is_active());
    finish(&mut writer);
}
#[tokio::test(start_paused = true)]
async fn a_shortened_grant_budget_cannot_restart_on_each_later_part() {
    use std::future::Future;
    struct TimedFlush {
        now: Arc<AtomicU64>,
        sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    }
    impl AsyncWrite for TimedFlush {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            let sleep = self
                .sleep
                .get_or_insert_with(|| Box::pin(tokio::time::sleep(Duration::from_millis(2))));
            if sleep.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.sleep = None;
            self.now.store(4995, Ordering::Release);
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let now = Arc::new(AtomicU64::new(1000));
    let (mut writer, _output, peer) = controlled_size(&dir, now.clone(), 5000, 200);
    let frame = capture(&peer);
    assert!(
        cc_desk_remote_terminal::SnapshotEncoder::new(
            frame.view(),
            frame.issued_stamp().unwrap().clone(),
            1
        )
        .unwrap()
        .count()
            >= 4
    );
    let start = tokio::time::Instant::now();
    let result = frame
        .deliver(
            RecordWriter::new(TimedFlush { now, sleep: None }, 1).unwrap(),
            Duration::from_secs(1),
        )
        .await;
    assert!(
        matches!(result, Err(Reject::Continuity)),
        "elapsed={:?}, error={:?}",
        start.elapsed(),
        result.as_ref().err()
    );
    assert_eq!(start.elapsed(), Duration::from_millis(7));
    assert!(!peer.control.scope.is_active());
    finish(&mut writer);
}
#[tokio::test]
async fn emission_clock_rollback_or_error_freezes_the_writer_namespace() {
    for fault in [1999, u64::MAX] {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(1000));
        let (mut writer, _output, peer) = controlled(&dir, now.clone(), 5000);
        now.store(2000, Ordering::Release);
        let frame = capture(&peer);
        now.store(fault, Ordering::Release);
        let sink = CapturedWrites::default();
        let calls = sink.calls.clone();
        assert!(matches!(
            frame
                .deliver(RecordWriter::new(sink, 1).unwrap(), Duration::from_secs(1))
                .await,
            Err(Reject::Policy)
        ));
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert!(*peer.control.scope.writer_closed.borrow());
        finish(&mut writer);
    }
}
