#![allow(non_snake_case)]
#[path = "support/identities.rs"]
mod identities;
use cc_desk_remote_security::*;
use std::time::Duration;
fn local(client: bool, corrupt: bool) -> LocalIdentity {
    let (k, p) = identities::identity(client, corrupt);
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
#[tokio::test]
async fn Crypto_MutualRpkDuplex_001() {
    let (c, s) = tokio::io::duplex(65536);
    let (_tx, rx) = cancellation();
    let (c, s) = tokio::join!(
        connect(
            Unbound::new(c),
            local(true, false),
            pin(false),
            Duration::from_secs(2),
            rx.clone()
        ),
        accept(
            Unbound::new(s),
            local(false, false),
            pin(true),
            Duration::from_secs(2),
            rx
        )
    );
    let c = c.expect("real client TLS");
    let s = s.expect("real server TLS");
    for e in [c.evidence(), s.evidence()] {
        assert_eq!(e.protocol, rustls::ProtocolVersion::TLSv1_3);
        assert_eq!(e.group, rustls::NamedGroup::X25519);
        assert!(e.full_handshake && e.raw_public_key && e.alpn_matches);
        assert!(matches!(
            e.cipher,
            rustls::CipherSuite::TLS13_AES_128_GCM_SHA256
                | rustls::CipherSuite::TLS13_CHACHA20_POLY1305_SHA256
        ));
    }
}

#[test]
fn Crypto_CanonicalSpkiAndSigner_001() {
    let (key, bytes) = identities::identity(false, false);
    for index in [0, 1, 4, 17, 25, 26] {
        let mut bad = bytes.clone();
        bad[index] ^= 1;
        assert!(matches!(P256Spki::new(&bad), Err(SecurityError::Key)));
    }
    assert!(matches!(P256Spki::new(&[0; 91]), Err(SecurityError::Key)));
    assert!(matches!(
        P256Spki::new(&bytes[..90]),
        Err(SecurityError::Key)
    ));
    let (_, other) = identities::identity(true, false);
    assert!(matches!(
        LocalIdentity::new(Role::Host, P256Spki::new(&other).unwrap(), key.key.clone()),
        Err(SecurityError::Identity)
    ));
    let public = P256Spki::new(&bytes).unwrap();
    assert_eq!(
        public
            .endpoint_id(Role::Host)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "27b908a684ebfe768e4613c594495dbf502be7f7ad9d92182f1a3dac35e94d54"
    );
    assert_eq!(
        P256Spki::new(&other)
            .unwrap()
            .endpoint_id(Role::Client)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "b76b5e237aa760c2b1e65bd44e86a696a8fda16c0fd970366e1f86c1f8a73de2"
    );
    assert_ne!(
        public.endpoint_id(Role::Host),
        public.endpoint_id(Role::Client)
    );
}
#[tokio::test]
async fn Crypto_RoleAndBudgetRejectBeforeIo_001() {
    use tokio::io::AsyncReadExt;
    for (budget, expected) in [
        (Duration::ZERO, SecurityError::Limit),
        (Duration::MAX, SecurityError::Limit),
        (Duration::from_secs(1), SecurityError::Role),
    ] {
        let (io, mut peer) = tokio::io::duplex(128);
        let (_tx, rx) = cancellation();
        assert_eq!(
            connect(
                Unbound::new(io),
                local(false, false),
                pin(false),
                budget,
                rx
            )
            .await
            .err(),
            Some(expected)
        );
        assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
    }
}
#[tokio::test]
async fn Crypto_WrongBothPinsAndCorruptSignature_001() {
    for case in 0..4 {
        let (c, s) = tokio::io::duplex(65536);
        let (_tx, rx) = cancellation();
        let client_pin = if case == 0 {
            let (_, p) = identities::identity(true, false);
            PeerPin::new(Role::Host, P256Spki::new(&p).unwrap())
        } else {
            pin(false)
        };
        let server_pin = if case == 1 {
            let (_, p) = identities::identity(false, false);
            PeerPin::new(Role::Client, P256Spki::new(&p).unwrap())
        } else {
            pin(true)
        };
        let (c, s) = tokio::join!(
            connect(
                Unbound::new(c),
                local(true, case == 2),
                client_pin,
                Duration::from_secs(1),
                rx.clone()
            ),
            accept(
                Unbound::new(s),
                local(false, case == 3),
                server_pin,
                Duration::from_secs(1),
                rx
            )
        );
        if case == 0 || case == 3 {
            assert_eq!(c.err(), Some(SecurityError::Tls));
        } else {
            assert_eq!(s.err(), Some(SecurityError::Tls));
        }
    }
}
#[tokio::test]
async fn Crypto_SilentDeadlineAndHalfClose_001() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for half_close in [false, true] {
        let (io, mut peer) = tokio::io::duplex(128);
        let (_tx, rx) = cancellation();
        if half_close {
            peer.shutdown().await.unwrap();
        }
        let start = tokio::time::Instant::now();
        let r = accept(
            Unbound::new(io),
            local(false, false),
            pin(true),
            Duration::from_millis(80),
            rx,
        )
        .await;
        assert_eq!(
            r.err(),
            Some(if half_close {
                SecurityError::Tls
            } else {
                SecurityError::Deadline
            })
        );
        if !half_close {
            assert!(start.elapsed() >= Duration::from_millis(70));
        }
        assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
    }
}
#[tokio::test]
async fn Crypto_HandshakeCancellationDropsOwnedIo_001() {
    use tokio::io::AsyncReadExt;
    for abort in [false, true] {
        let (io, mut peer) = tokio::io::duplex(128);
        let (tx, rx) = cancellation();
        let task = tokio::spawn(accept(
            Unbound::new(io),
            local(false, false),
            pin(true),
            Duration::from_secs(1),
            rx,
        ));
        tokio::task::yield_now().await;
        if abort {
            task.abort();
            assert!(task.await.is_err_and(|e| e.is_cancelled()));
        } else {
            tx.cancel();
            assert_eq!(
                task.await.ok().unwrap().err(),
                Some(SecurityError::Cancelled)
            );
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), peer.read(&mut [0; 1]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }
}
#[tokio::test]
async fn Crypto_CancellationNeverReopensHandshake_001() {
    use tokio::io::AsyncReadExt;
    let (io, mut peer) = tokio::io::duplex(128);
    let (tx, rx) = cancellation();
    let task = tokio::spawn(accept(
        Unbound::new(io),
        local(false, false),
        pin(true),
        Duration::from_millis(100),
        rx,
    ));
    tokio::task::yield_now().await;
    tx.cancel();
    tx.cancel();
    assert_eq!(
        task.await.ok().unwrap().err(),
        Some(SecurityError::Cancelled)
    );
    assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
}
fn external_client(identity: bool, wrong_alpn: bool) -> rustls::ClientConfig {
    external_with_suite(identity, wrong_alpn, None)
}
fn external_with_suite(
    identity: bool,
    wrong_alpn: bool,
    suite: Option<rustls::CipherSuite>,
) -> rustls::ClientConfig {
    use std::sync::Arc;
    let (_, host) = identities::identity(false, false);
    let (key, _) = identities::identity(true, false);
    let mut provider = rustls::crypto::ring::default_provider();
    if let Some(suite) = suite {
        provider.cipher_suites.retain(|s| s.suite() == suite);
    }
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(identities::Pin {
            expected: host,
            counts: Arc::new(Default::default()),
        }));
    let mut cfg = if identity {
        builder.with_client_cert_resolver(Arc::new(
            rustls::client::AlwaysResolvesClientRawPublicKeys::new(key),
        ))
    } else {
        builder.with_client_cert_resolver(Arc::new(identities::EmptyRpkClient))
    };
    cfg.alpn_protocols = vec![if wrong_alpn {
        b"wrong".to_vec()
    } else {
        ALPN.to_vec()
    }];
    cfg.enable_early_data = false;
    cfg.resumption = rustls::client::Resumption::disabled();
    cfg
}
#[tokio::test]
async fn Crypto_BothAllowedCipherSuitesActuallyNegotiate_001() {
    for suite in [
        rustls::CipherSuite::TLS13_AES_128_GCM_SHA256,
        rustls::CipherSuite::TLS13_CHACHA20_POLY1305_SHA256,
    ] {
        let (c, s) = tokio::io::duplex(65536);
        let (_tx, rx) = cancellation();
        let external = tokio_rustls::TlsConnector::from(std::sync::Arc::new(external_with_suite(
            true,
            false,
            Some(suite),
        )));
        let (client, server) = tokio::join!(
            external.connect(
                rustls::pki_types::ServerName::try_from("test.invalid").unwrap(),
                c
            ),
            accept(
                Unbound::new(s),
                local(false, false),
                pin(true),
                Duration::from_secs(1),
                rx
            )
        );
        assert!(client.is_ok());
        assert_eq!(server.as_ref().ok().unwrap().evidence().cipher, suite);
    }
}
#[tokio::test]
async fn Crypto_MissingClientIdentityAndWrongAlpn_001() {
    use std::sync::Arc;
    for (identity, alpn) in [(false, false), (true, true)] {
        let (c, s) = tokio::io::duplex(65536);
        let (_tx, rx) = cancellation();
        let raw = tokio_rustls::TlsConnector::from(Arc::new(external_client(identity, alpn)));
        let (_c, s) = tokio::join!(
            raw.connect(
                rustls::pki_types::ServerName::try_from("test.invalid").unwrap(),
                c
            ),
            accept(
                Unbound::new(s),
                local(false, false),
                pin(true),
                Duration::from_secs(1),
                rx
            )
        );
        assert_eq!(s.err(), Some(SecurityError::Tls));
    }
}
#[tokio::test]
async fn Crypto_ValidHelloDripCannotRefreshTotalDeadline_001() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut peer, s) = tokio::io::duplex(128);
    let (_tx, rx) = cancellation();
    let mut raw = rustls::ClientConnection::new(
        std::sync::Arc::new(external_client(true, false)),
        rustls::pki_types::ServerName::try_from("test.invalid").unwrap(),
    )
    .unwrap();
    let mut hello = Vec::new();
    raw.write_tls(&mut hello).unwrap();
    let start = tokio::time::Instant::now();
    let attack = async {
        let mut progress = 0;
        for byte in hello {
            if peer.write_all(&[byte]).await.is_err() {
                break;
            }
            progress += 1;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut tail = Vec::new();
        peer.read_to_end(&mut tail).await.unwrap();
        progress
    };
    let (server, progress) = tokio::join!(
        accept(
            Unbound::new(s),
            local(false, false),
            pin(true),
            Duration::from_millis(120),
            rx
        ),
        attack
    );
    assert_eq!(server.err(), Some(SecurityError::Deadline));
    assert!(progress >= 3);
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[tokio::test]
async fn Crypto_ClientSilenceAndHalfCloseDropIo_001() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for half in [false, true] {
        let (io, mut peer) = tokio::io::duplex(32);
        let (_tx, rx) = cancellation();
        if half {
            peer.shutdown().await.unwrap();
        }
        let result = connect(
            Unbound::new(io),
            local(true, false),
            pin(false),
            Duration::from_millis(80),
            rx,
        )
        .await;
        // With a tiny unread peer buffer, even a half-close may first block ClientHello.
        assert!(matches!(
            result.err(),
            Some(SecurityError::Deadline | SecurityError::Tls)
        ));
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), peer.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert!(bytes.len() <= 32);
    }
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
async fn Crypto_RealLoopbackWsMuxFragmentedRpk_001() {
    use futures_util::StreamExt;
    use std::collections::HashMap;
    let (cws, mut left) = ws_pair().await;
    let (sws, mut right) = ws_pair().await;
    let (cio, cr) = cc_desk_remote_transport::bounded_tunnel();
    let (sio, sr) = cc_desk_remote_transport::bounded_tunnel();
    let (cancel, rx) = tokio::sync::watch::channel(false);
    let client_drive = tokio::spawn(cc_desk_remote_transport::drive(
        cws,
        HashMap::from([([1; 16], cr)]),
        rx.clone(),
        Duration::from_secs(4),
    ));
    let server_drive = tokio::spawn(cc_desk_remote_transport::drive(
        sws,
        HashMap::from([([1; 16], sr)]),
        rx,
        Duration::from_secs(4),
    ));
    let relay = tokio::spawn(async move {
        let mut fragments = 0;
        loop {
            tokio::select! {
             m=left.next()=>match m{Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(b)))=>fragments+=fragment_to(&b,&mut right).await,_=>break},
             m=right.next()=>match m{Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(b)))=>fragments+=fragment_to(&b,&mut left).await,_=>break},
            }
        }
        fragments
    });
    let (_tls_cancel, rx) = cancellation();
    let (c, s) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            connect(
                Unbound::new(cio),
                local(true, false),
                pin(false),
                Duration::from_secs(3),
                rx.clone()
            ),
            accept(
                Unbound::new(sio),
                local(false, false),
                pin(true),
                Duration::from_secs(3),
                rx
            )
        )
    })
    .await
    .unwrap();
    assert!(
        c.is_ok() && s.is_ok(),
        "inner RPK over actual WS/mux failed"
    );
    assert!(
        c.as_ref().ok().unwrap().evidence().raw_public_key
            && s.as_ref().ok().unwrap().evidence().full_handshake
    );
    drop((c, s));
    let _ = cancel.send(true);
    tokio::time::timeout(Duration::from_secs(2), async {
        client_drive.await.unwrap().err().unwrap();
        server_drive.await.unwrap().err().unwrap();
    })
    .await
    .unwrap();
    let fragments = tokio::time::timeout(Duration::from_secs(2), relay)
        .await
        .unwrap()
        .unwrap();
    assert!(fragments > 2);
    eprintln!("TEST_ONLY_WS_MUX_FRAGMENTS={fragments}");
}
