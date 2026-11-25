//! Native P256 primitive qualification. No key selection, enrollment or IPC.
use sha2::{Digest, Sha256};
use std::{marker::PhantomData, rc::Rc, thread::ThreadId};
use windows::Win32::Security::Cryptography::{
    CryptEncodeObjectEx, CERT_ECC_SIGNATURE, CRYPT_ENCODE_OBJECT_FLAGS, CRYPT_INTEGER_BLOB,
    X509_ASN_ENCODING, X509_ECC_SIGNATURE,
};
use windows::Win32::Security::{Cryptography::*, OBJECT_SECURITY_INFORMATION};

const SPKI_HEAD:&[u8]=b"\x30\x59\x30\x13\x06\x07\x2a\x86\x48\xce\x3d\x02\x01\x06\x08\x2a\x86\x48\xce\x3d\x03\x01\x07\x03\x42\x00\x04";
const MAX_SIGN_BYTES: usize = 65536;
#[cfg(test)]
type FreeEffect = Box<dyn FnMut(NCRYPT_HANDLE) -> Result<(), CryptoError>>;

/// Backend use binding, never a deserialized request or a JS signing service.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // Native bootstrap/factory is deliberately not connected.
pub(crate) enum SigningPurpose {
    Endpoint,
    Relay,
    Issuer,
}

/// 原 worker 独占 native 资源；Host 封闭 loader 只填充已保管的空 slot。
/// 不提供 IPC 裸句柄构造或默认批准，真实配置选择和应用入口仍未安装。
#[must_use = "Retain original native ownership through explicit retirement; Unknown is not closed"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct NativeOwnedSoftwareKey {
    provider: NCRYPT_PROV_HANDLE,
    key: NCRYPT_KEY_HANDLE,
    approved_purpose: SigningPurpose,
    approved_spki: [u8; 91],
    #[cfg(feature = "remote-host")]
    silent_metadata: bool,
    worker: ThreadId,
    unknown: bool,
    _not_send_or_sync: PhantomData<Rc<()>>,
    #[cfg(test)]
    free_effect: Option<FreeEffect>,
    #[cfg(all(test, feature = "remote-host"))]
    mock_api: Option<std::sync::Arc<dyn existing::MockKeyApi>>,
}
impl std::fmt::Debug for NativeOwnedSoftwareKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeOwnedSoftwareKey(<redacted>)")
    }
}
#[cfg_attr(not(test), allow(dead_code))]
impl NativeOwnedSoftwareKey {
    fn check_worker(&self) -> Result<(), CryptoError> {
        if self.worker != std::thread::current().id() {
            return Err(CryptoError::Native);
        }
        if self.unknown {
            return Err(CryptoError::Unknown);
        }
        Ok(())
    }
    /// Synchronous: only the original dedicated native owner may call this.
    /// No timeout/cancellation or Drop is evidence of a successful free.
    pub(crate) fn try_retire(&mut self) -> Result<(), CryptoError> {
        self.check_worker()?;
        if self.key.0 != 0 {
            if self.free_object(NCRYPT_HANDLE(self.key.0)).is_err() {
                self.unknown = true;
                return Err(CryptoError::Unknown);
            }
            self.key = NCRYPT_KEY_HANDLE::default();
        }
        if self.provider.0 != 0 {
            if self.free_object(NCRYPT_HANDLE(self.provider.0)).is_err() {
                self.unknown = true;
                return Err(CryptoError::Unknown);
            }
            self.provider = NCRYPT_PROV_HANDLE::default();
        }
        Ok(())
    }
    fn read_property(&self, name: windows_core::PCWSTR) -> Result<Vec<u8>, CryptoError> {
        let flags = OBJECT_SECURITY_INFORMATION(0);
        #[cfg(feature = "remote-host")]
        let flags = if self.silent_metadata {
            OBJECT_SECURITY_INFORMATION(NCRYPT_SILENT_FLAG.0)
        } else {
            flags
        };
        #[cfg(all(test, feature = "remote-host"))]
        if let Some(api) = &self.mock_api {
            return api.property(self.key.0, name, flags.0);
        }
        property(self.key, name, flags)
    }
    fn free_object(&mut self, handle: NCRYPT_HANDLE) -> Result<(), CryptoError> {
        #[cfg(all(test, feature = "remote-host"))]
        if let Some(api) = &self.mock_api {
            return api.free(handle.0);
        }
        #[cfg(test)]
        if let Some(effect) = self.free_effect.as_mut() {
            return effect(handle);
        }
        unsafe { NCryptFreeObject(handle) }.map_err(|_| CryptoError::Unknown)
    }
    pub(crate) fn explicitly_retired(&self) -> bool {
        !self.unknown && self.key.0 == 0 && self.provider.0 == 0
    }
    #[cfg(test)]
    fn retain_unknown_for_test(self) -> ! {
        let _original = self;
        eprintln!("TEST_ONLY_CNG_CLEANUP_UNKNOWN: original native ownership retained; no retry");
        loop {
            std::thread::park();
        }
    }
}
impl Drop for NativeOwnedSoftwareKey {
    fn drop(&mut self) {
        // Failsafe only. Qualification and application retirement must use the
        // explicit result while retaining this original owner on Unknown.
        if !self.explicitly_retired() && self.try_retire().is_err() {
            log::warn!("native software key retirement unknown");
        }
    }
}

/// Borrowing prevents replacing/freeing the key while signing; !Send/!Sync
/// prevents moving custody between threads. Calls still require an original
/// dedicated native worker: this creates no executor or preemption guarantee.
/// No rustls SigningKey or NativeGrantSigner implementation is supplied.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct CngP256Signer<'key> {
    owned: &'key mut NativeOwnedSoftwareKey,
    purpose: SigningPurpose,
    public: [u8; 91],
}
impl std::fmt::Debug for CngP256Signer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CngP256Signer(<redacted>)")
    }
}
#[cfg_attr(not(test), allow(dead_code))]
impl<'key> CngP256Signer<'key> {
    /// Approval inputs come from the native owner, never raw wire/JS authority.
    pub(crate) fn admit(
        owned: &'key mut NativeOwnedSoftwareKey,
        purpose: SigningPurpose,
        expected_spki: &[u8],
    ) -> Result<Self, CryptoError> {
        owned.check_worker()?;
        // 假句柄只走完整测试 API；误发签名也不能落入真实 CNG。
        #[cfg(all(test, feature = "remote-host"))]
        if owned.mock_api.is_some() {
            return Err(CryptoError::Key);
        }
        if owned.key.0 == 0 || owned.provider.0 == 0 {
            return Err(CryptoError::Closed);
        }
        if owned.approved_purpose != purpose {
            return Err(CryptoError::Purpose);
        }
        if owned.approved_spki != expected_spki {
            return Err(CryptoError::Key);
        }
        if expected_spki.len() != 91 || !expected_spki.starts_with(SPKI_HEAD) {
            return Err(CryptoError::Key);
        }
        if owned.read_property(NCRYPT_LENGTH_PROPERTY)? != 256u32.to_le_bytes()
            || owned.read_property(NCRYPT_ALGORITHM_GROUP_PROPERTY)?
                != "ECDSA\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
        {
            return Err(CryptoError::Key);
        }
        let usage = owned.read_property(NCRYPT_KEY_USAGE_PROPERTY)?;
        let usage = u32::from_le_bytes(usage.as_slice().try_into().map_err(|_| CryptoError::Key)?);
        if usage & NCRYPT_ALLOW_SIGNING_FLAG == 0 {
            return Err(CryptoError::Key);
        }
        let mut blob = [0; 72];
        let mut size = 0;
        unsafe {
            NCryptExportKey(
                owned.key,
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
        let public = spki_from_blob(&blob)?;
        if public != expected_spki {
            return Err(CryptoError::Key);
        }
        Ok(Self {
            owned,
            purpose,
            public,
        })
    }
    pub(crate) fn public_spki(&self) -> &[u8] {
        &self.public
    }
    pub(crate) fn sign_fixed(
        &mut self,
        purpose: SigningPurpose,
        message: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.owned.check_worker()?;
        if self.purpose != purpose {
            return Err(CryptoError::Purpose);
        }
        if message.len() > MAX_SIGN_BYTES {
            return Err(CryptoError::Bounds);
        }
        let digest = Sha256::digest(message);
        let mut fixed = [0; 64];
        let mut size = 0;
        unsafe {
            NCryptSignHash(
                self.owned.key,
                None,
                &digest,
                Some(&mut fixed),
                &mut size,
                NCRYPT_SILENT_FLAG,
            )
        }
        .map_err(|_| CryptoError::Native)?;
        if size != 64 {
            return Err(CryptoError::Native);
        }
        Ok(fixed.to_vec())
    }
    pub(crate) fn sign_der(
        &mut self,
        purpose: SigningPurpose,
        message: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        fixed_to_der(&self.sign_fixed(purpose, message)?)
    }
}
fn property(
    key: NCRYPT_KEY_HANDLE,
    name: windows_core::PCWSTR,
    flags: OBJECT_SECURITY_INFORMATION,
) -> Result<Vec<u8>, CryptoError> {
    property_bounded::<64>(key, name, flags)
}
fn property_bounded<const MAX: usize>(
    key: NCRYPT_KEY_HANDLE,
    name: windows_core::PCWSTR,
    flags: OBJECT_SECURITY_INFORMATION,
) -> Result<Vec<u8>, CryptoError> {
    let mut output = [0u8; MAX];
    let mut size = 0;
    unsafe {
        NCryptGetProperty(
            NCRYPT_HANDLE(key.0),
            name,
            Some(&mut output),
            &mut size,
            flags,
        )
    }
    .map_err(|_| CryptoError::Native)?;
    let size = usize::try_from(size).map_err(|_| CryptoError::Key)?;
    if size > output.len() {
        return Err(CryptoError::Key);
    }
    Ok(output[..size].to_vec())
}
fn spki_from_blob(blob: &[u8]) -> Result<[u8; 91], CryptoError> {
    if blob.len() != 72
        || blob[..4] != BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes()
        || blob[4..8] != 32u32.to_le_bytes()
    {
        return Err(CryptoError::Key);
    }
    let mut spki = [0u8; 91];
    spki[..SPKI_HEAD.len()].copy_from_slice(SPKI_HEAD);
    spki[SPKI_HEAD.len()..].copy_from_slice(&blob[8..]);
    Ok(spki)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CryptoError {
    Bounds,
    Native,
    Purpose,
    Key,
    Closed,
    Unknown,
}

fn fixed_to_der(fixed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if fixed.len() != 64 {
        return Err(CryptoError::Bounds);
    }
    let mut r: [u8; 32] = fixed[..32].try_into().map_err(|_| CryptoError::Bounds)?;
    let mut s: [u8; 32] = fixed[32..].try_into().map_err(|_| CryptoError::Bounds)?;
    r.reverse();
    s.reverse(); // CERT_ECC_SIGNATURE's documented unsigned little endian.
    let signature = CERT_ECC_SIGNATURE {
        r: CRYPT_INTEGER_BLOB {
            cbData: 32,
            pbData: r.as_mut_ptr(),
        },
        s: CRYPT_INTEGER_BLOB {
            cbData: 32,
            pbData: s.as_mut_ptr(),
        },
    };
    let mut output = [0u8; 72];
    let mut size = output.len() as u32;
    unsafe {
        CryptEncodeObjectEx(
            X509_ASN_ENCODING,
            X509_ECC_SIGNATURE,
            &signature as *const _ as _,
            CRYPT_ENCODE_OBJECT_FLAGS(0),
            None,
            Some(output.as_mut_ptr().cast()),
            &mut size,
        )
    }
    .map_err(|_| CryptoError::Native)?;
    let size = usize::try_from(size).map_err(|_| CryptoError::Bounds)?;
    if !(8..=72).contains(&size) {
        return Err(CryptoError::Bounds);
    }
    Ok(output[..size].to_vec())
}
#[cfg(test)]
#[path = "native_crypto_tests.rs"]
mod tests;
#[cfg(all(test, feature = "remote-host"))]
pub(crate) use tests::{test_existing_host_supplier, test_host_supplier};

#[path = "native_crypto_owner.rs"]
pub(crate) mod owner;

#[cfg(any(test, feature = "remote-client-ipc"))]
#[path = "native_crypto_signers.rs"]
pub(crate) mod signers;

#[cfg(feature = "remote-host")]
#[path = "native_crypto_host.rs"]
pub(crate) mod host;

#[cfg(feature = "remote-host")]
#[path = "native_crypto_existing_key.rs"]
pub(crate) mod existing;
#[cfg(feature = "remote-host")]
#[path = "native_crypto_host_profile.rs"]
pub(crate) mod host_profile;

#[cfg(all(test, feature = "remote-host"))]
pub(crate) struct SingleRunTestKeys {
    pub(crate) custody: host::HostKeyCustody,
    pub(crate) ready: host::HostKeysReady,
    pub(crate) signers: host::HostSignerSet,
}
#[cfg(all(test, feature = "remote-host"))]
pub(crate) fn single_run_test_keys(supplier: host::FrozenHostSupplier) -> SingleRunTestKeys {
    tests::existing_key_tests::single_run_keys(supplier, None)
}

#[cfg(all(test, feature = "remote-host"))]
pub(crate) fn single_run_test_keys_with_hook(
    supplier: host::FrozenHostSupplier,
    hook: Box<dyn FnOnce() + Send>,
) -> SingleRunTestKeys {
    tests::existing_key_tests::single_run_keys(supplier, Some(hook))
}

#[cfg(all(test, feature = "remote-host"))]
#[derive(Clone, Copy, Default)]
pub(crate) enum EpochTestFault {
    #[default]
    None,
    WrongSpki,
    SecondOpen,
    FreeUnknown,
}
#[cfg(all(test, feature = "remote-host"))]
pub(crate) type EpochTestAction =
    std::sync::Arc<parking_lot::Mutex<Option<Box<dyn FnOnce() + Send>>>>;
#[cfg(all(test, feature = "remote-host"))]
pub(crate) type EpochTestEvents = std::sync::Arc<parking_lot::Mutex<Vec<(u64, &'static str)>>>;
#[cfg(all(test, feature = "remote-host"))]
pub(crate) type EpochTestObserver =
    std::sync::Arc<parking_lot::Mutex<Option<std::sync::Arc<host::HostKeyCore>>>>;
#[cfg(all(test, feature = "remote-host"))]
#[derive(Clone, Default)]
pub(crate) struct EpochTestTrace {
    pub(crate) events: EpochTestEvents,
    pub(crate) fault: EpochTestFault,
    pub(crate) after_join: EpochTestAction,
    pub(crate) observer: EpochTestObserver,
    pub(crate) after_second_verified: EpochTestAction,
}
#[cfg(all(test, feature = "remote-host"))]
pub(crate) fn epoch_test_supplier(
    supplier: host::FrozenHostSupplier,
    token: std::sync::Arc<owner::NativeRevocation>,
    generation: u64,
    trace: &EpochTestTrace,
) -> host::FrozenHostSupplier {
    tests::existing_key_tests::epoch_supplier(supplier, token, generation, trace)
}

#[cfg(feature = "remote-observer")]
#[path = "native_crypto_provisioning.rs"]
pub(crate) mod provisioning;
