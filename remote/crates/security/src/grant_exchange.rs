//! One opt-in exchange on an owned verified TLS/AppBind channel. Dropping any
//! exchange future drops its TLS owner; no public plaintext or signer bypass.
use super::*;
use crate::{GrantClock, GrantClockDomain, P256Spki};
use cc_desk_remote_record::{
    Header, Kind,
    appbind::Context,
    grant::{CanonicalGrant, DataRole, SessionScope},
    grant_management::{self as wire, Message},
};
use ring::digest;
use std::{future::Future, pin::Pin, sync::Mutex, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
    time::Instant,
};

pub struct NativeGrantPolicy {
    pub scope: SessionScope,
    pub role: DataRole,
    pub duration: Duration,
}
pub struct ClientGrantTrust {
    pub issuer: P256Spki,
    pub principal: [u8; 32],
    pub scope: SessionScope,
    pub role: DataRole,
}
pub struct VerifiedCanonicalGrant {
    payload: CanonicalGrant,
    kid: [u8; 32],
}
impl std::fmt::Debug for VerifiedCanonicalGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedCanonicalGrant(<native-presence-request>)")
    }
}
impl VerifiedCanonicalGrant {
    pub fn payload(&self) -> &CanonicalGrant {
        &self.payload
    }
    pub fn issuer_kid(&self) -> [u8; 32] {
        self.kid
    }
}
/// Trusted native administrative presence adapter, not an arbitrary signing API.
/// Construction and every future poll must return promptly and never block the
/// async owner. Drop must cooperatively cancel owned work; a platform callback
/// that cannot do this needs separately qualified bounded native isolation.
/// Async deadlines cannot preempt blocking native callbacks. No such platform
/// implementation or key provisioning is supplied by this software candidate.
pub trait NativeGrantSigner: Send {
    fn confirm_and_sign<'a>(
        &'a mut self,
        grant: &'a VerifiedCanonicalGrant,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>;
}
struct ClientDeadlineClock {
    source: Arc<dyn GrantClock>,
    anchor: u64,
    created: u64,
    last: Mutex<u64>,
    revoked: watch::Sender<bool>,
}
impl GrantClock for ClientDeadlineClock {
    fn now_ms(&self) -> Result<u64, GrantError> {
        let result = (|| {
            if *self.revoked.borrow() {
                return Err(GrantError::Revoked);
            }
            let mut last = self.last.lock().map_err(|_| GrantError::Clock)?;
            let now = self.source.now_ms()?;
            if now < *last {
                return Err(GrantError::Clock);
            }
            *last = now;
            self.created
                .checked_add(now.checked_sub(self.anchor).ok_or(GrantError::Clock)?)
                .ok_or(GrantError::Clock)
        })();
        if result.is_err() {
            self.revoked.send_replace(true);
        }
        result
    }
}
struct RemoteAccess {
    payload: CanonicalGrant,
    issuer: P256Spki,
    signed: Vec<u8>,
    clock: Arc<ClientDeadlineClock>,
    binding: Mutex<Option<[u8; 32]>>,
    closed: watch::Sender<bool>,
}
#[derive(Clone)]
pub struct RemoteGrant(Arc<RemoteAccess>);
impl std::fmt::Debug for RemoteGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RemoteGrant(<client-only-capability>)")
    }
}
impl RemoteGrant {
    fn remaining(&self) -> Result<u64, GrantError> {
        let now = self.0.clock.now_ms()?;
        if *self.0.closed.borrow() {
            return Err(GrantError::Revoked);
        }
        let end = self.expires_host_ms();
        if now >= end {
            self.0.closed.send_replace(true);
            return Err(GrantError::Expired);
        }
        Ok(end - now)
    }
    pub fn check(&self) -> Result<u64, GrantError> {
        if self
            .0
            .binding
            .lock()
            .map_err(|_| GrantError::Closed)?
            .is_none()
        {
            return Err(GrantError::Closed);
        }
        self.remaining()
    }
    pub fn scope(&self) -> &SessionScope {
        &self.0.payload.scope
    }
    pub fn role(&self) -> DataRole {
        self.0.payload.role
    }
    pub fn expires_host_ms(&self) -> u64 {
        self.0.payload.expires_host_ms.parse().unwrap()
    }
    pub(super) fn payload(&self) -> &CanonicalGrant {
        &self.0.payload
    }
    pub(super) fn bound_to(&self, binding: [u8; 32]) -> bool {
        self.0.binding.lock().is_ok_and(|b| *b == Some(binding))
    }
    pub(super) fn clock_domain(&self) -> GrantClockDomain {
        GrantClockDomain::new(self.0.clock.clone())
    }
    pub(super) fn revoked(&self) -> (watch::Receiver<bool>, watch::Receiver<bool>) {
        (self.0.clock.revoked.subscribe(), self.0.closed.subscribe())
    }
}
/// Memory-only custody. It has no deserialization/import constructor: retained
/// COSE without the original monotonic anchor cannot recreate client access.
pub struct ClientGrantState {
    clock: Arc<dyn GrantClock>,
    candidate: Option<RemoteGrant>,
}
impl ClientGrantState {
    pub fn new(clock: Arc<dyn GrantClock>) -> Self {
        Self {
            clock,
            candidate: None,
        }
    }
    pub fn remote_grant(&self) -> Option<RemoteGrant> {
        self.candidate
            .as_ref()
            .filter(|g| g.check().is_ok())
            .cloned()
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hash(bytes: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, bytes).as_ref())
}
fn kid(issuer: &P256Spki) -> [u8; 32] {
    let mut h = digest::Context::new(&digest::SHA256);
    h.update(b"CC-DESK-GRANT-ISSUER-v1\0");
    h.update(&issuer.0);
    h.finish().as_ref().try_into().unwrap()
}
fn matches(payload: &CanonicalGrant, context: &Context, trust: &ClientGrantTrust) -> bool {
    payload.context == *context
        && payload.principal_id == hex(&trust.principal)
        && payload.scope == trust.scope
        && payload.role == trust.role
}
async fn send<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut tokio_rustls::TlsStream<S>,
    bytes: &[u8],
) -> Result<(), GrantError> {
    let bytes =
        cc_desk_remote_record::encode(Kind::Control, 1, bytes).map_err(|_| GrantError::Bounds)?;
    io.write_all(&bytes).await.map_err(|_| GrantError::Closed)?;
    io.flush().await.map_err(|_| GrantError::Closed)
}
async fn receive<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut tokio_rustls::TlsStream<S>,
) -> Result<Vec<u8>, GrantError> {
    let mut bytes = [0; 8];
    io.read_exact(&mut bytes)
        .await
        .map_err(|_| GrantError::Closed)?;
    let h = Header::decode(bytes, 1).map_err(|_| GrantError::Encoding)?;
    if h.kind != Kind::Control {
        return Err(GrantError::Encoding);
    }
    let mut bytes = vec![0; h.payload_len];
    io.read_exact(&mut bytes)
        .await
        .map_err(|_| GrantError::Closed)?;
    Ok(bytes)
}
fn encode(message: &Message) -> Result<Vec<u8>, GrantError> {
    wire::encode(message).map_err(|_| GrantError::Encoding)
}
fn decode(bytes: &[u8]) -> Result<Message, GrantError> {
    wire::decode(bytes).map_err(|_| GrantError::Encoding)
}
async fn within<T, F: Future<Output = Result<T, GrantError>>>(
    deadline: Instant,
    token: &mut CancelToken,
    work: F,
) -> Result<T, GrantError> {
    if token.is_cancelled() || Instant::now() >= deadline {
        return Err(GrantError::Closed);
    }
    let result = tokio::select! {biased;_=token.cancelled()=>Err(GrantError::Closed),_=tokio::time::sleep_until(deadline)=>Err(GrantError::Expired),r=work=>r}?;
    if token.is_cancelled() || Instant::now() >= deadline {
        return Err(GrantError::Closed);
    }
    Ok(result)
}
impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> ManagedChannel<S> {
    fn exchange_io(
        &mut self,
        role: Role,
        budget: Duration,
    ) -> Result<(tokio_rustls::TlsStream<S>, Instant, CancelToken), GrantError> {
        if !self.grant_management
            || self.management_used
            || self.role != role
            || budget.is_zero()
            || budget > Duration::from_secs(30)
            || self.access.is_some()
        {
            return Err(GrantError::Policy);
        }
        self.management_used = true;
        let io = self.io.take().ok_or(GrantError::Closed)?;
        Ok((io, Instant::now() + budget, self.cancel.clone()))
    }
    pub async fn grant_host(
        mut self,
        gate: &mut crate::GrantGate,
        policy: NativeGrantPolicy,
        budget: Duration,
    ) -> Result<Self, GrantError> {
        let (mut io, deadline, mut token) = self.exchange_io(Role::Host, budget)?;
        let context = self.context.raw().clone();
        let binding = self.binding;
        let access = within(deadline, &mut token, async {
            if !gate.matches_context(&context) {
                return Err(GrantError::Context);
            }
            let Message::Request { nonce, reuse } = decode(&receive(&mut io).await?)? else {
                return Err(GrantError::Encoding);
            };
            let issued = if !reuse {
                let grant = gate.challenge(policy.scope.clone(), policy.role, policy.duration)?;
                let issued = grant.clone();
                send(
                    &mut io,
                    &encode(&Message::Challenge {
                        nonce: nonce.clone(),
                        binding: hex(&binding),
                        grant,
                    })?,
                )
                .await?;
                Some(issued)
            } else {
                None
            };
            let signed = receive(&mut io).await?;
            gate.verify_management(
                &signed,
                &policy.scope,
                policy.role,
                policy.duration,
                issued.as_ref(),
            )?;
            let access = gate.accept(&signed)?;
            send(
                &mut io,
                &encode(&Message::Accepted {
                    nonce,
                    binding: hex(&binding),
                    grant_id: hex(&access.id()),
                    digest: hash(&signed),
                })?,
            )
            .await?;
            access.check()?;
            Ok(access)
        })
        .await?;
        self.io = Some(io);
        self.install(&access).map_err(|_| GrantError::Policy)?;
        Ok(self)
    }
    pub async fn grant_client(
        mut self,
        trust: &ClientGrantTrust,
        signer: &mut dyn NativeGrantSigner,
        state: &mut ClientGrantState,
        budget: Duration,
    ) -> Result<Self, GrantError> {
        let (mut io, deadline, mut token) = self.exchange_io(Role::Client, budget)?;
        let context = self.context.raw().clone();
        let binding = self.binding;
        let remote = within(deadline, &mut token, async {
            if hex(&trust.issuer.endpoint_id(Role::Host)) == context.host_endpoint_id
                || hex(&trust.issuer.endpoint_id(Role::Client)) == context.client_endpoint_id
            {
                return Err(GrantError::Policy);
            }
            // This sample precedes the request, including any network or presence delay.
            let anchor = state.clock.now_ms()?;
            let nonce = crate::grant::fresh()?;
            let reuse = state.candidate.is_some();
            if let Some(old) = &state.candidate {
                old.remaining()?;
                if !matches(&old.0.payload, &context, trust) || old.0.issuer.0 != trust.issuer.0 {
                    return Err(GrantError::Context);
                }
            }
            send(
                &mut io,
                &encode(&Message::Request {
                    nonce: nonce.clone(),
                    reuse,
                })?,
            )
            .await?;
            if !reuse {
                let Message::Challenge {
                    nonce: echo,
                    binding: echo_binding,
                    grant,
                } = decode(&receive(&mut io).await?)?
                else {
                    return Err(GrantError::Encoding);
                };
                if echo != nonce
                    || echo_binding != hex(&binding)
                    || !matches(&grant, &context, trust)
                {
                    return Err(GrantError::Context);
                }
                let request = VerifiedCanonicalGrant {
                    payload: grant,
                    kid: kid(&trust.issuer),
                };
                let signed = signer.confirm_and_sign(&request).await?;
                crate::grant::signature(&signed, &request.payload, &trust.issuer, request.kid)?;
                let created = request
                    .payload
                    .created_host_ms
                    .parse()
                    .map_err(|_| GrantError::Encoding)?;
                let (revoked, _) = watch::channel(false);
                let (closed, _) = watch::channel(false);
                let remote = RemoteGrant(Arc::new(RemoteAccess {
                    payload: request.payload,
                    issuer: trust.issuer.clone(),
                    signed,
                    clock: Arc::new(ClientDeadlineClock {
                        source: state.clock.clone(),
                        anchor,
                        created,
                        last: Mutex::new(anchor),
                        revoked,
                    }),
                    binding: Mutex::new(None),
                    closed,
                }));
                remote.remaining()?;
                state.candidate = Some(remote);
            }
            let remote = state.candidate.as_ref().ok_or(GrantError::Closed)?.clone();
            remote.remaining()?;
            send(&mut io, &remote.0.signed).await?;
            let Message::Accepted {
                nonce: echo,
                binding: echo_binding,
                grant_id,
                digest,
            } = decode(&receive(&mut io).await?)?
            else {
                return Err(GrantError::Encoding);
            };
            if echo != nonce
                || echo_binding != hex(&binding)
                || grant_id != remote.0.payload.grant_id
                || digest != hash(&remote.0.signed)
            {
                return Err(GrantError::Context);
            }
            remote.remaining()?;
            *remote.0.binding.lock().map_err(|_| GrantError::Closed)? = Some(binding);
            Ok(remote)
        })
        .await?;
        self.io = Some(io);
        self.install_remote(&remote)
            .map_err(|_| GrantError::Policy)?;
        Ok(self)
    }
}

#[cfg(test)]
mod clock_order_tests {
    use super::*;
    use std::sync::{OnceLock, Weak, atomic::AtomicUsize, mpsc};
    #[test]
    fn grant_management_concurrent_clock_samples_are_serialized_before_sampling() {
        let slot = Arc::new(OnceLock::<Weak<ClientDeadlineClock>>::new());
        let (source_slot, source_calls) = (slot.clone(), Arc::new(AtomicUsize::new(0)));
        let (first_tx, first_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release = Mutex::new(release_rx);
        let source: Arc<dyn GrantClock> = Arc::new(move || {
            if source_calls.fetch_add(1, Ordering::AcqRel) == 0 {
                let clock = source_slot.get().unwrap().upgrade().unwrap();
                let serialized = clock.last.try_lock().is_err();
                first_tx.send(serialized).unwrap();
                release.lock().unwrap().recv().unwrap();
                Ok(200)
            } else {
                Ok(201)
            }
        });
        let (revoked, _) = watch::channel(false);
        let clock = Arc::new(ClientDeadlineClock {
            source,
            anchor: 200,
            created: 1000,
            last: Mutex::new(200),
            revoked,
        });
        assert!(slot.set(Arc::downgrade(&clock)).is_ok());
        let first = clock.clone();
        let a = std::thread::spawn(move || first.now_ms());
        let serialized = first_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let second = clock.clone();
        let mut b = Some(std::thread::spawn(move || second.now_ms()));
        // The old unsynchronized implementation can complete B before releasing A;
        // the corrected implementation holds its mutex, so release A first instead.
        let early = if serialized {
            None
        } else {
            Some(b.take().unwrap().join().unwrap())
        };
        release_tx.send(()).unwrap();
        let a = a.join().unwrap();
        let b = if let Some(value) = early {
            value
        } else {
            b.take().unwrap().join().unwrap()
        };
        assert_eq!(a, Ok(1000));
        assert_eq!(b, Ok(1001));
        assert!(serialized);
    }
}
