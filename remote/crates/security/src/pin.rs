use crate::tls::P256Spki;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};
#[derive(Debug)]
pub(crate) struct Pin(pub(crate) P256Spki);
impl Pin {
    fn check(&self, key: &CertificateDer<'_>, chain: &[CertificateDer<'_>]) -> Result<(), Error> {
        if key.as_ref() != self.0.0 || !chain.is_empty() {
            return Err(Error::General("RPK_PIN_REJECT".into()));
        }
        Ok(())
    }
    fn signature(
        &self,
        msg: &[u8],
        key: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        if dss.scheme == SignatureScheme::ECDSA_NISTP256_SHA256 {
            rustls::crypto::verify_tls13_signature_with_raw_key(
                msg,
                &SubjectPublicKeyInfoDer::from(key.as_ref()),
                dss,
                &rustls::crypto::ring::default_provider().signature_verification_algorithms,
            )
        } else {
            Err(Error::General("RPK_SCHEME_REJECT".into()))
        }
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
