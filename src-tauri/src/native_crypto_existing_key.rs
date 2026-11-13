//! 固定 Software KSP 的已有 key 引用。没有枚举、创建、导入或交互回退。
#![allow(dead_code, reason = "native profile selection is not installed")]
use super::*;
use owner::{NativeRevocation, SignFailure};
#[cfg(test)]
use std::sync::Arc;
use std::time::Instant;
/// 封闭加载器只借用原 worker 的 slot，不能接收外来裸句柄。
pub(crate) struct ExistingKeyLoader {
    name: Vec<u16>,
    purpose: SigningPurpose,
    spki: [u8; 91],
    #[cfg(test)]
    api: Option<Arc<dyn MockKeyApi>>,
    #[cfg(test)]
    anonymous: Option<[u8; 32]>,
    #[cfg(test)]
    opened: Option<(Arc<[std::sync::atomic::AtomicUsize; 2]>, usize)>,
}
impl ExistingKeyLoader {
    pub(super) fn new(
        name: &str,
        purpose: SigningPurpose,
        spki: [u8; 91],
    ) -> Result<Self, SignFailure> {
        if !matches!(purpose, SigningPurpose::Endpoint | SigningPurpose::Relay)
            || name.is_empty()
            || name.chars().any(char::is_control)
            || name.encode_utf16().count() > 256
            || cc_desk_remote_security::P256Spki::new(&spki).is_err()
        {
            return Err(SignFailure::Bounds);
        }
        Ok(Self {
            name: name.encode_utf16().chain(Some(0)).collect(),
            purpose,
            spki,
            #[cfg(test)]
            api: None,
            #[cfg(test)]
            anonymous: None,
            #[cfg(test)]
            opened: None,
        })
    }
    #[cfg(test)]
    pub(crate) fn test_api(&mut self, api: Arc<dyn MockKeyApi>) {
        self.api = Some(api);
        self.anonymous = None;
    }
    #[cfg(test)]
    pub(crate) fn test_anonymous(&mut self, scalar: [u8; 32]) {
        self.api = None;
        self.anonymous = Some(scalar);
    }
    #[cfg(test)]
    pub(crate) fn test_count(
        &mut self,
        counts: Arc<[std::sync::atomic::AtomicUsize; 2]>,
        index: usize,
    ) {
        assert!(index < 2);
        self.opened = Some((counts, index));
    }
    pub(super) fn fill(
        &self,
        slot: &mut NativeOwnedSoftwareKey,
        token: &NativeRevocation,
        cap: Instant,
    ) -> Result<(), CryptoError> {
        if slot.approved_purpose != self.purpose
            || slot.approved_spki != self.spki
            || slot.provider.0 != 0
            || slot.key.0 != 0
        {
            return Err(CryptoError::Key);
        }
        // 首个 native 调用前绑定整套拦截器；free 同样不允许真实回退。
        #[cfg(test)]
        {
            slot.mock_api = self.api.clone();
        }
        checkpoint(slot, token, cap)?;
        slot.provider = self.provider()?;
        checkpoint(slot, token, cap)?;
        slot.key = self.open(slot.provider)?;
        #[cfg(test)]
        if let Some((counts, index)) = &self.opened {
            counts[*index].fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        checkpoint(slot, token, cap)?;
        let length = slot.read_property(NCRYPT_LENGTH_PROPERTY)?;
        checkpoint(slot, token, cap)?;
        if length != 256u32.to_le_bytes() {
            return Err(CryptoError::Key);
        }
        let group = slot.read_property(NCRYPT_ALGORITHM_GROUP_PROPERTY)?;
        checkpoint(slot, token, cap)?;
        if group
            != "ECDSA\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        {
            return Err(CryptoError::Key);
        }
        let usage = slot.read_property(NCRYPT_KEY_USAGE_PROPERTY)?;
        checkpoint(slot, token, cap)?;
        let usage = u32::from_le_bytes(usage.as_slice().try_into().map_err(|_| CryptoError::Key)?);
        if usage & NCRYPT_ALLOW_SIGNING_FLAG == 0 {
            return Err(CryptoError::Key);
        }
        let public = self.public(slot.key)?;
        checkpoint(slot, token, cap)?;
        if spki_from_blob(&public)? != self.spki {
            return Err(CryptoError::Key);
        }
        Ok(())
    }
    fn provider(&self) -> Result<NCRYPT_PROV_HANDLE, CryptoError> {
        #[cfg(test)]
        if let Some(api) = &self.api {
            return api
                .provider(MS_KEY_STORAGE_PROVIDER, 0)
                .map(NCRYPT_PROV_HANDLE);
        }
        let mut provider = NCRYPT_PROV_HANDLE::default();
        // 只有成功返回才取得输出所有权；不猜测失败输出的非零位是否可 free。
        unsafe { NCryptOpenStorageProvider(&mut provider, MS_KEY_STORAGE_PROVIDER, 0) }
            .map_err(|_| CryptoError::Native)?;
        Ok(provider)
    }
    fn open(&self, provider: NCRYPT_PROV_HANDLE) -> Result<NCRYPT_KEY_HANDLE, CryptoError> {
        #[cfg(test)]
        if let Some(api) = &self.api {
            return api
                .open(provider.0, &self.name, 0, NCRYPT_SILENT_FLAG.0)
                .map(NCRYPT_KEY_HANDLE);
        }
        let mut key = NCRYPT_KEY_HANDLE::default();
        #[cfg(test)]
        if let Some(scalar) = &self.anonymous {
            // 公开 TEST_ONLY 标量；匿名导入仅替代测试打开，不写持久 key 或用户库。
            let mut blob = Vec::from(BCRYPT_ECDSA_PRIVATE_P256_MAGIC.to_le_bytes());
            blob.extend(32u32.to_le_bytes());
            blob.extend(&self.spki[SPKI_HEAD.len()..]);
            blob.extend(scalar);
            unsafe {
                NCryptImportKey(
                    provider,
                    None,
                    BCRYPT_ECCPRIVATE_BLOB,
                    None,
                    &mut key,
                    &blob,
                    NCRYPT_SILENT_FLAG,
                )
            }
            .map_err(|_| CryptoError::Native)?;
            return Ok(key);
        }
        unsafe {
            NCryptOpenKey(
                provider,
                &mut key,
                windows_core::PCWSTR(self.name.as_ptr()),
                CERT_KEY_SPEC(0),
                NCRYPT_SILENT_FLAG,
            )
        }
        .map_err(|_| CryptoError::Native)?;
        Ok(key)
    }
    fn public(&self, key: NCRYPT_KEY_HANDLE) -> Result<Vec<u8>, CryptoError> {
        #[cfg(test)]
        if let Some(api) = &self.api {
            return api.public(key.0, NCRYPT_SILENT_FLAG.0);
        }
        let mut blob = [0; 72];
        let mut size = 0;
        unsafe {
            NCryptExportKey(
                key,
                None,
                BCRYPT_ECCPUBLIC_BLOB,
                None,
                Some(&mut blob),
                &mut size,
                NCRYPT_SILENT_FLAG,
            )
        }
        .map_err(|_| CryptoError::Native)?;
        if size != 72 {
            return Err(CryptoError::Key);
        }
        Ok(blob.to_vec())
    }
}
fn checkpoint(
    slot: &NativeOwnedSoftwareKey,
    token: &NativeRevocation,
    cap: Instant,
) -> Result<(), CryptoError> {
    slot.check_worker()?;
    if token.is_revoked() || Instant::now() >= cap {
        token.revoke();
        return Err(CryptoError::Closed);
    }
    Ok(())
}
/// 没有默认方法；每种 fake handle 操作都必须显式拦截。
#[cfg(test)]
pub(crate) trait MockKeyApi: Send + Sync {
    fn provider(&self, name: windows_core::PCWSTR, flags: u32) -> Result<usize, CryptoError>;
    fn open(
        &self,
        provider: usize,
        name: &[u16],
        legacy: u32,
        flags: u32,
    ) -> Result<usize, CryptoError>;
    fn property(
        &self,
        key: usize,
        name: windows_core::PCWSTR,
        flags: u32,
    ) -> Result<Vec<u8>, CryptoError>;
    fn public(&self, key: usize, flags: u32) -> Result<Vec<u8>, CryptoError>;
    fn free(&self, handle: usize) -> Result<(), CryptoError>;
}
