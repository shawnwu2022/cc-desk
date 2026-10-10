//! Explicit ordinary signed installer admission, separate from reviewed output
//! inventory and transactional Return. No caller path or version is authority.
use super::catalog::SelectionMetadata;
use super::verified_package::{sha256, VerifiedPackage};
use crate::cli::{profiles::error, types::SafeError};

#[derive(Clone)]
pub(crate) struct OrdinaryInstallAdmission {
    selection: SelectionMetadata,
    digest: String,
}
impl std::fmt::Debug for OrdinaryInstallAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OrdinaryInstallAdmission(<held; installer output unverified>)")
    }
}

pub(crate) fn enabled() -> bool {
    cfg!(all(windows, target_arch = "x86_64")) && !cfg!(feature = "history-roundtrip-acceptance")
}
impl OrdinaryInstallAdmission {
    pub(crate) fn admit(package: &VerifiedPackage) -> Result<Self, SafeError> {
        package.revalidate(&|| Ok(()))?;
        let admission = Self::from_selection(package.selection())?;
        package.revalidate(&|| Ok(()))?;
        Ok(admission)
    }
    // Identity comparison only. Retained-manager callers must independently
    // verify the same held publisher-signed package before using this policy.
    pub(crate) fn from_selection(selection: &SelectionMetadata) -> Result<Self, SafeError> {
        let installer = selection.installer();
        let signature = selection.signature();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "policy": "cc-desk-ordinary-signed-install-fresh-settings-manual-restore-v1",
            "product": selection.product_identifier(),
            "release": selection.release_id(),
            "tag": selection.tag(),
            "version": selection.version(),
            "installer": [installer.id().to_string(), installer.name().to_owned(), installer.size().to_string(), installer.sha256().to_owned(), installer.download_url().to_owned()],
            "signature": [signature.id().to_string(), signature.name().to_owned(), signature.size().to_string(), signature.sha256().to_owned(), signature.download_url().to_owned()],
        })).map_err(|_| error("HISTORY_TARGET_CHANGED"))?;
        Ok(Self {
            selection: selection.clone(),
            digest: sha256(&bytes),
        })
    }
    pub(crate) fn policy_digest(&self) -> &str {
        &self.digest
    }
    pub(crate) fn verify_selection(&self, selection: &SelectionMetadata) -> Result<(), SafeError> {
        if &self.selection != selection {
            return Err(error("HISTORY_TARGET_CHANGED"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/version_history_ordinary_install.rs"]
mod tests;
