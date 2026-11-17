#![allow(non_snake_case)]
use super::super::resize;
use super::*;
use cc_desk_remote_record::lifecycle::{Kind, Request as Operation, State as Epochs};
use std::sync::{Mutex, atomic::AtomicU64};
const B: Duration = Duration::from_secs(1);
struct Fake(Arc<Mutex<Vec<resize::Size>>>);
impl resize::Backend for Fake {
    fn resize(&mut self, size: resize::Size) -> Result<resize::Size, resize::Fault> {
        self.0.lock().unwrap().push(size);
        Ok(size)
    }
}
fn target() -> Target {
    Target {
        host: [1; 32],
        session: [2; 16],
        run: "TEST_ONLY_RESIZE".into(),
        generation: 3,
        incarnation: 4,
    }
}
#[test]
fn ResizeCandidate_ActorCommit_001() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let now = Arc::new(AtomicU64::new(1000));
    let sampled = now.clone();
    let (mut native, _output, peer) = LocalWriter::start_guarded_with(
        Journal::open(&dir.path().join("input.sqlite")).unwrap(),
        std::io::sink(),
        target(),
        [5; 32],
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        (
            ControlStart::new(
                ViewStart {
                    terminal: CanonicalStart::new(3, 10, &HostViewBudget::new()).unwrap(),
                    binding: [0x33; 32],
                    minor: 1,
                },
                Arc::new(move || Ok(sampled.load(Ordering::Acquire))),
                control::GrantContext {
                    challenge_ms: 1000,
                    expires_host_ms: 11000,
                    grant: [7; 32],
                    lease: [8; 16],
                    fence: 4,
                    policy: 5,
                },
            ),
            Some(resize::State::fake(Fake(calls.clone()))),
        ),
    )
    .unwrap();
    let frame = peer.capture().unwrap().wait(B).unwrap().unwrap();
    let stamp = frame.issued_stamp().unwrap();
    let before = Epochs {
        generation: 3,
        fence: 4,
        mode: stamp.mode_epoch.parse().unwrap(),
        dimension: stamp.dimension_epoch.parse().unwrap(),
        sync: stamp.sync_epoch.parse().unwrap(),
    };
    drop(frame);
    let request = Operation {
        kind: Kind::Resize,
        operation: [1; 16],
        sequence: 1,
        state: before,
        size: Some((4, 12)),
    };
    let result = peer
        .resize_candidate(request.clone(), B)
        .unwrap()
        .wait()
        .unwrap();
    native.seal();
    let end = std::time::Instant::now() + B;
    while !native.try_join().unwrap() {
        assert!(std::time::Instant::now() < end);
        std::thread::yield_now();
    }
    assert_eq!(
        result,
        Ok(resize::Applied {
            request,
            after: Epochs {
                dimension: before.dimension + 1,
                ..before
            },
            size: resize::Size {
                rows: 4,
                columns: 12
            }
        })
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[resize::Size {
            rows: 4,
            columns: 12
        }]
    );
}
type Hook = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;
type ClockHook = Arc<Mutex<Option<Box<dyn FnMut() + Send>>>>;
#[derive(Clone, Copy)]
enum Behavior {
    Success,
    NotApplied,
    Unknown,
    WrongSize,
}
struct ScriptFake {
    calls: Arc<Mutex<Vec<resize::Size>>>,
    behavior: Behavior,
    hook: Hook,
}
impl resize::Backend for ScriptFake {
    fn resize(&mut self, size: resize::Size) -> Result<resize::Size, resize::Fault> {
        self.calls.lock().unwrap().push(size);
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook();
        }
        match self.behavior {
            Behavior::Success => Ok(size),
            Behavior::NotApplied => Err(resize::Fault::KnownNotApplied),
            Behavior::Unknown => Err(resize::Fault::Unknown),
            Behavior::WrongSize => Ok(resize::Size {
                rows: size.rows + 1,
                ..size
            }),
        }
    }
}
#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<Vec<u8>>>>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().push(b.to_vec());
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    native: LocalWriter,
    output: HostOutputHandle,
    peer: PeerViewHandle,
    calls: Arc<Mutex<Vec<resize::Size>>>,
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    now: Arc<AtomicU64>,
    hook: Hook,
    clock_hook: ClockHook,
    clock: Arc<dyn control::HostClock>,
}
impl Fixture {
    fn new(behavior: Behavior, enabled: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(1000));
        let sampled = now.clone();
        let clock_hook: ClockHook = Arc::new(Mutex::new(None));
        let sampled_hook = clock_hook.clone();
        let clock: Arc<dyn control::HostClock> = Arc::new(move || {
            if let Some(hook) = sampled_hook.lock().unwrap().as_mut() {
                hook();
            }
            Ok(sampled.load(Ordering::Acquire))
        });
        let calls = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let hook: Hook = Arc::new(Mutex::new(None));
        let resize = enabled.then(|| {
            resize::State::fake(ScriptFake {
                calls: calls.clone(),
                behavior,
                hook: hook.clone(),
            })
        });
        let (native, output, peer) = LocalWriter::start_guarded_with(
            Journal::open(&dir.path().join("input.sqlite")).unwrap(),
            Sink(writes.clone()),
            target(),
            [5; 32],
            9,
            DigestKey::from_backend_bytes([0x5a; 32]),
            (
                ControlStart::new(
                    ViewStart {
                        terminal: CanonicalStart::new(3, 10, &HostViewBudget::new()).unwrap(),
                        binding: [0x33; 32],
                        minor: 1,
                    },
                    clock.clone(),
                    control::GrantContext {
                        challenge_ms: 1000,
                        expires_host_ms: 11000,
                        grant: [7; 32],
                        lease: [8; 16],
                        fence: 4,
                        policy: 5,
                    },
                ),
                resize,
            ),
        )
        .unwrap();
        Self {
            _dir: dir,
            native,
            output,
            peer,
            calls,
            writes,
            now,
            hook,
            clock_hook,
            clock,
        }
    }
    fn capture(&self) -> projection::CapturedProjection {
        self.peer.capture().unwrap().wait(B).unwrap().unwrap()
    }
    fn request(&self, seq: u64, size: (u16, u16)) -> Operation {
        let frame = self.capture();
        let stamp = frame.issued_stamp().unwrap();
        Operation {
            kind: Kind::Resize,
            operation: [seq as u8; 16],
            sequence: seq,
            state: Epochs {
                generation: 3,
                fence: 4,
                mode: stamp.mode_epoch.parse().unwrap(),
                dimension: stamp.dimension_epoch.parse().unwrap(),
                sync: stamp.sync_epoch.parse().unwrap(),
            },
            size: Some(size),
        }
    }
    fn apply(&self, r: Operation) -> Result<resize::Applied, resize::Error> {
        self.peer.resize_candidate(r, B).unwrap().wait().unwrap()
    }
    async fn view(&self) -> SyncedView {
        let frame = self.capture();
        let ack = cc_desk_remote_record::projection::Ack {
            profile: 1,
            stamp: frame.issued_stamp().unwrap().clone(),
        };
        let (host, _client) = tokio::io::duplex(65536);
        let _writer = frame
            .deliver(
                cc_desk_remote_transport::records::RecordWriter::new(host, 1).unwrap(),
                B,
            )
            .await
            .unwrap();
        self.peer
            .acknowledge(ack)
            .unwrap()
            .wait(B)
            .unwrap()
            .unwrap()
    }
    fn set_hook(&self, hook: impl FnOnce() + Send + 'static) {
        *self.hook.lock().unwrap() = Some(Box::new(hook));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.native.seal();
        let end = std::time::Instant::now() + Duration::from_secs(3);
        while !self.native.try_join().unwrap() {
            assert!(std::time::Instant::now() < end);
            std::thread::yield_now();
        }
    }
}
fn operation(seq: u64) -> OperationKey {
    OperationKey {
        host: target().host,
        session: target().session,
        run: target().run,
        generation: 3,
        incarnation: 4,
        device: [5; 32],
        operation: [seq as u8; 16],
        sequence: seq,
    }
}
#[test]
fn ResizeCandidate_BoundsAndDefaultOff_002() {
    for (size, expected) in [
        ((0, 12), Reject::Bounds),
        ((4, 0), Reject::Bounds),
        ((129, 12), Reject::Bounds),
        ((4, 257), Reject::Bounds),
        ((100, 240), Reject::Bounds),
        ((101, 80), Reject::Unsupported),
        ((60, 241), Reject::Unsupported),
        ((4, 1), Reject::Unsupported),
    ] {
        let f = Fixture::new(Behavior::Success, true);
        assert_eq!(
            f.apply(f.request(1, size)),
            Err(resize::Error::Rejected(expected)),
            "{size:?}"
        );
        assert!(f.calls.lock().unwrap().is_empty());
    }
    let f = Fixture::new(Behavior::Success, false);
    assert_eq!(
        f.apply(f.request(1, (4, 12))),
        Err(resize::Error::Rejected(Reject::Unsupported))
    );
    assert!(f.calls.lock().unwrap().is_empty());
}
#[test]
fn ResizeCandidate_DuplicateConflictAndFloor_003() {
    let f = Fixture::new(Behavior::Success, true);
    let first = f.request(1, (4, 12));
    let applied = f.apply(first.clone()).unwrap();
    assert_eq!(f.apply(first.clone()), Ok(applied));
    let mut different = first.clone();
    different.size = Some((5, 12));
    assert_eq!(f.apply(different), Err(resize::Error::Conflict));
    let mut second = f.request(2, (5, 12));
    let prior = second.clone();
    second.operation = [1; 16];
    assert_eq!(f.apply(second), Err(resize::Error::Conflict));
    assert!(f.apply(prior).is_ok());
    assert!(f.apply(first).is_ok());
    assert_eq!(f.calls.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn ResizeCandidate_OldEligibilityAndSameSize_004() {
    for size in [(3, 10), (4, 12)] {
        let f = Fixture::new(Behavior::Success, true);
        let view = f.view().await;
        let old = f
            .peer
            .input_window(&view)
            .unwrap()
            .wait(B)
            .unwrap()
            .unwrap();
        let epochs = old.diagnostic_epochs();
        let r = Operation {
            kind: Kind::Resize,
            operation: [1; 16],
            sequence: 1,
            state: Epochs {
                generation: 3,
                fence: 4,
                mode: epochs.mode_epoch,
                dimension: epochs.dimension_epoch,
                sync: 1,
            },
            size: Some(size),
        };
        let applied = f.apply(r).unwrap();
        assert_eq!(applied.after.dimension, if size == (3, 10) { 0 } else { 1 });
        assert_eq!(f.calls.lock().unwrap().len(), usize::from(size != (3, 10)));
        assert!(f.peer.input_window(&old).unwrap().wait(B).unwrap().is_err());
        assert!(matches!(
            f.peer
                .submit(&old, operation(1), 9, b"old".to_vec())
                .unwrap()
                .wait(B)
                .unwrap(),
            WriteResult::Rejected(_)
        ));
        assert!(f.writes.lock().unwrap().is_empty());
        let frame = f.capture();
        assert_eq!(frame.view().rows(), size.0);
        assert_eq!(frame.view().columns(), size.1);
        assert_eq!(
            frame.view().epochs().structure_epoch,
            if size == (3, 10) { 0 } else { 1 }
        );
        drop(frame);
        let fresh = f.view().await;
        assert!(matches!(
            f.peer
                .submit(&fresh, operation(1), 9, b"no_window".to_vec())
                .unwrap()
                .wait(B)
                .unwrap(),
            WriteResult::Rejected(_)
        ));
        let window = f
            .peer
            .input_window(&fresh)
            .unwrap()
            .wait(B)
            .unwrap()
            .unwrap();
        assert!(matches!(
            f.peer
                .submit(&window, operation(1), 9, b"new".to_vec())
                .unwrap()
                .wait(B)
                .unwrap(),
            WriteResult::Receipt(ReceiptEnvelope {
                outcome: Outcome::HostWritten,
                ..
            })
        ));
        assert_eq!(f.writes.lock().unwrap().as_slice(), &[b"new".to_vec()]);
    }
}
#[tokio::test]
async fn ResizeCandidate_InputOrderAndReceiptRetained_005() {
    let f = Fixture::new(Behavior::Success, true);
    let view = f.view().await;
    let window = f
        .peer
        .input_window(&view)
        .unwrap()
        .wait(B)
        .unwrap()
        .unwrap();
    let epochs = window.diagnostic_epochs();
    let r = Operation {
        kind: Kind::Resize,
        operation: [1; 16],
        sequence: 1,
        state: Epochs {
            generation: 3,
            fence: 4,
            mode: epochs.mode_epoch,
            dimension: epochs.dimension_epoch,
            sync: 1,
        },
        size: Some((4, 12)),
    };
    let input = f
        .peer
        .submit(&window, operation(1), 9, b"before".to_vec())
        .unwrap();
    let resize = f.peer.resize_candidate(r, B).unwrap();
    let late = f
        .peer
        .submit(&window, operation(2), 9, b"after".to_vec())
        .unwrap();
    let receipt = match input.wait(B).unwrap() {
        WriteResult::Receipt(r) => r,
        v => panic!("{v:?}"),
    };
    assert!(resize.wait().unwrap().is_ok());
    assert!(matches!(late.wait(B).unwrap(), WriteResult::Rejected(_)));
    let known = f
        .peer
        .pending_receipt(operation(1))
        .unwrap()
        .wait(B)
        .unwrap()
        .unwrap();
    assert_eq!(known, ReceiptLookup::Known(receipt));
    let fresh = f.view().await;
    let newwindow = f
        .peer
        .input_window(&fresh)
        .unwrap()
        .wait(B)
        .unwrap()
        .unwrap();
    assert!(matches!(
        f.peer
            .submit(&newwindow, operation(2), 9, b"unsettled".to_vec())
            .unwrap()
            .wait(B)
            .unwrap(),
        WriteResult::Rejected(Reject::Pending)
    ));
    assert_eq!(f.writes.lock().unwrap().as_slice(), &[b"before".to_vec()]);
}
#[tokio::test]
async fn ResizeCandidate_FailureAndLateResult_006() {
    for behavior in [Behavior::NotApplied, Behavior::Unknown, Behavior::WrongSize] {
        let f = Fixture::new(behavior, true);
        let view = f.view().await;
        let window = f
            .peer
            .input_window(&view)
            .unwrap()
            .wait(B)
            .unwrap()
            .unwrap();
        let e = window.diagnostic_epochs();
        let r = Operation {
            kind: Kind::Resize,
            operation: [1; 16],
            sequence: 1,
            state: Epochs {
                generation: 3,
                fence: 4,
                mode: e.mode_epoch,
                dimension: e.dimension_epoch,
                sync: 1,
            },
            size: Some((4, 12)),
        };
        let result = f.apply(r.clone());
        assert_eq!(
            result,
            Err(if matches!(behavior, Behavior::NotApplied) {
                resize::Error::NotApplied
            } else {
                resize::Error::Uncertain
            })
        );
        match f.peer.submit(&window, operation(1), 9, b"old".to_vec()) {
            Err(_) => {}
            Ok(p) => assert!(matches!(p.wait(B).unwrap(), WriteResult::Rejected(_))),
        }
        if matches!(behavior, Behavior::NotApplied) {
            assert_eq!(f.apply(r), result);
            let fresh = f.capture();
            assert_eq!(fresh.view().epochs().dimension_epoch, 0);
        }
        assert_eq!(f.calls.lock().unwrap().len(), 1);
        assert!(f.writes.lock().unwrap().is_empty());
    }
}
#[test]
fn ResizeCandidate_ExpiryRevocationAndStale_007() {
    for mode in 0..6 {
        let f = Fixture::new(Behavior::Success, true);
        let mut r = f.request(1, (4, 12));
        match mode {
            0 => f.now.store(11000, Ordering::Release),
            1 => f.peer.close(),
            2 => r.state.fence += 1,
            3 => r.state.sync += 1,
            4 => r.state.generation += 1,
            _ => r.state.dimension += 1,
        }
        assert!(f.apply(r).is_err());
        assert!(f.calls.lock().unwrap().is_empty());
    }
    for expired in [false, true] {
        let f = Fixture::new(Behavior::Success, true);
        let r = f.request(1, (4, 12));
        let scope = f.peer.control.scope.clone();
        let now = f.now.clone();
        f.set_hook(move || {
            if expired {
                now.store(11000, Ordering::Release)
            } else {
                scope.close()
            }
        });
        assert_eq!(f.apply(r), Err(resize::Error::Rejected(Reject::Policy)));
        let fact = f.native.capture().unwrap().wait(B).unwrap().unwrap();
        assert_eq!((fact.view().rows(), fact.view().columns()), (4, 12));
        assert_eq!(fact.view().epochs().dimension_epoch, 1);
        assert_eq!(f.calls.lock().unwrap().len(), 1);
        assert!(f.writes.lock().unwrap().is_empty());
    }
}
#[test]
fn ResizeCandidate_CancelAndDeadline_008() {
    for cancel in [false, true] {
        let f = Fixture::new(Behavior::Success, true);
        let r = f.request(1, (4, 12));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        f.set_hook(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        });
        let p = f
            .peer
            .resize_candidate(r.clone(), Duration::from_millis(50))
            .unwrap();
        entered_rx.recv_timeout(B).unwrap();
        if cancel {
            drop(p);
        } else {
            assert_eq!(p.wait(), Err(WaitError::Uncertain));
            drop(p);
        }
        release_tx.send(()).unwrap();
        // The late success must retire the actor without re-opening eligibility.
        let end = std::time::Instant::now() + B;
        while !f.native.handle.sealed.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < end);
            std::thread::yield_now();
        }
        assert_eq!(f.calls.lock().unwrap().len(), 1);
        assert!(f.peer.capture().is_err());
        assert!(f.writes.lock().unwrap().is_empty());
    }
}
#[test]
fn ResizeCandidate_QueuedReplyPastDeadline_009() {
    let f = Fixture::new(Behavior::Success, true);
    let r = f.request(1, (4, 12));
    let p = f
        .peer
        .resize_candidate(r, Duration::from_millis(50))
        .unwrap();
    // An actor status barrier proves Resize has returned before delaying receipt.
    f.native.control_status().unwrap().wait(B).unwrap().unwrap();
    std::thread::sleep(Duration::from_millis(60));
    match f.peer.capture() {
        Err(_) => {}
        Ok(pending) => assert!(pending.wait(B).unwrap().is_err()),
    }
    assert_eq!(p.wait(), Err(WaitError::Uncertain));
    assert_eq!(f.calls.lock().unwrap().len(), 1);
}
#[test]
fn ResizeCandidate_AuthoritySamplingDeadline_010() {
    for sample in [2, 3] {
        let f = Fixture::new(Behavior::Success, true);
        let r = f.request(1, (4, 12));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let mut count = 0;
        *f.clock_hook.lock().unwrap() = Some(Box::new(move || {
            count += 1;
            if count == sample {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            }
        }));
        let p = f
            .peer
            .resize_candidate(r, Duration::from_millis(100))
            .unwrap();
        entered_rx.recv_timeout(B).unwrap();
        assert_eq!(p.wait(), Err(WaitError::Uncertain));
        drop(p);
        release_tx.send(()).unwrap();
        let end = std::time::Instant::now() + B;
        // No explicit seal: only the pending wait/drop may stop the late fake effect.
        let mut f = f;
        while !f.native.try_join().unwrap() {
            assert!(std::time::Instant::now() < end);
            std::thread::yield_now();
        }
        assert_eq!(
            f.calls.lock().unwrap().len(),
            if sample == 2 { 0 } else { 1 }
        );
        assert!(f.writes.lock().unwrap().is_empty());
    }
}
#[test]
fn ResizeCandidate_UnobservedCannotBypassOrRebind_011() {
    let f = Fixture::new(Behavior::Success, true);
    let r = f.request(1, (4, 12));
    let p = f.peer.resize_candidate(r.clone(), B).unwrap();
    f.native.control_status().unwrap().wait(B).unwrap().unwrap();
    assert!(matches!(
        f.peer.capture().unwrap().wait(B).unwrap(),
        Err(Reject::Pending)
    ));
    assert_eq!(
        f.peer.resize_candidate(r, B).unwrap().wait().unwrap(),
        Err(resize::Error::Rejected(Reject::Pending))
    );
    let rebind = f
        .native
        .rebind(
            [5; 32],
            4,
            control::ControlAdmission {
                authority: None,
                binding: [0x44; 32],
                minor: 1,
                clock: f.clock.clone(),
                grant: control::GrantContext {
                    challenge_ms: 1000,
                    expires_host_ms: 11000,
                    grant: [7; 32],
                    lease: [9; 16],
                    fence: 5,
                    policy: 5,
                },
            },
        )
        .unwrap();
    assert!(matches!(rebind.wait(B).unwrap(), Err(Reject::Pending)));
    assert!(p.wait().unwrap().is_ok());
    assert_eq!(
        f.native
            .control_status()
            .unwrap()
            .wait(B)
            .unwrap()
            .unwrap()
            .fence,
        4
    );
    assert!(f.peer.capture().unwrap().wait(B).unwrap().is_ok());
    assert_eq!(f.calls.lock().unwrap().len(), 1);
}
#[test]
fn ResizeCandidate_CacheBoundAndWrongActor_012() {
    let f = Fixture::new(Behavior::Success, true);
    let other = Fixture::new(Behavior::Success, true);
    let r = f.request(1, (4, 12));
    assert_eq!(
        resize::enqueue(&f.peer.input, other.peer.control.scope.clone(), r, B)
            .unwrap()
            .wait()
            .unwrap(),
        Err(resize::Error::Rejected(Reject::Policy))
    );
    assert!(f.calls.lock().unwrap().is_empty());
    for seq in 1..=64 {
        assert!(
            f.apply(f.request(seq, if seq % 2 == 1 { (4, 12) } else { (3, 10) }))
                .is_ok()
        );
    }
    assert_eq!(
        f.apply(f.request(65, (4, 12))),
        Err(resize::Error::Rejected(Reject::Full))
    );
    assert_eq!(f.calls.lock().unwrap().len(), 64);
}
#[tokio::test]
async fn ResizeCandidate_OldAckAndModeChange_013() {
    let mut f = Fixture::new(Behavior::Success, true);
    let frame = f.capture();
    let ack = cc_desk_remote_record::projection::Ack {
        profile: 1,
        stamp: frame.issued_stamp().unwrap().clone(),
    };
    let s = &ack.stamp;
    let r = Operation {
        kind: Kind::Resize,
        operation: [1; 16],
        sequence: 1,
        state: Epochs {
            generation: 3,
            fence: 4,
            mode: s.mode_epoch.parse().unwrap(),
            dimension: s.dimension_epoch.parse().unwrap(),
            sync: s.sync_epoch.parse().unwrap(),
        },
        size: Some((4, 12)),
    };
    assert!(f.apply(r).is_ok());
    assert!(f.peer.acknowledge(ack).unwrap().wait(B).unwrap().is_err());
    let (host, _client) = tokio::io::duplex(65536);
    assert!(
        frame
            .deliver(
                cc_desk_remote_transport::records::RecordWriter::new(host, 1).unwrap(),
                B
            )
            .await
            .is_err()
    );
    let mut r = f.request(2, (5, 12));
    assert!(matches!(
        f.output
            .feed_at(0, b"\x1b[?1004h".to_vec())
            .unwrap()
            .wait(B)
            .unwrap(),
        OutputResult::Processed { .. }
    ));
    assert_eq!(
        f.apply(r.clone()),
        Err(resize::Error::Rejected(Reject::Epoch))
    );
    r.state.dimension = u64::MAX;
    assert_eq!(f.apply(r), Err(resize::Error::Rejected(Reject::Epoch)));
    assert_eq!(f.calls.lock().unwrap().len(), 1);
}
