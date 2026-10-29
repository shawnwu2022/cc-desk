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
    access: Option<VerifiedGrant>,
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
            transferred: false,

            active: Arc::new(AtomicBool::new(true)),
        }
    }
    pub fn context(&self) -> &BoundContext {
        &self.context
    }
    pub fn binding_id(&self) -> [u8; 32] {
        self.binding
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
        self.access = Some(access.clone());
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
        )?;
        self.transferred = true;
        Ok(owner)
    }
}
#[cfg(test)]
#[path = "data_adversarial_tests.rs"]
mod tests;
