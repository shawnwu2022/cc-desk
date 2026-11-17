//! Current-user DPAPI over the entire bounded envelope. Not application-exclusive
//! and not proof of the original GUI approval or protection against old-blob rollback.
use super::{Result, Sealer, StoreError};
#[cfg(not(test))]
use super::{MAX_PLAINTEXT, MAX_SEALED};
pub(crate) struct CurrentUserDpapi;
impl Sealer for CurrentUserDpapi {
    fn seal(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        protect(bytes, false)
    }
    fn unseal(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        protect(bytes, true)
    }
}
#[cfg(test)]
fn protect(_: &[u8], _: bool) -> Result<Vec<u8>> {
    Err(StoreError::Unauthorized)
}
#[cfg(not(test))]
fn protect(bytes: &[u8], decrypt: bool) -> Result<Vec<u8>> {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    use windows_core::PCWSTR;
    if bytes.is_empty() || bytes.len() > if decrypt { MAX_SEALED } else { MAX_PLAINTEXT } {
        return Err(StoreError::Invalid);
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let entropy = super::DOMAIN.as_bytes();
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // No LOCAL_MACHINE flag, UI prompt, caller entropy, alternate user or fallback.
    let result = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    struct Allocation(*mut u8);
    impl Drop for Allocation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.0.cast())));
                }
            }
        }
    }
    let _allocation = Allocation(output.pbData);
    result.map_err(|_| StoreError::Invalid)?;
    let max = if decrypt { MAX_PLAINTEXT } else { MAX_SEALED };
    if output.pbData.is_null() || output.cbData == 0 || output.cbData as usize > max {
        return Err(StoreError::Invalid);
    }
    Ok(unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec())
}
