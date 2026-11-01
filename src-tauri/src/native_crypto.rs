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

/// Sole native ownership, bound to its original worker. There is intentionally
/// no production constructor from raw handles, names or booleans in this slice.
/// A later trusted native bootstrap must open the fixed Software KSP internally.
#[must_use = "Retain original native ownership through explicit retirement; Unknown is not closed"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct NativeOwnedSoftwareKey {
    provider: NCRYPT_PROV_HANDLE,
    key: NCRYPT_KEY_HANDLE,
    approved_purpose: SigningPurpose,
    approved_spki: [u8; 91],
    worker: ThreadId,
    unknown: bool,
    _not_send_or_sync: PhantomData<Rc<()>>,
    #[cfg(test)]
    free_effect: Option<FreeEffect>,
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
    fn free_object(&mut self, handle: NCRYPT_HANDLE) -> Result<(), CryptoError> {
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
        if property(owned.key, NCRYPT_LENGTH_PROPERTY)? != 256u32.to_le_bytes()
            || property(owned.key, NCRYPT_ALGORITHM_GROUP_PROPERTY)?
                != "ECDSA\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
        {
            return Err(CryptoError::Key);
        }
        let usage = property(owned.key, NCRYPT_KEY_USAGE_PROPERTY)?;
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
fn property(key: NCRYPT_KEY_HANDLE, name: windows_core::PCWSTR) -> Result<Vec<u8>, CryptoError> {
    let mut output = [0u8; 64];
    let mut size = 0;
    unsafe {
        NCryptGetProperty(
            NCRYPT_HANDLE(key.0),
            name,
            Some(&mut output),
            &mut size,
            OBJECT_SECURITY_INFORMATION(0),
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
