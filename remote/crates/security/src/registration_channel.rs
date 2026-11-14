//! 独立登记传输，不产生 grant、终端能力或持久身份权威。
use crate::{
    CancelToken, LocalIdentity, PeerPin, Pending, SecurityError, TransportEvidence, Unbound,
};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
pub const REGISTRATION_ALPN: &[u8] = b"cc-desk-enroll/1";
pub struct RegistrationPending<S> {
    pending: Pending<S>,
    end: crate::owner_clock::Instant,
}
impl<S> RegistrationPending<S> {
    pub fn evidence(&self) -> &TransportEvidence {
        self.pending.evidence()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum RegistrationOutcome {
    Rejected,
    /// Historical information only. This is not a persistent authorization or grant.
    RunLocalCandidateInstalled {
        receipt: String,
        scope: cc_desk_remote_record::grant::SessionScope,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationError {
    Closed,
    Order,
    Bounds,
    Io,
    Deadline,
    Cancelled,
    Transcript,
}
const MAX_FRAME: usize = 16384;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    ClientCreate,
    ClientCreated,
    ClientAsserted,
    ClientAck,
    HostCreate,
    HostConfirm,
    HostFinish,
    Closed,
}
struct Wire<S> {
    io: tokio_rustls::TlsStream<S>,
    cancel: CancelToken,
    end: crate::owner_clock::Instant,
    transcript: ring::digest::Context,
}
pub struct RegistrationChannel<S> {
    wire: Option<Wire<S>>,
    stage: Stage,
    host: [u8; 32],
    client: [u8; 32],
    role: crate::Role,
    end: crate::owner_clock::Instant,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    version: u16,
    transcript: [u8; 32],
    outcome: RegistrationOutcome,
}
impl RegistrationOutcome {
    fn validate(&self) -> Result<(), RegistrationError> {
        if let Self::RunLocalCandidateInstalled { receipt, scope } = self {
            if !receipt
                .parse::<u64>()
                .ok()
                .is_some_and(|n| n > 0 && n.to_string() == *receipt)
            {
                return Err(RegistrationError::Bounds);
            }
            cc_desk_remote_record::grant::encode_scope(scope)
                .map_err(|_| RegistrationError::Bounds)?;
        }
        Ok(())
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> RegistrationPending<S> {
    pub fn into_channel(self) -> Result<RegistrationChannel<S>, RegistrationError> {
        let (role, local, _, peer) = self.pending._context;
        let (host, client) = if role == crate::Role::Host {
            (local, peer)
        } else {
            (peer, local)
        };
        let exporter = match &self.pending._io {
            tokio_rustls::TlsStream::Client(io) => io.get_ref().1.export_keying_material(
                [0u8; 32],
                b"CC-DESK-REGISTRATION-CHANNEL-v1",
                None,
            ),
            tokio_rustls::TlsStream::Server(io) => io.get_ref().1.export_keying_material(
                [0u8; 32],
                b"CC-DESK-REGISTRATION-CHANNEL-v1",
                None,
            ),
        }
        .map_err(|_| RegistrationError::Transcript)?;
        let mut transcript = ring::digest::Context::new(&ring::digest::SHA256);
        transcript.update(b"CC-DESK-REGISTRATION-TRANSCRIPT-v1\0");
        transcript.update(&host);
        transcript.update(&client);
        transcript.update(&exporter);
        let wire = Wire {
            io: self.pending._io,
            cancel: self.pending._cancel,
            end: self.end,
            transcript,
        };
        wire.check()?;
        Ok(RegistrationChannel {
            wire: Some(wire),
            stage: if role == crate::Role::Host {
                Stage::HostCreate
            } else {
                Stage::ClientCreate
            },
            host,
            client,
            role,
            end: self.end,
        })
    }
}
async fn bounded<T>(
    end: crate::owner_clock::Instant,
    cancel: &mut CancelToken,
    work: impl std::future::Future<Output = Result<T, RegistrationError>>,
) -> Result<T, RegistrationError> {
    if cancel.is_cancelled() {
        return Err(RegistrationError::Cancelled);
    }
    if crate::owner_clock::Instant::now() >= end {
        return Err(RegistrationError::Deadline);
    }
    let result = tokio::select! {biased;_=cancel.cancelled()=>Err(RegistrationError::Cancelled),_=crate::owner_clock::sleep_until(end)=>Err(RegistrationError::Deadline),result=work=>result};
    if cancel.is_cancelled() {
        return Err(RegistrationError::Cancelled);
    }
    if crate::owner_clock::Instant::now() >= end {
        return Err(RegistrationError::Deadline);
    }
    result
}
impl<S> Wire<S> {
    fn check(&self) -> Result<(), RegistrationError> {
        if self.cancel.is_cancelled() {
            Err(RegistrationError::Cancelled)
        } else if crate::owner_clock::Instant::now() >= self.end {
            Err(RegistrationError::Deadline)
        } else {
            Ok(())
        }
    }
    fn hash(&self) -> [u8; 32] {
        self.transcript
            .clone()
            .finish()
            .as_ref()
            .try_into()
            .unwrap()
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> Wire<S> {
    async fn write(&mut self, kind: u8, bytes: &[u8]) -> Result<(), RegistrationError> {
        use tokio::io::AsyncWriteExt;
        if bytes.is_empty() || bytes.len() > MAX_FRAME {
            return Err(RegistrationError::Bounds);
        }
        let mut frame = Vec::with_capacity(bytes.len() + 9);
        frame.extend(b"CDR1");
        frame.push(kind);
        frame.extend((bytes.len() as u32).to_be_bytes());
        frame.extend(bytes);
        let io = &mut self.io;
        bounded(self.end, &mut self.cancel, async {
            io.write_all(&frame)
                .await
                .map_err(|_| RegistrationError::Io)?;
            io.flush().await.map_err(|_| RegistrationError::Io)
        })
        .await?;
        self.transcript.update(&frame);
        Ok(())
    }
    async fn read(&mut self, kind: u8) -> Result<Vec<u8>, RegistrationError> {
        use tokio::io::AsyncReadExt;
        let io = &mut self.io;
        let (header, bytes) = bounded(self.end, &mut self.cancel, async {
            let mut header = [0u8; 9];
            io.read_exact(&mut header)
                .await
                .map_err(|_| RegistrationError::Io)?;
            if header[..4] != *b"CDR1" || header[4] != kind {
                return Err(RegistrationError::Order);
            }
            let length = u32::from_be_bytes(header[5..].try_into().unwrap()) as usize;
            if length == 0 || length > MAX_FRAME {
                return Err(RegistrationError::Bounds);
            }
            let mut bytes = vec![0; length];
            io.read_exact(&mut bytes)
                .await
                .map_err(|_| RegistrationError::Io)?;
            Ok((header, bytes))
        })
        .await?;
        self.transcript.update(&header);
        self.transcript.update(&bytes);
        Ok(bytes)
    }
    async fn shutdown(&mut self) -> Result<(), RegistrationError> {
        use tokio::io::AsyncWriteExt;
        let io = &mut self.io;
        bounded(self.end, &mut self.cancel, async {
            io.shutdown().await.map_err(|_| RegistrationError::Io)
        })
        .await
    }
    async fn eof(&mut self) -> Result<(), RegistrationError> {
        use tokio::io::AsyncReadExt;
        let io = &mut self.io;
        bounded(self.end, &mut self.cancel, async {
            let mut b = [0];
            let n = io.read(&mut b).await.map_err(|_| RegistrationError::Io)?;
            if n == 0 {
                Ok(())
            } else {
                Err(RegistrationError::Order)
            }
        })
        .await
    }
}
impl<S> RegistrationChannel<S> {
    pub fn check_host_start(&self) -> Result<(), RegistrationError> {
        self.check_stage(Stage::HostCreate)
    }
    pub fn check_host_decision(&self) -> Result<(), RegistrationError> {
        self.check_stage(Stage::HostFinish)
    }
    fn check_stage(&self, expected: Stage) -> Result<(), RegistrationError> {
        if self.role != crate::Role::Host || self.stage != expected {
            return Err(RegistrationError::Order);
        }
        self.wire.as_ref().ok_or(RegistrationError::Closed)?.check()
    }
    pub fn host_endpoint_id(&self) -> [u8; 32] {
        self.host
    }
    pub fn client_endpoint_id(&self) -> [u8; 32] {
        self.client
    }
    pub fn local_role(&self) -> crate::Role {
        self.role
    }
    pub fn deadline(&self) -> crate::owner_clock::Instant {
        self.end
    }
    /// Drop the original TLS IO; external mux/transport owners still require their own joins.
    pub fn close(&mut self) {
        self.stage = Stage::Closed;
        self.wire.take();
    }
    fn take(&mut self, expected: Stage) -> Result<Wire<S>, RegistrationError> {
        let wire = self.wire.take().ok_or(RegistrationError::Closed)?;
        let stage = std::mem::replace(&mut self.stage, Stage::Closed);
        if stage != expected {
            return Err(RegistrationError::Order);
        }
        wire.check()?;
        Ok(wire)
    }
    fn keep(&mut self, wire: Wire<S>, next: Stage) -> Result<(), RegistrationError> {
        wire.check()?;
        self.wire = Some(wire);
        self.stage = next;
        Ok(())
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> RegistrationChannel<S> {
    pub async fn client_create(&mut self) -> Result<Vec<u8>, RegistrationError> {
        let mut w = self.take(Stage::ClientCreate)?;
        let bytes = w.read(1).await?;
        self.keep(w, Stage::ClientCreated)?;
        Ok(bytes)
    }
    pub async fn client_created(&mut self, bytes: &[u8]) -> Result<Vec<u8>, RegistrationError> {
        let mut w = self.take(Stage::ClientCreated)?;
        w.write(2, bytes).await?;
        let bytes = w.read(3).await?;
        self.keep(w, Stage::ClientAsserted)?;
        Ok(bytes)
    }
    pub async fn client_asserted(
        &mut self,
        bytes: &[u8],
    ) -> Result<RegistrationOutcome, RegistrationError> {
        let mut w = self.take(Stage::ClientAsserted)?;
        w.write(4, bytes).await?;
        let expected = w.hash();
        let bytes = w.read(5).await?;
        let d: Decision =
            serde_json::from_slice(&bytes).map_err(|_| RegistrationError::Transcript)?;
        if d.version != 1 || d.transcript != expected {
            return Err(RegistrationError::Transcript);
        }
        d.outcome.validate()?;
        self.keep(w, Stage::ClientAck)?;
        Ok(d.outcome)
    }
    pub async fn client_ack(&mut self) -> Result<(), RegistrationError> {
        let mut w = self.take(Stage::ClientAck)?;
        w.write(6, &w.hash()).await?;
        w.shutdown().await?;
        w.eof().await
    }
    pub async fn host_create(&mut self, bytes: &[u8]) -> Result<Vec<u8>, RegistrationError> {
        let mut w = self.take(Stage::HostCreate)?;
        w.write(1, bytes).await?;
        let bytes = w.read(2).await?;
        self.keep(w, Stage::HostConfirm)?;
        Ok(bytes)
    }
    pub async fn host_confirm(&mut self, bytes: &[u8]) -> Result<Vec<u8>, RegistrationError> {
        let mut w = self.take(Stage::HostConfirm)?;
        w.write(3, bytes).await?;
        let bytes = w.read(4).await?;
        self.keep(w, Stage::HostFinish)?;
        Ok(bytes)
    }
    /// A failure here cannot undo a native installation performed before this call.
    pub async fn host_finish(
        &mut self,
        outcome: RegistrationOutcome,
    ) -> Result<(), RegistrationError> {
        let mut w = self.take(Stage::HostFinish)?;
        outcome.validate()?;
        let d = Decision {
            version: 1,
            transcript: w.hash(),
            outcome,
        };
        let bytes = serde_json::to_vec(&d).map_err(|_| RegistrationError::Bounds)?;
        w.write(5, &bytes).await?;
        let expected = w.hash();
        if w.read(6).await?.as_slice() != expected {
            return Err(RegistrationError::Transcript);
        }
        w.eof().await?;
        w.shutdown().await
    }
}
fn lifetime(
    setup: Duration,
    total: Duration,
) -> Result<crate::owner_clock::Instant, SecurityError> {
    if setup.is_zero()
        || setup > crate::MAX_HANDSHAKE_BUDGET
        || total.is_zero()
        || total > Duration::from_secs(120)
    {
        return Err(SecurityError::Limit);
    }
    #[cfg(not(target_arch = "wasm32"))]
    return Ok(crate::owner_clock::Instant::now() + total);
    #[cfg(target_arch = "wasm32")]
    crate::owner_clock::Instant::from_owner_deadline(
        crate::browser_wasm_registration::total_deadline(),
    )
    .ok_or(SecurityError::Deadline)
}
fn admitted<S>(
    pending: Pending<S>,
    end: crate::owner_clock::Instant,
) -> Result<RegistrationPending<S>, SecurityError> {
    if pending._cancel.is_cancelled() {
        return Err(SecurityError::Cancelled);
    }
    if crate::owner_clock::Instant::now() >= end {
        return Err(SecurityError::Deadline);
    }
    Ok(RegistrationPending { pending, end })
}
/// Inputs are independently approved existing identities, never pairing candidates.
pub async fn registration_connect<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    io: Unbound<S>,
    local: LocalIdentity,
    pin: PeerPin,
    setup: Duration,
    total: Duration,
    cancel: CancelToken,
) -> Result<RegistrationPending<S>, SecurityError> {
    let end = lifetime(setup, total)?;
    let pending = crate::owner_clock::timeout_at(
        end,
        crate::tls::connect_protocol(io, local, pin, setup.min(total), cancel, REGISTRATION_ALPN),
    )
    .await
    .map_err(|_| SecurityError::Deadline)??;
    admitted(pending, end)
}
pub async fn registration_accept<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    io: Unbound<S>,
    local: LocalIdentity,
    pin: PeerPin,
    setup: Duration,
    total: Duration,
    cancel: CancelToken,
) -> Result<RegistrationPending<S>, SecurityError> {
    let end = lifetime(setup, total)?;
    let pending = crate::owner_clock::timeout_at(
        end,
        crate::tls::accept_protocol(io, local, pin, setup.min(total), cancel, REGISTRATION_ALPN),
    )
    .await
    .map_err(|_| SecurityError::Deadline)??;
    admitted(pending, end)
}

/// Preserve the original native ceremony deadlines across Relay admission and TLS.
/// Inputs remain independently approved identities, never a public pairing candidate.
#[cfg(not(target_arch = "wasm32"))]
pub async fn registration_accept_until<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    io: Unbound<S>,
    local: LocalIdentity,
    pin: PeerPin,
    original_setup_end: tokio::time::Instant,
    original_total_end: tokio::time::Instant,
    cancel: CancelToken,
) -> Result<RegistrationPending<S>, SecurityError> {
    if cancel.is_cancelled() {
        return Err(SecurityError::Cancelled);
    }
    let now = tokio::time::Instant::now();
    if now >= original_setup_end || now >= original_total_end {
        return Err(SecurityError::Deadline);
    }
    if original_setup_end - now > crate::MAX_HANDSHAKE_BUDGET
        || original_total_end - now > Duration::from_secs(120)
    {
        return Err(SecurityError::Limit);
    }
    let setup_end = original_setup_end.min(original_total_end);
    let pending = tokio::time::timeout_at(
        setup_end,
        crate::tls::accept_protocol(io, local, pin, setup_end - now, cancel, REGISTRATION_ALPN),
    )
    .await
    .map_err(|_| SecurityError::Deadline)??;
    if pending._cancel.is_cancelled() {
        return Err(SecurityError::Cancelled);
    }
    if tokio::time::Instant::now() >= setup_end {
        return Err(SecurityError::Deadline);
    }
    admitted(pending, original_total_end)
}
#[cfg(test)]
#[path = "../tests/support/registration_channel.rs"]
mod tests;
