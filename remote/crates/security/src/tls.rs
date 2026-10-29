use crate::pin::Pin;
use rustls::pki_types::{CertificateDer, ServerName};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};
pub const ALPN: &[u8] = b"cc-desk-e2e/1";
pub const MAX_HANDSHAKE_BUDGET: Duration = Duration::from_secs(10);
const SPKI_HEAD:&[u8]=b"\x30\x59\x30\x13\x06\x07\x2a\x86\x48\xce\x3d\x02\x01\x06\x08\x2a\x86\x48\xce\x3d\x03\x01\x07\x03\x42\x00\x04";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityError {
    Key,
    Identity,
    Role,
    Limit,
    Tls,
    Negotiated,
    Deadline,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Host,
    Client,
}
#[derive(Clone)]
pub struct P256Spki(pub(crate) Vec<u8>);
impl P256Spki {
    pub fn new(bytes: &[u8]) -> Result<Self, SecurityError> {
        if bytes.len() != 91 || !bytes.starts_with(SPKI_HEAD) {
            return Err(SecurityError::Key);
        }
        Ok(Self(bytes.to_vec()))
    }
    pub fn endpoint_id(&self, role: Role) -> [u8; 32] {
        let domain: &[u8] = match role {
            Role::Host => b"CC-DESK-HOST-ID-v1\0",
            Role::Client => b"CC-DESK-CLIENT-ID-v1\0",
        };
        let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
        digest.update(domain);
        digest.update(&self.0);
        digest.finish().as_ref().try_into().unwrap()
    }
}
impl std::fmt::Debug for P256Spki {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("P256Spki(<public-key>)")
    }
}
pub struct LocalIdentity {
    role: Role,
    public: P256Spki,
    signer: Arc<dyn rustls::sign::SigningKey>,
}
impl LocalIdentity {
    pub fn new(
        role: Role,
        public: P256Spki,
        signer: Arc<dyn rustls::sign::SigningKey>,
    ) -> Result<Self, SecurityError> {
        if signer.algorithm() != rustls::SignatureAlgorithm::ECDSA
            || signer.public_key().is_none_or(|p| p.as_ref() != public.0)
            || signer
                .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
                .is_none_or(|s| s.scheme() != rustls::SignatureScheme::ECDSA_NISTP256_SHA256)
        {
            return Err(SecurityError::Identity);
        }
        Ok(Self {
            role,
            public,
            signer,
        })
    }
    fn certified(self) -> Arc<rustls::sign::CertifiedKey> {
        Arc::new(rustls::sign::CertifiedKey::new(
            vec![CertificateDer::from(self.public.0)],
            self.signer,
        ))
    }
}
#[derive(Clone)]
pub struct PeerPin {
    role: Role,
    public: P256Spki,
}
impl PeerPin {
    pub fn new(role: Role, public: P256Spki) -> Self {
        Self { role, public }
    }
}
pub struct Unbound<S>(S);
impl<S> Unbound<S> {
    pub fn new(io: S) -> Self {
        Self(io)
    }
}
#[derive(Debug)]
pub struct TransportEvidence {
    pub protocol: rustls::ProtocolVersion,
    pub cipher: rustls::CipherSuite,
    pub group: rustls::NamedGroup,
    pub full_handshake: bool,
    pub raw_public_key: bool,
    pub alpn_matches: bool,
    pub peer_endpoint_id: [u8; 32],
}
pub struct Pending<S> {
    pub(crate) _io: tokio_rustls::TlsStream<S>,
    pub(crate) _context: (Role, [u8; 32], Role, [u8; 32]),
    pub(crate) _cancel: crate::CancelToken,
    pub(crate) _deadline: tokio::time::Instant,
    evidence: TransportEvidence,
}
impl<S> Pending<S> {
    pub fn evidence(&self) -> &TransportEvidence {
        &self.evidence
    }
}
fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    let mut p = rustls::crypto::ring::default_provider();
    p.kx_groups = vec![rustls::crypto::ring::kx_group::X25519];
    p.cipher_suites = vec![
        rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256,
        rustls::crypto::ring::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
    ];
    Arc::new(p)
}
fn evidence(
    state: &rustls::CommonState,
    pin: &PeerPin,
) -> Result<TransportEvidence, SecurityError> {
    let e = TransportEvidence {
        protocol: state.protocol_version().ok_or(SecurityError::Negotiated)?,
        cipher: state
            .negotiated_cipher_suite()
            .ok_or(SecurityError::Negotiated)?
            .suite(),
        group: state
            .negotiated_key_exchange_group()
            .ok_or(SecurityError::Negotiated)?
            .name(),
        full_handshake: state.handshake_kind() == Some(rustls::HandshakeKind::Full),
        raw_public_key: state
            .peer_certificates()
            .is_some_and(|p| p.len() == 1 && p[0].as_ref() == pin.public.0),
        alpn_matches: state.alpn_protocol() == Some(ALPN),
        peer_endpoint_id: pin.public.endpoint_id(pin.role),
    };
    if e.protocol != rustls::ProtocolVersion::TLSv1_3
        || e.group != rustls::NamedGroup::X25519
        || !e.full_handshake
        || !e.raw_public_key
        || !e.alpn_matches
        || !matches!(
            e.cipher,
            rustls::CipherSuite::TLS13_AES_128_GCM_SHA256
                | rustls::CipherSuite::TLS13_CHACHA20_POLY1305_SHA256
        )
    {
        return Err(SecurityError::Negotiated);
    }
    Ok(e)
}
fn end(budget: Duration) -> Result<tokio::time::Instant, SecurityError> {
    if budget.is_zero() || budget > MAX_HANDSHAKE_BUDGET {
        return Err(SecurityError::Limit);
    }
    tokio::time::Instant::now()
        .checked_add(budget)
        .ok_or(SecurityError::Limit)
}
async fn bounded<F, T>(
    future: F,
    deadline: tokio::time::Instant,
    cancel: &mut crate::CancelToken,
) -> Result<T, SecurityError>
where
    F: std::future::Future<Output = Result<T, std::io::Error>>,
{
    if cancel.is_cancelled() {
        return Err(SecurityError::Cancelled);
    }
    tokio::select! {biased;
     _=cancel.cancelled()=>Err(SecurityError::Cancelled),
     _=tokio::time::sleep_until(deadline)=>Err(SecurityError::Deadline),
     r=future=>{if tokio::time::Instant::now()>=deadline{Err(SecurityError::Deadline)}else if cancel.is_cancelled(){Err(SecurityError::Cancelled)}else{r.map_err(|_|SecurityError::Tls)}}
    }
}
fn finish<T>(
    deadline: tokio::time::Instant,
    cancel: &crate::CancelToken,
    validate: impl FnOnce() -> Result<T, SecurityError>,
) -> Result<T, SecurityError> {
    let value = validate()?;
    if tokio::time::Instant::now() >= deadline {
        return Err(SecurityError::Deadline);
    }
    if cancel.is_cancelled() {
        return Err(SecurityError::Cancelled);
    }
    Ok(value)
}
pub async fn connect<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    io: Unbound<S>,
    local: LocalIdentity,
    pin: PeerPin,
    budget: Duration,
    mut cancel: crate::CancelToken,
) -> Result<Pending<S>, SecurityError> {
    let deadline = end(budget)?;
    if local.role != Role::Client || pin.role != Role::Host {
        return Err(SecurityError::Role);
    }
    let context = (
        local.role,
        local.public.endpoint_id(local.role),
        pin.role,
        pin.public.endpoint_id(pin.role),
    );
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| SecurityError::Identity)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pin(pin.public.clone())))
        .with_client_cert_resolver(Arc::new(
            rustls::client::AlwaysResolvesClientRawPublicKeys::new(local.certified()),
        ));
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.enable_early_data = false;
    config.enable_sni = false;
    config.key_log = Arc::new(rustls::NoKeyLog {});
    config.resumption = rustls::client::Resumption::disabled();
    let tls = bounded(
        tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("endpoint.invalid").unwrap(), io.0),
        deadline,
        &mut cancel,
    )
    .await?;
    let e = finish(deadline, &cancel, || evidence(tls.get_ref().1, &pin))?;
    Ok(Pending {
        _io: tokio_rustls::TlsStream::Client(tls),
        _context: context,
        _cancel: cancel,
        _deadline: deadline,
        evidence: e,
    })
}
pub async fn accept<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    io: Unbound<S>,
    local: LocalIdentity,
    pin: PeerPin,
    budget: Duration,
    mut cancel: crate::CancelToken,
) -> Result<Pending<S>, SecurityError> {
    let deadline = end(budget)?;
    if local.role != Role::Host || pin.role != Role::Client {
        return Err(SecurityError::Role);
    }
    let context = (
        local.role,
        local.public.endpoint_id(local.role),
        pin.role,
        pin.public.endpoint_id(pin.role),
    );
    let mut config = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| SecurityError::Identity)?
        .with_client_cert_verifier(Arc::new(Pin(pin.public.clone())))
        .with_cert_resolver(Arc::new(
            rustls::server::AlwaysResolvesServerRawPublicKeys::new(local.certified()),
        ));
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.send_tls13_tickets = 0;
    config.max_early_data_size = 0;
    config.key_log = Arc::new(rustls::NoKeyLog {});
    if config.ticketer.enabled() {
        return Err(SecurityError::Identity);
    }
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    let tls = bounded(
        tokio_rustls::TlsAcceptor::from(Arc::new(config)).accept(io.0),
        deadline,
        &mut cancel,
    )
    .await?;
    let e = finish(deadline, &cancel, || evidence(tls.get_ref().1, &pin))?;
    Ok(Pending {
        _io: tokio_rustls::TlsStream::Server(tls),
        _context: context,
        _cancel: cancel,
        _deadline: deadline,
        evidence: e,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn final_validation_cannot_publish_after_deadline_or_cancel() {
        let (tx, rx) = crate::cancellation();
        let end = tokio::time::Instant::now() + Duration::from_millis(10);
        assert_eq!(
            finish(end, &rx, || {
                std::thread::sleep(Duration::from_millis(30));
                Ok(())
            }),
            Err(SecurityError::Deadline)
        );
        let end = tokio::time::Instant::now() + Duration::from_secs(1);
        assert_eq!(
            finish(end, &rx, || {
                tx.cancel();
                Ok(())
            }),
            Err(SecurityError::Cancelled)
        );
        let (tx, rx) = crate::cancellation();
        assert_eq!(
            finish(
                tokio::time::Instant::now() + Duration::from_secs(1),
                &rx,
                || {
                    tx.cancel();
                    tx.cancel();
                    Ok(())
                }
            ),
            Err(SecurityError::Cancelled)
        );
    }
}
