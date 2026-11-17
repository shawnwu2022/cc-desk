#![allow(non_snake_case)]
#[path = "support/identities.rs"]
#[allow(dead_code)]
mod identities;
use cc_desk_remote_record::appbind::CanonicalOrigin;
use cc_desk_remote_security::*;
use std::time::Duration;
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
#[tokio::test]
async fn Bind_HostFirstLearnsCurrentEpochsAndClosesOwners_001() {
    let (c, s) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let (c, s) = tokio::join!(
        connect(
            Unbound::new(c),
            local(true),
            pin(false),
            Duration::from_secs(2),
            token.clone()
        ),
        accept(
            Unbound::new(s),
            local(false),
            pin(true),
            Duration::from_secs(2),
            token
        )
    );
    let cached_policy_epoch = 1u64;
    let cached_issuer_epoch = 2u64;
    let (c, h) = tokio::join!(
        bind_client(c.ok().unwrap(), trust()),
        bind_host(s.ok().unwrap(), snapshot())
    );
    let c = c.expect("client management binding");
    let h = h.expect("host management binding");
    assert_eq!(c.context(), h.context());
    assert_eq!(c.context().policy_epoch(), "13");
    assert_eq!(c.context().issuer_epoch(), "17");
    assert!(cached_policy_epoch < 13 && cached_issuer_epoch < 17);
    cancel.cancel();
    let (_, _) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(c.close(), h.close())
    })
    .await
    .unwrap();
}
struct Track<S> {
    io: S,
    drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl<S> Drop for Track<S> {
    fn drop(&mut self) {
        self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Track<S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().io).poll_read(cx, buf)
    }
}
impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Track<S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().io).poll_write(cx, bytes)
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
async fn tracked_pair() -> (
    BoundManagement,
    BoundManagement,
    CancelHandle,
    CancelHandle,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (c, s) = tokio::io::duplex(65536);
    let (ch, ct) = cancellation();
    let (hh, ht) = cancellation();
    let (c, s) = tokio::join!(
        connect(
            Unbound::new(Track {
                io: c,
                drops: drops.clone()
            }),
            local(true),
            pin(false),
            Duration::from_secs(2),
            ct
        ),
        accept(
            Unbound::new(Track {
                io: s,
                drops: drops.clone()
            }),
            local(false),
            pin(true),
            Duration::from_secs(2),
            ht
        )
    );
    let (c, h) = tokio::join!(
        bind_client(c.unwrap(), trust()),
        bind_host(s.unwrap(), snapshot())
    );
    (c.unwrap(), h.unwrap(), ch, hh, drops)
}
#[tokio::test]
async fn Bind_SingleSideCancelClosesBothOwnedDirectionsAndPeer_001() {
    let (mut c, mut h, ch, _hh, drops) = tracked_pair().await;
    ch.cancel();
    let (c, h) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(c.wait_closed(), h.wait_closed())
    })
    .await
    .unwrap();
    assert_eq!(c, OwnerExit::Cancelled);
    assert!(matches!(h, OwnerExit::PeerClosed | OwnerExit::Io));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn Bind_LastExternalHandleDropActuallyCancelsOwner_001() {
    let (mut c, mut h, ch, _hh, drops) = tracked_pair().await;
    let last = ch.clone();
    drop(ch);
    tokio::task::yield_now().await;
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(last);
    let (c, h) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(c.wait_closed(), h.wait_closed())
    })
    .await
    .unwrap();
    assert_eq!(c, OwnerExit::Cancelled);
    assert!(matches!(h, OwnerExit::PeerClosed | OwnerExit::Io));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn Bind_OwnerDropRequestsAbortAndPeerConverges_001() {
    let (c, mut h, _ch, _hh, drops) = tracked_pair().await;
    drop(c);
    let h = tokio::time::timeout(Duration::from_secs(1), h.wait_closed())
        .await
        .unwrap();
    assert!(matches!(h, OwnerExit::PeerClosed | OwnerExit::Io));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn Bind_CloseJoinsOwnedTaskAndPeerEof_001() {
    let (c, mut h, _ch, _hh, drops) = tracked_pair().await;
    let (c, h) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(c.close(), h.wait_closed())
    })
    .await
    .unwrap();
    assert_eq!(c, OwnerExit::LocalClose);
    assert!(matches!(h, OwnerExit::PeerClosed | OwnerExit::Io));
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 2);
}

async fn ws_pair() -> (
    tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client = async {
        tokio_tungstenite::client_async_with_config(
            "ws://test.invalid/",
            tokio::net::TcpStream::connect(addr).await.unwrap(),
            Some(cc_desk_remote_transport::websocket_config()),
        )
        .await
        .unwrap()
        .0
    };
    let server = async {
        let (s, peer) = listener.accept().await.unwrap();
        assert!(peer.ip().is_loopback());
        tokio_tungstenite::accept_async_with_config(
            s,
            Some(cc_desk_remote_transport::websocket_config()),
        )
        .await
        .unwrap()
    };
    tokio::join!(client, server)
}
async fn fragment_to<S>(bytes: &[u8], dest: &mut tokio_tungstenite::WebSocketStream<S>) -> usize
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::SinkExt;
    let mut count = 0;
    for frame in cc_desk_remote_protocol::decode_outer(bytes).unwrap() {
        for chunk in frame.payload.chunks(17) {
            let envelope =
                cc_desk_remote_protocol::encode_outer(&[cc_desk_remote_protocol::Subframe {
                    tunnel: frame.tunnel,
                    payload: chunk,
                }])
                .unwrap();
            dest.send(tokio_tungstenite::tungstenite::Message::Binary(
                envelope.into(),
            ))
            .await
            .unwrap();
            count += 1;
        }
    }
    count
}

#[tokio::test]
async fn Bind_ActualWsMuxFragmentedFourMessageProtocol_001() {
    use futures_util::StreamExt;
    use std::collections::HashMap;
    let (cws, mut left) = ws_pair().await;
    let (hws, mut right) = ws_pair().await;
    let (cio, cr) = cc_desk_remote_transport::bounded_tunnel();
    let (hio, hr) = cc_desk_remote_transport::bounded_tunnel();
    let (drive_cancel, rx) = tokio::sync::watch::channel(false);
    let cdrive = tokio::spawn(cc_desk_remote_transport::drive(
        cws,
        HashMap::from([([1; 16], cr)]),
        rx.clone(),
        Duration::from_secs(4),
    ));
    let hdrive = tokio::spawn(cc_desk_remote_transport::drive(
        hws,
        HashMap::from([([1; 16], hr)]),
        rx,
        Duration::from_secs(4),
    ));
    let bridge = tokio::spawn(async move {
        let mut fragments = 0;
        loop {
            tokio::select! {
             m=left.next()=>match m{Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(b)))=>fragments+=fragment_to(&b,&mut right).await,_=>break},
             m=right.next()=>match m{Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(b)))=>fragments+=fragment_to(&b,&mut left).await,_=>break},
            }
        }
        fragments
    });
    let (keep, token) = cancellation();
    let (c, h) = tokio::time::timeout(Duration::from_secs(5), async {
        let (c, h) = tokio::join!(
            connect(
                Unbound::new(cio),
                local(true),
                pin(false),
                Duration::from_secs(3),
                token.clone()
            ),
            accept(
                Unbound::new(hio),
                local(false),
                pin(true),
                Duration::from_secs(3),
                token
            )
        );
        tokio::join!(
            bind_client(c.unwrap(), trust()),
            bind_host(h.unwrap(), snapshot())
        )
    })
    .await
    .unwrap();
    let (c, h) = (c.unwrap(), h.unwrap());
    assert_eq!(c.context(), h.context());
    assert_eq!(c.context().policy_epoch(), "13");
    keep.cancel();
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(c.close(), h.close())
    })
    .await
    .unwrap();
    let _ = drive_cancel.send(true);
    tokio::time::timeout(Duration::from_secs(2), async {
        cdrive.await.unwrap().err().unwrap();
        hdrive.await.unwrap().err().unwrap();
    })
    .await
    .unwrap();
    let fragments = tokio::time::timeout(Duration::from_secs(2), bridge)
        .await
        .unwrap()
        .unwrap();
    assert!(fragments > 2);
    eprintln!("TEST_ONLY_BIND_WS_MUX_FRAGMENTS={fragments}");
}
