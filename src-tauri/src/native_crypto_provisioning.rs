//! Fresh native pairing keys. User scope, generated names, no overwrite,
//! no import/export of private bytes, and cleanup only for this transaction's keys.
use super::*;

pub(crate) struct PendingPair {
    provider: NCRYPT_PROV_HANDLE,
    keys: [NCRYPT_KEY_HANDLE; 2],
    keep: bool,
}
pub(crate) struct PublicPair {
    pub(crate) endpoint: [u8; 91],
    pub(crate) relay: [u8; 91],
}
impl PendingPair {
    pub(crate) fn create(names: [&str; 2]) -> Result<(Self, PublicPair), CryptoError> {
        if names[0] == names[1]
            || names.iter().any(|n| {
                !n.starts_with("ccdesk-native-v1-")
                    || n.len() > 80
                    || !n
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        {
            return Err(CryptoError::Key);
        }
        let mut owner = Self {
            provider: Default::default(),
            keys: Default::default(),
            keep: false,
        };
        unsafe { NCryptOpenStorageProvider(&mut owner.provider, MS_KEY_STORAGE_PROVIDER, 0) }
            .map_err(|_| CryptoError::Native)?;
        let endpoint = owner.create_one(0, names[0])?;
        let relay = owner.create_one(1, names[1])?;
        if endpoint == relay {
            return Err(CryptoError::Key);
        }
        Ok((owner, PublicPair { endpoint, relay }))
    }
    fn create_one(&mut self, index: usize, name: &str) -> Result<[u8; 91], CryptoError> {
        let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        // Public key handle retained only after successful create. A preexisting
        // name collision is never opened, overwritten, or deleted.
        let mut made = NCRYPT_KEY_HANDLE::default();
        unsafe {
            NCryptCreatePersistedKey(
                self.provider,
                &mut made,
                NCRYPT_ECDSA_P256_ALGORITHM,
                windows_core::PCWSTR(name.as_ptr()),
                CERT_KEY_SPEC(0),
                NCRYPT_SILENT_FLAG,
            )
        }
        .map_err(|_| CryptoError::Native)?;
        self.keys[index] = made;
        let handle = NCRYPT_HANDLE(made.0);
        use crate::version_history::windows::security::CurrentUser;
        use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
        let user = CurrentUser::capture().map_err(|_| CryptoError::Key)?;
        let descriptor = user.native_key_descriptor().map_err(|_| CryptoError::Key)?;
        let access =
            DACL_SECURITY_INFORMATION.0 | OWNER_SECURITY_INFORMATION.0 | NCRYPT_SILENT_FLAG.0;

        unsafe {
            NCryptSetProperty(
                handle,
                NCRYPT_KEY_USAGE_PROPERTY,
                &NCRYPT_ALLOW_SIGNING_FLAG.to_le_bytes(),
                NCRYPT_SILENT_FLAG,
            )
            .and_then(|_| {
                NCryptSetProperty(
                    handle,
                    NCRYPT_EXPORT_POLICY_PROPERTY,
                    &0u32.to_le_bytes(),
                    NCRYPT_SILENT_FLAG,
                )
            })
            .and_then(|_| NCryptFinalizeKey(made, NCRYPT_SILENT_FLAG))
        }
        .map_err(|_| CryptoError::Native)?;
        unsafe {
            NCryptSetProperty(
                handle,
                NCRYPT_SECURITY_DESCR_PROPERTY,
                &descriptor,
                NCRYPT_FLAGS(access | NCRYPT_PERSIST_FLAG.0),
            )
        }
        .map_err(|_| CryptoError::Key)?;
        let silent = OBJECT_SECURITY_INFORMATION(NCRYPT_SILENT_FLAG.0);
        if !exact_policy(
            &property(made, NCRYPT_KEY_USAGE_PROPERTY, silent)?,
            &property(made, NCRYPT_EXPORT_POLICY_PROPERTY, silent)?,
        ) || property(made, NCRYPT_LENGTH_PROPERTY, silent)? != 256u32.to_le_bytes()
        {
            return Err(CryptoError::Key);
        }
        user.verify_native_key_descriptor(&security_descriptor(
            made,
            OBJECT_SECURITY_INFORMATION(access),
        )?)
        .map_err(|_| CryptoError::Key)?;
        let mut public = [0; 72];
        let mut bytes = 0;
        unsafe {
            NCryptExportKey(
                made,
                None,
                BCRYPT_ECCPUBLIC_BLOB,
                None,
                Some(&mut public),
                &mut bytes,
                NCRYPT_SILENT_FLAG,
            )
        }
        .map_err(|_| CryptoError::Native)?;
        if bytes != 72 {
            return Err(CryptoError::Key);
        }
        spki_from_blob(&public)
    }
    /// Also used on an uncertain durable commit; a published profile must never
    /// be invalidated by cleanup before the original result can be rechecked.
    pub(crate) fn retain(&mut self) {
        self.keep = true;
    }
}
impl Drop for PendingPair {
    fn drop(&mut self) {
        for key in self.keys.iter_mut().filter(|key| key.0 != 0) {
            let deleted =
                !self.keep && unsafe { NCryptDeleteKey(*key, NCRYPT_SILENT_FLAG.0) }.is_ok();
            if !deleted {
                let _ = unsafe { NCryptFreeObject(NCRYPT_HANDLE(key.0)) };
            }
            *key = Default::default();
        }
        if self.provider.0 != 0 {
            let _ = unsafe { NCryptFreeObject(NCRYPT_HANDLE(self.provider.0)) };
        }
    }
}
pub(super) fn verify_persisted(key: NCRYPT_KEY_HANDLE) -> Result<(), CryptoError> {
    use crate::version_history::windows::security::CurrentUser;
    use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
    let silent = OBJECT_SECURITY_INFORMATION(NCRYPT_SILENT_FLAG.0);
    if !exact_policy(
        &property(key, NCRYPT_KEY_USAGE_PROPERTY, silent)?,
        &property(key, NCRYPT_EXPORT_POLICY_PROPERTY, silent)?,
    ) {
        return Err(CryptoError::Key);
    }
    let acl = OBJECT_SECURITY_INFORMATION(
        OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0 | NCRYPT_SILENT_FLAG.0,
    );
    CurrentUser::capture()
        .and_then(|user| {
            user.verify_native_key_descriptor(
                &security_descriptor(key, acl)
                    .map_err(|_| std::io::Error::other("key descriptor read failed"))?,
            )
        })
        .map_err(|_| CryptoError::Key)
}
const MAX_KEY_DESCRIPTOR: usize = 4096;

fn security_descriptor(
    key: NCRYPT_KEY_HANDLE,
    flags: OBJECT_SECURITY_INFORMATION,
) -> Result<Vec<u8>, CryptoError> {
    property_bounded::<MAX_KEY_DESCRIPTOR>(key, NCRYPT_SECURITY_DESCR_PROPERTY, flags)
}

#[cfg(test)]
#[test]
#[allow(non_snake_case)]
fn NativeSetup_DescriptorNeedsDedicatedBound_004() {
    use crate::version_history::windows::security::CurrentUser;
    let user = CurrentUser::capture().expect("capture fixture user");
    let descriptor = user.native_key_descriptor().expect("build fixture ACL");
    assert!(descriptor.len() > 64);
    assert!(descriptor.len() <= MAX_KEY_DESCRIPTOR);
    user.verify_native_key_descriptor(&descriptor)
        .expect("check current-user-only fixture ACL");
    let mut incomplete = descriptor;
    incomplete.truncate(64);
    assert!(user.verify_native_key_descriptor(&incomplete).is_err());
}

fn exact_policy(usage: &[u8], export: &[u8]) -> bool {
    usage == NCRYPT_ALLOW_SIGNING_FLAG.to_le_bytes() && export == 0u32.to_le_bytes()
}
#[cfg(test)]
#[test]
#[allow(non_snake_case)]
fn NativeSetup_RejectsExportableOrMultipurposeKeyPolicy_001() {
    assert!(exact_policy(
        &NCRYPT_ALLOW_SIGNING_FLAG.to_le_bytes(),
        &0u32.to_le_bytes()
    ));
    assert!(!exact_policy(
        &NCRYPT_ALLOW_SIGNING_FLAG.to_le_bytes(),
        &1u32.to_le_bytes()
    ));
    assert!(!exact_policy(
        &(NCRYPT_ALLOW_SIGNING_FLAG | NCRYPT_ALLOW_DECRYPT_FLAG).to_le_bytes(),
        &0u32.to_le_bytes()
    ));
}
