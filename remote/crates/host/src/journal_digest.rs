//! 后端请求摘要，不是设备授权；不得导出 key 或把它写入日志。
use crate::journal::{KeyedDigest, OperationKey};
pub struct DigestKey(ring::hmac::Key);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestError {
    Bounds,
}
impl DigestKey {
    #[cfg(feature = "experimental-host-owner")]
    pub(crate) fn lifecycle_result(&self, b: &[u8]) -> Result<[u8; 32], DigestError> {
        self.lifecycle_mac(b"CC-DESK-LIFECYCLE-RESULT-v2\0", b)
    }
    #[cfg(feature = "experimental-host-owner")]
    pub(crate) fn lifecycle_key_id(&self) -> [u8; 32] {
        self.lifecycle_mac(b"CC-DESK-LIFECYCLE-KEY-ID-v1\0", b"journal binding")
            .unwrap()
    }
    #[cfg(feature = "experimental-host-owner")]
    pub(crate) fn lifecycle_scope(&self, identity: &[u8]) -> Result<[u8; 32], DigestError> {
        self.lifecycle_mac(b"CC-DESK-LIFECYCLE-SCOPE-v1\0", identity)
    }
    #[cfg(feature = "experimental-host-owner")]
    pub(crate) fn lifecycle_request(&self, metadata: &[u8]) -> Result<[u8; 32], DigestError> {
        self.lifecycle_mac(b"CC-DESK-LIFECYCLE-REQUEST-v1\0", metadata)
    }
    #[cfg(feature = "experimental-host-owner")]
    fn lifecycle_mac(&self, label: &[u8], bytes: &[u8]) -> Result<[u8; 32], DigestError> {
        if bytes.is_empty() || bytes.len() > 32768 {
            return Err(DigestError::Bounds);
        }
        let mut h = ring::hmac::Context::with_key(&self.0);
        h.update(label);
        h.update(&(bytes.len() as u32).to_be_bytes());
        h.update(bytes);
        Ok(h.sign().as_ref().try_into().unwrap())
    }
    /// Synchronization metadata is keyed with the exact input; it is not an
    /// endpoint grant. No raw terminal bytes or dictionary-testable hash persists.
    #[cfg(feature = "experimental-host-owner")]
    pub(crate) fn input_in_context(
        &self,
        operation: &OperationKey,
        epoch: u64,
        bytes: &[u8],
        minor: u16,
        context: &[u8],
    ) -> Result<KeyedDigest, DigestError> {
        if minor > 1 || context.is_empty() || context.len() > 16384 {
            return Err(DigestError::Bounds);
        }
        let base = self.input(operation, epoch, bytes)?;
        let mut h = ring::hmac::Context::with_key(&self.0);
        h.update(b"CC-DESK-VIEW-INPUT-DIGEST-v1\0");
        h.update(&base.0);
        h.update(&minor.to_be_bytes());
        h.update(&(context.len() as u32).to_be_bytes());
        h.update(context);
        let mut value = [0; 32];
        value.copy_from_slice(h.sign().as_ref());
        Ok(KeyedDigest(value))
    }
    pub fn from_backend_bytes(bytes: [u8; 32]) -> Self {
        Self(ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &bytes))
    }
    pub fn input(
        &self,
        operation: &OperationKey,
        epoch: u64,
        bytes: &[u8],
    ) -> Result<KeyedDigest, DigestError> {
        if operation.run.is_empty()
            || operation.run.len() > 128
            || bytes.is_empty()
            || bytes.len() > 16384
        {
            return Err(DigestError::Bounds);
        }
        let mut h = ring::hmac::Context::with_key(&self.0);
        h.update(b"CC-DESK-LOCAL-INPUT-DIGEST-v1\0");
        h.update(&operation.host);
        h.update(&operation.session);
        h.update(&(operation.run.len() as u16).to_be_bytes());
        h.update(operation.run.as_bytes());
        h.update(&operation.generation.to_be_bytes());
        h.update(&operation.incarnation.to_be_bytes());
        h.update(&operation.device);
        h.update(&operation.operation);
        h.update(&operation.sequence.to_be_bytes());
        h.update(&epoch.to_be_bytes());
        h.update(&(bytes.len() as u32).to_be_bytes());
        h.update(bytes);
        let mut value = [0; 32];
        value.copy_from_slice(h.sign().as_ref());
        Ok(KeyedDigest(value))
    }
}
