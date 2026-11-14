//! Disposable explicit TEST_ONLY loopback bridge. Original Relay admission/forwarding.
use cc_desk_relay::forwarding::{Device, Relay, Role, Route};
use cc_desk_remote_protocol::{decode_outer, encode_outer, Subframe};
use futures_util::{SinkExt, StreamExt};
use std::{io::Write, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
const RENDEZVOUS: &str = "cc-desk-relay-rendezvous-v1";
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn point(which: u8) -> Vec<u8> {
    hex(if which == 1 {
        "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"
    } else {
        "0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299"
    })
}
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(
            &std::env::var("CC_DESK_TEST_ONLY_TLS_FIXTURES")
                .expect("explicit public TEST_ONLY fixture directory"),
        )
        .join(name),
    )
    .unwrap()
}
fn acceptor() -> tokio_rustls::TlsAcceptor {
    let certs = rustls_pemfile::certs(&mut &fixture("chain.pem")[..])
        .unwrap()
        .into_iter()
        .map(rustls::pki_types::CertificateDer::from)
        .collect();
    let key = rustls_pemfile::pkcs8_private_keys(&mut &fixture("end.key")[..])
        .unwrap()
        .remove(0);
    tokio_rustls::TlsAcceptor::from(Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key.into()))
        .unwrap(),
    ))
}
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    assert_eq!(
        std::env::var("CC_DESK_TEST_ONLY_BROWSER_RELAY").as_deref(),
        Ok("1")
    );
    let external = std::env::var("CC_DESK_TEST_ONLY_EXTERNAL_RELAY").ok();
    let (address, origin, embedded) = if let Some(origin) = external.as_ref() {
        // 测试外层仅允许 fixture 名称及 loopback；不创建新的内层身份或 authority。
        let port: u16 = origin
            .strip_prefix("wss://foobar.com:")
            .and_then(|v| v.strip_suffix("/relay"))
            .expect("TEST_ONLY endpoint")
            .parse()
            .unwrap();
        assert_ne!(port, 0);
        (
            std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            origin.clone(),
            None,
        )
    } else {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let origin = format!("wss://foobar.com:{}/relay", address.port());
        let relay = Relay::with_browser_origins(
            origin.clone(),
            vec![
                Device {
                    id: [6; 16],
                    public_key_sec1: point(1).try_into().unwrap(),
                    role: Role::Host,
                },
                Device {
                    id: [7; 16],
                    public_key_sec1: point(2).try_into().unwrap(),
                    role: Role::Phone,
                },
            ],
            vec![Route {
                tunnel: [9; 16],
                host: [6; 16],
                phone: [7; 16],
            }],
            vec!["https://desk.invalid".into()],
        )
        .unwrap();
        let (stop, cancel) = tokio::sync::watch::channel(false);
        let server_relay = relay.clone();
        let tls = acceptor();
        let server = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            let mut cancel = cancel;
            loop {
                tokio::select! {_=cancel.changed()=>break,accepted=listener.accept()=>{let(socket,_)=accepted.unwrap();let relay=server_relay.clone();let tls=tls.clone();let token=cancel.clone();tasks.spawn(async move{relay.serve(socket,tls,token).await});},_=tasks.join_next(),if !tasks.is_empty()=>{}}
            }
            while tasks.join_next().await.is_some() {}
        });
        (address, origin, Some((relay, stop, server)))
    };
    println!(
        "{{\"port\":{},\"endpoint\":\"{}\"}}",
        address.port(),
        origin
    );
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    let target: u16 = line.trim().parse().unwrap();
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut &fixture("root.pem")[..]).unwrap() {
        roots
            .add(rustls::pki_types::CertificateDer::from(cert))
            .unwrap();
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(
            rustls::pki_types::ServerName::try_from("foobar.com").unwrap(),
            socket,
        )
        .await
        .unwrap();
    assert_eq!(
        tls.get_ref().1.protocol_version(),
        Some(rustls::ProtocolVersion::TLSv1_3)
    );
    let mut request = origin.as_str().into_client_request().unwrap();
    if external.is_some() {
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", RENDEZVOUS.parse().unwrap());
    }
    let (mut ws, response) = tokio_tungstenite::client_async_with_config(
        request,
        tls,
        Some(cc_desk_relay::forwarding::websocket_config()),
    )
    .await
    .unwrap();
    if external.is_some() {
        assert_eq!(
            response.headers().get("Sec-WebSocket-Protocol").unwrap(),
            RENDEZVOUS
        );
    }
    let challenge = ws.next().await.unwrap().unwrap().into_data();
    assert_eq!(challenge.len(), 66);
    assert_eq!(&challenge[..2], [1, 0]);
    let mut transcript = b"CC_DESK_RELAY_ADMISSION\0\x01".to_vec();
    transcript.extend_from_slice(&(origin.len() as u16).to_be_bytes());
    transcript.extend_from_slice(origin.as_bytes());
    transcript.extend_from_slice(&challenge[2..]);
    transcript.extend_from_slice(&[6; 16]);
    transcript.push(1);
    let mut scalar = [0; 32];
    scalar[31] = 1;
    let signer = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &scalar,
        &point(1),
        &ring::rand::SystemRandom::new(),
    )
    .unwrap();
    let signature = signer
        .sign(&ring::rand::SystemRandom::new(), &transcript)
        .unwrap();
    let proof = [&[1, 1][..], &[6; 16], &[1][..], signature.as_ref()].concat();
    ws.send(Message::Binary(proof.into())).await.unwrap();
    assert_eq!(
        ws.next().await.unwrap().unwrap().into_data().as_ref(),
        [1, 2]
    );
    println!("{{\"hostAdmitted\":true}}");
    std::io::stdout().flush().unwrap();
    if external.is_some() {
        // Accepted 先报告给浏览器启动方，不能等待双端 Ready 后才打开浏览器。
        ws.send(Message::Binary([&[2, 0][..], &[9; 16]].concat().into()))
            .await
            .unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap().into_data().as_ref(),
            [&[2, 1][..], &[9; 16]].concat()
        );
        println!(
            "{{\"hostReady\":true,\"protocol\":\"{}\",\"tls\":\"TLSv1.3\"}}",
            RENDEZVOUS
        );
        std::io::stdout().flush().unwrap();
    }
    if target == 0 {
        let message = ws.next().await.unwrap().unwrap().into_data();
        let frames = decode_outer(&message).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, b"TEST_ONLY_BROWSER_TO_RUST");
        ws.send(Message::Binary(
            encode_outer(&[Subframe {
                tunnel: [9; 16],
                payload: b"TEST_ONLY_RUST_TO_BROWSER",
            }])
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    } else {
        let first = ws.next().await.unwrap().unwrap().into_data();
        let initial = decode_outer(&first).unwrap();
        assert!(initial.iter().all(|f| f.tunnel == [9; 16]));
        let mut socket = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, target))
            .await
            .unwrap();
        for frame in initial {
            socket.write_all(frame.payload).await.unwrap();
        }
        let (mut reader, mut writer) = socket.into_split();
        let (mut sink, mut stream) = ws.split();
        let send = async {
            let mut bytes = [0; 16384];
            loop {
                let n = reader.read(&mut bytes).await.unwrap();
                if n == 0 {
                    break;
                }
                sink.send(Message::Binary(
                    encode_outer(&[Subframe {
                        tunnel: [9; 16],
                        payload: &bytes[..n],
                    }])
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            }
        };
        let receive = async {
            while let Some(Ok(Message::Binary(bytes))) = stream.next().await {
                for frame in decode_outer(&bytes).unwrap() {
                    assert_eq!(frame.tunnel, [9; 16]);
                    writer.write_all(frame.payload).await.unwrap();
                }
            }
        };
        tokio::select! {_=send=>{},_=receive=>{}}
    }
    if let Some((relay, stop, server)) = embedded {
        stop.send_replace(true);
        server.await.unwrap();
        assert_eq!(relay.connected(), 0);
        assert_eq!(relay.queued_bytes(), 0);
        println!("{{\"originalRelayRetired\":true,\"connected\":0,\"queuedBytes\":0}}");
    } else {
        println!("{{\"hostBridgeRetired\":true}}");
    }
}
