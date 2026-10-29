//! 后端请求摘要，不是设备授权；不得导出 key 或把它写入日志。
use crate::journal::{KeyedDigest, OperationKey};
pub struct DigestKey(ring::hmac::Key);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestError {
    Bounds,
}
impl DigestKey {
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
