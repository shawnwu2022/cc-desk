//! Mode is admitted from publisher-bound selection and the protected store's
//! constructor origin. Ordinary admission never supplies installed-output proof.
use super::{
    context::{HeldBundle, HeldTree},
    package::RetainedPackage,
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        catalog::SelectionMetadata,
        ordinary_install::OrdinaryInstallAdmission,
        payload_policy::{PayloadAdmission, PreservedCompanions, VerifiedInstalledPayload},
    },
};

pub(crate) enum InstallAdmission {
    Reviewed(PayloadAdmission),
    Ordinary(OrdinaryInstallAdmission),
}
impl InstallAdmission {
    pub(crate) fn is_ordinary(&self) -> bool {
        matches!(self, Self::Ordinary(_))
    }
    pub(crate) fn verify_selection(&self, selection: &SelectionMetadata) -> Result<(), SafeError> {
        match self {
            Self::Reviewed(a) => a.verify_selection(selection),
            Self::Ordinary(a) => a.verify_selection(selection),
        }
    }
    pub(crate) fn inventory_digest(&self) -> &str {
        match self {
            Self::Reviewed(a) => a.inventory_digest(),
            Self::Ordinary(a) => a.policy_digest(),
        }
    }
    pub(crate) fn installed_bytes(&self) -> u64 {
        match self {
            Self::Reviewed(a) => a.installed_bytes(),
            Self::Ordinary(_) => 0,
        }
    }
    pub(crate) fn admit_retained(
        package: &RetainedPackage,
        expected: &str,
        ordinary_store: bool,
    ) -> Result<Self, SafeError> {
        package.verify_retained()?;
        let admission = if ordinary_store {
            Self::Ordinary(OrdinaryInstallAdmission::from_selection(
                package.selection(),
            )?)
        } else {
            Self::Reviewed(PayloadAdmission::admit_retained(package)?)
        };
        if admission.inventory_digest() != expected {
            return Err(error("HISTORY_TARGET_CHANGED"));
        }
        package.verify_retained()?;
        Ok(admission)
    }
    pub(crate) fn retain_source(
        &self,
        bundle: &HeldBundle,
    ) -> Result<Option<PreservedCompanions>, SafeError> {
        match self {
            Self::Reviewed(a) => a.retain_source(bundle).map(Some),
            Self::Ordinary(_) => {
                bundle
                    .tree()
                    .verify()
                    .map_err(|_| error("HISTORY_SOURCE_CHANGED"))?;
                Ok(None)
            }
        }
    }
    pub(crate) fn verify_installed<'a>(
        &'a self,
        tree: &'a HeldTree,
        companions: &'a Option<PreservedCompanions>,
    ) -> Result<VerifiedInstalledPayload<'a>, SafeError> {
        match (self, companions) {
            (Self::Reviewed(a), Some(c)) => a.verify_installed(tree, c),
            _ => Err(error("HISTORY_PAYLOAD_UNVERIFIED")),
        }
    }
}
