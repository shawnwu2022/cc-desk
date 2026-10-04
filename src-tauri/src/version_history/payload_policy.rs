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
// Reviewed clean + seeded disposable evidence: source
// b708427aaae52c81d6d8c821ca08c097e4258108, workflow 37199938820, attempts 1/2.
// All nine exact PrepareService/Minisign-verified packages have paired matching
// outputs (exit 0, owned job empty). Public measurements and immutable artifact
// identities are in tests/fixtures/version-history-payload/reviewed-measurements.json.
// The first entry retains the exact prior 0.17.7 policy: source
// a5e409966888bb0c6c766a6dbed49dd1fe51fb6b, workflow 37053252570. Source-only
// companions are NOT target-produced outputs.
// These constants do not attest source-to-binary correspondence, app launch or
// a complete roundtrip. No runtime fixture import or broader package fallback.
const REVIEWED: &[MeasuredPayload] = &[
    MeasuredPayload {
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
    },
    MeasuredPayload {
        version: "0.14.0",
        installer_digest: "826b44d86bd4ff610119f762b30cc30e9e532c502517b2910dd5a489064a0249",
        installer_size: 6_007_615,
        installed_inventory: &[
            MeasuredFile {
                path: "cc-desk.exe",
                size: 22_103_552,
                sha256: "218a562fa82e4fc8e0f67db42a30e8e190a2f6fc4026a7f5080220b6ad364e23",
                attributes: 32,
            },
            MeasuredFile {
                path: "uninstall.exe",
                size: 79_152,
                sha256: "abbb77aff7c1b2b7b9d86224225b119049050086523cbc427dfc884a512adc62",
                attributes: 32,
            },
        ],
        installed_bytes: 22_182_704,
    },
    MeasuredPayload {
        version: "0.15.0",
        installer_digest: "14f86e0858467ef397b61ba165993dd3b3c2d26744aaa9c8737f7dd63a80155a",
        installer_size: 6_004_307,
        installed_inventory: &[
            MeasuredFile {
                path: "cc-desk.exe",
                size: 22_103_552,
                sha256: "ca5a98ba3409925dd2fbe752264ecaf32cadf8249272eb37ddd2a2557985b9d9",
                attributes: 32,
            },
            MeasuredFile {
                path: "uninstall.exe",
                size: 79_152,
                sha256: "b34235dac0e1aa0fa2f7a6f856cfed5f05a626dd22a2a8386298d3346f8a9692",
                attributes: 32,
            },
        ],
        installed_bytes: 22_182_704,
    },
    MeasuredPayload {
        version: "0.16.0",
        installer_digest: "547d9251286f0a3e78199590617913ac26d0ef2614325ad54a885ac75fd4fd3a",
        installer_size: 6_102_539,
        installed_inventory: &[
            MeasuredFile {
                path: "cc-desk.exe",
                size: 22_434_816,
                sha256: "eae2d87f1a9f8add5b8bc74976765b0451bbe097f99f7b2ad2c46b7f94547afd",
                attributes: 32,
            },
            MeasuredFile {
                path: "uninstall.exe",
                size: 79_152,
                sha256: "c6d09b95a12320a5bfcf83b6a98e672d55f957ca07323f929cd84b87d62061cf",
                attributes: 32,
            },
        ],
        installed_bytes: 22_513_968,
    },
    MeasuredPayload {
        version: "0.17.0",
        installer_digest: "81a649eae03c2b248bae512f5db0f0e6d28ffac66691d0db9dbc5f9e95b9c61b",
        installer_size: 6_072_998,
        installed_inventory: &[
            MeasuredFile {
                path: "cc-desk.exe",
                size: 22_344_704,
                sha256: "5bc6a2842ed2c52ce221a01256602da8b4901fc721d23dad78a1a15412ad3fce",
                attributes: 32,
            },
            MeasuredFile {
                path: "uninstall.exe",
                size: 79_152,
                sha256: "f17a915e43a458255466e20035f0a4fcb4dd26a8a20e910e83bb65cac51ff7e7",
                attributes: 32,
            },
        ],
        installed_bytes: 22_423_856,
    },
    MeasuredPayload {
        version: "0.17.1",
        installer_digest: "2d9eca64b6f0cdd43ce63bf674b4544ec9e0d01b79d70eacb42ffc37be04377c",
        installer_size: 6_068_768,
        installed_inventory: &[
            MeasuredFile {
                path: "cc-desk.exe",
                size: 22_344_192,
                sha256: "ce1d00dd1df825a07376396a0544bcb7820d7a921ebd3f391abba246e0856fb5",
                attributes: 32,
            },
            MeasuredFile {
                path: "uninstall.exe",
                size: 79_152,
                sha256: "9cfcd224c391bee115d4579feed05e0fa7fdb4bf0b1d4e1968e4edc50dd95abf",
                attributes: 32,
            },
        ],
        installed_bytes: 22_423_344,
    },
    MeasuredPayload {
        version: "0.17.2",
        installer_digest: "d54c7854f716fd05c2617300e08dd46303e06588818c0122412401e57ca7b094",
        installer_size: 6_078_917,
        installed_inventory: &[
            MeasuredFile {
                path: "cc-desk.exe",
                size: 22_345_216,
                sha256: "a8316650a3d34c7ee2b1055495320cade04c951fce8bcbcb9a063033cc4e87a7",
                attributes: 32,
            },
            MeasuredFile {
                path: "uninstall.exe",
                size: 79_152,
                sha256: "cb097cd05caa037e7e5dfa3bf09a5fbcaf87895a7a97024359188d1309a98821",
                attributes: 32,
            },
        ],
        installed_bytes: 22_424_368,
    },
    MeasuredPayload {
        version: "0.17.5",
        installer_digest: "e23df98c3bd789cc199fa80aae805acad2c668bcd7c0417815c347553e1b8041",
        installer_size: 4_485_401,
        installed_inventory: &[
            MeasuredFile {
                path: "cc-desk.exe",
                size: 17_276_416,
                sha256: "461b97e8b38ddee0d08bf5e565ec54b83d9acf3cc0ccfa4c7c3471403e451005",
                attributes: 32,
            },
            MeasuredFile {
                path: "uninstall.exe",
                size: 79_152,
                sha256: "2d608670e23e0c0fd0dca701a181a1f05d7354677d0898cbef2753696584a57d",
                attributes: 32,
            },
        ],
        installed_bytes: 17_355_568,
    },
    MeasuredPayload {
        version: "0.17.6",
        installer_digest: "a0df0178b4d4c4ab08f1229bbdf9b6d4c2c2e402139cbeaf9acd6bb9eb053afd",
        installer_size: 4_486_992,
        installed_inventory: &[
            MeasuredFile {
                path: "cc-desk.exe",
                size: 17_280_512,
                sha256: "093ffc84ad8dde074fc1efbcd4d731c7b40d5dc3c701dcc4dd11bb6cc80a0020",
                attributes: 32,
            },
            MeasuredFile {
                path: "uninstall.exe",
                size: 79_152,
                sha256: "e373f7d5adc7c32c0af960582776acc46f0bb9673717814683dd0f0af007551e",
                attributes: 32,
            },
        ],
        installed_bytes: 17_359_664,
    },
];

// Enabled only with the complete reviewed source/installer/return composition
// and its disposable native roundtrip acceptance. A measured installer alone
// cannot enable an unfinished coordinator.
const SUPPORTED_ROUNDTRIP_ENABLED: bool = false;

// Called only after the measured selection matched. Unit tests and every
// ordinary artifact keep the production denial, even when Cargo enables tests
// and this opt-in feature together. Review grants no later begin authority.
fn roundtrip_enabled(_selection: &SelectionMetadata) -> bool {
    if SUPPORTED_ROUNDTRIP_ENABLED {
        return true;
    }
    #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
    {
        super::acceptance::require_source_target(_selection).is_ok()
    }
    #[cfg(not(all(feature = "history-roundtrip-acceptance", not(test))))]
    false
}

pub(crate) fn review_block(
    selection: &SelectionMetadata,
) -> Option<super::manager::SwitchReviewBlock> {
    use super::manager::SwitchReviewBlock;
    if PayloadAdmission::for_selection(selection).is_err() {
        Some(SwitchReviewBlock::PayloadUnverified)
    } else if !roundtrip_enabled(selection) {
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
        if !roundtrip_enabled(package.selection()) {
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
