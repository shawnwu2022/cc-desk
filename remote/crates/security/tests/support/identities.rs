use base64::Engine;
use ring::signature::KeyPair;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::sign::{CertifiedKey, Signer, SigningKey};
use rustls::{DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Default, Debug)]
pub struct Counts {
    pub pins: AtomicUsize,
    pub signatures: AtomicUsize,
}
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|n| u8::from_str_radix(&s[n..n + 2], 16).unwrap())
        .collect()
}
struct TestKey {
    key: Arc<ring::signature::EcdsaKeyPair>,
    spki: Vec<u8>,
    corrupt: bool,
}
impl std::fmt::Debug for TestKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TEST_ONLY_P256_REDACTED")
    }
}
impl Signer for TestKey {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, Error> {
        let mut sig = self
            .key
            .sign(&ring::rand::SystemRandom::new(), message)
            .map_err(|_| Error::General("TEST_SIGN".into()))?
            .as_ref()
            .to_vec();
        if self.corrupt {
            let last = sig.len() - 1;
            sig[last] ^= 1;
        }
        Ok(sig)
    }
    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ECDSA_NISTP256_SHA256
    }
}
impl SigningKey for TestKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered
            .contains(&SignatureScheme::ECDSA_NISTP256_SHA256)
            .then(|| {
                Box::new(Self {
                    key: self.key.clone(),
                    spki: self.spki.clone(),
                    corrupt: self.corrupt,
                }) as Box<dyn Signer>
            })
    }
    fn algorithm(&self) -> rustls::SignatureAlgorithm {
        rustls::SignatureAlgorithm::ECDSA
    }
    fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
        Some(SubjectPublicKeyInfoDer::from(self.spki.as_slice()))
    }
}
pub fn identity(client: bool, corrupt: bool) -> (Arc<CertifiedKey>, Vec<u8>) {
    // Both scalars are openly published TEST_ONLY fixtures; never enroll these keys.
    let (scalar, point) = if client {
        let v: serde_json::Value =
            serde_json::from_slice(include_bytes!("../vectors/cose/public-sign1-02.json")).unwrap();
        let mut p = vec![4];
        for name in ["x", "y"] {
            p.extend(
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(v[name].as_str().unwrap())
                    .unwrap(),
            )
        }
        (
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode("V8kgd2ZBRuh2dgyVINBUqpPDr7BOMGcF22CQMIUHtNM")
                .unwrap(),
            p,
        )
    } else {
        let v: serde_json::Value =
            serde_json::from_slice(include_bytes!("../vectors/signatures/rfc6979-p256.json"))
                .unwrap();
        (
            hex("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"),
            hex(v["public_key_sec1"].as_str().unwrap()),
        )
    };
    let key = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
        &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
        &scalar,
        &point,
        &ring::rand::SystemRandom::new(),
    )
    .unwrap();
    assert_eq!(key.public_key().as_ref(), point);
    let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(point);
    let signing = Arc::new(TestKey {
        key: Arc::new(key),
        spki: spki.clone(),
        corrupt,
    });
    (
        Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(spki.clone())],
            signing,
        )),
        spki,
    )
}
pub struct Pin {
    pub expected: Vec<u8>,
    pub counts: Arc<Counts>,
}
#[derive(Debug)]
pub struct EmptyRpkClient;
impl rustls::client::ResolvesClientCert for EmptyRpkClient {
    fn resolve(&self, _: &[&[u8]], _: &[SignatureScheme]) -> Option<Arc<CertifiedKey>> {
        None
    }
    fn has_certs(&self) -> bool {
        false
    }
    fn only_raw_public_keys(&self) -> bool {
        true
    }
}
impl std::fmt::Debug for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TEST_ONLY_EXACT_RPK_PIN")
    }
}
impl Pin {
    fn check(&self, key: &CertificateDer<'_>, chain: &[CertificateDer<'_>]) -> Result<(), Error> {
        if key.as_ref() != self.expected || !chain.is_empty() {
            self.counts.pins.fetch_add(1, Ordering::SeqCst);
            return Err(Error::General("TEST_PIN_REJECT".into()));
        }
        Ok(())
    }
    fn signature(
        &self,
        msg: &[u8],
        key: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        let result = if dss.scheme == SignatureScheme::ECDSA_NISTP256_SHA256 {
            rustls::crypto::verify_tls13_signature_with_raw_key(
                msg,
                &SubjectPublicKeyInfoDer::from(key.as_ref()),
                dss,
                &rustls::crypto::ring::default_provider().signature_verification_algorithms,
            )
        } else {
            Err(Error::General("TEST_SCHEME_REJECT".into()))
        };
        if result.is_err() {
            self.counts.signatures.fetch_add(1, Ordering::SeqCst);
        }
        result
    }
}
impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.check(end, chain)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("TLS12_DISABLED".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        k: &CertificateDer<'_>,
        s: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.signature(m, k, s)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}
impl ClientCertVerifier for Pin {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        end: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        self.check(end, chain)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("TLS12_DISABLED".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        k: &CertificateDer<'_>,
        s: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.signature(m, k, s)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}
