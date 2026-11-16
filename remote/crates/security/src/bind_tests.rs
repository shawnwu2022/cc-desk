use super::*;
use crate::test_identities as identities;
use crate::{
    CancelHandle, LocalIdentity, P256Spki, PeerPin, Unbound, accept, cancellation, connect,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
fn local(client: bool) -> LocalIdentity {
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
fn snapshot() -> HostSnapshot {
    HostSnapshot {
        host_key_epoch: 5,
        client_key_epoch: 7,
        activation: [0x33; 32],
        incarnation: 11,
        policy_epoch: 13,
        issuer_epoch: 17,
        origin: CanonicalOrigin::trusted("wss://relay.invalid/relay").unwrap(),
    }
}
fn trust() -> ClientTrust {
    ClientTrust {
        host_key_epoch: 5,
        client_key_epoch: 7,
        origin: CanonicalOrigin::trusted("wss://relay.invalid/relay").unwrap(),
    }
}
type Results = (
    Result<BoundManagement, BindError>,
    Result<BoundManagement, BindError>,
    CancelHandle,
);
async fn pair(c: Hooks, h: Hooks) -> Results {
    pair_context(c, h, snapshot(), trust()).await
}
async fn pair_context(c: Hooks, h: Hooks, snapshot: HostSnapshot, trust: ClientTrust) -> Results {
    let (cio, hio) = tokio::io::duplex(65536);
    let (handle, token) = cancellation();
    let (cp, hp) = tokio::join!(
        connect(
            Unbound::new(cio),
            local(true),
            pin(false),
            Duration::from_secs(2),
            token.clone()
        ),
        accept(
            Unbound::new(hio),
            local(false),
            pin(true),
            Duration::from_secs(2),
            token
        )
    );
    let (cp, hp) = (cp.unwrap(), hp.unwrap());
    assert!(cp.evidence().full_handshake && hp.evidence().full_handshake);
    let (c, h) = tokio::join!(
        bind_client_with(cp, trust, c),
        bind_host_with(hp, snapshot, h)
    );
    (c, h, handle)
}
async fn settle(results: Results) {
    let (c, h, _keep) = results;
    for owner in [c, h].into_iter().flatten() {
        tokio::time::timeout(Duration::from_secs(1), owner.close())
            .await
            .unwrap();
    }
}
fn alter(f: impl Fn(Message) -> Message + Send + Sync + 'static) -> Hooks {
    Hooks {
        alter: Some(Arc::new(f)),
        ..Default::default()
    }
}
#[tokio::test]
async fn authenticated_ids_keyepochs_origin_and_immutable_echo_reject() {
    for field in 0..5 {
        let h = alter(move |message| match message {
            Message::HostOffer(mut h) => {
                match field {
                    0 => h.context.host_endpoint_id = "aa".repeat(32),
                    1 => h.context.client_endpoint_id = "bb".repeat(32),
                    2 => h.context.host_key_epoch = "6".into(),
                    3 => h.context.client_key_epoch = "8".into(),
                    _ => {
                        h.context.origin =
                            CanonicalOrigin::trusted("wss://other.invalid/relay").unwrap()
                    }
                }
                Message::HostOffer(h)
            }
            m => m,
        });
        let results = pair(Hooks::default(), h).await;
        assert!(matches!(&results.0, Err(BindError::Context)));
        settle(results).await;
    }
    for field in 0..4 {
        let c = alter(move |message| match message {
            Message::ClientOffer(mut c) => {
                match field {
                    0 => c.context.policy_epoch = "14".into(),
                    1 => c.context.issuer_epoch = "18".into(),
                    2 => c.context.activation = "44".repeat(32),
                    _ => c.host_nonce = "55".repeat(32),
                }
                Message::ClientOffer(c)
            }
            m => m,
        });
        let results = pair(c, Hooks::default()).await;
        assert!(results.1.is_err());
        settle(results).await;
    }
}
#[tokio::test]
async fn explicit_profile_floor_role_caps_and_no_common_minor_reject() {
    for field in 0..8 {
        let c = alter(move |message| match message {
            Message::ClientOffer(mut c) => {
                match field {
                    0 => c.profile = 2,
                    1 => c.major = 2,
                    2 => c.minimum_security = "2".into(),
                    3 => c.requested_role = RequestedRole::Observer,
                    4 => c.required_capabilities = vec![],
                    5 => c.minors = vec![1],
                    6 => c.required_capabilities = vec!["unknown".into()],
                    _ => c.requested_role = RequestedRole::Controller,
                }
                Message::ClientOffer(c)
            }
            m => m,
        });
        let results = pair(c, Hooks::default()).await;
        assert!(matches!(&results.1, Err(BindError::Unsupported)));
        settle(results).await;
    }
}
#[tokio::test]
async fn authenticated_peer_cannot_mac_a_policy_invalid_selection() {
    for field in 0..5 {
        let h = Hooks {
            selected: Some(Arc::new(move |mut s| {
                match field {
                    0 => s.profile = 2,
                    1 => s.major = 2,
                    2 => s.minor = 1,
                    3 => s.security_version = "0".into(),
                    _ => s.requested_role = RequestedRole::Controller,
                }
                s
            })),
            ..Default::default()
        };
        // Host computes exporter and genuine MAC after this selection alteration.
        let results = pair(Hooks::default(), h).await;
        assert!(matches!(&results.0, Err(BindError::Context)));
        settle(results).await;
    }
}
#[tokio::test]
async fn wrong_direction_reflection_order_and_malformed_wire_reject() {
    let results = pair(
        Hooks::default(),
        Hooks {
            client_direction_host_mac: true,
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(&results.0, Err(BindError::Mac)));
    settle(results).await;
    let seen = Arc::new(Mutex::new(None::<Mac>));
    let save = seen.clone();
    let load = seen.clone();
    let host = alter(move |m| {
        if let Message::HostConfirm(h) = &m {
            *save.lock().unwrap() = Some(h.mac.clone());
        }
        m
    });
    let client = alter(move |m| match m {
        Message::ClientConfirm(mut c) => {
            c.mac = load.lock().unwrap().clone().unwrap();
            Message::ClientConfirm(c)
        }
        m => m,
    });
    let results = pair(client, host).await;
    assert!(matches!(&results.1, Err(BindError::Mac)));
    settle(results).await;
    let results = pair(
        Hooks::default(),
        alter(|_| {
            Message::ClientConfirm(ClientConfirm {
                mac: Mac::from_bytes([0; 32]),
            })
        }),
    )
    .await;
    assert!(matches!(&results.0, Err(BindError::Order)));
    settle(results).await;
    for case in 0..4 {
        let h = Hooks {
            wire: Some(Arc::new(move |mut bytes| match case {
                0 => {
                    bytes.insert(0, b' ');
                    bytes
                }
                1 => {
                    bytes.pop();
                    bytes
                }
                2 => String::from_utf8(bytes)
                    .unwrap()
                    .replacen("\"profile\":1", "\"profile\":1,\"profile\":1", 1)
                    .into_bytes(),
                _ => String::from_utf8(bytes)
                    .unwrap()
                    .replacen("\"profile\":1", "\"unknown\":1,\"profile\":1", 1)
                    .into_bytes(),
            })),
            ..Default::default()
        };
        let results = pair(Hooks::default(), h).await;
        assert!(matches!(&results.0, Err(BindError::Codec)));
        settle(results).await;
    }
}
type Captured = Arc<Mutex<Option<Mac>>>;
type Observed = Arc<Mutex<Vec<Observation>>>;
fn fixed(client: bool, observed: Observed, captured: Captured, replay: Option<Mac>) -> Hooks {
    Hooks {
        fixed_nonce: Some([if client { 0x22 } else { 0x11 }; 32]),
        observed: Some(observed),
        alter: Some(Arc::new(move |m| match m {
            Message::HostConfirm(mut c) if !client => {
                *captured.lock().unwrap() = Some(c.mac.clone());
                if let Some(tag) = &replay {
                    c.mac = tag.clone();
                }
                Message::HostConfirm(c)
            }
            Message::ClientConfirm(mut c) if client => {
                *captured.lock().unwrap() = Some(c.mac.clone());
                if let Some(tag) = &replay {
                    c.mac = tag.clone();
                }
                Message::ClientConfirm(c)
            }
            m => m,
        })),
        ..Default::default()
    }
}
#[tokio::test]
async fn identical_hcs_on_fresh_tls_reject_both_captured_confirm_directions() {
    let a_obs = Arc::new(Mutex::new(Vec::new()));
    let a_host = Arc::new(Mutex::new(None));
    let a_client = Arc::new(Mutex::new(None));
    let a = pair(
        fixed(true, a_obs.clone(), a_client.clone(), None),
        fixed(false, a_obs.clone(), a_host.clone(), None),
    )
    .await;
    assert!(a.0.is_ok() && a.1.is_ok());
    settle(a).await;
    let ah = a_host.lock().unwrap().clone().unwrap();
    let ac = a_client.lock().unwrap().clone().unwrap();
    assert!(a_obs.lock().unwrap().iter().all(
        |o| hex(&o.hash) == "f430d1db63dd8dca60e82e54eba1441fbc255bfb6b56638d75597145e5203e1f"
    ));
    for mode in 0..3 {
        let obs = Arc::new(Mutex::new(Vec::new()));
        let host = Arc::new(Mutex::new(None));
        let client = Arc::new(Mutex::new(None));
        let results = pair(
            fixed(
                true,
                obs.clone(),
                client.clone(),
                if mode == 2 { Some(ac.clone()) } else { None },
            ),
            fixed(
                false,
                obs.clone(),
                host.clone(),
                if mode == 1 { Some(ah.clone()) } else { None },
            ),
        )
        .await;
        assert!(
            obs.lock()
                .unwrap()
                .iter()
                .all(|o| a_obs.lock().unwrap().iter().any(|a| a == o)),
            "H/C/S/nonces/T must be identical, not merely application-nonce rejection"
        );
        assert!(!obs.lock().unwrap().is_empty());
        if mode == 0 {
            assert!(results.0.is_ok() && results.1.is_ok());
            assert!(host.lock().unwrap().as_ref().unwrap() != &ah);
            assert!(client.lock().unwrap().as_ref().unwrap() != &ac);
        } else if mode == 1 {
            assert!(matches!(&results.0, Err(BindError::Mac)));
        } else {
            assert!(matches!(&results.1, Err(BindError::Mac)));
        }
        settle(results).await;
    }
}

struct ProbeIo {
    io: tokio::io::DuplexStream,
    drops: Arc<std::sync::atomic::AtomicUsize>,
    written: Arc<std::sync::atomic::AtomicUsize>,
}
impl Drop for ProbeIo {
    fn drop(&mut self) {
        self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
impl AsyncRead for ProbeIo {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().io).poll_read(cx, buf)
    }
}
impl AsyncWrite for ProbeIo {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let r = std::pin::Pin::new(&mut this.io).poll_write(cx, bytes);
        if let std::task::Poll::Ready(Ok(n)) = r {
            this.written
                .fetch_add(n, std::sync::atomic::Ordering::SeqCst);
        }
        r
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}
struct ProbePair {
    client: Pending<ProbeIo>,
    host: Pending<ProbeIo>,
    keep: CancelHandle,
    drops: Arc<std::sync::atomic::AtomicUsize>,
    host_written: Arc<std::sync::atomic::AtomicUsize>,
}
async fn probes(capacity: usize, budget: Duration) -> ProbePair {
    let (c, h) = tokio::io::duplex(capacity);
    let (keep, token) = cancellation();
    let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let host_written = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (client, host) = tokio::join!(
        connect(
            Unbound::new(ProbeIo {
                io: c,
                drops: drops.clone(),
                written: Arc::new(std::sync::atomic::AtomicUsize::new(0))
            }),
            local(true),
            pin(false),
            budget,
            token.clone()
        ),
        accept(
            Unbound::new(ProbeIo {
                io: h,
                drops: drops.clone(),
                written: host_written.clone()
            }),
            local(false),
            pin(true),
            budget,
            token
        )
    );
    ProbePair {
        client: client.unwrap(),
        host: host.unwrap(),
        keep,
        drops,
        host_written,
    }
}
#[tokio::test]
async fn binding_uses_remaining_tls_deadline_instead_of_resetting() {
    let ProbePair {
        client,
        host,
        keep: _keep,
        drops,
        ..
    } = probes(65536, Duration::from_millis(400)).await;
    let original = client._deadline;
    tokio::time::sleep_until(original - Duration::from_millis(80)).await;
    let start = Instant::now();
    let result = bind_client(client, trust()).await;
    assert!(matches!(result, Err(BindError::Deadline)));
    assert!(
        start.elapsed() < Duration::from_millis(180),
        "binding must not receive a fresh400ms"
    );
    assert!(Instant::now() < original + Duration::from_millis(100));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(host);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn binding_silence_and_real_bounded_write_backpressure_expire() {
    for capacity in [65536, 32] {
        let ProbePair {
            client,
            host,
            keep: _keep,
            drops,
            host_written,
        } = probes(capacity, Duration::from_millis(150)).await;
        let before = host_written.load(std::sync::atomic::Ordering::SeqCst);
        let result = bind_host(host, snapshot()).await;
        assert!(matches!(result, Err(BindError::Deadline)));
        let progress = host_written.load(std::sync::atomic::Ordering::SeqCst) - before;
        if capacity == 32 {
            assert!(
                progress > 0 && progress <= 32,
                "ciphertext writes must fill the bounded32-byte unread channel"
            );
        }
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
        drop(client);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
#[tokio::test]
async fn binding_partial_records_and_oversized_headers_reject_without_owner() {
    for oversized in [false, true] {
        let ProbePair {
            client,
            host,
            keep: _keep,
            drops,
            ..
        } = probes(65536, Duration::from_secs(1)).await;
        let attack = async move {
            let mut tls = client._io;
            if oversized {
                tls.write_all(&[0, 0, 64, 5, 0, 4, 0, 0]).await.unwrap();
            } else {
                tls.write_all(&[0, 0, 0]).await.unwrap();
            }
            tls.flush().await.unwrap();
            tls.shutdown().await.unwrap();
            tls
        };
        let (host, peer) = tokio::join!(bind_host(host, snapshot()), attack);
        assert!(matches!(host, Err(BindError::Io | BindError::Codec)));
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
        drop(peer);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
#[tokio::test]
async fn binding_half_close_retains_peer_read_half_and_releases_self() {
    let ProbePair {
        client,
        mut host,
        keep: _keep,
        drops,
        ..
    } = probes(65536, Duration::from_secs(1)).await;
    host._io.shutdown().await.unwrap();
    assert!(matches!(
        bind_client(client, trust()).await,
        Err(BindError::Io)
    ));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(host);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn binding_cancel_and_aborted_future_drop_self_owned_tls() {
    for abort in [false, true] {
        let ProbePair {
            client,
            host,
            keep,
            drops,
            ..
        } = probes(65536, Duration::from_secs(1)).await;
        let task = tokio::spawn(bind_client(client, trust()));
        tokio::task::yield_now().await;
        if abort {
            task.abort();
            assert!(task.await.is_err_and(|e| e.is_cancelled()));
        } else {
            keep.cancel();
            assert!(matches!(task.await.unwrap(), Err(BindError::Cancelled)));
        }
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
        drop(host);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
#[tokio::test]
async fn authenticated_host_offer_drip_does_not_renew_binding_deadline() {
    let ProbePair {
        client,
        host,
        keep: _keep,
        drops,
        ..
    } = probes(65536, Duration::from_millis(180)).await;
    let original = client._deadline;
    let vector: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../protocol/remote/inner/v1/vectors/appbind-independent.json"
    ))
    .unwrap();
    let json = vector["canonicalParts"][0]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    let frame = cc_desk_remote_record::encode(Kind::AppBind, 0, &json).unwrap();
    let attack = async move {
        let mut tls = host._io;
        tls.write_all(&frame[..8]).await.unwrap();
        tls.flush().await.unwrap();
        let mut progress = 0;
        for byte in &frame[8..] {
            if tls.write_all(&[*byte]).await.is_err() || tls.flush().await.is_err() {
                break;
            }
            progress += 1;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        progress
    };
    let (result, progress) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(bind_client(client, trust()), attack)
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(BindError::Deadline)));
    assert!(progress >= 3);
    assert!(Instant::now() < original + Duration::from_millis(100));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn early_control_record_and_post_bind_data_never_dispatch() {
    let ProbePair {
        client,
        host,
        keep: _keep,
        drops,
        ..
    } = probes(65536, Duration::from_secs(1)).await;
    let mut peer = client._io;
    peer.write_all(&cc_desk_remote_record::encode(Kind::Control, 0, b"{}").unwrap())
        .await
        .unwrap();
    peer.flush().await.unwrap();
    assert!(matches!(
        bind_host(host, snapshot()).await,
        Err(BindError::Order)
    ));
    drop(peer);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
    let ProbePair {
        client,
        host,
        keep: _keep,
        drops,
        ..
    } = probes(65536, Duration::from_secs(1)).await;
    let malicious = async move {
        let mut io = client._io;
        let h = match receive(&mut io).await.unwrap() {
            Message::HostOffer(h) => h,
            _ => unreachable!(),
        };
        let c = ClientOffer {
            context: h.context.clone(),
            host_nonce: h.host_nonce.clone(),
            client_nonce: "22".repeat(32),
            profile: 1,
            major: 1,
            minors: vec![0],
            minimum_security: "1".into(),
            required_capabilities: vec!["canonical-json".into()],
            requested_role: RequestedRole::ManagementOnly,
        };
        let s = appbind::negotiate(&h, &c).unwrap();
        send_with(&mut io, &Hooks::default(), Message::ClientOffer(c.clone()))
            .await
            .unwrap();
        let confirm = match receive(&mut io).await.unwrap() {
            Message::HostConfirm(h) => h,
            _ => unreachable!(),
        };
        let t = hash(&h, &c, &s).unwrap();
        let key = exporter(&io, &t).unwrap();
        verify(&key, true, &t, &confirm.mac).unwrap();
        send_with(
            &mut io,
            &Hooks::default(),
            Message::ClientConfirm(ClientConfirm {
                mac: sign(&key, false, &t),
            }),
        )
        .await
        .unwrap();
        io.write_all(b"x").await.unwrap();
        io.flush().await.unwrap();
        io
    };
    let (owner, peer) = tokio::join!(bind_host(host, snapshot()), malicious);
    let mut owner = owner.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), owner.wait_closed())
            .await
            .unwrap(),
        OwnerExit::Unsupported
    );
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(peer);
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}
