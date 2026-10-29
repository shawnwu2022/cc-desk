use super::*;
use crate::bound_test_support as support;
use cc_desk_remote_record::{
    input,
    projection::{self as wire, Ack, Content},
};
use cc_desk_remote_security::{DataRecord, RunningChannel};
use owner::authorized::AuthorizedPeer;
const B: Duration = Duration::from_secs(1);
#[test]
#[allow(non_snake_case)]
fn AuthorizedBound_AdmissionExpiry_004() {
    if !supervision::run("authorized_pty_tests::AuthorizedBound_AdmissionExpiry_004") {
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            use cc_desk_remote_record::grant::DataRole;
            let mut owner =
                owner::WriterHostOwner::new_authorized(support::host(), 11, [4; 32]).unwrap();
            let dir = tempfile::tempdir().unwrap();
            for (index, principal, role) in [
                (0, [5; 32], DataRole::Controller),
                (1, [4; 32], DataRole::Observer),
            ] {
                let (c, h, _gate, _cancel, _grant) = support::channels_as(
                    Arc::new(std::sync::atomic::AtomicU64::new(1000)),
                    principal,
                    role,
                )
                .await;
                assert!(
                    owner
                        .start_authorized(
                            Journal::open(&dir.path().join(format!("wrong{index}.sqlite")))
                                .unwrap(),
                            std::io::sink(),
                            start(),
                            h.authority(),
                            [8; 16],
                            4
                        )
                        .is_err()
                );
                h.close().await;
                c.close().await;
            }
            let now = Arc::new(std::sync::atomic::AtomicU64::new(1000));
            let (c, h, _gate, _cancel, _grant) = support::channels(now.clone()).await;
            assert!(
                owner
                    .start_authorized(
                        Journal::open(&dir.path().join("client-role.sqlite")).unwrap(),
                        std::io::sink(),
                        start(),
                        c.authority(),
                        [8; 16],
                        4
                    )
                    .is_err()
            );
            struct Count(Arc<AtomicUsize>);
            impl Write for Count {
                fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                    self.0.fetch_add(1, Ordering::AcqRel);
                    Ok(b.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            let effects = Arc::new(AtomicUsize::new(0));
            let started = owner
                .start_authorized(
                    Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
                    Count(effects.clone()),
                    start(),
                    h.authority(),
                    [8; 16],
                    4,
                )
                .unwrap();
            let peer = started.peer;
            let window = sync(&peer, &c, &h, 4).await;
            let descriptor = peer.window_token(&window).unwrap();
            assert_eq!(descriptor.1, 6000);
            let held = record(&c, &h, 4, &command(descriptor, 1)).await;
            now.store(6000, Ordering::Release);
            assert_eq!(
                peer.submit_received(&held, &window)
                    .unwrap()
                    .wait(B)
                    .unwrap(),
                WriteResult::Rejected(Reject::Policy)
            );
            assert_eq!(effects.load(Ordering::Acquire), 0);
            now.store(11000, Ordering::Release);
            assert!(peer.capture().is_err());
            assert!(peer.pending_receipt([1; 16], 1).is_err());
            owner.seal(&target()).unwrap();
            let end = Instant::now() + B;
            while !owner.try_reap(&target()).unwrap() {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            // The native journal guard permits one owner per guarded directory;
            // release it before opening a second journal. Refusal must be the
            // expired proof, not the already retired session or storage guard.
            assert!(matches!(
                owner.start_authorized(
                    Journal::open(&dir.path().join("old-grant.sqlite")).unwrap(),
                    std::io::sink(),
                    start(),
                    h.authority(),
                    [8; 16],
                    4
                ),
                Err(owner::OwnerError::Writer(Reject::Policy))
            ));
            h.close().await;
            c.close().await;
        });
}
fn target() -> Target {
    Target {
        host: support::host(),
        session: [2; 16],
        run: "TEST_ONLY_BOUND_PTY".into(),
        generation: 3,
        incarnation: 11,
    }
}
fn start() -> owner::SessionStart {
    owner::SessionStart {
        target: target(),
        device: support::device(),
        writer_epoch: 9,
        key: DigestKey::from_backend_bytes([0x5a; 32]),
        rows: 24,
        columns: 80,
    }
}
async fn record(
    client: &RunningChannel,
    host: &RunningChannel,
    fence: u64,
    payload: &[u8],
) -> DataRecord {
    client
        .send_control(&support::scope(), fence, payload, B)
        .await
        .unwrap();
    host.receive(&support::scope(), fence, B).await.unwrap()
}
async fn sync(
    peer: &AuthorizedPeer,
    client: &RunningChannel,
    host: &RunningChannel,
    fence: u64,
) -> view_gate::SyncedView {
    let frame = peer.capture().unwrap().wait(B).unwrap().unwrap();
    let early = wire::encode_ack(
        &Ack {
            profile: 1,
            stamp: frame.issued_stamp().unwrap().clone(),
        },
        1,
    )
    .unwrap();
    let premature = record(client, host, fence, &early).await;
    assert!(matches!(
        peer.acknowledge_received(&premature)
            .unwrap()
            .wait(B)
            .unwrap(),
        Err(Reject::Sync)
    ));
    drop(premature);
    let count = cc_desk_remote_terminal::SnapshotEncoder::new(
        frame.view(),
        frame.issued_stamp().unwrap().clone(),
        1,
    )
    .unwrap()
    .count();
    let received = async {
        let mut stamp = None;
        for _ in 0..count {
            let part = client.receive(&support::scope(), fence, B).await.unwrap();
            let decoded = wire::decode(part.payload(), 1).unwrap();
            let Content::SnapshotPart(part) = decoded.content else {
                panic!("snapshot required")
            };
            if let Some(previous) = &stamp {
                assert!(*previous == part.stamp)
            }
            stamp = Some(part.stamp);
        }
        wire::encode_ack(
            &Ack {
                profile: 1,
                stamp: stamp.unwrap(),
            },
            1,
        )
        .unwrap()
    };
    let (sent, ack) = tokio::join!(frame.deliver_bound(host, fence, B), received);
    sent.unwrap();
    let ack = record(client, host, fence, &ack).await;
    let view = peer
        .acknowledge_received(&ack)
        .unwrap()
        .wait(B)
        .unwrap()
        .unwrap();
    peer.input_window(&view).unwrap().wait(B).unwrap().unwrap()
}
fn command((token, not_after_host_ms): ([u8; 32], u64), sequence: u64) -> Vec<u8> {
    input::encode(&input::Input {
        operation: [sequence as u8; 16],
        sequence,
        window: token,
        not_after_host_ms,
        bytes: b"PUBLIC_TEST_ONLY\r".to_vec(),
    })
    .unwrap()
}
#[allow(non_snake_case)]
#[test]
fn AuthorizedPty_BoundIo_001() {
    if !supervision::run("authorized_pty_tests::AuthorizedPty_BoundIo_001") {
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            use cc_desk_native_runtime::{BackendPtyLaunch, NativeOwnedPtyRun, NativePtySize};
            use std::io::Read;
            let now = Arc::new(std::sync::atomic::AtomicU64::new(1000));
            let (client, host, mut gate, _cancel, grant) = support::channels(now.clone()).await;
            let mut owner =
                owner::WriterHostOwner::new_authorized(support::host(), 11, [4; 32]).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let marker = dir.path().join("public-bound-pty-effects");
            assert!(
                owner
                    .start_guarded(
                        Journal::open(&dir.path().join("no-view-fallback.sqlite")).unwrap(),
                        std::io::sink(),
                        start(),
                        [3; 32],
                        1
                    )
                    .is_err()
            );
            assert!(
                owner
                    .start_controlled(
                        Journal::open(&dir.path().join("no-control-fallback.sqlite")).unwrap(),
                        std::io::sink(),
                        start(),
                        view_gate::control::ControlAdmission {
                            authority: None,
                            binding: [3; 32],
                            minor: 1,
                            clock: Arc::new(|| Ok(1000)),
                            grant: view_gate::control::GrantContext {
                                challenge_ms: 1000,
                                expires_host_ms: 11000,
                                grant: [7; 32],
                                lease: [8; 16],
                                fence: 4,
                                policy: 13
                            }
                        }
                    )
                    .is_err()
            );
            assert!(
                owner
                    .start(
                        Journal::open(&dir.path().join("no-fallback.sqlite")).unwrap(),
                        std::io::sink(),
                        start()
                    )
                    .is_err()
            );
            // Refuse mismatched native identities before any PTY writer is installed.
            for field in 0..5 {
                let mut wrong = start();
                match field {
                    0 => wrong.target.session = [3; 16],
                    1 => wrong.target.generation += 1,
                    2 => wrong.target.run.push('x'),
                    3 => wrong.device = [3; 32],
                    _ => wrong.target.incarnation += 1,
                };
                assert!(
                    owner
                        .start_authorized(
                            Journal::open(&dir.path().join(format!("refused{field}.sqlite")))
                                .unwrap(),
                            std::io::sink(),
                            wrong,
                            host.authority(),
                            [8; 16],
                            4
                        )
                        .is_err()
                );
            }
            let executable = std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("local-writer-fixture.exe");
            let recipe = BackendPtyLaunch::from_backend_recipe(
                executable,
                dir.path().to_owned(),
                vec!["TEST_ONLY_PIPE".into(), marker.as_os_str().to_owned()],
                Vec::new(),
            )
            .unwrap();
            let end = Instant::now() + Duration::from_secs(9);
            let mut run =
                NativeOwnedPtyRun::launch(recipe, NativePtySize::new(24, 80).unwrap(), end)
                    .unwrap();
            let mut output = run.take_reader().unwrap();
            let started = owner
                .start_authorized(
                    Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
                    run.take_writer().unwrap(),
                    start(),
                    host.authority(),
                    [8; 16],
                    4,
                )
                .unwrap();
            assert!(run.take_writer().is_none());
            let mut output_capability = started.output;
            let peer = started.peer;
            let (done_tx, done_rx) = mpsc::sync_channel(1);
            let reader = std::thread::spawn(move || {
                let mut offset = 0u64;
                let mut buf = [0; 4096];
                let mut forward = true;
                let result = (|| -> std::io::Result<u64> {
                    loop {
                        let n = output.read(&mut buf)?;
                        if n == 0 {
                            return Ok(offset);
                        }
                        if offset + n as u64 > 65536 {
                            return Err(std::io::Error::other("public output cap"));
                        }
                        if forward {
                            match output_capability.feed_at(offset, buf[..n].to_vec()) {
                                Ok(p) => match p.wait(B) {
                                    Ok(OutputResult::Processed { .. }) => (),
                                    Ok(OutputResult::Rejected(Reject::Sealed)) => forward = false,
                                    _ => return Err(std::io::Error::other("output processing")),
                                },
                                Err(Reject::Sealed) => forward = false,
                                Err(_) => return Err(std::io::Error::other("output admission")),
                            }
                        }
                        offset += n as u64;
                    }
                })();
                let _ = done_tx.send(result);
            });
            let window = sync(&peer, &client, &host, 4).await;
            let payload = command(peer.window_token(&window).unwrap(), 1);
            let first = record(&client, &host, 4, &payload).await;
            let a = peer
                .submit_received(&first, &window)
                .unwrap()
                .wait(B)
                .unwrap();
            let duplicate = record(&client, &host, 4, &payload).await;
            let b = peer
                .submit_received(&duplicate, &window)
                .unwrap()
                .wait(B)
                .unwrap();
            for result in [&a, &b] {
                assert!(matches!(
                    result,
                    WriteResult::Receipt(ReceiptEnvelope {
                        outcome: Outcome::HostWritten,
                        receipt_proof: Some(_),
                        ..
                    })
                ));
            }
            effects(&marker, 1);
            let wrong = record(&client, &host, 4, &command(([1; 32], 6000), 2)).await;
            assert!(matches!(
                peer.submit_received(&wrong, &window),
                Err(Reject::Policy)
            ));
            let (token, deadline) = peer.window_token(&window).unwrap();
            let wrong_deadline =
                record(&client, &host, 4, &command((token, deadline + 1), 2)).await;
            assert!(matches!(
                peer.submit_received(&wrong_deadline, &window),
                Err(Reject::Policy)
            ));
            let (mut c2, mut h2, _cancel2) = support::pair().await;
            c2.install(&grant).unwrap();
            h2.install(&grant).unwrap();
            let c2 = c2.start().unwrap();
            let h2 = h2.start().unwrap();
            let wrong_source = record(&c2, &h2, 4, &payload).await;
            assert!(matches!(
                peer.submit_received(&wrong_source, &window),
                Err(Reject::Target)
            ));
            drop(wrong_source);
            let successor = owner
                .rebind_authorized(&target(), support::device(), 4, h2.authority(), [9; 16])
                .unwrap()
                .wait(B)
                .unwrap()
                .unwrap();
            assert!(host.authority().check().is_err());
            host.wait_closed().await;
            assert!(peer.capture().is_err());
            assert!(successor.submit_received(&first, &window).is_err());
            assert!(peer.pending_receipt([1; 16], 1).is_err());
            let fresh = sync(&successor, &c2, &h2, 5).await;
            let copied = record(&c2, &h2, 5, &payload).await;
            assert!(matches!(
                successor.submit_received(&copied, &fresh),
                Err(Reject::Policy)
            ));
            let ReceiptLookup::Known(receipt) = successor
                .pending_receipt([1; 16], 1)
                .unwrap()
                .wait(B)
                .unwrap()
                .unwrap()
            else {
                panic!("exact pending receipt")
            };
            let proof = receipt.into_proof().unwrap();
            let mut wrong = proof.clone();
            wrong.outcome = Outcome::Rejected;
            assert!(matches!(
                successor
                    .settle_receipt(wrong)
                    .unwrap()
                    .wait(B)
                    .unwrap()
                    .unwrap(),
                AckDecision::Rejected(_)
            ));
            let second = record(
                &c2,
                &h2,
                5,
                &command(successor.window_token(&fresh).unwrap(), 2),
            )
            .await;
            assert_eq!(
                successor
                    .submit_received(&second, &fresh)
                    .unwrap()
                    .wait(B)
                    .unwrap(),
                WriteResult::Rejected(Reject::Pending)
            );
            assert_eq!(
                successor
                    .settle_receipt(proof)
                    .unwrap()
                    .wait(B)
                    .unwrap()
                    .unwrap(),
                AckDecision::Advanced(1)
            );
            assert!(matches!(
                successor
                    .submit_received(&second, &fresh)
                    .unwrap()
                    .wait(B)
                    .unwrap(),
                WriteResult::Receipt(ReceiptEnvelope {
                    outcome: Outcome::HostWritten,
                    ..
                })
            ));
            effects(&marker, 2);
            let held = record(
                &c2,
                &h2,
                5,
                &command(successor.window_token(&fresh).unwrap(), 3),
            )
            .await;
            gate.revoke();
            assert!(successor.submit_received(&held, &fresh).is_err());
            assert!(successor.capture().is_err());
            owner.seal(&target()).unwrap();
            while !owner.try_reap(&target()).unwrap() {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            assert_ne!(run.stop_root_until(end).unwrap().exit_code, 90);
            drop(run);
            let observed = done_rx
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .unwrap()
                .unwrap();
            assert!(observed > 0);
            while !reader.is_finished() {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            reader.join().unwrap();
            assert_eq!(std::fs::read(&marker).unwrap(), b"xx");
            host.close().await;
            client.wait_closed().await;
            h2.close().await;
            c2.wait_closed().await;
        });
}

// These cases use a native sink/blocking fixture, not an OS PTY. They isolate
// cancellation schedules while still traversing real RPK TLS record owners.
#[test]
#[allow(non_snake_case)]
fn AuthorizedBound_Backpressure_002() {
    if !supervision::run("authorized_pty_tests::AuthorizedBound_Backpressure_002") {
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, host, _gate, _cancel, grant) =
                support::channels(Arc::new(std::sync::atomic::AtomicU64::new(1000))).await;
            let mut owner =
                owner::WriterHostOwner::new_authorized(support::host(), 11, [4; 32]).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let mut config = start();
            config.rows = 64;
            config.columns = 160;
            let started = owner
                .start_authorized(
                    Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
                    std::io::sink(),
                    config,
                    host.authority(),
                    [8; 16],
                    4,
                )
                .unwrap();
            let peer = started.peer;
            let frame = peer.capture().unwrap().wait(B).unwrap().unwrap();
            // No subscriber consumes records: four retained RX credits plus the bounded
            // transport fill, so the original whole-send deadline must retire this scope.
            assert!(
                frame
                    .deliver_bound(&host, 4, Duration::from_millis(50))
                    .await
                    .is_err()
            );
            assert!(peer.capture().is_err());
            assert!(host.authority().check().is_err());
            tokio::time::timeout(B, host.wait_closed()).await.unwrap();
            // The shared grant remains valid. Rebind only the same writer with fresh TLS.
            let (mut c2, mut h2, _cancel2) = support::pair().await;
            c2.install(&grant).unwrap();
            h2.install(&grant).unwrap();
            let c2 = c2.start().unwrap();
            let h2 = h2.start().unwrap();
            let next = owner
                .rebind_authorized(&target(), support::device(), 4, h2.authority(), [9; 16])
                .unwrap()
                .wait(B)
                .unwrap()
                .unwrap();
            let window = sync(&next, &c2, &h2, 5).await;
            assert!(next.window_token(&window).is_ok());
            owner.seal(&target()).unwrap();
            let end = Instant::now() + B;
            while !owner.try_reap(&target()).unwrap() {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            h2.close().await;
            c2.close().await;
            client.close().await;
        });
}
#[test]
#[allow(non_snake_case)]
fn AuthorizedBound_QueuedRevoke_003() {
    if !supervision::run("authorized_pty_tests::AuthorizedBound_QueuedRevoke_003") {
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            struct Blocked {
                entered: mpsc::SyncSender<()>,
                release: mpsc::Receiver<()>,
                effects: Arc<AtomicUsize>,
            }
            impl Write for Blocked {
                fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                    self.entered.send(()).unwrap();
                    self.release
                        .recv_timeout(Duration::from_secs(2))
                        .map_err(std::io::Error::other)?;
                    self.effects.fetch_add(1, Ordering::AcqRel);
                    Ok(b.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            let (client, host, mut gate, _cancel, _grant) =
                support::channels(Arc::new(std::sync::atomic::AtomicU64::new(1000))).await;
            let mut owner =
                owner::WriterHostOwner::new_authorized(support::host(), 11, [4; 32]).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let (entered_tx, entered_rx) = mpsc::sync_channel(1);
            let (release_tx, release_rx) = mpsc::sync_channel(1);
            let effects = Arc::new(AtomicUsize::new(0));
            let started = owner
                .start_authorized(
                    Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
                    Blocked {
                        entered: entered_tx,
                        release: release_rx,
                        effects: effects.clone(),
                    },
                    start(),
                    host.authority(),
                    [8; 16],
                    4,
                )
                .unwrap();
            let peer = started.peer;
            let view = sync(&peer, &client, &host, 4).await;
            let bytes = command(peer.window_token(&view).unwrap(), 1);
            let first = record(&client, &host, 4, &bytes).await;
            let in_flight = peer.submit_received(&first, &view).unwrap();
            entered_rx.recv_timeout(B).unwrap();
            let queued_record = record(&client, &host, 4, &bytes).await;
            let queued = peer.submit_received(&queued_record, &view).unwrap();
            gate.revoke();
            release_tx.send(()).unwrap();
            // Already-started write may complete; revocation cannot recall PTY bytes.
            assert!(matches!(
                in_flight.wait(B).unwrap(),
                WriteResult::Receipt(ReceiptEnvelope {
                    outcome: Outcome::HostWritten,
                    ..
                })
            ));
            assert_eq!(
                queued.wait(B).unwrap(),
                WriteResult::Rejected(Reject::Policy)
            );
            assert_eq!(effects.load(Ordering::Acquire), 1);
            assert!(peer.pending_receipt([1; 16], 1).is_err());
            owner.seal(&target()).unwrap();
            let end = Instant::now() + B;
            while !owner.try_reap(&target()).unwrap() {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            host.close().await;
            client.wait_closed().await;
        });
}
