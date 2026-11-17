use crate::grant_tests::{admin, scope, signed};
use crate::test_identities as identities;
use crate::*;
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
fn identity(client: bool) -> LocalIdentity {
    let (k, p) = identities::identity(client, false);
    LocalIdentity::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
        k.key.clone(),
    )
    .unwrap()
}
fn pin(client: bool) -> PeerPin {
    let (_, p) = identities::identity(client, false);
    PeerPin::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
    )
}
pub(super) async fn bound_pair(
    client_grant: bool,
    host_grant: bool,
) -> (
    Result<ManagedChannel<tokio::io::DuplexStream>, BindError>,
    Result<ManagedChannel<tokio::io::DuplexStream>, BindError>,
    CancelHandle,
) {
    let (c, h) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(c),
            identity(true),
            pin(false),
            Duration::from_secs(2),
            token.clone()
        ),
        accept(
            Unbound::new(h),
            identity(false),
            pin(true),
            Duration::from_secs(2),
            token
        )
    );
    let origin = CanonicalOrigin::trusted("wss://relay.invalid/").unwrap();
    let trust = ClientTrust {
        host_key_epoch: 5,
        client_key_epoch: 7,
        origin: origin.clone(),
    };
    let snapshot = HostSnapshot {
        host_key_epoch: 5,
        client_key_epoch: 7,
        activation: [0x33; 32],
        incarnation: 11,
        policy_epoch: 13,
        issuer_epoch: 17,
        origin,
    };
    let (c, h) = tokio::join!(
        async {
            if client_grant {
                bind_client_peer_channel(c.unwrap(), trust).await
            } else {
                bind_client_channel(c.unwrap(), trust).await
            }
        },
        async {
            if host_grant {
                bind_host_peer_channel(h.unwrap(), snapshot).await
            } else {
                bind_host_channel(h.unwrap(), snapshot).await
            }
        }
    );
    (c, h, cancel)
}
pub(super) struct PublicSigner {
    pub(super) calls: usize,
}
impl NativeGrantSigner for PublicSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        grant: &'a VerifiedCanonicalGrant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>
    {
        self.calls += 1;
        Box::pin(async move { Ok(signed(grant.payload(), grant.issuer_kid())) })
    }
}
pub(super) fn policy() -> NativeGrantPolicy {
    NativeGrantPolicy {
        scope: scope(),
        role: DataRole::Controller,
        duration: Duration::from_secs(4),
    }
}
pub(super) fn trust() -> ClientGrantTrust {
    ClientGrantTrust {
        issuer: admin(),
        principal: [4; 32],
        scope: scope(),
        role: DataRole::Controller,
    }
}
const B: Duration = Duration::from_secs(1);
pub(crate) async fn granted() -> (
    ManagedChannel<tokio::io::DuplexStream>,
    ManagedChannel<tokio::io::DuplexStream>,
    CancelHandle,
    GrantGate,
) {
    let (c, h, cancel) = bound_pair(true, true).await;
    let (c, h) = (c.unwrap(), h.unwrap());
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let mut state = ClientGrantState::new(Arc::new(|| Ok(1000)));
    let mut signer = PublicSigner { calls: 0 };
    let trust = trust();
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, B),
        h.grant_host(&mut gate, policy(), B)
    );
    assert_eq!(signer.calls, 1);
    (c.unwrap(), h.unwrap(), cancel, gate)
}
async fn pending() -> (RunningChannel, RunningChannel, CancelHandle, GrantGate) {
    let (mut c, mut h, cancel, gate) = granted().await;
    assert!(c.start().is_err());
    assert!(h.start().is_err());
    let d = tokio::time::Instant::now() + Duration::from_secs(3);
    (
        c.start_peer_pending(d).unwrap(),
        h.start_peer_pending(d).unwrap(),
        cancel,
        gate,
    )
}
async fn install(c: &RunningChannel, h: &RunningChannel, f: u64) -> ClientInstalledPeer {
    let request = h.host_install_request().await.unwrap();
    let (proof, sent) = tokio::join!(c.client_installed(), h.complete_host_install(request, f));
    sent.unwrap();
    proof.unwrap()
}
#[tokio::test]
async fn peer_install_new_factories_refuse_legacy_both_directions() {
    for (a, b) in [(true, false), (false, true)] {
        let (c, h, _cancel) = bound_pair(a, b).await;
        assert!(c.is_err() || h.is_err());
    }
}
#[tokio::test]
async fn peer_install_pending_apis_zero_queue_effect_then_exact_fence_and_original_authority() {
    let (c, h, _cancel, _gate) = pending().await;
    let original = c.authority();
    assert_eq!(
        c.send_control(&scope(), 7, b"must-not-queue", B).await,
        Err(DataError::Context)
    );
    assert!(matches!(
        h.receive(&scope(), 7, B).await,
        Err(DataError::Context)
    ));
    assert_eq!(
        h.send_projection(&scope(), 7, b"must-not-queue", B).await,
        Err(DataError::Context)
    );
    assert!(matches!(
        c.receive(&scope(), 7, B).await,
        Err(DataError::Context)
    ));
    let proof = install(&c, &h, 7).await;
    assert_eq!(proof.consume_for(&c).unwrap(), 7);
    assert!(original.same_channel(&c.authority()));
    assert_eq!(original.grant_id(), c.authority().grant_id());
    assert_eq!(original.scope(), c.authority().scope());
    assert_eq!(
        c.send_control(&scope(), 8, b"wrong", B).await,
        Err(DataError::Fence)
    );
    let scope = scope();
    let (sent, record) = tokio::join!(
        c.send_control(&scope, 7, b"only-after-ready", B),
        h.receive(&scope, 7, B)
    );
    sent.unwrap();
    assert_eq!(record.unwrap().payload(), b"only-after-ready");
    original.request_close();
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
    assert!(c.authority().check().is_err());
    assert!(h.authority().check().is_err());
}
#[tokio::test]
async fn peer_install_cancel_before_start_and_original_stop_before_first_poll() {
    let (mut c, mut h, cancel, _gate) = granted().await;
    cancel.cancel();
    let d = tokio::time::Instant::now() + B;
    assert!(matches!(c.start_peer_pending(d), Err(DataError::Cancelled)));
    assert!(matches!(h.start_peer_pending(d), Err(DataError::Cancelled)));
    let (c, h, _cancel, _gate) = pending().await;
    h.authority().request_close();
    assert!(h.authority().check().is_err());
    assert!(h.host_install_request().await.is_err());
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
}
#[tokio::test]
async fn peer_install_drop_waiter_closes_exact_owner_and_preserves_join() {
    let (c, h, _cancel, _gate) = pending().await;
    let request = h.host_install_request().await.unwrap();
    let mut waiter = Box::pin(c.client_installed());
    assert!(futures_util::poll!(waiter.as_mut()).is_pending());
    drop(waiter);
    assert!(c.authority().check().is_err());
    assert!(h.complete_host_install(request, 7).await.is_err());
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
}
#[tokio::test]
async fn peer_install_single_use_request_and_client_proof() {
    let (c, h, _cancel, _gate) = pending().await;
    let proof = install(&c, &h, 7).await;
    assert!(matches!(
        h.host_install_request().await,
        Err(DataError::Full)
    ));
    assert!(matches!(c.client_installed().await, Err(DataError::Full)));
    assert_eq!(proof.consume_for(&c).unwrap(), 7);
    c.authority().request_close();
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
}
#[tokio::test]
async fn peer_install_other_channel_proof_and_stopped_proof_rejected() {
    let (c, h, _cancel, _gate) = pending().await;
    let p = install(&c, &h, 7).await;
    let (other, oh, _cancel2, _gate2) = pending().await;
    assert!(p.consume_for(&other).is_err());
    c.authority().request_close();
    other.authority().request_close();
    let _ = tokio::join!(
        c.wait_closed(),
        h.wait_closed(),
        other.wait_closed(),
        oh.wait_closed()
    );
    let (c, h, _cancel, _gate) = pending().await;
    let p = install(&c, &h, 7).await;
    c.authority().request_close();
    assert!(p.consume_for(&c).is_err());
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
}
#[tokio::test]
async fn peer_install_fixed_deadline_without_api_waiter_and_late_completion() {
    let (mut c, mut h, _cancel, _gate) = granted().await;
    let d = tokio::time::Instant::now() + Duration::from_millis(40);
    let c = c.start_peer_pending(d).unwrap();
    let h = h.start_peer_pending(d).unwrap();
    assert_eq!(c.peer_install_deadline(), Some(d));
    assert_eq!(h.peer_install_deadline(), Some(d));
    let request = h.host_install_request().await.unwrap();
    tokio::time::sleep_until(d + Duration::from_millis(1)).await;
    assert!(h.complete_host_install(request, 7).await.is_err());
    assert!(c.client_installed().await.is_err());
    assert!(c.installed_fence().is_err());
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
}
#[tokio::test]
async fn peer_install_stop_after_request_no_late_ready() {
    let (c, h, _cancel, _gate) = pending().await;
    let request = h.host_install_request().await.unwrap();
    let a = h.authority();
    a.request_close();
    assert!(h.complete_host_install(request, 7).await.is_err());
    assert!(h.installed_fence().is_err());
    assert!(c.client_installed().await.is_err());
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
}
// Tests below use the authenticated peer's original TLS object, never a second reader.
async fn wire_request(
    h: &mut ManagedChannel<tokio::io::DuplexStream>,
) -> cc_desk_remote_record::peer_install::Message {
    use tokio::io::AsyncReadExt;
    let io = h.io.as_mut().unwrap();
    let mut header = [0; 8];
    io.read_exact(&mut header).await.unwrap();
    let header = cc_desk_remote_record::Header::decode(header, 1).unwrap();
    assert_eq!(header.kind, cc_desk_remote_record::Kind::Control);
    let mut body = vec![0; header.payload_len];
    io.read_exact(&mut body).await.unwrap();
    cc_desk_remote_record::peer_install::decode(&body).unwrap()
}
#[tokio::test]
async fn peer_install_wrong_metadata_each_field_and_unsolicited_data_fail_closed() {
    use cc_desk_remote_record::{Kind, peer_install::Message};
    use tokio::io::AsyncWriteExt;
    for wrong in 0..6 {
        let (mut c, mut h, _cancel, _gate) = granted().await;
        let c = c
            .start_peer_pending(tokio::time::Instant::now() + B)
            .unwrap();
        let Message::Request {
            mut nonce,
            mut binding,
            mut grant_id,
            mut scope_digest,
        } = wire_request(&mut h).await
        else {
            panic!("request expected")
        };
        let bad = "aa".repeat(32);
        match wrong {
            0 => nonce = bad,
            1 => binding = bad,
            2 => grant_id = bad,
            3 => scope_digest = bad,
            _ => (),
        }
        let m = if wrong == 4 {
            Message::Request {
                nonce,
                binding,
                grant_id,
                scope_digest,
            }
        } else {
            Message::Installed {
                nonce,
                binding,
                grant_id,
                scope_digest,
                fence: "7".into(),
            }
        };
        let body = cc_desk_remote_record::peer_install::encode(&m).unwrap();
        let kind = if wrong == 5 {
            Kind::Data
        } else {
            Kind::Control
        };
        let bytes = cc_desk_remote_record::encode(kind, 1, &body).unwrap();
        let io = h.io.as_mut().unwrap();
        io.write_all(&bytes).await.unwrap();
        io.flush().await.unwrap();
        assert!(c.client_installed().await.is_err());
        assert!(c.installed_fence().is_err());
        c.wait_closed().await;
    }
}
#[tokio::test]
async fn peer_install_drip_header_cannot_refresh_absolute_deadline() {
    use tokio::io::AsyncWriteExt;
    let (mut c, mut h, _cancel, _gate) = granted().await;
    let d = tokio::time::Instant::now() + Duration::from_millis(50);
    let c = c.start_peer_pending(d).unwrap();
    let _ = wire_request(&mut h).await;
    let io = h.io.as_mut().unwrap();
    io.write_all(&[1]).await.unwrap();
    io.flush().await.unwrap();
    tokio::time::sleep_until(d + Duration::from_millis(1)).await;
    assert!(c.client_installed().await.is_err());
    assert!(c.installed_fence().is_err());
    c.wait_closed().await;
}

// Armed only AFTER real TLS/AppBind/grant_client+grant_host complete. This gate
// lets TLS ciphertext reach the peer before its local poll_flush may return.
#[derive(Default)]
struct FlushGate {
    blocked: std::sync::atomic::AtomicBool,
    entered: std::sync::atomic::AtomicBool,
    reads: std::sync::atomic::AtomicUsize,
    dropped: std::sync::atomic::AtomicBool,
    wake: std::sync::Mutex<Option<std::task::Waker>>,
}
impl FlushGate {
    fn arm(&self) {
        self.blocked.store(true, Ordering::Release);
    }
    fn release(&self) {
        self.blocked.store(false, Ordering::Release);
        if let Some(w) = self.wake.lock().unwrap().take() {
            w.wake();
        }
    }
    async fn wait_entered(&self) {
        tokio::time::timeout(B, async {
            while !self.entered.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
struct FlushIo {
    io: tokio::io::DuplexStream,
    gate: Arc<FlushGate>,
}
impl tokio::io::AsyncRead for FlushIo {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        b: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.gate.reads.fetch_add(1, Ordering::AcqRel);
        std::pin::Pin::new(&mut self.io).poll_read(cx, b)
    }
}
impl tokio::io::AsyncWrite for FlushIo {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        b: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.io).poll_write(cx, b)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match std::pin::Pin::new(&mut self.io).poll_flush(cx) {
            std::task::Poll::Ready(Ok(())) => (),
            other => return other,
        }
        let mut wake = self.gate.wake.lock().unwrap();
        if self.gate.blocked.load(Ordering::Acquire) {
            *wake = Some(cx.waker().clone());
            self.gate.entered.store(true, Ordering::Release);
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(Ok(()))
        }
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_shutdown(cx)
    }
}
impl Drop for FlushIo {
    fn drop(&mut self) {
        self.gate.dropped.store(true, Ordering::Release);
    }
}
async fn flush_granted() -> (
    ManagedChannel<FlushIo>,
    ManagedChannel<FlushIo>,
    Arc<FlushGate>,
    Arc<FlushGate>,
    CancelHandle,
    GrantGate,
) {
    let (ci, hi) = tokio::io::duplex(65536);
    let cg = Arc::new(FlushGate::default());
    let hg = Arc::new(FlushGate::default());
    let (cancel, token) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(FlushIo {
                io: ci,
                gate: cg.clone()
            }),
            identity(true),
            pin(false),
            B,
            token.clone()
        ),
        accept(
            Unbound::new(FlushIo {
                io: hi,
                gate: hg.clone()
            }),
            identity(false),
            pin(true),
            B,
            token
        )
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
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let mut state = ClientGrantState::new(Arc::new(|| Ok(1000)));
    let mut signer = PublicSigner { calls: 0 };
    let trust = trust();
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, B),
        h.grant_host(&mut gate, policy(), B)
    );
    assert_eq!(signer.calls, 1);
    assert!(!cg.entered.load(Ordering::Acquire));
    assert!(!hg.entered.load(Ordering::Acquire));
    (c.unwrap(), h.unwrap(), cg, hg, cancel, gate)
}
#[tokio::test]
async fn peer_install_independent_dual_flush_release_orders_commit_ready_before_buffered_data() {
    for client_first in [true, false] {
        let (mut c, mut h, cg, hg, _cancel, _gate) = flush_granted().await;
        cg.arm();
        hg.arm();
        let d = tokio::time::Instant::now() + Duration::from_secs(3);
        let c = Arc::new(c.start_peer_pending(d).unwrap());
        let h = Arc::new(h.start_peer_pending(d).unwrap());
        cg.wait_entered().await;
        // Host sees the complete Request while Client's own flush is still Pending.
        let request = h.host_install_request().await.unwrap();
        assert!(cg.blocked.load(Ordering::Acquire));
        let hh = h.clone();
        let response = tokio::spawn(async move { hh.complete_host_install(request, 7).await });
        hg.wait_entered().await;
        let host_reads = hg.reads.load(Ordering::Acquire);
        assert!(c.installed_fence().is_err());
        assert!(h.installed_fence().is_err());
        if client_first {
            cg.release();
            let proof = c.client_installed().await.unwrap();
            assert_eq!(proof.consume_for(&c).unwrap(), 7);
            assert!(!response.is_finished());
            assert!(h.installed_fence().is_err());
            let cc = c.clone();
            let sent = tokio::spawn(async move {
                cc.send_control(&scope(), 7, b"buffered-during-host-flush", B)
                    .await
            });
            sent.await.unwrap().unwrap();
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            assert_eq!(
                hg.reads.load(Ordering::Acquire),
                host_reads,
                "Host parsed input before its Installed flush returned"
            );
            assert!(matches!(
                h.receive(&scope(), 7, B).await,
                Err(DataError::Context)
            ));
            hg.release();
            response.await.unwrap().unwrap();
            assert_eq!(h.installed_fence().unwrap(), 7);
            assert_eq!(
                h.receive(&scope(), 7, B).await.unwrap().payload(),
                b"buffered-during-host-flush"
            );
        } else {
            hg.release();
            response.await.unwrap().unwrap();
            assert_eq!(h.installed_fence().unwrap(), 7);
            assert!(c.installed_fence().is_err());
            assert_eq!(
                c.send_control(&scope(), 7, b"pre-proof", B).await,
                Err(DataError::Context)
            );
            cg.release();
            let proof = c.client_installed().await.unwrap();
            assert_eq!(proof.consume_for(&c).unwrap(), 7);
            let scope = scope();
            let (sent, record) = tokio::join!(
                c.send_control(&scope, 7, b"host-first", B),
                h.receive(&scope, 7, B)
            );
            sent.unwrap();
            assert_eq!(record.unwrap().payload(), b"host-first");
        }
        c.authority().request_close();
        let _ = tokio::join!(c.wait_closed(), h.wait_closed());
        assert!(cg.dropped.load(Ordering::Acquire));
        assert!(hg.dropped.load(Ordering::Acquire));
    }
}
#[tokio::test]
async fn peer_install_stop_and_fixed_deadline_retire_io_while_both_flushes_blocked() {
    for stop in [true, false] {
        let (mut c, mut h, cg, hg, _cancel, _gate) = flush_granted().await;
        cg.arm();
        hg.arm();
        let d = tokio::time::Instant::now() + Duration::from_millis(300);
        let c = c.start_peer_pending(d).unwrap();
        let h = Arc::new(h.start_peer_pending(d).unwrap());
        cg.wait_entered().await;
        let request = h.host_install_request().await.unwrap();
        let hh = h.clone();
        let response = tokio::spawn(async move { hh.complete_host_install(request, 7).await });
        hg.wait_entered().await;
        if stop {
            c.authority().request_close();
            h.authority().request_close();
        } else {
            tokio::time::sleep_until(d + Duration::from_millis(1)).await;
        }
        assert!(response.await.unwrap().is_err());
        assert!(c.client_installed().await.is_err());
        let _ = tokio::join!(c.wait_closed(), h.wait_closed());
        assert!(cg.dropped.load(Ordering::Acquire));
        assert!(hg.dropped.load(Ordering::Acquire));
        assert!(c.installed_fence().is_err());
        assert!(h.installed_fence().is_err());
    }
}
#[tokio::test]
async fn peer_install_host_rejects_raw_data_and_single_partial_prefix_before_native_completion() {
    use cc_desk_remote_record::{Kind, peer_install::Message};
    use tokio::io::AsyncWriteExt;
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    for scenario in 0..3 {
        let (mut c, mut h, _cancel, _gate) = granted().await;
        let h = h
            .start_peer_pending(tokio::time::Instant::now() + B)
            .unwrap();
        let access = c.access.as_ref().unwrap();
        let body = cc_desk_remote_record::peer_install::encode(&Message::Request {
            nonce: "09".repeat(32),
            binding: hex(&c.binding),
            grant_id: hex(&access.id()),
            scope_digest: hex(&access.scope_digest()),
        })
        .unwrap();
        let request = cc_desk_remote_record::encode(Kind::Control, 1, &body).unwrap();
        let io = c.io.as_mut().unwrap();
        io.write_all(&request).await.unwrap();
        io.flush().await.unwrap();
        let request = h.host_install_request().await.unwrap();
        if scenario == 1 {
            io.write_all(&[1]).await.unwrap();
        } else if scenario == 2 {
            let frame = cc_desk_remote_record::encode(Kind::Control, 1, b"partial-body").unwrap();
            io.write_all(&frame[..9]).await.unwrap();
        } else {
            io.write_all(&cc_desk_remote_record::encode(Kind::Data, 1, b"premature").unwrap())
                .await
                .unwrap();
        }
        io.flush().await.unwrap();
        // Waiting for actual owner exit before completion removes scheduler ambiguity.
        assert_eq!(h.wait_closed().await, DataError::Context);
        assert!(h.complete_host_install(request, 7).await.is_err());
        assert!(h.installed_fence().is_err());
        assert!(h.authority().check().is_err());
        assert!(matches!(
            h.receive(&scope(), 7, B).await,
            Err(DataError::Closed)
        ));
    }
}

async fn mode_pair(
    client_mode: u8,
    host_mode: u8,
) -> (
    Result<ManagedChannel<tokio::io::DuplexStream>, BindError>,
    Result<ManagedChannel<tokio::io::DuplexStream>, BindError>,
    CancelHandle,
) {
    let (c, h) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(c),
            identity(true),
            pin(false),
            B,
            token.clone()
        ),
        accept(Unbound::new(h), identity(false), pin(true), B, token)
    );
    let origin = CanonicalOrigin::trusted("wss://relay.invalid/").unwrap();
    let ct = ClientTrust {
        host_key_epoch: 5,
        client_key_epoch: 7,
        origin: origin.clone(),
    };
    let hs = HostSnapshot {
        host_key_epoch: 5,
        client_key_epoch: 7,
        activation: [0x33; 32],
        incarnation: 11,
        policy_epoch: 13,
        issuer_epoch: 17,
        origin,
    };
    let (c, h) = tokio::join!(
        async {
            match client_mode {
                0 => bind_client_channel(c.unwrap(), ct).await,
                1 => bind_client_grant_channel(c.unwrap(), ct).await,
                _ => bind_client_peer_channel(c.unwrap(), ct).await,
            }
        },
        async {
            match host_mode {
                0 => bind_host_channel(h.unwrap(), hs).await,
                1 => bind_host_grant_channel(h.unwrap(), hs).await,
                _ => bind_host_peer_channel(h.unwrap(), hs).await,
            }
        }
    );
    (c, h, cancel)
}
#[tokio::test]
async fn peer_install_full_negotiated_legacy_grant_peer_matrix_and_successful_data_routes() {
    for cm in 0..3 {
        for hm in 0..3 {
            let (c, h, _cancel) = mode_pair(cm, hm).await;
            if cm != hm {
                assert!(
                    c.is_err() || h.is_err(),
                    "incompatible modes {cm}/{hm} negotiated"
                );
                continue;
            }
            let (mut c, mut h) = (c.unwrap(), h.unwrap());
            let mut gate =
                GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000)))
                    .unwrap();
            if cm == 0 {
                let challenge = gate
                    .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
                    .unwrap();
                let grant = gate.accept(&signed(&challenge, gate.issuer_kid())).unwrap();
                c.install(&grant).unwrap();
                h.install(&grant).unwrap();
            } else {
                let mut signer = PublicSigner { calls: 0 };
                let mut state = ClientGrantState::new(Arc::new(|| Ok(1000)));
                let t = trust();
                let (cc, hh) = tokio::join!(
                    c.grant_client(&t, &mut signer, &mut state, B),
                    h.grant_host(&mut gate, policy(), B)
                );
                c = cc.unwrap();
                h = hh.unwrap();
                assert_eq!(signer.calls, 1);
            }
            let (c, h) = if cm == 2 {
                let d = tokio::time::Instant::now() + B;
                (
                    c.start_peer_pending(d).unwrap(),
                    h.start_peer_pending(d).unwrap(),
                )
            } else {
                (c.start().unwrap(), h.start().unwrap())
            };
            if cm == 2 {
                assert_eq!(install(&c, &h, 7).await.consume_for(&c).unwrap(), 7);
            }
            let scope = scope();
            let (sent, record) = tokio::join!(
                c.send_control(&scope, 7, b"matrix-route", B),
                h.receive(&scope, 7, B)
            );
            sent.unwrap();
            assert_eq!(record.unwrap().payload(), b"matrix-route");
            c.authority().request_close();
            let _ = tokio::join!(c.wait_closed(), h.wait_closed());
        }
    }
}

#[tokio::test]
async fn peer_install_ready_engine_rejects_wrong_ingress_fence_without_delivery() {
    use cc_desk_remote_record::{
        Kind,
        grant::{self, DataHeader},
        peer_install::Message,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut c, mut h, _cancel, _gate) = granted().await;
    let h = h
        .start_peer_pending(tokio::time::Instant::now() + B)
        .unwrap();
    let hex = |b: &[u8]| b.iter().map(|v| format!("{v:02x}")).collect::<String>();
    let access = c.access.as_ref().unwrap();
    let body = cc_desk_remote_record::peer_install::encode(&Message::Request {
        nonce: "09".repeat(32),
        binding: hex(&c.binding),
        grant_id: hex(&access.id()),
        scope_digest: hex(&access.scope_digest()),
    })
    .unwrap();
    let io = c.io.as_mut().unwrap();
    io.write_all(&cc_desk_remote_record::encode(Kind::Control, 1, &body).unwrap())
        .await
        .unwrap();
    io.flush().await.unwrap();
    let request = h.host_install_request().await.unwrap();
    let (complete, ()) = tokio::join!(h.complete_host_install(request, 7), async {
        let mut bytes = [0; 8];
        io.read_exact(&mut bytes).await.unwrap();
        let header = cc_desk_remote_record::Header::decode(bytes, 1).unwrap();
        assert_eq!(header.kind, Kind::Control);
        let mut body = vec![0; header.payload_len];
        io.read_exact(&mut body).await.unwrap();
        assert!(
            matches!(cc_desk_remote_record::peer_install::decode(&body).unwrap(),Message::Installed{fence,..} if fence=="7")
        );
    });
    complete.unwrap();
    assert_eq!(h.installed_fence().unwrap(), 7);
    let access = c.access.as_ref().unwrap();
    let header = DataHeader {
        binding: c.binding,
        grant: access.id(),
        scope: access.scope_digest(),
        fence: 8,
        sequence: 1,
        kind: Kind::Control,
    };
    let body = grant::encode_data(&header, b"WRONG_FENCE_MUST_NOT_DELIVER").unwrap();
    io.write_all(&cc_desk_remote_record::encode(Kind::Data, 1, &body).unwrap())
        .await
        .unwrap();
    io.flush().await.unwrap();
    assert_eq!(h.wait_closed().await, DataError::Fence);
    assert!(h.receive(&scope(), 7, B).await.is_err());
    assert!(h.authority().check().is_err());
}

#[tokio::test]
async fn peer_install_host_first_frame_data_and_replayed_request_never_delivered() {
    use cc_desk_remote_record::{Kind, peer_install::Message};
    use tokio::io::AsyncWriteExt;
    for replay in [false, true] {
        let (mut c, mut h, _cancel, _gate) = granted().await;
        let h = h
            .start_peer_pending(tokio::time::Instant::now() + B)
            .unwrap();
        if replay {
            let hex = |b: &[u8]| b.iter().map(|v| format!("{v:02x}")).collect::<String>();
            let a = c.access.as_ref().unwrap();
            let body = cc_desk_remote_record::peer_install::encode(&Message::Request {
                nonce: "09".repeat(32),
                binding: hex(&c.binding),
                grant_id: hex(&a.id()),
                scope_digest: hex(&a.scope_digest()),
            })
            .unwrap();
            let frame = cc_desk_remote_record::encode(Kind::Control, 1, &body).unwrap();
            let io = c.io.as_mut().unwrap();
            io.write_all(&frame).await.unwrap();
            io.flush().await.unwrap();
            let request = h.host_install_request().await.unwrap();
            io.write_all(&frame).await.unwrap();
            io.flush().await.unwrap();
            assert_eq!(h.wait_closed().await, DataError::Context);
            assert!(h.complete_host_install(request, 7).await.is_err());
        } else {
            let io = c.io.as_mut().unwrap();
            io.write_all(&cc_desk_remote_record::encode(Kind::Data, 1, b"PREMATURE").unwrap())
                .await
                .unwrap();
            io.flush().await.unwrap();
            assert_eq!(h.wait_closed().await, DataError::Codec);
        }
        assert!(h.receive(&scope(), 7, B).await.is_err());
        assert!(h.installed_fence().is_err());
    }
}
