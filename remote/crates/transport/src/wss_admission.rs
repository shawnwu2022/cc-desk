//! Separate native relay-admission signer; never imports endpoint identity keys.
use super::ConnectError;
use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};
use tokio::time::Instant;
pub type AdmittedSocket =
    tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RelayRole {
    Host = 1,
    Phone = 2,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignError;
pub struct SigningRequest {
    origin: String,
    key_id: [u8; 16],
    role: RelayRole,
    transcript: Vec<u8>,
}
impl SigningRequest {
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn key_id(&self) -> [u8; 16] {
        self.key_id
    }
    pub fn role(&self) -> RelayRole {
        self.role
    }
    pub fn transcript(&self) -> &[u8] {
        &self.transcript
    }
}
/// Dedicated native relay signer. Check request origin, key ID and role before signing.
/// Constructing the returned future and every poll MUST be nonblocking/cooperative.
/// Deadlines cannot preempt synchronous provider code; never auto-reuse endpoint keys.
pub trait RelaySigner: Send + Sync {
    fn sign<'a>(
        &'a self,
        request: &'a SigningRequest,
    ) -> Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send + 'a>>;
}
pub struct RelayAdmissionIdentity {
    key_id: [u8; 16],
    role: RelayRole,
    signer: Arc<dyn RelaySigner>,
}
impl RelayAdmissionIdentity {
    pub fn new(key_id: [u8; 16], role: RelayRole, signer: Arc<dyn RelaySigner>) -> Self {
        Self {
            key_id,
            role,
            signer,
        }
    }
}
fn endpoint(origin: &str, address: SocketAddr, budget: Duration) -> Result<url::Url, ConnectError> {
    if budget.is_zero() || budget > crate::MAX_IO_BUDGET {
        return Err(ConnectError::Limit);
    }
    if origin.len() > 512 || !origin.is_ascii() || origin.contains(['%', '\\']) {
        return Err(ConnectError::Origin);
    }
    let url = url::Url::parse(origin).map_err(|_| ConnectError::Origin)?;
    if origin.len() > 512
        || !origin.is_ascii()
        || url.as_str() != origin
        || url.scheme() != "wss"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || origin.contains(['%', '\\'])
        || url.host_str().is_none()
        || url.port_or_known_default() != Some(address.port())
        || address.port() == 0
    {
        return Err(ConnectError::Origin);
    }
    Ok(url)
}
pub async fn connect_admitted(
    origin: &str,
    address: SocketAddr,
    budget: Duration,
    identity: &RelayAdmissionIdentity,
) -> Result<AdmittedSocket, ConnectError> {
    endpoint(origin, address, budget)?;
    let deadline = Instant::now()
        .checked_add(budget)
        .ok_or(ConnectError::Limit)?;
    crate::deadline::bounded(deadline, async {
        let ws = super::connect(origin, address, budget).await?;
        admit(ws, origin, identity, deadline).await
    })
    .await
    .map_err(|_| ConnectError::Deadline)?
}
async fn admit(
    mut ws: AdmittedSocket,
    origin: &str,
    identity: &RelayAdmissionIdentity,
    deadline: Instant,
) -> Result<AdmittedSocket, ConnectError> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let challenge = crate::deadline::bounded(deadline, ws.next())
        .await
        .map_err(|_| ConnectError::Deadline)?
        .ok_or(ConnectError::Admission)?
        .map_err(|_| ConnectError::Admission)?;
    let Message::Binary(challenge) = challenge else {
        return Err(ConnectError::Admission);
    };
    if challenge.len() != 66 || challenge[..2] != [1, 0] {
        return Err(ConnectError::Admission);
    }
    let mut transcript = b"CC_DESK_RELAY_ADMISSION\0\x01".to_vec();
    transcript.extend_from_slice(&(origin.len() as u16).to_be_bytes());
    transcript.extend_from_slice(origin.as_bytes());
    transcript.extend_from_slice(&challenge[2..]);
    transcript.extend_from_slice(&identity.key_id);
    transcript.push(identity.role as u8);
    let request = SigningRequest {
        origin: origin.into(),
        key_id: identity.key_id,
        role: identity.role,
        transcript,
    };
    let signature = crate::deadline::bounded(deadline, identity.signer.sign(&request))
        .await
        .map_err(|_| ConnectError::Deadline)?
        .map_err(|_| ConnectError::Signer)?;
    let mut proof = vec![1, 1];
    proof.extend_from_slice(&identity.key_id);
    proof.push(identity.role as u8);
    proof.extend_from_slice(&signature);
    crate::deadline::bounded(deadline, ws.send(Message::Binary(proof.into())))
        .await
        .map_err(|_| ConnectError::Deadline)?
        .map_err(|_| ConnectError::Admission)?;
    let accepted = crate::deadline::bounded(deadline, ws.next())
        .await
        .map_err(|_| ConnectError::Deadline)?
        .ok_or(ConnectError::Admission)?
        .map_err(|_| ConnectError::Admission)?;
    if !matches!(accepted,Message::Binary(ref b) if b.as_ref()==[1,2]) {
        return Err(ConnectError::Admission);
    }
    Ok(ws)
}
/// Public known fixture trust, only for explicit local qualification; never production.
#[cfg(any(test, feature = "test-only-loopback"))]
#[doc(hidden)]
pub async fn test_only_connect_admitted_loopback(
    origin: &str,
    address: SocketAddr,
    budget: Duration,
    identity: &RelayAdmissionIdentity,
) -> Result<AdmittedSocket, ConnectError> {
    let url = endpoint(origin, address, budget)?;
    if !address.ip().is_loopback() || url.host_str() != Some("foobar.com") {
        return Err(ConnectError::Origin);
    }
    let deadline = Instant::now()
        .checked_add(budget)
        .ok_or(ConnectError::Limit)?;
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut &include_bytes!("../tests/fixtures/root.pem")[..])
        .map_err(|_| ConnectError::Transport)?
    {
        roots
            .add(rustls::pki_types::CertificateDer::from(cert))
            .map_err(|_| ConnectError::Transport)?;
    }
    crate::deadline::bounded(deadline, async {
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .map_err(|_| ConnectError::Transport)?;
        let ws = super::upgrade(
            origin,
            rustls::pki_types::ServerName::try_from("foobar.com").expect("fixed test name"),
            socket,
            super::with_roots(roots),
        )
        .await?;
        admit(ws, origin, identity, deadline).await
    })
    .await
    .map_err(|_| ConnectError::Deadline)?
}
#[cfg(test)]
#[path = "wss_admission_tests.rs"]
mod tests;
