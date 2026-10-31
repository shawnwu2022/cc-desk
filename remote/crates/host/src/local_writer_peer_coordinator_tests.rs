use super::*;
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
        scope: support::scope(),
        role: DataRole::Controller,
    };
    let policy = NativeGrantPolicy {
        scope: support::scope(),
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
