use super::*;

#[test]
fn native_ingress_continuity_partial_and_unknown_still_seal_original_writer() {
    struct Fault {
        kind: u8,
        writes: Arc<std::sync::Mutex<Vec<u8>>>,
    }
    impl std::io::Write for Fault {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.kind == 1 && self.writes.lock().unwrap().is_empty() {
                self.writes.lock().unwrap().push(b[0]);
                return Ok(1);
            }
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for kind in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut producer = NativeRunProducer::start(
            Journal::open(&dir.path().join("native.sqlite")).unwrap(),
            Fault {
                kind,
                writes: writes.clone(),
            },
            "TEST_ONLY_INGRESS_TRUE_FAILURE".into(),
            3,
            3,
            10,
        )
        .unwrap();
        let input = producer.local_input();
        let mut reader = producer.take_output().unwrap();
        reader.feed(if kind == 0 { 1 } else { 0 }, b"\x1b[6n");
        let end = std::time::Instant::now() + Duration::from_secs(1);
        while input.canonical_status() != crate::native_host::NativeCanonicalStatus::Unavailable
            && std::time::Instant::now() < end
        {
            std::thread::yield_now();
        }
        reader.feed(4, b"next");
        let mode = input.canonical_status();
        let rejected = input.commit(b"MUST_NOT_WRITE", || true);
        producer.stop();
        let retired = producer.join(Duration::from_secs(1));
        assert!(retired.is_ok());
        assert_eq!(mode, crate::native_host::NativeCanonicalStatus::Unavailable);
        assert_eq!(rejected, Err(Reject::Sealed));
        assert_eq!(writes.lock().unwrap().len(), usize::from(kind == 1));
    }
}

#[tokio::test]
async fn native_ingress_second_feed_keeps_original_worker_after_local_only_publication() {
    struct Sink(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    struct Release(std::sync::mpsc::SyncSender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.try_send(());
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut producer = NativeRunProducer::start(
        Journal::open(&dir.path().join("native.sqlite")).unwrap(),
        Sink(writes.clone()),
        "TEST_ONLY_INGRESS_INTERLEAVING".into(),
        3,
        3,
        10,
    )
    .unwrap();
    let original = producer.writer.worker.as_ref().unwrap().thread().id();
    let input = producer.local_input();
    let mut reader = producer.take_output().unwrap();
    let scope = producer.native_scope();
    let (c, h, _gate, _cancel) = peer_channels_scope(Arc::new(|| Ok(1000)), scope).await;
    let h = Arc::new(h);
    let mut owner =
        WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
    owner.principal = Some(h.authority().principal_id());
    let mut host = NativeHost::from_owner(owner);
    host.anchor = Some(h.clone());
    let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
    let (release, release_rx) = std::sync::mpsc::sync_channel(1);
    let _release = Release(release.clone());
    host.adopt_native_producer(
        producer,
        h.clone(),
        Box::new(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).is_ok()
        }),
    )
    .unwrap_or_else(|_| panic!("verified original admission"));
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    let bytes = b"\x1b]52;c;x\x07";
    reader.feed(0, bytes);
    let barrier_result = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let recorded = barrier_result.clone();
    let barrier = input.clone();
    reader.test_after_initial_load(move || {
        release.send(()).unwrap();
        // This actual no-effect request follows the original OutputRequest in
        // the same actor and physical worker, proving its result was published.
        recorded.store(
            barrier.commit(b"DENIED_BARRIER", || false) == Err(Reject::Policy),
            Ordering::Release,
        );
    });
    reader.feed(bytes.len() as u64, b"next native read");
    let mode = input.canonical_status();
    let revoked = h.authority().check().is_err();
    let same = host.owner.active[0]
        .writer
        .worker
        .as_ref()
        .unwrap()
        .thread()
        .id()
        == original;
    let protocol = input.protocol_from_source(b"DESKTOP_REPLY", Some(bytes.len() as u64), || true);
    let local = input.commit(b"local", || true);
    host.begin_shutdown();
    host.retire_peer_installs().await;
    let end = Instant::now() + Duration::from_secs(2);
    while host.observe_shutdown() == WriterRetirement::Pending {
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    let retired = host.observe_shutdown();
    host.network_retirement().unwrap().wait().await;
    let not_ready = c.client_installed().await.is_err();
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
    // All owned threads/channels are retired before the behavioral RED assertion.
    assert_eq!(retired, WriterRetirement::Joined);
    assert!(barrier_result.load(Ordering::Acquire));
    assert_eq!(mode, crate::native_host::NativeCanonicalStatus::LocalOnly);
    assert!(same && revoked && not_ready);
    assert!(protocol.is_ok() && local.is_ok());
    assert_eq!(writes.lock().unwrap().as_slice(), b"DESKTOP_REPLYlocal");
}

#[test]
fn native_projection_fault_preserves_authorized_local_io_without_duplicate_reply() {
    struct Sink(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut producer = NativeRunProducer::start(
        Journal::open(&dir.path().join("native.sqlite")).unwrap(),
        Sink(writes.clone()),
        "TEST_ONLY_NATIVE_PROJECTION_FAULT".into(),
        3,
        3,
        10,
    )
    .unwrap();
    let input = producer.local_input();
    let mut reader = producer.take_output().unwrap();
    let bytes = b"\x1b[6n\x1b]52;c;x\x07";
    reader.feed(0, bytes);
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while input.canonical_status() == crate::native_host::NativeCanonicalStatus::Complete
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    let mode = input.canonical_status();
    let protocol = input.protocol_from_source(b"DESKTOP_REPLY", Some(bytes.len() as u64), || true);
    let local = input.commit(b"local", || true);
    producer.stop();
    producer.join(Duration::from_secs(1)).unwrap();
    assert_eq!(mode, crate::native_host::NativeCanonicalStatus::LocalOnly);
    assert!(protocol.is_ok());
    assert!(local.is_ok());
    assert_eq!(writes.lock().unwrap().as_slice(), b"DESKTOP_REPLYlocal");
}

#[test]
fn native_local_resize_preserves_full_size_and_cuts_delayed_original_reply() {
    use crate::native_host::{
        NativeFrameResult, NativeOriginalIo, NativeProtocolResult, NativeWriterRetirement,
    };
    struct Original {
        writes: Arc<std::sync::Mutex<Vec<u8>>>,
        size: Arc<std::sync::Mutex<(u16, u16)>>,
    }
    impl NativeOriginalIo for Original {
        fn frame(
            &mut self,
            bytes: &[u8],
            authorize: Box<dyn FnOnce() -> bool + Send>,
        ) -> Result<NativeFrameResult, Reject> {
            if !authorize() {
                return Err(Reject::Policy);
            }
            self.writes.lock().unwrap().extend_from_slice(bytes);
            Ok(NativeFrameResult {
                complete: true,
                confirmed_bytes: bytes.len() as u64,
            })
        }
        fn resize(
            &mut self,
            rows: u16,
            columns: u16,
            authorize: Box<dyn FnOnce() -> bool + Send>,
        ) -> Result<(u16, u16), Reject> {
            if !authorize() {
                return Err(Reject::Policy);
            }
            *self.size.lock().unwrap() = (rows, columns);
            Ok(*self.size.lock().unwrap())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let size = Arc::new(std::sync::Mutex::new((3, 10)));
    let mut producer = NativeRunProducer::start_resource(
        Journal::open(&dir.path().join("native.sqlite")).unwrap(),
        Original {
            writes: writes.clone(),
            size: size.clone(),
        },
        "TEST_ONLY_SOURCE_CUT".into(),
        3,
        3,
        10,
    )
    .unwrap();
    producer.output(0, b"\x1b[6n".to_vec()).unwrap();
    let input = producer.local_input();
    assert_eq!(
        input
            .protocol_from_source(b"OLD", Some(4), || true)
            .unwrap(),
        NativeProtocolResult::NativeOwned
    );
    input.resize(101, 500, || true).unwrap();
    assert_eq!(*size.lock().unwrap(), (101, 500));
    assert!(!producer.complete_from_zero());
    let delayed = input.protocol_from_source(b"OLD", Some(4), || true);
    let fresh = input.protocol_from_source(b"NEW", Some(5), || true);
    let missing = input.protocol_from_source(b"MISSING", None, || true);
    let denied = input.protocol_from_source(b"DENIED", Some(6), || false);
    input.commit(b"local", || true).unwrap();
    producer.stop();
    producer.join(Duration::from_secs(1)).unwrap();
    assert_eq!(input.retirement(), NativeWriterRetirement::Joined);
    assert_eq!(delayed.unwrap(), NativeProtocolResult::NativeOwned);
    assert!(matches!(
        fresh,
        Ok(NativeProtocolResult::Written(NativeFrameResult {
            complete: true,
            confirmed_bytes: 3
        }))
    ));
    assert_eq!(missing, Err(Reject::Policy));
    assert_eq!(denied, Err(Reject::Policy));
    assert_eq!(writes.lock().unwrap().as_slice(), b"\x1b[1;1RNEWlocal");
}

#[test]
fn native_blocked_original_write_still_processes_canonical_output() {
    struct Blocked {
        entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>,
    }
    impl std::io::Write for Blocked {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
    let (release, release_rx) = std::sync::mpsc::sync_channel(1);
    let mut producer = NativeRunProducer::start(
        Journal::open(&dir.path().join("native.sqlite")).unwrap(),
        Blocked {
            entered: entered_tx,
            release: release_rx,
        },
        "TEST_ONLY_BLOCKED_ORIGINAL".into(),
        3,
        3,
        10,
    )
    .unwrap();
    let input = producer.local_input();
    let original = std::thread::spawn(move || input.commit(b"x", || true));
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    let pending = producer.test_output(0, b"abc".to_vec()).unwrap();
    let before_release = pending.wait(Duration::from_millis(100));
    // Release and join the original owned writer even when the assertion is RED.
    release.send(()).unwrap();
    assert_eq!(original.join().unwrap().unwrap().confirmed_bytes, 1);
    let snapshot = producer
        .writer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.through_offset(), 3);
    assert_eq!(snapshot.view().cell(0, 2).unwrap().scalar, 'c');
    producer.stop();
    producer.join(Duration::from_secs(1)).unwrap();
    assert!(
        matches!(before_release, Ok(OutputResult::Processed { replies: 0 })),
        "canonical output stalled behind original native IO: {before_release:?}"
    );
}

#[test]
fn native_producer_before_peer_preserves_complete_canonical_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut producer = NativeRunProducer::start(
        Journal::open(&dir.path().join("native.sqlite")).unwrap(),
        Vec::<u8>::new(),
        "TEST_ONLY_NATIVE_SOURCE".into(),
        3,
        3,
        10,
    )
    .unwrap();
    producer.output(0, b"abc".to_vec()).unwrap();
    let captured = producer
        .writer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!((captured.view().rows(), captured.view().columns()), (3, 10));
    assert_eq!(captured.through_offset(), 3);
    assert_eq!(captured.view().cell(0, 0).unwrap().scalar, 'a');
    assert_eq!(captured.view().cell(0, 2).unwrap().scalar, 'c');
    assert!(producer.output(4, b"gap".to_vec()).is_err());
    producer.stop();
    assert!(producer.join(Duration::from_secs(1)).is_ok());
}
use crate::bound_test_support as support;
use cc_desk_remote_record::control_io as wire;
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use cc_desk_remote_security::*;
use std::sync::Arc;
const BUDGET: Duration = Duration::from_secs(1);
#[path = "../../security/tests/support/identities.rs"]
#[allow(dead_code)]
#[allow(clippy::duplicate_mod)]
// The independent peer fixture uses the existing public TEST_ONLY identities.
mod identities;
struct PublicSigner;
impl NativeGrantSigner for PublicSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        request: &'a VerifiedCanonicalGrant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>
    {
        Box::pin(async move { Ok(support::signed(request.payload(), request.issuer_kid())) })
    }
}
async fn peer_channels(
    clock: Arc<dyn GrantClock>,
) -> (RunningChannel, RunningChannel, GrantGate, CancelHandle) {
    peer_channels_scope(clock, support::scope()).await
}
async fn peer_channels_scope(
    clock: Arc<dyn GrantClock>,
    scope: cc_desk_remote_record::grant::SessionScope,
) -> (RunningChannel, RunningChannel, GrantGate, CancelHandle) {
    let local = |client| {
        let (k, p) = identities::identity(client, false);
        LocalIdentity::new(
            if client { Role::Client } else { Role::Host },
            P256Spki::new(&p).unwrap(),
            k.key.clone(),
        )
        .unwrap()
    };
    let pin = |client| {
        let (_, p) = identities::identity(client, false);
        PeerPin::new(
            if client { Role::Client } else { Role::Host },
            P256Spki::new(&p).unwrap(),
        )
    };
    let (c, h) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(c),
            local(true),
            pin(false),
            BUDGET,
            token.clone()
        ),
        accept(Unbound::new(h), local(false), pin(true), BUDGET, token)
    );
    let origin = CanonicalOrigin::trusted("wss://relay.invalid/").unwrap();
    let (c, h) = tokio::join!(
        bind_client_peer_channel(
            c.unwrap(),
            ClientTrust {
                host_key_epoch: 5,
                client_key_epoch: 7,
                origin: origin.clone()
            }
        ),
        bind_host_peer_channel(
            h.unwrap(),
            HostSnapshot {
                host_key_epoch: 5,
                client_key_epoch: 7,
                activation: [0x33; 32],
                incarnation: 11,
                policy_epoch: 13,
                issuer_epoch: 17,
                origin
            }
        )
    );
    let (c, h) = (c.unwrap(), h.unwrap());
    let mut gate =
        GrantGate::for_binding(h.context(), support::admin(), [4; 32], clock.clone()).unwrap();
    let mut state = ClientGrantState::new(clock);
    let mut signer = PublicSigner;
    let trust = ClientGrantTrust {
        issuer: support::admin(),
        principal: [4; 32],
        scope: scope.clone(),
        role: DataRole::Controller,
    };
    let policy = NativeGrantPolicy {
        scope,
        role: DataRole::Controller,
        duration: Duration::from_secs(10),
    };
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, BUDGET),
        h.grant_host(&mut gate, policy, BUDGET)
    );
    let (mut c, mut h) = (c.unwrap(), h.unwrap());
    (
        c.start_peer_pending(Instant::now() + BUDGET).unwrap(),
        h.start_peer_pending(Instant::now() + BUDGET).unwrap(),
        gate,
        cancel,
    )
}

#[tokio::test]
async fn native_producer_verified_install_keeps_original_prefix_and_denies_wrong_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut producer = NativeRunProducer::start(
        Journal::open(&dir.path().join("native.sqlite")).unwrap(),
        Vec::<u8>::new(),
        "TEST_ONLY_NATIVE_SOURCE".into(),
        3,
        3,
        10,
    )
    .unwrap();
    producer.output(0, b"abc".to_vec()).unwrap();
    let original_worker = producer.writer.worker.as_ref().unwrap().thread().id();
    let original_seal = producer.writer.handle.sealed.clone();
    let scope = producer.native_scope();
    let (c, h, _gate, _cancel) = peer_channels_scope(Arc::new(|| Ok(1000)), scope.clone()).await;
    let h = Arc::new(h);
    let wrong_dir = tempfile::tempdir().unwrap();
    let wrong = NativeRunProducer::start(
        Journal::open(&wrong_dir.path().join("wrong.sqlite")).unwrap(),
        Vec::<u8>::new(),
        "TEST_ONLY_WRONG_NATIVE_RESOURCE".into(),
        3,
        3,
        10,
    )
    .unwrap();
    let wrong_worker = wrong.writer.worker.as_ref().unwrap().thread().id();
    assert!(
        NativeHost::for_native_resource(h.clone(), &wrong).is_err(),
        "wrong resource must reject before consuming permanent process admission"
    );
    let mut host = NativeHost::for_native_resource(h.clone(), &producer).unwrap();
    let mut wrong = match host.adopt_native_producer(wrong, h.clone(), Box::new(|| true)) {
        Err(rejected) => rejected.1,
        Ok(_) => panic!("wrong verified resource scope must reject before custody moves"),
    };
    assert_eq!(
        wrong.writer.worker.as_ref().unwrap().thread().id(),
        wrong_worker
    );
    wrong.stop();
    wrong.join(BUDGET).unwrap();
    assert!(
        h.authority().check().is_ok(),
        "the SAME Host/channel must remain available for legitimate retry"
    );
    host.adopt_native_producer(producer, h.clone(), Box::new(|| true))
        .unwrap_or_else(|_| panic!("actual original producer must be installed"));
    let end = Instant::now() + BUDGET;
    while !matches!(host.peer_installs[0].stage, Stage::Done(_)) {
        host.drive_peer_installs();
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    let proof = c.client_installed().await.unwrap();
    assert_eq!(proof.consume_for(&c).unwrap(), 1);
    assert_eq!(host.attempts.len(), 1);
    assert!(matches!(
        host.attempts[0].state,
        InstallState::Installed { fence: 1, .. }
    ));
    assert_eq!(
        host.owner.active[0]
            .writer
            .worker
            .as_ref()
            .unwrap()
            .thread()
            .id(),
        original_worker
    );
    assert!(Arc::ptr_eq(
        &host.owner.active[0].writer.handle.sealed,
        &original_seal
    ));
    let InstallState::Installed { peer, .. } = &host.attempts[0].state else {
        panic!("installed peer");
    };
    let prefix = peer.capture().unwrap().wait(BUDGET).unwrap().unwrap();
    assert_eq!(prefix.through_offset(), 3);
    assert_eq!(prefix.view().cell(0, 0).unwrap().scalar, 'a');
    assert_eq!(prefix.view().cell(0, 2).unwrap().scalar, 'c');
    host.begin_shutdown();
    host.retire_peer_installs().await;
    while host.observe_shutdown() == WriterRetirement::Pending {
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    assert_eq!(host.observe_shutdown(), WriterRetirement::Joined);
    host.network_retirement().unwrap().wait().await;
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
}

#[tokio::test]
async fn native_rebind_cannot_revive_local_only_and_stop_closes_current_authority() {
    for fault in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut producer = NativeRunProducer::start(
            Journal::open(&dir.path().join("native.sqlite")).unwrap(),
            Vec::<u8>::new(),
            "TEST_ONLY_NATIVE_REBIND".into(),
            3,
            3,
            10,
        )
        .unwrap();
        let input = producer.local_input();
        let mut output = producer.take_output().unwrap();
        let scope = producer.native_scope();
        let clock: Arc<dyn GrantClock> = Arc::new(|| Ok(1000));
        let (c, h, _gate, _cancel) = peer_channels_scope(clock.clone(), scope.clone()).await;
        let h = Arc::new(h);
        // Supplemental actor scenarios use the existing isolated owner fixture.
        // The public permanent process-admission factory is proven once above.
        let mut owner =
            WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
        owner.principal = Some(h.authority().principal_id());
        let mut host = NativeHost::from_owner(owner);
        host.anchor = Some(h.clone());
        host.adopt_native_producer(producer, h.clone(), Box::new(|| true))
            .unwrap_or_else(|_| panic!("native initial install"));
        let end = Instant::now() + Duration::from_secs(3);
        while !matches!(host.peer_installs[0].stage, Stage::Done(_)) {
            host.drive_peer_installs();
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        assert_eq!(
            c.client_installed().await.unwrap().consume_for(&c).unwrap(),
            1
        );
        let target = host.owner.active[0].target.clone();
        if fault {
            output.feed(0, b"\x1b]52;c;x\x07");
            while input.canonical_status() == crate::native_host::NativeCanonicalStatus::Complete
                || h.authority().check().is_ok()
            {
                assert!(Instant::now() < end);
                tokio::task::yield_now().await;
            }
            assert_eq!(
                input.canonical_status(),
                crate::native_host::NativeCanonicalStatus::LocalOnly
            );
        }
        let (c2, h2, _gate2, _cancel2) = peer_channels_scope(clock, scope).await;
        let h2 = Arc::new(h2);
        assert!(
            h2.authority().check().is_ok(),
            "fresh verified grant must be live before refusal"
        );
        host.begin_peer_rebind(target, support::device(), 1, h2.clone(), [9; 16])
            .unwrap();
        while !matches!(host.peer_installs[1].stage, Stage::Done(_) | Stage::Failed) {
            host.drive_peer_installs();
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        if fault {
            assert!(matches!(host.peer_installs[1].stage, Stage::Failed));
            assert!(c2.client_installed().await.is_err());
            assert!(input.commit(b"local", || true).unwrap().complete);
        } else {
            assert_eq!(
                c2.client_installed()
                    .await
                    .unwrap()
                    .consume_for(&c2)
                    .unwrap(),
                2
            );
            input.stop();
            assert!(
                h2.authority().check().is_err(),
                "Stop must close the CURRENT original authority synchronously"
            );
            assert!(h.authority().check().is_err(), "old scope cannot revive");
        }
        host.begin_shutdown();
        host.retire_peer_installs().await;
        while host.observe_shutdown() == WriterRetirement::Pending {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        assert_eq!(host.observe_shutdown(), WriterRetirement::Joined);
        host.network_retirement().unwrap().wait().await;
        let _ = tokio::join!(
            c.wait_closed(),
            h.wait_closed(),
            c2.wait_closed(),
            h2.wait_closed()
        );
    }
}

#[tokio::test]
async fn peer_install_coordinator_original_native_pending_success_cancel_and_cas_rejection() {
    coordinator_scenarios(0..6).await;
}

#[tokio::test]
async fn peer_install_cancel_before_original_success_delivery_never_revives_ready() {
    coordinator_scenarios(6..7).await;
}

async fn coordinator_scenarios(modes: std::ops::Range<u8>) {
    for mode in modes {
        let trace_start = std::time::Instant::now();
        let clock: Arc<dyn GrantClock> = Arc::new(|| Ok(1000));
        let (mut old_c, mut old_h, _cancel) = support::pair().await;
        let mut old_gate =
            GrantGate::for_binding(old_h.context(), support::admin(), [4; 32], clock.clone())
                .unwrap();
        let challenge = old_gate
            .challenge(
                support::scope(),
                DataRole::Controller,
                Duration::from_secs(10),
            )
            .unwrap();
        let grant = old_gate
            .accept(&support::signed(&challenge, old_gate.issuer_kid()))
            .unwrap();
        old_c.install(&grant).unwrap();
        old_h.install(&grant).unwrap();
        let (old_c, old_h) = (old_c.start().unwrap(), old_h.start().unwrap());
        let (c, h, _new_gate, _new_cancel) = peer_channels(clock).await;
        let h = Arc::new(h);
        let mut owner =
            WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
        owner.principal = Some([4; 32]);
        let target = Target {
            host: support::host(),
            incarnation: 11,
            session: [2; 16],
            run: support::scope().run,
            generation: 3,
        };
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        struct Block(
            std::sync::mpsc::SyncSender<()>,
            std::sync::mpsc::Receiver<()>,
            Arc<std::sync::atomic::AtomicUsize>,
        );
        impl Write for Block {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                if self.2.fetch_add(1, std::sync::atomic::Ordering::AcqRel) == 0 {
                    self.0.send(()).unwrap();
                    self.1
                        .recv_timeout(Duration::from_secs(4))
                        .map_err(std::io::Error::other)?;
                }
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
        let (release, release_rx) = std::sync::mpsc::sync_channel(1);
        let dir = tempfile::tempdir().unwrap();
        let mut started = owner
            .start_authorized(
                Journal::open(&dir.path().join("peer.sqlite")).unwrap(),
                Block(entered_tx, release_rx, writes.clone()),
                SessionStart {
                    target: target.clone(),
                    device: support::device(),
                    writer_epoch: 9,
                    key: DigestKey::from_backend_bytes([0x5a; 32]),
                    rows: 3,
                    columns: 10,
                },
                old_h.authority(),
                [8; 16],
                4,
            )
            .unwrap();
        let output = started.output.feed_at(0, b"\x1b[c".to_vec()).unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut host = NativeHost::from_owner(owner);
        host.begin_peer_rebind(
            target.clone(),
            support::device(),
            if mode == 2 { 99 } else { 4 },
            h.clone(),
            [9; 16],
        )
        .unwrap();
        let end = Instant::now() + Duration::from_millis(700);
        loop {
            host.drive_peer_installs();
            if !host.attempts.is_empty() || matches!(host.peer_installs[0].stage, Stage::Failed) {
                break;
            }
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        if mode != 2 {
            assert_eq!(host.attempts.len(), 1);
            assert!(matches!(host.attempts[0].state, InstallState::Pending(_)));
        }
        let original = host.attempts.first().map(|attempt| attempt.id);
        let reply_barrier = (mode == 6)
            .then(|| view_gate::rebind::InstallReplyBarrier::arm(h.authority().binding_id(), 4));
        if mode == 1 {
            h.authority().request_close();
        }
        release.send(()).unwrap();
        assert!(matches!(
            output.wait(Duration::from_secs(1)).unwrap(),
            OutputResult::Processed { replies: 1 }
        ));
        let baseline_writes = writes.load(std::sync::atomic::Ordering::Acquire);
        if let Some(barrier) = &reply_barrier {
            let (native_effect, actual_fence) = barrier.installed();
            assert_eq!(
                actual_fence, 5,
                "fence must come from the mutated original actor"
            );
            assert!(barrier.published.lock().unwrap().is_none());
            assert!(matches!(host.attempts[0].state, InstallState::Pending(_)));
            let original_pending = match &host.attempts[0].state {
                InstallState::Pending(p) => p as *const _,
                _ => unreachable!(),
            };
            assert!(
                matches!(host.peer_installs[0].stage,Stage::Native{attempt,..} if Some(attempt)==original)
            );
            h.authority().request_close();
            assert!(h.authority().check().is_err());
            let cancel_observed = std::time::Instant::now();
            host.drive_peer_installs();
            assert!(matches!(host.peer_installs[0].stage, Stage::Failed));
            match &host.attempts[0].state {
                InstallState::Pending(p) => assert_eq!(p as *const _, original_pending),
                _ => panic!("cancel replaced original pending"),
            };
            assert_eq!(
                host.observe_install(original.unwrap()),
                InstallObservation::Unknown
            );
            assert!(matches!(host.attempts[0].state, InstallState::Pending(_)));
            assert!(host.peer_connection(&h).is_err());
            barrier.release();
            loop {
                NativeHost::poll(&mut host.attempts[0]);
                if matches!(
                    host.attempts[0].state,
                    InstallState::Installed { fence: 5, .. }
                ) {
                    break;
                }
                assert!(Instant::now() < end);
                tokio::task::yield_now().await;
            }
            let result_observed = std::time::Instant::now();
            let published = loop {
                if let Some(at) = *barrier.published.lock().unwrap() {
                    break at;
                }
                assert!(Instant::now() < end);
                tokio::task::yield_now().await;
            };
            assert!(
                native_effect < cancel_observed
                    && cancel_observed < published
                    && cancel_observed < result_observed
            );
            assert_eq!(host.attempts.len(), 1);
            assert_eq!(host.attempts[0].id, original.unwrap());
            assert_eq!(host.attempts[0].next_fence, actual_fence);
            for _ in 0..3 {
                host.drive_peer_installs();
                assert_eq!(
                    host.observe_install(original.unwrap()),
                    InstallObservation::Unknown
                );
            }
            assert!(matches!(host.peer_installs[0].stage, Stage::Failed));
            assert!(host.peer_connection(&h).is_err());
            assert!(h.installed_fence().is_err());
            assert!(c.client_installed().await.is_err());
            assert!(c.installed_fence().is_err());
            assert_eq!(
                c.send_control(&support::scope(), actual_fence, b"NEVER_REVIVE", BUDGET)
                    .await,
                Err(DataError::Closed)
            );
            assert_eq!(
                writes.load(std::sync::atomic::Ordering::Acquire),
                baseline_writes
            );
            eprintln!(
                "P06 original_attempt={} actual_actor_fence={} native_effect_us={} cancellation_observed_us={} original_reply_published_us={} original_result_observed_us={}",
                original.unwrap().0,
                actual_fence,
                native_effect.duration_since(trace_start).as_micros(),
                cancel_observed.duration_since(trace_start).as_micros(),
                published.duration_since(trace_start).as_micros(),
                result_observed.duration_since(trace_start).as_micros()
            );
        }
        if mode == 3 {
            // Observe the ORIGINAL native successful pending without advancing
            // coordinator transport; then close before any response can be spawned.
            loop {
                NativeHost::poll(&mut host.attempts[0]);
                if matches!(
                    host.attempts[0].state,
                    InstallState::Installed { fence: 5, .. }
                ) {
                    break;
                }
                assert!(Instant::now() < end);
                tokio::task::yield_now().await;
            }
            assert!(matches!(host.peer_installs[0].stage, Stage::Native { .. }));
            h.authority().request_close();
            host.drive_peer_installs();
            assert!(matches!(
                host.attempts[0].state,
                InstallState::Installed { fence: 5, .. }
            ));
            assert!(matches!(host.peer_installs[0].stage, Stage::Failed));
            assert_eq!(
                host.observe_install(host.attempts[0].id),
                InstallObservation::Unknown
            );
            assert!(h.installed_fence().is_err());
        }
        loop {
            host.drive_peer_installs();
            if matches!(host.peer_installs[0].stage, Stage::Done(_) | Stage::Failed) {
                break;
            }
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        if mode == 0 || mode == 4 || mode == 5 {
            let proof = c.client_installed().await.unwrap();
            assert_eq!(proof.consume_for(&c).unwrap(), 5);
            assert_eq!(h.installed_fence().unwrap(), 5);
            let mut connection = host.peer_connection(&h).unwrap();
            assert!(host.peer_connection(&h).is_err());
            if mode == 4 {
                assert_exact_snapshot_ack_window_and_input(&c, &h, &mut connection).await;
                assert_eq!(
                    writes.load(std::sync::atomic::Ordering::Acquire) - baseline_writes,
                    1,
                    "post-ACK input must write exactly once"
                );
            } else {
                // Request a window through the existing dispatcher; no snapshot ACK is synthesized.
                let input =
                    cc_desk_remote_record::input::encode(&cc_desk_remote_record::input::Input {
                        operation: [3; 16],
                        sequence: 1,
                        window: [9; 32],
                        not_after_host_ms: 2000,
                        bytes: b"DENY_BEFORE_ACK".to_vec(),
                    })
                    .unwrap();
                let bytes: &[u8] = if mode == 5 {
                    &input
                } else {
                    br#"{"message":"Window","profile":1,"request":"1"}"#
                };
                let scope = support::scope();
                let (send, record) = tokio::join!(
                    c.send_control(&scope, 5, bytes, BUDGET),
                    h.receive(&scope, 5, BUDGET)
                );
                send.unwrap();
                let record = record.unwrap();
                // This uses the ORIGINAL dispatcher and does not synthesize a snapshot ACK.
                assert_eq!(
                    connection.dispatch_received(&record, BUDGET).await,
                    Err(Reject::Sync)
                );
                assert_eq!(
                    writes.load(std::sync::atomic::Ordering::Acquire) - baseline_writes,
                    0,
                    "installation granted user-writer input without snapshot ACK"
                );
                assert!(
                    !matches!(
                        tokio::time::timeout(
                            Duration::from_millis(10),
                            c.receive(&support::scope(), 5, BUDGET)
                        )
                        .await,
                        Ok(Ok(_))
                    ),
                    "a WindowGranted or ACK was emitted without snapshot synchronization"
                );
            }
            drop(connection);
        } else {
            assert!(c.client_installed().await.is_err());
            assert!(h.installed_fence().is_err());
        }
        assert!(
            host.attempts.len() <= 1,
            "coordinator retried original pending"
        );
        h.authority().request_close();
        assert!(
            host.owner.capture(&target).is_ok(),
            "phone disconnect sealed writer"
        );
        host.retire_peer_installs().await;
        assert!(
            host.peer_installs
                .iter()
                .all(|job| matches!(job.stage, Stage::Failed))
        );
        host.begin_shutdown();
        let end = Instant::now() + Duration::from_secs(2);
        while host.observe_shutdown() == WriterRetirement::Pending {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        assert_eq!(host.observe_shutdown(), WriterRetirement::Joined);
        assert!(
            host.owner.active.is_empty(),
            "original writer actor was not actually reaped"
        );
        host.network_retirement().unwrap().wait().await;
        old_c.authority().request_close();
        old_h.authority().request_close();
        c.authority().request_close();
        let _ = tokio::join!(
            old_c.wait_closed(),
            old_h.wait_closed(),
            c.wait_closed(),
            h.wait_closed()
        );
    }
}

async fn dispatch_new_mode(
    c: &RunningChannel,
    h: &RunningChannel,
    connection: &mut NativeConnection,
    bytes: &[u8],
) -> Result<(), Reject> {
    c.send_control(&support::scope(), 5, bytes, BUDGET)
        .await
        .unwrap();
    let record = h.receive(&support::scope(), 5, BUDGET).await.unwrap();
    connection.dispatch_received(&record, BUDGET).await
}
async fn assert_exact_snapshot_ack_window_and_input(
    c: &RunningChannel,
    h: &RunningChannel,
    connection: &mut NativeConnection,
) {
    use cc_desk_remote_record::{input, projection as projection_wire};
    dispatch_new_mode(
        c,
        h,
        connection,
        br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
    )
    .await
    .unwrap();
    let mut cells = 0;
    let mut stamp = None;
    while cells < 30 {
        let record = c.receive(&support::scope(), 5, BUDGET).await.unwrap();
        let projection_wire::Content::SnapshotPart(part) =
            projection_wire::decode(record.payload(), 1)
                .unwrap()
                .content
        else {
            panic!("snapshot part expected")
        };
        assert_eq!(usize::from(part.start), cells);
        cells += part.cells.len();
        if let Some(previous) = &stamp {
            assert!(previous == &part.stamp);
        }
        stamp = Some(part.stamp);
    }
    assert_eq!(cells, 30);
    let stamp = stamp.unwrap();
    let ack = projection_wire::encode_ack(
        &projection_wire::Ack {
            profile: 1,
            stamp: stamp.clone(),
        },
        1,
    )
    .unwrap();
    dispatch_new_mode(c, h, connection, &ack).await.unwrap();
    let record = c.receive(&support::scope(), 5, BUDGET).await.unwrap();
    assert!(
        matches!(wire::decode(record.payload()).unwrap(),wire::Message::ProjectionAcknowledged{stamp:ack,..} if ack==stamp)
    );
    dispatch_new_mode(
        c,
        h,
        connection,
        br#"{"message":"Window","profile":1,"request":"2"}"#,
    )
    .await
    .unwrap();
    let record = c.receive(&support::scope(), 5, BUDGET).await.unwrap();
    let wire::Message::WindowGranted {
        request,
        window,
        not_after_host_ms,
        ..
    } = wire::decode(record.payload()).unwrap()
    else {
        panic!("window expected after exact ACK")
    };
    assert_eq!(request, "2");
    assert_eq!(window.len(), 64);
    let mut token = [0; 32];
    for (i, pair) in window.as_bytes().chunks_exact(2).enumerate() {
        token[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    let bytes = input::encode(&input::Input {
        operation: [2; 16],
        sequence: 1,
        window: token,
        not_after_host_ms: not_after_host_ms.parse().unwrap(),
        bytes: b"TEST_ONLY_AFTER_ACK".to_vec(),
    })
    .unwrap();
    dispatch_new_mode(c, h, connection, &bytes).await.unwrap();
    let record = c.receive(&support::scope(), 5, BUDGET).await.unwrap();
    assert!(
        matches!(wire::decode(record.payload()).unwrap(),wire::Message::InputReceipt{operation,sequence,outcome:wire::Outcome::HostWritten,proof:Some(_),..} if operation=="02".repeat(16)&&sequence=="1")
    );
}

#[tokio::test]
async fn peer_install_cancelled_retire_keeps_original_controlled_response_task_in_host() {
    let (c, h, _gate, _cancel) = peer_channels(Arc::new(|| Ok(1000))).await;
    let h = Arc::new(h);
    let request = h.host_install_request().await.unwrap();
    let mut owner =
        WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
    owner.principal = Some([4; 32]);
    let mut host = NativeHost::from_owner(owner);
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = finished.clone();
    let channel = h.clone();
    // Gate the original response future before transport completion, retaining its
    // actual request and original authority. No alternate success producer exists.
    let task = tokio::spawn(async move {
        wait.await.unwrap();
        let result = channel.complete_host_install(request, 7).await;
        done.store(true, std::sync::atomic::Ordering::Release);
        result
    });
    let task_id = task.id();
    host.peer_installs.push(Coordinator {
        native_attempt: None,
        target: Target {
            host: support::host(),
            incarnation: 11,
            session: [2; 16],
            run: support::scope().run,
            generation: 3,
        },
        device: support::device(),
        expected: 6,
        lease: [9; 16],
        channel: h.clone(),
        stage: Stage::Response {
            attempt: InstallAttempt(1),
            task,
        },
    });
    let mut retire = Box::pin(host.retire_peer_installs());
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(
            std::future::Future::poll(retire.as_mut(), cx).is_pending()
        ))
        .await
    );
    drop(retire);
    assert_eq!(host.peer_installs.len(), 1);
    match &host.peer_installs[0].stage {
        Stage::Response { task, .. } => {
            assert_eq!(task.id(), task_id);
            assert!(!task.is_finished());
        }
        _ => panic!("original response task lost"),
    }
    assert!(!finished.load(std::sync::atomic::Ordering::Acquire));
    assert!(h.authority().check().is_err());
    assert!(c.client_installed().await.is_err());
    release.send(()).unwrap();
    host.retire_peer_installs().await;
    assert!(finished.load(std::sync::atomic::Ordering::Acquire));
    assert!(matches!(host.peer_installs[0].stage, Stage::Failed));
    assert!(h.installed_fence().is_err());
    host.begin_shutdown();
    assert_eq!(host.observe_shutdown(), WriterRetirement::Joined);
    host.network_retirement().unwrap().wait().await;
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
}

#[tokio::test]
async fn native_remote_resize_reserves_on_original_canonical_actor() {
    use cc_desk_remote_record::lifecycle::{
        Kind, Request as ResizeOperation, State as ResizeState,
    };
    for mode in 0..7 {
        struct Original {
            calls: Arc<std::sync::Mutex<Vec<(u16, u16)>>>,
            mode: u8,
            entered: std::sync::mpsc::SyncSender<()>,
            release: std::sync::mpsc::Receiver<()>,
        }
        impl crate::native_host::NativeOriginalIo for Original {
            fn frame(
                &mut self,
                _: &[u8],
                authorize: Box<dyn FnOnce() -> bool + Send>,
            ) -> Result<crate::native_host::NativeFrameResult, Reject> {
                if !authorize() {
                    return Err(Reject::Policy);
                }
                Ok(crate::native_host::NativeFrameResult {
                    complete: true,
                    confirmed_bytes: 0,
                })
            }
            fn resize(
                &mut self,
                rows: u16,
                columns: u16,
                authorize: Box<dyn FnOnce() -> bool + Send>,
            ) -> Result<(u16, u16), Reject> {
                if !authorize() {
                    return Err(Reject::Policy);
                }
                self.calls.lock().unwrap().push((rows, columns));
                if matches!(self.mode, 3 | 5) {
                    self.entered.send(()).unwrap();
                    self.release
                        .recv_timeout(Duration::from_secs(3))
                        .map_err(|_| Reject::Thread)?;
                }
                Ok((rows, columns + u16::from(self.mode == 4)))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
        let (release, release_rx) = std::sync::mpsc::sync_channel(1);
        let mut producer = NativeRunProducer::start_resource(
            Journal::open(&dir.path().join("native.sqlite")).unwrap(),
            Original {
                calls: calls.clone(),
                mode,
                entered: entered_tx,
                release: release_rx,
            },
            "TEST_ONLY_NATIVE_REMOTE_RESIZE".into(),
            3,
            3,
            10,
        )
        .unwrap();
        let original = producer.writer.worker.as_ref().unwrap().thread().id();
        let scope = producer.native_scope();
        let (c, h, _gate, _cancel) =
            peer_channels_scope(Arc::new(|| Ok(1000)), scope.clone()).await;
        let proof = h.authority();
        let mut owner =
            WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
        owner.principal = Some(proof.principal_id());
        let mut session = [0; 16];
        for (i, pair) in scope.session.as_bytes().chunks_exact(2).enumerate() {
            session[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let target = Target {
            host: support::host(),
            incarnation: 11,
            session,
            run: scope.run,
            generation: 3,
        };
        let admission = owner
            .authorized_admission(&target, support::device(), &proof, [8; 16], 1)
            .unwrap();
        let peer = producer
            .bind(target, support::device(), admission, Box::new(|| true))
            .unwrap()
            .wait(BUDGET)
            .unwrap()
            .unwrap();
        let frame = peer.capture().unwrap().wait(BUDGET).unwrap().unwrap();
        let stamp = frame.issued_stamp().unwrap();
        let operation = ResizeOperation {
            kind: Kind::Resize,
            operation: [5; 16],
            sequence: 1,
            state: ResizeState {
                generation: 3,
                fence: 1,
                mode: stamp.mode_epoch.parse().unwrap(),
                dimension: stamp.dimension_epoch.parse().unwrap(),
                sync: stamp.sync_epoch.parse().unwrap(),
            },
            size: Some((4, 12)),
        };
        drop(frame);
        let epochs = cc_desk_remote_terminal::TerminalEpochs::default();
        let current = crate::lifecycle_resize_completion::Fact {
            request: ResizeOperation {
                size: Some((3, 10)),
                ..operation.clone()
            },
            resource: crate::lifecycle_router::ResourceIdentity::from_proof(&proof),
            backend: [6; 32],
            digest: [7; 32],
            before_size: (3, 10),
            after_size: (3, 10),
            before_terminal: epochs,
            after_terminal: epochs,
            after: operation.state,
            outcome: crate::lifecycle_resize_completion::Outcome::Applied,
        };
        let result_dir = dir.path().join("result");
        std::fs::create_dir(&result_dir).unwrap();
        let durable = crate::lifecycle_resize_durable::Owner::fake(
            crate::lifecycle_journal::Store::open_resize_candidate(
                &result_dir.join("resize.sqlite"),
            )
            .unwrap(),
            DigestKey::from_backend_bytes([0x5a; 32]),
            proof.clone(),
            current,
            true,
        )
        .unwrap();
        let mut coordinator =
            crate::local_writer::resize::durable::Coordinator::fake(peer, durable);
        let budget = if mode == 5 {
            Duration::from_millis(200)
        } else {
            BUDGET
        };
        let result = coordinator.begin(operation.clone(), budget);
        let reentry = if mode == 6 && result.is_ok() {
            Some(coordinator.reservation().test_cross_native_guard_deadline())
        } else {
            None
        };
        if result.is_ok() {
            if mode == 1 {
                coordinator.reservation().cancel();
            }
            if mode == 2 {
                proof.request_close();
            }
        }
        let completed = if result.is_ok() {
            let executed = if matches!(mode, 3 | 5) {
                let reservation = coordinator.reservation();
                let worker = std::thread::spawn(move || {
                    let result = coordinator.execute();
                    (coordinator, result)
                });
                let reached = entered.recv_timeout(Duration::from_secs(1));
                if mode == 3 {
                    reservation.cancel();
                } else {
                    std::thread::sleep(Duration::from_millis(220));
                }
                let _ = release.try_send(());
                let (returned, result) = worker.join().unwrap();
                coordinator = returned;
                if reached.is_err() {
                    Err(Reject::Thread)
                } else {
                    result
                }
            } else {
                coordinator.execute()
            };
            executed.and_then(|f| {
                let stored = coordinator.record()?;
                if stored != f {
                    return Err(Reject::Continuity);
                }
                coordinator.publish()?;
                Ok(f)
            })
        } else {
            Err(Reject::Unsupported)
        };
        let replay = if completed.is_ok() {
            coordinator.begin(operation, BUDGET)
        } else {
            Err(Reject::Unsupported)
        };
        let ready = coordinator.reservation().ready();
        let same = producer.writer.worker.as_ref().unwrap().thread().id() == original;
        producer.stop();
        let retired = producer.join(BUDGET);
        h.authority().request_close();
        c.authority().request_close();
        let _ = tokio::join!(h.wait_closed(), c.wait_closed());
        assert!(retired.is_ok() && same);
        assert!(
            result.is_ok(),
            "native remote Resize must reserve on the SAME canonical actor: {:?}",
            result.err()
        );
        if mode == 0 {
            assert!(
                completed.is_ok(),
                "original native effect must finalize and publish: {:?}",
                completed.err()
            );
            assert!(
                replay.unwrap().is_some(),
                "durable replay is a fact lookup, never another effect"
            );
            assert!(ready.is_ok());
        } else {
            assert!(
                completed.is_err() && ready.is_err(),
                "failed/cancelled resize must never become Ready: mode{mode}"
            );
        }
        assert_eq!(
            calls.lock().unwrap().len(),
            usize::from(!matches!(mode, 1 | 2 | 6)),
            "mode{mode}"
        );
        assert!(
            !reentry.is_some_and(|probe| probe.load(Ordering::Acquire)),
            "native IO cancellation must not reenter its original publication guard"
        );
    }
}
