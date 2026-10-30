#![allow(non_snake_case)]
use super::super::resize;
use super::*;
use crate::bound_test_support as support;
use cc_desk_remote_record::lifecycle::{Kind, Request, State};
use std::sync::atomic::AtomicU64;
struct Fake(Arc<AtomicUsize>);
#[tokio::test]
async fn DurableResizeActor_ExpiredSenderHasNoEffect_014() {
    let mut f = Fixture::new().await;
    f.coordinator
        .begin(f.request.clone(), Duration::from_millis(200))
        .unwrap();
    std::thread::sleep(Duration::from_millis(220));
    let result = f.coordinator.execute();
    let calls = f.calls.load(Ordering::Acquire);
    f.close().await;
    assert!(result.is_err());
    assert_eq!(calls, 0);
}
#[tokio::test]
async fn DurableResizeActor_PublicationGateWaitDoesNotRenewQualification_015() {
    let mut f = Fixture::new().await;
    f.coordinator
        .begin(f.request.clone(), Duration::from_millis(200))
        .unwrap();
    let fact = f.coordinator.execute().unwrap();
    f.coordinator.record().unwrap();
    let p = f.coordinator.test_prepared();
    resize::durable::ready(&p, fact.clone())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    let witness = f.coordinator.owner().publication_witness(&fact).unwrap();
    let scope = f.coordinator.peer().control.scope.clone();
    let guard = scope.publication_gate.lock().unwrap();
    let pending = resize::durable::publish(&p, witness).unwrap();
    std::thread::sleep(Duration::from_millis(220));
    drop(guard);
    let result = pending.wait();
    let eligible = f.coordinator.reservation().ready();
    f.close().await;
    assert!(
        !matches!(result, Ok(Ok(()))),
        "waiting for the gate cannot renew the original deadline"
    );
    assert!(
        eligible.is_err(),
        "an unobserved/timed-out publication must not leave qualification open"
    );
}
#[tokio::test]
async fn DurableResizeActor_ClosedScopeStillCancelsReservation_013() {
    let mut f = Fixture::new().await;
    f.begin();
    f.coordinator.peer().close();
    f.coordinator.cancel();
    let sealed = f.writer.handle.sealed.load(Ordering::Acquire);
    f.close().await;
    assert!(
        sealed,
        "scope closure must not prevent outstanding reservation cleanup"
    );
}
#[tokio::test]
async fn DurableResizeActor_CancelBetweenValidationAndClaimHasNoEffect_012() {
    let mut f = Fixture::new().await;
    f.begin();
    let cap = f.coordinator.reservation();
    let weak = Arc::downgrade(&cap);
    cap.test_before_claim(move || weak.upgrade().unwrap().cancel());
    let result = f.coordinator.execute();
    let calls = f.calls.load(Ordering::Acquire);
    f.close().await;
    assert!(result.is_err());
    assert_eq!(
        calls, 0,
        "reservation cancellation before claim must prevent effects"
    );
}
impl resize::Backend for Fake {
    fn resize(&mut self, size: resize::Size) -> Result<resize::Size, resize::Fault> {
        self.0.fetch_add(1, Ordering::AcqRel);
        Ok(size)
    }
}
struct Fixture {
    coordinator: resize::durable::Coordinator,
    writer: LocalWriter,
    calls: Arc<AtomicUsize>,
    request: Request,
    _dir: tempfile::TempDir,
    c: cc_desk_remote_security::RunningChannel,
    h: cc_desk_remote_security::RunningChannel,
    _gate: cc_desk_remote_security::GrantGate,
    _cancel: cc_desk_remote_security::CancelHandle,
}
impl Fixture {
    async fn new() -> Self {
        let now = Arc::new(AtomicU64::new(1000));
        let (c, h, gate, cancel, _) = support::channels(now).await;
        let proof = h.authority();
        let domain = proof.clock_domain();
        let calls = Arc::new(AtomicUsize::new(0));
        let target = Target {
            host: support::host(),
            session: [2; 16],
            run: support::scope().run,
            generation: 3,
            incarnation: 11,
        };
        let dir = tempfile::tempdir().unwrap();
        let (writer, _output, peer) = LocalWriter::start_guarded_with(
            Journal::open(&dir.path().join("input.sqlite")).unwrap(),
            std::io::sink(),
            target,
            support::device(),
            9,
            DigestKey::from_backend_bytes([0x5a; 32]),
            (
                ControlStart::new(
                    ViewStart {
                        terminal: CanonicalStart::new(3, 10, &HostViewBudget::new()).unwrap(),
                        binding: proof.binding_id(),
                        minor: 1,
                    },
                    Arc::new(move || domain.now_ms().map_err(|_| Reject::Policy)),
                    control::GrantContext {
                        challenge_ms: proof.created_host_ms(),
                        expires_host_ms: proof.expires_host_ms(),
                        grant: proof.grant_id(),
                        lease: [8; 16],
                        fence: 4,
                        policy: 13,
                    },
                )
                .with_authority(Some(proof.clone())),
                Some(resize::State::fake_durable(Fake(calls.clone()))),
            ),
        )
        .unwrap();
        let frame = peer
            .capture()
            .unwrap()
            .wait(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let stamp = frame.issued_stamp().unwrap();
        let state = State {
            generation: 3,
            fence: 4,
            mode: stamp.mode_epoch.parse().unwrap(),
            dimension: stamp.dimension_epoch.parse().unwrap(),
            sync: stamp.sync_epoch.parse().unwrap(),
        };
        drop(frame);
        let request = Request {
            kind: Kind::Resize,
            operation: [1; 16],
            sequence: 1,
            state,
            size: Some((4, 12)),
        };
        let epochs = cc_desk_remote_terminal::TerminalEpochs::default();
        let current = crate::lifecycle_resize_completion::Fact {
            request: Request {
                size: Some((3, 10)),
                ..request.clone()
            },
            resource: crate::lifecycle_router::ResourceIdentity::from_proof(&proof),
            backend: [6; 32],
            digest: [7; 32],
            before_size: (3, 10),
            after_size: (3, 10),
            before_terminal: epochs,
            after_terminal: epochs,
            after: state,
            outcome: crate::lifecycle_resize_completion::Outcome::Applied,
        };
        let result_dir = dir.path().join("result");
        std::fs::create_dir(&result_dir).unwrap();
        let owner = crate::lifecycle_resize_durable::Owner::fake(
            crate::lifecycle_journal::Store::open_resize_candidate(
                &result_dir.join("resize.sqlite"),
            )
            .unwrap(),
            DigestKey::from_backend_bytes([0x5a; 32]),
            proof,
            current,
            true,
        )
        .unwrap();
        Self {
            coordinator: resize::durable::Coordinator::fake(peer, owner),
            writer,
            calls,
            request,
            _dir: dir,
            c,
            h,
            _gate: gate,
            _cancel: cancel,
        }
    }
    fn begin(&mut self) {
        assert!(
            self.coordinator
                .begin(self.request.clone(), Duration::from_secs(1))
                .unwrap()
                .is_none()
        );
    }
    fn complete(&mut self) -> crate::lifecycle_resize_completion::Fact {
        self.begin();
        let f = self.coordinator.execute().unwrap();
        assert_eq!(self.coordinator.record().unwrap(), f);
        self.coordinator.publish().unwrap();
        f
    }
    async fn close(mut self) {
        self.writer.seal();
        let end = std::time::Instant::now() + Duration::from_secs(1);
        while !self.writer.try_join().unwrap() {
            assert!(std::time::Instant::now() < end);
            std::thread::yield_now();
        }
        self.h.close().await;
        self.c.close().await;
    }
}
#[tokio::test]
async fn DurableResizeActor_StaleSenderHasNoEffect_002() {
    let mut f = Fixture::new().await;
    f.begin();
    f.coordinator.owner().cancel();
    assert!(f.coordinator.execute().is_err());
    let calls = f.calls.load(Ordering::Acquire);
    f.close().await;
    assert_eq!(
        calls, 0,
        "cancelled accepted Sender must reject before backend"
    );
}
#[tokio::test]
async fn DurableResizeActor_LateCancelPreservesPublishedFact_003() {
    let mut f = Fixture::new().await;
    let fact = f.complete();
    f.coordinator.cancel();
    let result = f.coordinator.owner().result();
    f.close().await;
    assert_eq!(result.unwrap(), Some(fact));
}
#[tokio::test]
async fn DurableResizeActor_CloseBeforePublicationRejects_004() {
    let mut f = Fixture::new().await;
    f.begin();
    f.coordinator.execute().unwrap();
    f.coordinator.record().unwrap();
    f.coordinator.peer().close();
    let result = f.coordinator.publish();
    f.close().await;
    assert!(result.is_err());
}
#[tokio::test]
async fn DurableResizeActor_NoRecordCannotPublish_005() {
    let mut f = Fixture::new().await;
    f.begin();
    f.coordinator.execute().unwrap();
    assert!(f.coordinator.publish().is_err());
    f.close().await;
}
#[tokio::test]
async fn DurableResizeActor_PublishedSenderIsRevoked_006() {
    let mut f = Fixture::new().await;
    f.begin();
    let sender = f.coordinator.test_sender();
    let fact = f.coordinator.execute().unwrap();
    f.coordinator.record().unwrap();
    f.coordinator.publish().unwrap();
    let late = sender.complete(fact);
    f.close().await;
    assert!(
        late.is_err(),
        "publication must retire old completion ability"
    );
}
#[tokio::test]
async fn DurableResizeActor_RawPublicationRequiresDurableWitness_007() {
    let mut f = Fixture::new().await;
    f.begin();
    let p = f.coordinator.test_prepared();
    let fact = f.coordinator.execute().unwrap();
    resize::durable::ready(&p, fact.clone())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    let raw = f.coordinator.owner().publication_witness(&fact);
    f.close().await;
    assert!(
        raw.is_err(),
        "actor result alone is not a durable publication witness"
    );
}
#[tokio::test]
async fn DurableResizeActor_SameFactWrongAcceptedSlotHasNoEffect_008() {
    let mut a = Fixture::new().await;
    let mut b = Fixture::new().await;
    a.begin();
    b.begin();
    assert_eq!(
        a.coordinator.test_prepared().fact,
        b.coordinator.test_prepared().fact
    );
    a.coordinator
        .test_replace_sender(b.coordinator.test_sender());
    let result = a.coordinator.execute();
    let calls = a.calls.load(Ordering::Acquire);
    a.close().await;
    b.close().await;
    assert!(result.is_err());
    assert_eq!(calls, 0);
}
#[tokio::test]
async fn DurableResizeActor_SameFactWrongDurableWitnessRejected_009() {
    let mut a = Fixture::new().await;
    let mut b = Fixture::new().await;
    a.begin();
    b.begin();
    let af = a.coordinator.execute().unwrap();
    let bf = b.coordinator.execute().unwrap();
    assert_eq!(af, bf);
    b.coordinator.record().unwrap();
    let witness = b.coordinator.owner().publication_witness(&bf).unwrap();
    let p = a.coordinator.test_prepared();
    resize::durable::ready(&p, af)
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    let result = resize::durable::publish(&p, witness)
        .unwrap()
        .wait()
        .unwrap();
    a.close().await;
    b.close().await;
    assert!(
        result.is_err(),
        "identical bytes cannot substitute another accepted slot"
    );
}
#[tokio::test]
async fn DurableResizeActor_CloseAndPublicationShareOneGate_010() {
    let f = Fixture::new().await;
    let scope = f.coordinator.peer().control.scope.clone();
    for writer_close in [false, true] {
        let control = PeerControl::new(
            scope.target.clone(),
            scope.device,
            scope.binding,
            scope.minor,
        )
        .unwrap();
        let scope = control.scope.clone();
        let guard = scope.publication_gate.lock().unwrap();
        let (start_tx, start_rx) = std::sync::mpsc::sync_channel(1);
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        let closing = scope.clone();
        let closer = std::thread::spawn(move || {
            start_tx.send(()).unwrap();
            if writer_close {
                closing.close_writer();
            } else {
                closing.close();
            }
            done_tx.send(()).unwrap();
        });
        start_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let blocked = matches!(
            done_rx.recv_timeout(Duration::from_millis(30)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        );
        drop(guard);
        if blocked {
            done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        closer.join().unwrap();
        assert!(
            blocked,
            "close cannot cross an in-flight guarded publication decision"
        );
        assert!(scope.with_publication(|| Ok(())).is_err());
    }
    f.close().await;
}
#[tokio::test]
async fn DurableResizeActor_PublishedReplyNotObservedKeepsKnownFact_011() {
    let mut f = Fixture::new().await;
    f.begin();
    let fact = f.coordinator.execute().unwrap();
    f.coordinator.record().unwrap();
    let p = f.coordinator.test_prepared();
    resize::durable::ready(&p, fact.clone())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    let witness = f.coordinator.owner().publication_witness(&fact).unwrap();
    // The actor publishes, but the coordinator has not run its local confirmation.
    resize::durable::publish(&p, witness)
        .unwrap()
        .wait()
        .unwrap()
        .unwrap();
    f.coordinator.cancel();
    let result = f.coordinator.owner().result();
    let next = Request {
        operation: [2; 16],
        sequence: 2,
        state: fact.after,
        size: Some((5, 13)),
        ..f.request.clone()
    };
    let next_accepted = f.coordinator.begin(next, Duration::from_secs(1));
    f.close().await;
    assert_eq!(result.unwrap(), Some(fact));
    assert!(
        matches!(next_accepted, Ok(None)),
        "published slot must retire even when publication confirmation was interrupted"
    );
}
#[tokio::test]
async fn DurableResizeActor_PrepareDoesNotExecuteOrCapture_001() {
    let now = Arc::new(AtomicU64::new(1000));
    let (c, h, _grant_gate, _cancel, _) = support::channels(now).await;
    let proof = h.authority();
    let domain = proof.clock_domain();
    let calls = Arc::new(AtomicUsize::new(0));
    let target = Target {
        host: support::host(),
        session: [2; 16],
        run: support::scope().run,
        generation: 3,
        incarnation: 11,
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut writer, _output, peer) = LocalWriter::start_guarded_with(
        Journal::open(&dir.path().join("input.sqlite")).unwrap(),
        std::io::sink(),
        target,
        [support::device()][0],
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        (
            ControlStart::new(
                ViewStart {
                    terminal: CanonicalStart::new(3, 10, &HostViewBudget::new()).unwrap(),
                    binding: proof.binding_id(),
                    minor: 1,
                },
                Arc::new(move || domain.now_ms().map_err(|_| Reject::Policy)),
                control::GrantContext {
                    challenge_ms: proof.created_host_ms(),
                    expires_host_ms: proof.expires_host_ms(),
                    grant: proof.grant_id(),
                    lease: [8; 16],
                    fence: 4,
                    policy: 13,
                },
            )
            .with_authority(Some(proof.clone())),
            Some(resize::State::fake_durable(Fake(calls.clone()))),
        ),
    )
    .unwrap();
    let frame = peer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let stamp = frame.issued_stamp().unwrap();
    let request = Request {
        kind: Kind::Resize,
        operation: [1; 16],
        sequence: 1,
        state: State {
            generation: 3,
            fence: 4,
            mode: stamp.mode_epoch.parse().unwrap(),
            dimension: stamp.dimension_epoch.parse().unwrap(),
            sync: stamp.sync_epoch.parse().unwrap(),
        },
        size: Some((4, 12)),
    };
    drop(frame);
    let pending = peer
        .prepare_durable_resize(resize::durable::Prepare {
            request: request.clone(),
            proof,
            backend: [6; 32],
            digest: [7; 32],
            end: std::time::Instant::now() + Duration::from_secs(1),
        })
        .unwrap();
    let result = pending.wait().unwrap();
    writer.seal();
    let end = std::time::Instant::now() + Duration::from_secs(1);
    while !writer.try_join().unwrap() {
        assert!(std::time::Instant::now() < end);
        std::thread::yield_now();
    }
    assert!(
        result.is_ok(),
        "actor must reserve before durable acceptance"
    );
    let prepared = result.ok().unwrap();
    assert_eq!(prepared.fact.request, request);
    assert_eq!(prepared.fact.before_size, (3, 10));
    assert_eq!(prepared.fact.request.state.sync, 1);
    assert_eq!(calls.load(Ordering::Acquire), 0);
    h.close().await;
    c.close().await;
}
