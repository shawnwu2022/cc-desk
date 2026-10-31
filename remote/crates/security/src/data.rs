//! Unsplit real TLS owner. Native grant installation is separate from enrollment.
use crate::{BoundContext, CancelToken, GrantError, Role, VerifiedGrant};
use cc_desk_remote_record::grant::SessionScope;
#[cfg(test)]
use cc_desk_remote_record::{
    Kind,
    grant::{self, DataHeader, DataRole},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncRead, AsyncWrite};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataError {
    Grant(GrantError),
    Context,
    Scope,
    Role,
    Fence,
    Replay,
    Codec,
    Closed,
    Cancelled,
    Deadline,
    Io,
    Bounds,
    Full,
    Uncertain,
}
pub struct DataRecord {
    pub(super) source: Arc<crate::BoundDataAuthority>,
    pub(super) scope: SessionScope,
    pub(super) fence: u64,
    pub(super) sequence: u64,
    pub(super) payload: Vec<u8>,
    pub(super) _credit: Option<tokio::sync::OwnedSemaphorePermit>,
}
impl std::fmt::Debug for DataRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataRecord(<untrusted-scoped-payload>)")
    }
}
impl DataRecord {
    pub fn source(&self) -> &crate::BoundDataAuthority {
        &self.source
    }
    pub fn scope(&self) -> &SessionScope {
        &self.scope
    }
    pub fn fence(&self) -> u64 {
        self.fence
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    /// Untrusted operation/projection bytes; this does not attest PTY commit or CLI completion.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}
pub struct ManagedChannel<S> {
    io: Option<tokio_rustls::TlsStream<S>>,
    cancel: CancelToken,
    context: BoundContext,
    binding: [u8; 32],
    role: Role,
    access: Option<ChannelGrant>,
    grant_management: bool,
    management_used: bool,
    peer_install: bool,
    transferred: bool,

    active: Arc<AtomicBool>,
}
impl<S> ManagedChannel<S> {
    pub(crate) fn new(
        io: tokio_rustls::TlsStream<S>,
        cancel: CancelToken,
        context: BoundContext,
        binding: [u8; 32],
        role: Role,
    ) -> Self {
        Self {
            io: Some(io),
            cancel,
            context,
            binding,
            role,
            access: None,
            grant_management: false,
            management_used: false,
            peer_install: false,
            transferred: false,

            active: Arc::new(AtomicBool::new(true)),
        }
    }
    pub(crate) fn with_grant_management(mut self) -> Self {
        self.grant_management = true;
        self
    }
    pub(crate) fn with_peer_install(mut self) -> Self {
        self.peer_install = true;
        self
    }
    pub fn grant_management_enabled(&self) -> bool {
        self.grant_management
    }
    pub fn context(&self) -> &BoundContext {
        &self.context
    }
    pub fn binding_id(&self) -> [u8; 32] {
        self.binding
    }
    pub fn install_remote(&mut self, access: &RemoteGrant) -> Result<(), DataError> {
        if self.role != Role::Client || !self.grant_management {
            return Err(DataError::Role);
        }
        if !self.active.load(Ordering::Acquire) || self.io.is_none() {
            return Err(DataError::Closed);
        }
        if self.cancel.is_cancelled() {
            return Err(DataError::Cancelled);
        }
        if access.payload().context != *self.context.raw() || !access.bound_to(self.binding) {
            return Err(DataError::Context);
        }
        access.check().map_err(DataError::Grant)?;
        self.access = Some(ChannelGrant::Remote(access.clone()));
        Ok(())
    }
    pub fn install(&mut self, access: &VerifiedGrant) -> Result<(), DataError> {
        if !self.active.load(Ordering::Acquire) || self.io.is_none() {
            return Err(DataError::Closed);
        }
        if self.cancel.is_cancelled() {
            return Err(DataError::Cancelled);
        }
        if access.context() != self.context.raw() {
            return Err(DataError::Context);
        }
        access.check().map_err(DataError::Grant)?;
        if self.grant_management && self.role == Role::Client {
            return Err(DataError::Role);
        }
        self.access = Some(ChannelGrant::Native(access.clone()));
        Ok(())
    }
}
impl<S> Drop for ManagedChannel<S> {
    fn drop(&mut self) {
        if !self.transferred {
            self.active.store(false, Ordering::Release);
        }
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> ManagedChannel<S> {
    pub fn start(&mut self) -> Result<crate::RunningChannel, DataError> {
        if self.peer_install {
            return Err(DataError::Context);
        }
        self.start_mode(None)
    }
    /// Creates the original owner, with Data still denied. D is fixed before spawn.
    pub fn start_peer_pending(
        &mut self,
        caller_end: tokio::time::Instant,
    ) -> Result<crate::RunningChannel, DataError> {
        if !self.peer_install || !self.grant_management {
            return Err(DataError::Context);
        }
        self.start_mode(Some(caller_end))
    }
    fn start_mode(
        &mut self,
        peer_end: Option<tokio::time::Instant>,
    ) -> Result<crate::RunningChannel, DataError> {
        let access = self
            .access
            .as_ref()
            .ok_or(DataError::Grant(GrantError::Closed))?
            .clone();
        access.check().map_err(DataError::Grant)?;
        if self.cancel.is_cancelled() {
            self.io.take();
            self.active.store(false, Ordering::Release);
            return Err(DataError::Cancelled);
        }
        let io = self.io.take().ok_or(DataError::Closed)?;
        let owner = crate::data_owner::start(
            io,
            self.cancel.clone(),
            access,
            self.role,
            self.binding,
            self.active.clone(),
            peer_end,
        )?;
        self.transferred = true;
        Ok(owner)
    }
}
#[cfg(test)]
#[path = "data_adversarial_tests.rs"]
mod tests;

#[path = "grant_exchange.rs"]
mod grant_exchange;
pub use grant_exchange::{
    ClientGrantState, ClientGrantTrust, NativeGrantPolicy, NativeGrantSigner, RemoteGrant,
    VerifiedCanonicalGrant,
};
#[derive(Clone)]
pub(crate) enum ChannelGrant {
    Native(VerifiedGrant),
    Remote(RemoteGrant),
}
impl ChannelGrant {
    pub(crate) fn is_remote(&self) -> bool {
        matches!(self, Self::Remote(_))
    }
    pub(crate) fn check(&self) -> Result<u64, GrantError> {
        match self {
            Self::Native(g) => g.check(),
            Self::Remote(g) => g.check(),
        }
    }
    pub(crate) fn scope(&self) -> &SessionScope {
        match self {
            Self::Native(g) => g.scope(),
            Self::Remote(g) => g.scope(),
        }
    }
    pub(crate) fn role(&self) -> cc_desk_remote_record::grant::DataRole {
        match self {
            Self::Native(g) => g.role(),
            Self::Remote(g) => g.role(),
        }
    }
    pub(crate) fn context(&self) -> &cc_desk_remote_record::appbind::Context {
        match self {
            Self::Native(g) => g.context(),
            Self::Remote(g) => &g.payload().context,
        }
    }
    pub(crate) fn principal_id(&self) -> [u8; 32] {
        match self {
            Self::Native(g) => g.principal_id(),
            Self::Remote(g) => crate::grant::hex_id(&g.payload().principal_id),
        }
    }
    pub(crate) fn id(&self) -> [u8; 32] {
        match self {
            Self::Native(g) => g.id(),
            Self::Remote(g) => crate::grant::hex_id(&g.payload().grant_id),
        }
    }
    pub(crate) fn created_host_ms(&self) -> u64 {
        match self {
            Self::Native(g) => g.created_host_ms(),
            Self::Remote(g) => g.payload().created_host_ms.parse().unwrap(),
        }
    }
    pub(crate) fn expires_host_ms(&self) -> u64 {
        match self {
            Self::Native(g) => g.expires_host_ms(),
            Self::Remote(g) => g.expires_host_ms(),
        }
    }
    pub(crate) fn clock_domain(&self) -> crate::GrantClockDomain {
        match self {
            Self::Native(g) => g.clock_domain(),
            Self::Remote(g) => g.clock_domain(),
        }
    }
    pub(crate) fn revoked(
        &self,
    ) -> (
        tokio::sync::watch::Receiver<bool>,
        tokio::sync::watch::Receiver<bool>,
    ) {
        match self {
            Self::Native(g) => g.revoked(),
            Self::Remote(g) => g.revoked(),
        }
    }
    pub(crate) fn scope_digest(&self) -> [u8; 32] {
        match self {
            Self::Native(g) => g.scope_digest(),
            Self::Remote(g) => {
                let mut h = ring::digest::Context::new(&ring::digest::SHA256);
                h.update(b"CC-DESK-DATA-SCOPE-v1\0");
                h.update(&cc_desk_remote_record::grant::encode_scope(g.scope()).unwrap());
                h.finish().as_ref().try_into().unwrap()
            }
        }
    }
}

#[cfg(test)]
#[path = "grant_exchange_tests.rs"]
mod grant_exchange_tests;

#[cfg(test)]
#[path = "peer_install_tests.rs"]
pub(crate) mod peer_install_tests;
