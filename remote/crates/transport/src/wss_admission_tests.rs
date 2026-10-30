use super::*;
use futures_util::{SinkExt, StreamExt};
use ring::{rand::SystemRandom, signature};
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn point() -> Vec<u8> {
    hex(
        "0460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299",
    )
}
struct PublicFixture(signature::EcdsaKeyPair);
impl RelaySigner for PublicFixture {
    fn sign<'a>(
        &'a self,
        r: &'a SigningRequest,
    ) -> Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send + 'a>> {
        Box::pin(async move {
            assert_eq!(r.key_id(), [1; 16]);
            assert!(r.role() == RelayRole::Host);
            assert!(r.origin().starts_with("wss://foobar.com:"));
            let sig = self
                .0
                .sign(&SystemRandom::new(), r.transcript())
                .map_err(|_| SignError)?;
            Ok(sig.as_ref().try_into().unwrap())
        })
    }
}
fn identity() -> RelayAdmissionIdentity {
    let scalar = hex("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721");
    let key = signature::EcdsaKeyPair::from_private_key_and_public_key(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &scalar,
        &point(),
        &SystemRandom::new(),
    )
    .unwrap();
    RelayAdmissionIdentity::new([1; 16], RelayRole::Host, Arc::new(PublicFixture(key)))
}
fn tls() -> tokio_rustls::TlsAcceptor {
    let certs = rustls_pemfile::certs(&mut &include_bytes!("../tests/fixtures/chain.pem")[..])
        .unwrap()
        .into_iter()
        .map(rustls::pki_types::CertificateDer::from)
        .collect();
    let key =
        rustls_pemfile::pkcs8_private_keys(&mut &include_bytes!("../tests/fixtures/end.key")[..])
            .unwrap()
            .remove(0);
    let mut c = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key.into()))
    .unwrap();
    c.alpn_protocols = vec![b"http/1.1".to_vec()];
    tokio_rustls::TlsAcceptor::from(Arc::new(c))
}
async fn trial(mode: u8) -> Result<AdmittedSocket, ConnectError> {
    trial_budget(mode, Duration::from_secs(2)).await
}
async fn trial_budget(mode: u8, budget: Duration) -> Result<AdmittedSocket, ConnectError> {
    trial_with_identity(mode, budget, identity()).await
}
async fn trial_with_identity(
    mode: u8,
    budget: Duration,
    id: RelayAdmissionIdentity,
) -> Result<AdmittedSocket, ConnectError> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = l.local_addr().unwrap();
    let origin = format!("wss://foobar.com:{}/relay", address.port());
    let endpoint = origin.clone();
    let server = tokio::spawn(async move {
        let (s, _) = l.accept().await.unwrap();
        let tls = tls().accept(s).await.unwrap();
        let mut ws =
            tokio_tungstenite::accept_async_with_config(tls, Some(crate::websocket_config()))
                .await
                .unwrap();
        if mode == 7 {
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        let mut challenge = vec![1, 0];
        challenge.extend_from_slice(&[3; 32]);
        challenge.extend_from_slice(&[4; 32]);
        if mode == 1 {
            challenge[0] = 2;
        }
        let first = match mode {
            3 => tokio_tungstenite::tungstenite::Message::Binary(vec![1, 0].into()),
            4 => tokio_tungstenite::tungstenite::Message::Text("invalid".into()),
            5 => tokio_tungstenite::tungstenite::Message::Close(None),
            _ => tokio_tungstenite::tungstenite::Message::Binary(challenge.into()),
        };
        ws.send(first).await.unwrap();
        if let Some(Ok(proof)) = ws.next().await {
            let b = proof.into_data();
            assert_eq!(b.len(), 83);
            assert_eq!(&b[..19], &[&[1, 1][..], &[1; 16], &[1][..]].concat());
            let mut transcript = b"CC_DESK_RELAY_ADMISSION\0\x01".to_vec();
            transcript.extend_from_slice(&(endpoint.len() as u16).to_be_bytes());
            transcript.extend_from_slice(endpoint.as_bytes());
            transcript.extend_from_slice(&[3; 32]);
            transcript.extend_from_slice(&[4; 32]);
            transcript.extend_from_slice(&[1; 16]);
            transcript.push(1);
            signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point())
                .verify(&transcript, &b[19..])
                .unwrap();
            if mode == 7 {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            ws.send(tokio_tungstenite::tungstenite::Message::Binary(
                vec![1, if mode == 2 { 3 } else { 2 }].into(),
            ))
            .await
            .unwrap();
        }
    });
    let result = test_only_connect_admitted_loopback(&origin, address, budget, &id).await;
    server.abort();
    let _ = server.await;
    result
}
#[tokio::test]
async fn real_tls_admission_requires_verified_raw_p256_proof() {
    assert!(trial(0).await.is_ok());
}

#[tokio::test]
async fn rejects_unexpected_challenge_version_length_type_close_and_status() {
    for mode in 1..=5 {
        assert!(matches!(trial(mode).await, Err(ConnectError::Admission)));
    }
}
#[tokio::test]
async fn original_shared_budget_does_not_reset_between_challenge_and_ack() {
    let start = Instant::now();
    assert!(matches!(
        trial_budget(7, Duration::from_millis(250)).await,
        Err(ConnectError::Deadline)
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[tokio::test]
async fn test_helper_cannot_dial_external_address_or_another_hostname() {
    let id = identity();
    for (origin, address) in [
        ("wss://foobar.com/relay", "192.0.2.1:443"),
        ("wss://another.example/relay", "127.0.0.1:443"),
    ] {
        assert!(matches!(
            test_only_connect_admitted_loopback(
                origin,
                address.parse().unwrap(),
                Duration::from_secs(1),
                &id
            )
            .await,
            Err(ConnectError::Origin)
        ));
    }
}

struct RefusingSigner;
impl RelaySigner for RefusingSigner {
    fn sign<'a>(
        &'a self,
        _r: &'a SigningRequest,
    ) -> Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send + 'a>> {
        Box::pin(async { Err(SignError) })
    }
}
struct PendingSigner;
impl RelaySigner for PendingSigner {
    fn sign<'a>(
        &'a self,
        _r: &'a SigningRequest,
    ) -> Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send + 'a>> {
        Box::pin(std::future::pending())
    }
}
#[tokio::test]
async fn async_signer_refusal_and_stall_never_complete_admission() {
    let reject = RelayAdmissionIdentity::new([1; 16], RelayRole::Host, Arc::new(RefusingSigner));
    assert!(matches!(
        trial_with_identity(0, Duration::from_secs(1), reject).await,
        Err(ConnectError::Signer)
    ));
    let wait = RelayAdmissionIdentity::new([1; 16], RelayRole::Host, Arc::new(PendingSigner));
    let start = Instant::now();
    assert!(matches!(
        trial_with_identity(0, Duration::from_millis(100), wait).await,
        Err(ConnectError::Deadline)
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
}
struct LateReadySigner;
impl RelaySigner for LateReadySigner {
    fn sign<'a>(
        &'a self,
        _r: &'a SigningRequest,
    ) -> Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send + 'a>> {
        Box::pin(async {
            std::thread::sleep(Duration::from_millis(150));
            Ok([0; 64])
        })
    }
}
#[tokio::test]
async fn expired_ready_signer_result_is_rejected_before_proof_write() {
    let id = RelayAdmissionIdentity::new([1; 16], RelayRole::Host, Arc::new(LateReadySigner));
    assert!(matches!(
        trial_with_identity(0, Duration::from_millis(100), id).await,
        Err(ConnectError::Deadline)
    ));
}
#[tokio::test]
async fn admitted_endpoint_rejects_unsafe_or_noncanonical_origin_before_tcp() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let p = address.port();
    let id = identity();
    for origin in [
        format!("wss://FOOBAR.COM:{p}/relay"),
        format!("wss://foobar.com:{p}/relay?token=x"),
        format!("wss://foobar.com:{p}/%2f"),
        format!("wss://user@foobar.com:{p}/relay"),
        format!("wss://foobar.com:{p}/{}", "x".repeat(1024)),
    ] {
        assert!(matches!(
            connect_admitted(&origin, address, Duration::from_secs(1), &id).await,
            Err(ConnectError::Origin)
        ));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
}
