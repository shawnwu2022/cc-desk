//! Native trait adapters. No persistent loader, enrollment, IPC or default approval.
use super::{owner::*, SigningPurpose};
#[cfg(feature = "remote-client-ipc")]
use super::{CryptoError, NativeOwnedSoftwareKey};
use cc_desk_remote_record::appbind::CanonicalOrigin;
use cc_desk_remote_security::{GrantError, NativeGrantSigner, P256Spki, VerifiedCanonicalGrant};
use cc_desk_remote_transport::wss::{RelayRole, RelaySigner, SignError, SigningRequest};
use coset::TaggedCborSerializable;
#[cfg(feature = "remote-client-ipc")]
use parking_lot::Mutex;
use rustls::sign::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};
use std::{future::Future, pin::Pin};

/// Synchronous rustls boundary. The calling thread may block up to its own
/// observer horizon; opaque NCrypt work is not preempted and stays custodied.
pub(crate) struct CngTlsKey {
    attempt: Option<NativeSigningAttempt>,
    handle: KeyRequestHandle,
    end: Instant,
}
impl CngTlsKey {
    #[cfg(test)]
    pub(crate) fn new(handle: KeyRequestHandle, end: Instant) -> Result<Self, SignFailure> {
        Self::bounded(handle, end, None)
    }
    #[cfg(feature = "remote-client-ipc")]
    fn for_attempt(
        handle: KeyRequestHandle,
        attempt: NativeSigningAttempt,
    ) -> Result<Self, SignFailure> {
        Self::bounded(handle, attempt.end(), Some(attempt))
    }
    fn bounded(
        handle: KeyRequestHandle,
        end: Instant,
        attempt: Option<NativeSigningAttempt>,
    ) -> Result<Self, SignFailure> {
        if handle.binding().0 != SigningPurpose::Endpoint
            || end
                .checked_duration_since(Instant::now())
                .is_none_or(|d| d > Duration::from_secs(5))
        {
            return Err(SignFailure::Bounds);
        }
        Ok(Self {
            handle,
            end,
            attempt,
        })
    }
}

pub(crate) struct CngRelaySigner {
    attempt: Option<NativeSigningAttempt>,
    handle: KeyRequestHandle,
    origin: CanonicalOrigin,
    key_id: [u8; 16],
    role: RelayRole,
}
impl CngRelaySigner {
    #[cfg(test)]
    pub(crate) fn new(
        handle: KeyRequestHandle,
        origin: CanonicalOrigin,
        key_id: [u8; 16],
        role: RelayRole,
    ) -> Result<Self, SignFailure> {
        Self::bound(handle, origin, key_id, role, None)
    }
    fn bound(
        handle: KeyRequestHandle,
        origin: CanonicalOrigin,
        key_id: [u8; 16],
        role: RelayRole,
        attempt: Option<NativeSigningAttempt>,
    ) -> Result<Self, SignFailure> {
        if handle.binding().0 != SigningPurpose::Relay
            || key_id == [0; 16]
            || origin.as_str().len() > 512
        {
            return Err(SignFailure::Bounds);
        }
        Ok(Self {
            attempt,
            handle,
            origin,
            key_id,
            role,
        })
    }
}
impl RelaySigner for CngRelaySigner {
    fn sign<'a>(
        &'a self,
        request: &'a SigningRequest,
    ) -> Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send + 'a>> {
        Box::pin(async move {
            if request.origin() != self.origin.as_str()
                || request.key_id() != self.key_id
                || request.role() != self.role
            {
                return Err(SignError);
            }
            // SigningRequest retains its existing verified challenge provenance.
            let ticket = match &self.attempt {
                Some(attempt) => self.handle.begin_attempt(
                    request.transcript().to_vec(),
                    SignatureEncoding::Fixed,
                    attempt.clone(),
                ),
                None => self
                    .handle
                    .begin(request.transcript().to_vec(), SignatureEncoding::Fixed),
            };
            let signature = ticket
                .map_err(|_| SignError)?
                .await
                .map_err(|_| SignError)?;
            if self.attempt.as_ref().is_some_and(|a| !a.is_current()) {
                return Err(SignError);
            }
            signature.try_into().map_err(|_| SignError)
        })
    }
}

pub(crate) type PresenceFuture<'a> =
    Pin<Box<dyn Future<Output = Result<[u8; 32], GrantError>> + Send + 'a>>;
/// Trusted native presence callback. Returned approval MUST identify this exact
/// verified canonical request and issuer fingerprint; never JS/default approval.
/// Construction/poll must be cooperative; an outer timer cannot preempt a
/// callback that violates this native contract. No production UI approval is supplied.
pub(crate) trait NativeGrantPresence: Send + Sync {
    fn approve<'a>(
        &'a self,
        request: &'a VerifiedCanonicalGrant,
        fingerprint: [u8; 32],
    ) -> PresenceFuture<'a>;
}
#[cfg(test)]
pub(crate) struct UnavailablePresence;
#[cfg(test)]
impl NativeGrantPresence for UnavailablePresence {
    fn approve<'a>(&'a self, _: &'a VerifiedCanonicalGrant, _: [u8; 32]) -> PresenceFuture<'a> {
        Box::pin(async { Err(GrantError::Policy) })
    }
}

/// Native approved public tuple, never an IPC/Serde profile or Ready flag.
#[cfg(feature = "remote-client-ipc")]
pub(crate) struct NativeSignerBinding {
    /// Required backend signing approval cap; NOT canonical Host grant expiry.
    pub(crate) approved_until: Option<Instant>,
    pub(crate) origin: CanonicalOrigin,
    pub(crate) relay_id: [u8; 16],
    pub(crate) relay_role: RelayRole,
    pub(crate) tls_end: Instant,
}
#[cfg_attr(not(test), allow(dead_code))] // Native provider awaits explicit profile/presence configuration.
#[cfg(feature = "remote-client-ipc")]
pub(crate) struct NativeSignerSet {
    pub(crate) attempt: NativeSigningAttempt,
    pub(crate) endpoint: Arc<dyn SigningKey>,
    pub(crate) relay: cc_desk_remote_transport::wss::RelayAdmissionIdentity,
    pub(crate) grant: Box<dyn NativeGrantSigner>,
}
#[cfg(feature = "remote-client-ipc")]
pub(crate) struct NativeSignerAuthority {
    handles: Vec<KeyRequestHandle>,
    origin: CanonicalOrigin,
    key_id: [u8; 16],
    role: RelayRole,
    presence: Arc<dyn NativeGrantPresence>,
    _profile: Arc<dyn NativeKeyProfile>,
    pub(crate) fence: Arc<SigningFence>,
    approved_until: Instant,
}
#[cfg(feature = "remote-client-ipc")]
impl NativeSignerAuthority {
    pub(crate) fn new(
        handles: Vec<KeyRequestHandle>,
        binding: &NativeSignerBinding,
        presence: Arc<dyn NativeGrantPresence>,
        profile: Arc<dyn NativeKeyProfile>,
    ) -> Result<Arc<Self>, SignFailure> {
        let approved_until = binding.approved_until.ok_or(SignFailure::Closed)?;
        Ok(Arc::new(Self {
            handles,
            origin: binding.origin.clone(),
            key_id: binding.relay_id,
            role: binding.relay_role,
            presence,
            _profile: profile,
            fence: SigningFence::new(approved_until),
            approved_until,
        }))
    }
    pub(crate) fn matches_origin(&self, origin: &CanonicalOrigin) -> bool {
        self.origin == *origin
    }
    pub(crate) fn assemble(&self, end: Instant) -> Result<NativeSignerSet, SignFailure> {
        let attempt = self
            .fence
            .advance(&self.handles, end.min(self.approved_until))?;
        Ok(NativeSignerSet {
            endpoint: Arc::new(CngTlsKey::for_attempt(
                self.handles[0].clone(),
                attempt.clone(),
            )?),
            relay: cc_desk_remote_transport::wss::RelayAdmissionIdentity::new(
                self.key_id,
                self.role,
                Arc::new(CngRelaySigner::bound(
                    self.handles[1].clone(),
                    self.origin.clone(),
                    self.key_id,
                    self.role,
                    Some(attempt.clone()),
                )?),
            ),
            grant: Box::new(CngGrantSigner::bound(
                self.handles[2].clone(),
                self.presence.clone(),
                Some(attempt.clone()),
            )?),
            attempt,
        })
    }
}
pub(crate) struct CngGrantSigner {
    attempt: Option<NativeSigningAttempt>,
    handle: KeyRequestHandle,
    issuer: P256Spki,
    presence: Arc<dyn NativeGrantPresence>,
}
impl CngGrantSigner {
    #[cfg(test)]
    pub(crate) fn new(
        handle: KeyRequestHandle,
        presence: Arc<dyn NativeGrantPresence>,
    ) -> Result<Self, SignFailure> {
        Self::bound(handle, presence, None)
    }
    fn bound(
        handle: KeyRequestHandle,
        presence: Arc<dyn NativeGrantPresence>,
        attempt: Option<NativeSigningAttempt>,
    ) -> Result<Self, SignFailure> {
        if handle.binding().0 != SigningPurpose::Issuer {
            return Err(SignFailure::Bounds);
        }
        let issuer = P256Spki::new(handle.binding().1).map_err(|_| SignFailure::Bounds)?;
        Ok(Self {
            attempt,
            handle,
            issuer,
            presence,
        })
    }
}
impl NativeGrantSigner for CngGrantSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        request: &'a VerifiedCanonicalGrant,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>> {
        Box::pin(async move {
            if !request.is_issuer(&self.issuer) {
                return Err(GrantError::Policy);
            }
            if self.attempt.as_ref().is_some_and(|a| !a.is_current()) {
                return Err(GrantError::Closed);
            }
            let end = self
                .attempt
                .as_ref()
                .map_or_else(|| Instant::now() + Duration::from_secs(5), |a| a.end());
            let (mut cose, tbs) = request.signing_parts()?;
            // tbs includes the exact canonical grant, protected issuer kid and
            // existing grant domain/AAD. No caller-supplied hash authorizes work.
            let fingerprint: [u8; 32] = Sha256::digest(&tbs).into();
            let approved = tokio::time::timeout_at(
                tokio::time::Instant::from_std(end),
                self.presence.approve(request, fingerprint),
            )
            .await
            .map_err(|_| GrantError::Expired)??;
            if approved != fingerprint {
                return Err(GrantError::Policy);
            }
            let ticket = match &self.attempt {
                Some(attempt) => {
                    self.handle
                        .begin_attempt(tbs, SignatureEncoding::Fixed, attempt.clone())
                }
                None => self.handle.begin_until(tbs, SignatureEncoding::Fixed, end),
            };
            cose.signature = ticket
                .map_err(|_| GrantError::Closed)?
                .await
                .map_err(|_| GrantError::Closed)?;
            if self.attempt.as_ref().is_some_and(|a| !a.is_current()) {
                return Err(GrantError::Closed);
            }
            cose.to_tagged_vec().map_err(|_| GrantError::Encoding)
        })
    }
}

/// Native-approved metadata is selected by the application owner, never IPC.
/// binding() is metadata-only/nonblocking; open() runs on the original key worker.
#[cfg(feature = "remote-client-ipc")]
pub(crate) trait NativeKeyProfile: Send + Sync {
    fn binding(&self, purpose: SigningPurpose) -> Result<Vec<u8>, SignFailure>;
    fn open(&self, purpose: SigningPurpose) -> Result<NativeOwnedSoftwareKey, CryptoError>;
}
#[cfg(all(test, feature = "remote-client-ipc"))]
pub(crate) struct UnavailableProfile;
#[cfg(all(test, feature = "remote-client-ipc"))]
impl NativeKeyProfile for UnavailableProfile {
    fn binding(&self, _: SigningPurpose) -> Result<Vec<u8>, SignFailure> {
        Err(SignFailure::Closed)
    }
    fn open(&self, _: SigningPurpose) -> Result<NativeOwnedSoftwareKey, CryptoError> {
        Err(CryptoError::Key)
    }
}
#[cfg(feature = "remote-client-ipc")]
pub(crate) struct NativeKeySlot {
    owner: Mutex<KeyCustodian>,
    pub(crate) signal: KeyRequestHandle,
}
#[cfg(feature = "remote-client-ipc")]
impl NativeKeySlot {
    pub(crate) fn new(owner: KeyCustodian, signal: KeyRequestHandle) -> Self {
        Self {
            owner: Mutex::new(owner),
            signal,
        }
    }
    pub(crate) fn observe(&self) -> KeyRetirement {
        self.owner.lock().observe()
    }
}
impl fmt::Debug for CngTlsKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CngTlsKey(<native-owned>)")
    }
}
impl SigningKey for CngTlsKey {
    fn algorithm(&self) -> rustls::SignatureAlgorithm {
        rustls::SignatureAlgorithm::ECDSA
    }
    fn choose_scheme(&self, offered: &[rustls::SignatureScheme]) -> Option<Box<dyn Signer>> {
        (Instant::now() < self.end
            && self.attempt.as_ref().is_none_or(|a| a.is_current())
            && offered.contains(&rustls::SignatureScheme::ECDSA_NISTP256_SHA256))
        .then(|| {
            Box::new(Self {
                attempt: self.attempt.clone(),
                handle: self.handle.clone(),
                end: self.end,
            }) as Box<dyn Signer>
        })
    }
    fn public_key(&self) -> Option<rustls::pki_types::SubjectPublicKeyInfoDer<'_>> {
        Some(rustls::pki_types::SubjectPublicKeyInfoDer::from(
            self.handle.binding().1,
        ))
    }
}
impl Signer for CngTlsKey {
    fn scheme(&self) -> rustls::SignatureScheme {
        rustls::SignatureScheme::ECDSA_NISTP256_SHA256
    }
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        if message.len() > super::MAX_SIGN_BYTES {
            return Err(rustls::Error::General("NATIVE_SIGN_BOUNDS".into()));
        }
        let ticket = match &self.attempt {
            Some(attempt) => {
                self.handle
                    .begin_attempt(message.to_vec(), SignatureEncoding::Der, attempt.clone())
            }
            None => self
                .handle
                .begin_until(message.to_vec(), SignatureEncoding::Der, self.end),
        };
        let signature = ticket
            .and_then(SignTicket::wait_blocking)
            .map_err(|_| rustls::Error::General("NATIVE_SIGN_UNAVAILABLE_OR_UNKNOWN".into()))?;
        if self.attempt.as_ref().is_some_and(|a| !a.is_current()) {
            return Err(rustls::Error::General("NATIVE_SIGN_STALE".into()));
        }
        Ok(signature)
    }
}
