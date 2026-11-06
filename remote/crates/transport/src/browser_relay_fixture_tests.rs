//! Explicit loopback oracle. Public RFC6979/test CA only; no enrollment.
use futures_util::{SinkExt, StreamExt};
use std::{io::Write, sync::Arc};
use tokio_tungstenite::tungstenite::Message;
#[tokio::test]
#[allow(non_snake_case)]
async fn BrowserRelay_OriginalRustWssOracle_001() {
    if std::env::var("CC_DESK_TEST_ONLY_BROWSER_RELAY").as_deref() != Ok("1") {
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let endpoint = format!("wss://foobar.com:{port}/relay");
    println!("{{\"port\":{port}}}");
    std::io::stdout().flush().unwrap();
    let socket = tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
        .await
        .unwrap()
        .unwrap()
        .0;
    let certs = rustls_pemfile::certs(&mut &include_bytes!("../tests/fixtures/chain.pem")[..])
        .unwrap()
        .into_iter()
        .map(rustls::pki_types::CertificateDer::from)
        .collect();
    let key =
        rustls_pemfile::pkcs8_private_keys(&mut &include_bytes!("../tests/fixtures/end.key")[..])
            .unwrap()
            .remove(0);
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key.into()))
    .unwrap();
    let tls = tokio_rustls::TlsAcceptor::from(Arc::new(config))
        .accept(socket)
        .await
        .unwrap();
    let mut ws = tokio_tungstenite::accept_async_with_config(tls, Some(crate::websocket_config()))
        .await
        .unwrap();
    let challenge = [&[1, 0][..], &[3; 32], &[4; 32]].concat();
    ws.send(Message::Binary(challenge.clone().into()))
        .await
        .unwrap();
    let proof = ws.next().await.unwrap().unwrap().into_data();
    assert_eq!(proof.len(), 83);
    assert_eq!(&proof[..19], &[&[1, 1][..], &[7; 16], &[2][..]].concat());
    let mut transcript = b"CC_DESK_RELAY_ADMISSION\0\x01".to_vec();
    transcript.extend_from_slice(&(endpoint.len() as u16).to_be_bytes());
    transcript.extend_from_slice(endpoint.as_bytes());
    transcript.extend_from_slice(&challenge[2..]);
    transcript.extend_from_slice(&[7; 16]);
    transcript.push(2);
    let point_hex = "0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299";
    let point: Vec<_> = (0..point_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&point_hex[i..i + 2], 16).unwrap())
        .collect();
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, point)
        .verify(&transcript, &proof[19..])
        .unwrap();
    ws.send(Message::Binary(vec![1, 2].into())).await.unwrap();
    let message = ws.next().await.unwrap().unwrap().into_data();
    let frames = cc_desk_remote_protocol::decode_outer(&message).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].tunnel, [9; 16]);
    assert_eq!(frames[0].payload, b"TEST_ONLY_BROWSER_TO_RUST");
    let reply = cc_desk_remote_protocol::encode_outer(&[cc_desk_remote_protocol::Subframe {
        tunnel: [9; 16],
        payload: b"TEST_ONLY_RUST_TO_BROWSER",
    }])
    .unwrap();
    ws.send(Message::Binary(reply.into())).await.unwrap();
    assert!(matches!(ws.next().await, Some(Ok(Message::Close(_)))));
    let _ = ws.flush().await;
    println!("{{\"oracle\":\"admission-signature-and-original-codec-bidirectional\"}}");
}
