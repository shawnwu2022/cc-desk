//! Native policy input and verified grant records; no enrollment or persistent keys.
use crate::{P256Spki, Role};
use cc_desk_remote_record::{
    appbind::Context,
    grant::{self, CanonicalGrant, DataRole, SessionScope},
};
use coset::{CoseSign1Builder, HeaderBuilder, TaggedCborSerializable, iana};
use ring::{digest, rand::SecureRandom};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
pub(crate) const CONTENT_TYPE: &str = "application/cc-desk-grant+json";
pub(crate) const AAD: &[u8] = b"CC-DESK-DATA-GRANT-v1\0";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantError {
    Policy,
    Clock,
    Revoked,
    Expired,
    Replay,
    Busy,
    Bounds,
    Signature,
    Encoding,
    Context,
    Closed,
}
pub trait GrantClock: Send + Sync {
    fn now_ms(&self) -> Result<u64, GrantError>;
}
impl<F: Fn() -> Result<u64, GrantError> + Send + Sync> GrantClock for F {
    fn now_ms(&self) -> Result<u64, GrantError> {
        self()
    }
}
struct LivePolicy {
    clock: Arc<dyn GrantClock>,
    last: Mutex<u64>,
    revoked: watch::Sender<bool>,
}
impl LivePolicy {
    fn sample(&self) -> Result<u64, GrantError> {
        let result = (|| {
            if *self.revoked.borrow() {
                return Err(GrantError::Revoked);
            }
            let mut last = self.last.lock().map_err(|_| GrantError::Clock)?;
            let now = self.clock.now_ms()?;
            if now < *last {
                return Err(GrantError::Clock);
            }
            *last = now;
            Ok(now)
        })();
        if result.is_err() {
            self.revoked.send_replace(true);
        }
        result
    }
}
struct Accepted {
    payload: CanonicalGrant,
    state: Arc<LivePolicy>,
    closed: watch::Sender<bool>,
}
#[derive(Clone)]
pub struct VerifiedGrant(Arc<Accepted>);
impl std::fmt::Debug for VerifiedGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedGrant(<scoped-native-capability>)")
    }
}
impl VerifiedGrant {
    pub(crate) fn principal_id(&self) -> [u8; 32] {
        hex_id(&self.0.payload.principal_id)
    }
    pub(crate) fn created_host_ms(&self) -> u64 {
        self.0.payload.created_host_ms.parse().unwrap()
    }
    pub(crate) fn clock_domain(&self) -> crate::GrantClockDomain {
        crate::GrantClockDomain::new(self.0.state.clock.clone())
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
    pub fn check(&self) -> Result<u64, GrantError> {
        let now = self.0.state.sample()?;
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
    pub(crate) fn context(&self) -> &Context {
        &self.0.payload.context
    }
    pub(crate) fn id(&self) -> [u8; 32] {
        hex_id(&self.0.payload.grant_id)
    }
    pub(crate) fn scope_digest(&self) -> [u8; 32] {
        let mut hash = digest::Context::new(&digest::SHA256);
        hash.update(b"CC-DESK-DATA-SCOPE-v1\0");
        hash.update(&grant::encode_scope(self.scope()).unwrap());
        hash.finish().as_ref().try_into().unwrap()
    }
    pub(crate) fn revoked(&self) -> (watch::Receiver<bool>, watch::Receiver<bool>) {
        (self.0.state.revoked.subscribe(), self.0.closed.subscribe())
    }
}
pub struct GrantGate {
    context: Context,
    issuer: P256Spki,
    kid: [u8; 32],
    principal: [u8; 32],
    state: Arc<LivePolicy>,
    pending: Option<(CanonicalGrant, u8)>,
    accepted: Option<VerifiedGrant>,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hex_id(text: &str) -> [u8; 32] {
    let mut id = [0; 32];
    for (i, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap()
    }
    id
}
fn fresh() -> Result<String, GrantError> {
    let mut id = [0; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut id)
        .map_err(|_| GrantError::Policy)?;
    if id == [0; 32] {
        return Err(GrantError::Policy);
    }
    Ok(hex(&id))
}
fn signature(
    bytes: &[u8],
    payload: &CanonicalGrant,
    issuer: &P256Spki,
    kid: [u8; 32],
) -> Result<(), GrantError> {
    if bytes.len() > 16384 || bytes.len() < 64 {
        return Err(GrantError::Bounds);
    }
    let canonical = grant::encode(payload).map_err(|_| GrantError::Encoding)?;
    let mut candidate = CoseSign1Builder::new()
        .protected(
            HeaderBuilder::new()
                .algorithm(iana::Algorithm::ES256)
                .content_type(CONTENT_TYPE.into())
                .key_id(kid.to_vec())
                .build(),
        )
        .payload(canonical)
        .signature(vec![0; 64])
        .build();
    let expected = candidate
        .clone()
        .to_tagged_vec()
        .map_err(|_| GrantError::Encoding)?;
    // Compare the complete known canonical structure; never allocate from attacker CBOR lengths.
    if bytes.len() != expected.len() || bytes[..bytes.len() - 64] != expected[..expected.len() - 64]
    {
        return Err(GrantError::Encoding);
    }
    candidate.signature = bytes[bytes.len() - 64..].to_vec();
    candidate.verify_signature(AAD, |s, m| {
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::ECDSA_P256_SHA256_FIXED,
            &issuer.0[26..],
        )
        .verify(m, s)
        .map_err(|_| GrantError::Signature)
    })
}
impl GrantGate {
    pub fn for_binding(
        binding: &crate::BoundContext,
        issuer: P256Spki,
        principal: [u8; 32],
        clock: Arc<dyn GrantClock>,
    ) -> Result<Self, GrantError> {
        Self::new(binding.raw().clone(), issuer, principal, clock)
    }
    /// Caller is the trusted native policy owner. No wire/JS policy enrollment occurs here.
    pub(crate) fn new(
        context: Context,
        issuer: P256Spki,
        principal: [u8; 32],
        clock: Arc<dyn GrantClock>,
    ) -> Result<Self, GrantError> {
        if principal == [0; 32]
            || hex(&issuer.endpoint_id(Role::Host)) == context.host_endpoint_id
            || hex(&issuer.endpoint_id(Role::Client)) == context.client_endpoint_id
        {
            return Err(GrantError::Policy);
        }
        let mut hash = digest::Context::new(&digest::SHA256);
        hash.update(b"CC-DESK-GRANT-ISSUER-v1\0");
        hash.update(&issuer.0);
        let kid = hash.finish().as_ref().try_into().unwrap();
        let now = clock.now_ms()?;
        let (revoked, _) = watch::channel(false);
        Ok(Self {
            context,
            issuer,
            kid,
            principal,
            state: Arc::new(LivePolicy {
                clock,
                last: Mutex::new(now),
                revoked,
            }),
            pending: None,
            accepted: None,
        })
    }
    pub fn issuer_kid(&self) -> [u8; 32] {
        self.kid
    }
    pub fn revoke(&mut self) {
        self.state.revoked.send_replace(true);
        self.pending = None;
    }
    pub fn challenge(
        &mut self,
        scope: SessionScope,
        role: DataRole,
        duration: Duration,
    ) -> Result<CanonicalGrant, GrantError> {
        let now = self.state.sample()?;
        if let Some((old, _)) = &self.pending {
            let start: u64 = old.created_host_ms.parse().unwrap();
            let end: u64 = old.expires_host_ms.parse().unwrap();
            if now < start + 30000 && now < end {
                return Err(GrantError::Busy);
            }
        }
        let ms = u64::try_from(duration.as_millis()).map_err(|_| GrantError::Bounds)?;
        if ms == 0 || ms > 900000 {
            return Err(GrantError::Bounds);
        }
        let end = now.checked_add(ms).ok_or(GrantError::Bounds)?;
        let value = CanonicalGrant {
            version: 1,
            context: self.context.clone(),
            principal_id: hex(&self.principal),
            scope,
            role,
            challenge: fresh()?,
            grant_id: fresh()?,
            created_host_ms: now.to_string(),
            expires_host_ms: end.to_string(),
        };
        grant::encode(&value).map_err(|_| GrantError::Policy)?;
        if self.state.sample()? >= now.checked_add(30000).ok_or(GrantError::Bounds)? {
            return Err(GrantError::Expired);
        }
        self.pending = Some((value.clone(), 0));
        Ok(value)
    }
    pub fn accept(&mut self, bytes: &[u8]) -> Result<VerifiedGrant, GrantError> {
        let now = self.state.sample()?;
        if let Some(old) = &self.accepted
            && signature(bytes, &old.0.payload, &self.issuer, self.kid).is_ok()
        {
            old.check()?;
            return Ok(old.clone());
        }
        let (pending, attempts) = self.pending.as_mut().ok_or(GrantError::Replay)?;
        let created: u64 = pending.created_host_ms.parse().unwrap();
        let end: u64 = pending.expires_host_ms.parse().unwrap();
        if now >= created.checked_add(30000).ok_or(GrantError::Bounds)? || now >= end {
            self.pending = None;
            return Err(GrantError::Expired);
        }
        if let Err(error) = signature(bytes, pending, &self.issuer, self.kid) {
            *attempts += 1;
            if *attempts == 3 {
                self.pending = None
            }
            return Err(error);
        }
        let now = self.state.sample()?;
        if now >= created + 30000 || now >= end {
            self.pending = None;
            return Err(GrantError::Expired);
        }
        let (payload, _) = self.pending.take().unwrap();
        if let Some(old) = self.accepted.take() {
            old.0.closed.send_replace(true);
        }
        let (closed, _) = watch::channel(false);
        let accepted = VerifiedGrant(Arc::new(Accepted {
            payload,
            state: self.state.clone(),
            closed,
        }));
        self.accepted = Some(accepted.clone());
        Ok(accepted)
    }
}
impl Drop for GrantGate {
    fn drop(&mut self) {
        self.state.revoked.send_replace(true);
    }
}
