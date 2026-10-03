//! Exact installer-digest policy. Entries may be added only from inspected
//! disposable Windows fixture evidence; publisher signature alone is insufficient.
use super::{
    catalog::SelectionMetadata,
    journal::validate_digest,
    snapshot::{EntryType, ManifestEntry, PermissionRecord, SnapshotLimits},
    verified_package::{sha256, VerifiedPackage},
};
use crate::cli::{profiles::error, types::SafeError};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
struct MeasuredFile {
    path: &'static str,
    size: u64,
    sha256: &'static str,
    attributes: u32,
}

struct MeasuredPayload {
    version: &'static str,
    installer_digest: &'static str,
    installer_size: u64,
    installed_inventory: &'static [MeasuredFile],
    installed_bytes: u64,
}
// Accepted clean + seeded disposable installer evidence: source
// a5e409966888bb0c6c766a6dbed49dd1fe51fb6b, workflow run 37053252570.
// Exact production PrepareService/Minisign-verified installer bytes yielded
// these five identical exports in both cases (exit 0, owned job empty). The
// seeded source-only companion is NOT a target-produced output. This does not
// attest source-to-binary correspondence, app launch or a complete roundtrip.
const REVIEWED: &[MeasuredPayload] = &[MeasuredPayload {
    version: "0.17.7",
    installer_digest: "e9ffbc5ba627f0c133a4385db404342a7344729339185e6f9b8ee6b5969086ac",
    installer_size: 4_966_193,
    installed_inventory: &[
        MeasuredFile {
            path: "LICENSE-Microsoft-ConPTY.txt",
            size: 1_116,
            sha256: "5d177f23ecfeb0ea8e050b6a5a16355e1ae9a0b286436ca8f83ed08b3795be6b",
            attributes: 32,
        },
        MeasuredFile {
            path: "OpenConsole.exe",
            size: 1_066_296,
            sha256: "b7fd936c2668b87b9ecf7b3366dc6568afc1c6f981874cba3e955a1c35cf8160",
            attributes: 32,
        },
        MeasuredFile {
            path: "cc-desk.exe",
            size: 17_342_976,
            sha256: "07d55b6b0841bd84357e71fd306a6a4265546181ccead573f3e756617a943719",
            attributes: 32,
        },
        MeasuredFile {
            path: "conpty.dll",
            size: 109_920,
            sha256: "39fba2713e2495117b1591ae8c32a3b904bea7aa66069cf7815e2844c76d75d8",
            attributes: 32,
        },
        MeasuredFile {
            path: "uninstall.exe",
            size: 79_267,
            sha256: "1b80e884a3a5106a7ccb5309940edc955c5def6448b9576777427717988a0381",
            attributes: 32,
        },
    ],
    installed_bytes: 18_599_575,
}];

// Enabled only with the complete reviewed source/installer/return composition
// and its disposable native roundtrip acceptance. A measured installer alone
// cannot enable an unfinished coordinator.
const SUPPORTED_ROUNDTRIP_ENABLED: bool = false;

pub(crate) fn review_block(
    selection: &SelectionMetadata,
) -> Option<super::manager::SwitchReviewBlock> {
    use super::manager::SwitchReviewBlock;
    if PayloadAdmission::for_selection(selection).is_err() {
        Some(SwitchReviewBlock::PayloadUnverified)
    } else if !SUPPORTED_ROUNDTRIP_ENABLED {
        Some(SwitchReviewBlock::CoordinatorUnavailable)
    } else {
        None
    }
}

pub(crate) struct PayloadAdmission {
    measured: &'static MeasuredPayload,
    digest: String,
}
impl PayloadAdmission {
    pub(crate) fn admit_begin(package: &VerifiedPackage) -> Result<Self, SafeError> {
        let admission = Self::admit(package)?;
        if !SUPPORTED_ROUNDTRIP_ENABLED {
            return Err(error("HISTORY_COORDINATOR_UNAVAILABLE"));
        }
        Ok(admission)
    }
    pub(crate) fn admit(package: &VerifiedPackage) -> Result<Self, SafeError> {
        Self::for_selection(package.selection())
    }
    fn for_selection(selection: &SelectionMetadata) -> Result<Self, SafeError> {
        let measured = REVIEWED
            .iter()
            .find(|entry| {
                entry.version == selection.version()
                    && entry.installer_digest == selection.installer().sha256()
                    && entry.installer_size == selection.installer().size()
            })
            .ok_or_else(|| error("HISTORY_PAYLOAD_UNVERIFIED"))?;
        if measured.installed_inventory.is_empty() || measured.installed_bytes == 0 {
            return Err(error("HISTORY_PAYLOAD_UNVERIFIED"));
        }
        Ok(Self {
            measured,
            digest: sha256(
                &serde_json::to_vec(&(
                    "cc-desk-measured-payload-v1",
                    measured.version,
                    measured.installer_digest,
                    measured.installer_size,
                    measured.installed_inventory,
                ))
                .map_err(|_| error("HISTORY_PAYLOAD_UNVERIFIED"))?,
            ),
        })
    }
    pub(crate) fn verify_selection(&self, selection: &SelectionMetadata) -> Result<(), SafeError> {
        if selection.version() != self.measured.version
            || selection.installer().sha256() != self.measured.installer_digest
            || selection.installer().size() != self.measured.installer_size
        {
            return Err(error("HISTORY_TARGET_CHANGED"));
        }
        Ok(())
    }
    pub(crate) fn inventory_digest(&self) -> &str {
        &self.digest
    }
    pub(crate) fn installed_bytes(&self) -> u64 {
        self.measured.installed_bytes
    }
}

fn inventory_error() -> SafeError {
    error("HISTORY_PAYLOAD_UNVERIFIED")
}

/// This is a comparison predicate, not an admission or filesystem capability.
/// Its production callers below supply only freshly verified held observations.
fn validate_entries(
    entries: &[ManifestEntry],
) -> Result<BTreeMap<&str, &ManifestEntry>, SafeError> {
    let limits = SnapshotLimits::default();
    if entries.is_empty() || entries.len() > limits.max_entries {
        return Err(inventory_error());
    }
    let mut paths = BTreeMap::new();
    let mut folded = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut bytes = 0u64;
    let mut manifest_bytes = 2usize;
    for entry in entries {
        let meta = &entry.metadata;
        let path = meta.path.as_str();
        if !normal_path(path, limits.max_depth)
            || !folded.insert(path.to_lowercase())
            || !identities.insert(&meta.object_identity)
            || meta.link_count != 1
            || validate_digest(&meta.object_identity).is_err()
            || meta.size > limits.max_file_bytes
        {
            return Err(inventory_error());
        }
        let PermissionRecord::Windows {
            descriptor,
            attributes,
        } = &meta.permissions
        else {
            return Err(inventory_error());
        };
        // The held Windows adapter validates the actual descriptor and streams.
        // This predicate cannot grant equivalent authority to descriptor bytes.
        if descriptor.is_empty() || descriptor.len() > 65_536 || attributes & !0x20b7 != 0 {
            return Err(inventory_error());
        }
        match meta.kind {
            EntryType::File
                if !path.is_empty()
                    && attributes & 16 == 0
                    && entry
                        .sha256
                        .as_deref()
                        .is_some_and(|digest| validate_digest(digest).is_ok()) => {}
            EntryType::Directory
                if meta.size == 0 && entry.sha256.is_none() && attributes & 16 != 0 => {}
            _ => return Err(inventory_error()),
        }
        bytes = bytes.checked_add(meta.size).ok_or_else(inventory_error)?;
        manifest_bytes = manifest_bytes
            .checked_add(
                serde_json::to_vec(entry)
                    .map_err(|_| inventory_error())?
                    .len()
                    + 1,
            )
            .ok_or_else(inventory_error)?;
        if bytes > limits.max_bytes || manifest_bytes > 32 * 1024 * 1024 {
            return Err(inventory_error());
        }
        paths.insert(path, entry);
    }
    if paths
        .get("")
        .is_none_or(|root| root.metadata.kind != EntryType::Directory)
    {
        return Err(inventory_error());
    }
    for path in paths.keys().filter(|path| !path.is_empty()) {
        let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        if paths
            .get(parent)
            .is_none_or(|entry| entry.metadata.kind != EntryType::Directory)
        {
            return Err(inventory_error());
        }
    }
    Ok(paths)
}

fn normal_path(path: &str, max_depth: usize) -> bool {
    if path.is_empty() {
        return true;
    }
    path.len() <= 32_768
        && path.split('/').count() <= max_depth
        && path.split('/').all(|part| {
            let base = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            let device = matches!(
                base.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ) || ["COM", "LPT"].iter().any(|prefix| {
                base.strip_prefix(*prefix).is_some_and(|n| {
                    matches!(
                        n,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            });
            !part.is_empty()
                && part.encode_utf16().count() <= 255
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !device
                && !part
                    .chars()
                    .any(|ch| ch.is_control() || "\\:*?\"<>|".contains(ch))
        })
}

fn validate_source_inventory(
    measured: &MeasuredPayload,
    original_image: &str,
    source: &[ManifestEntry],
) -> Result<(), SafeError> {
    let source = validate_entries(source)?;
    if original_image.is_empty()
        || original_image.contains('/')
        || source
            .get(original_image)
            .is_none_or(|entry| entry.metadata.kind != EntryType::File)
    {
        return Err(inventory_error());
    }
    for output in measured.installed_inventory {
        for entry in source
            .values()
            .filter(|entry| entry.metadata.path.eq_ignore_ascii_case(output.path))
        {
            if entry.metadata.path != output.path || entry.metadata.kind != EntryType::File {
                return Err(inventory_error());
            }
        }
    }
    Ok(())
}

fn validate_installed_inventory(
    measured: &MeasuredPayload,
    original_image: &str,
    source: &[ManifestEntry],
    installed: &[ManifestEntry],
) -> Result<(), SafeError> {
    validate_source_inventory(measured, original_image, source)?;
    let actual = validate_entries(installed)?;
    let mut allowed = BTreeSet::new();
    for output in measured.installed_inventory {
        let entry = actual.get(output.path).ok_or_else(inventory_error)?;
        if entry.metadata.kind != EntryType::File
            || entry.metadata.size != output.size
            || entry.sha256.as_deref() != Some(output.sha256)
            || !matches!(&entry.metadata.permissions, PermissionRecord::Windows { attributes, .. } if *attributes == output.attributes)
        {
            return Err(inventory_error());
        }
        allowed.insert(output.path);
    }
    for original in source {
        let path = original.metadata.path.as_str();
        if path == original_image || allowed.contains(path) {
            continue;
        }
        if actual.get(path).copied() != Some(original) {
            return Err(inventory_error());
        }
        allowed.insert(path);
    }
    if actual.len() != allowed.len() {
        return Err(inventory_error());
    }
    Ok(())
}

/// Process-local pre-effect observation only. No Deserialize, public manifest
/// constructor, path opener, terminal/job assertion or executable authority.
#[cfg(windows)]
pub(crate) struct PreservedCompanions {
    policy_digest: String,
    source_location: String,
    original_image: String,
    source: Vec<ManifestEntry>,
}

/// A borrowed complete held-tree observation. The coordinator must separately
/// prove source quiescence, sealed backups, installer exit and an empty owned
/// job before using this observation; it cannot authorize launch or restore.
#[cfg(windows)]
pub(crate) struct VerifiedInstalledPayload<'a> {
    tree: &'a super::windows::context::HeldTree,
    source: &'a PreservedCompanions,
    admission: &'a PayloadAdmission,
}

#[cfg(windows)]
impl PayloadAdmission {
    /// Measured policy evidence for the copied manager's retained package.
    /// This does not pass the ordinary begin/roundtrip coordinator gate.
    pub(crate) fn admit_retained(
        package: &super::windows::package::RetainedPackage,
    ) -> Result<Self, SafeError> {
        package.verify_retained()?;
        let admission = Self::for_selection(package.selection())?;
        package.verify_retained()?;
        Ok(admission)
    }

    pub(crate) fn retain_source(
        &self,
        source: &super::windows::context::HeldBundle,
    ) -> Result<PreservedCompanions, SafeError> {
        source.tree().verify().map_err(|_| inventory_error())?;
        let manifest = source.manifest();
        validate_source_inventory(
            self.measured,
            manifest.original_image_name(),
            &manifest.tree.entries,
        )?;
        let retained = PreservedCompanions {
            policy_digest: self.digest.clone(),
            source_location: manifest.tree.location_identity.clone(),
            original_image: manifest.original_image_name().into(),
            source: manifest.tree.entries.clone(),
        };
        source.tree().verify().map_err(|_| inventory_error())?;
        Ok(retained)
    }

    pub(crate) fn verify_installed<'a>(
        &'a self,
        installed: &'a super::windows::context::HeldTree,
        source: &'a PreservedCompanions,
    ) -> Result<VerifiedInstalledPayload<'a>, SafeError> {
        let receipt = VerifiedInstalledPayload {
            tree: installed,
            source,
            admission: self,
        };
        receipt.verify()?;
        Ok(receipt)
    }
}

#[cfg(windows)]
impl PreservedCompanions {
    /// Recheck against the complete sealed/post-exit held source before any
    /// effects. This equality check itself is not a source-quiescence proof.
    pub(crate) fn verify_source(
        &self,
        source: &super::windows::context::HeldBundle,
    ) -> Result<(), SafeError> {
        source.tree().verify().map_err(|_| inventory_error())?;
        let manifest = source.manifest();
        if manifest.original_image_name() != self.original_image
            || manifest.tree.location_identity != self.source_location
            || manifest.tree.entries != self.source
        {
            return Err(inventory_error());
        }
        source.tree().verify().map_err(|_| inventory_error())
    }
}

#[cfg(windows)]
impl VerifiedInstalledPayload<'_> {
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        self.tree.verify().map_err(|_| inventory_error())?;
        if self.source.policy_digest != self.admission.digest
            || self.tree.manifest().location_identity != self.source.source_location
        {
            return Err(inventory_error());
        }
        validate_installed_inventory(
            self.admission.measured,
            &self.source.original_image,
            &self.source.source,
            &self.tree.manifest().entries,
        )?;
        self.tree.verify().map_err(|_| inventory_error())
    }
}

#[cfg(test)]
#[path = "../tests/version_history_payload_policy.rs"]
#[allow(non_snake_case)]
mod tests;
