//! Backend-only cache identities for native projections.
//!
//! This is NOT an authorized SourceScope or a filesystem capability. Callers must
//! still authorize reads and hold suitable root handles before exposing new IPC.
//! Legacy callers below remain Claude-only; no default-root fallback occurs here.
use super::profiles::error;
use super::source_scope::{is_verified_key, resolve_path_key, ResolvedPathKey};
use super::types::{CliKind, SafeError, WireU64};
use std::path::{Path, PathBuf};

pub(crate) struct SourcePartition {
    cli: CliKind,
    selected_root: PathBuf,
    canonical_root: PathBuf,
    source_root_key: String,
    identity_epoch: WireU64,
}

fn verified_directory(path: &Path) -> Option<ResolvedPathKey> {
    let resolved = resolve_path_key(path).ok()?;
    if !is_verified_key(&resolved.key) || resolved.canonical_path.is_none() {
        return None;
    }
    Some(resolved)
}

impl SourcePartition {
    pub(crate) fn new(
        cli: CliKind,
        root: &Path,
        identity_epoch: WireU64,
    ) -> Result<Self, SafeError> {
        if cli == CliKind::Shell {
            return Err(error("SOURCE_UNSUPPORTED"));
        }
        let resolved =
            verified_directory(root).ok_or_else(|| error("SOURCE_IDENTITY_UNAVAILABLE"))?;
        Ok(Self {
            cli,
            selected_root: resolved.selected_path,
            canonical_root: resolved.canonical_path.expect("verified directory"),
            source_root_key: resolved.key,
            identity_epoch,
        })
    }

    /// A key is usable only while both root and project identities are known.
    /// Revalidation detects replacement; it is not a race-free read sandbox.
    pub(crate) fn project_index_key(&self, project_dir: &Path) -> Option<String> {
        let root = verified_directory(&self.selected_root)?;
        if root.key != self.source_root_key
            || root.canonical_path.as_ref() != Some(&self.canonical_root)
        {
            return None;
        }
        let project = verified_directory(project_dir)?;
        let canonical_project = project.canonical_path.as_ref()?;
        if !canonical_project.starts_with(&self.canonical_root) {
            return None;
        }
        serde_json::to_string(&[
            "source-v2",
            "local",
            self.cli.as_str(),
            &self.source_root_key,
            &self.identity_epoch.to_string(),
            &project.key,
        ])
        .ok()
    }
}

/// Existing Claude readers know this on-disk layout, but do not own a run scope.
/// Unknown directory identity disables index reuse, never the underlying read.
pub(crate) fn legacy_project_index_key(project_dir: &Path) -> Option<String> {
    let root = project_dir.parent()?.parent()?;
    SourcePartition::new(CliKind::Claude, root, WireU64::parse("0").ok()?)
        .ok()?
        .project_index_key(project_dir)
}

/// Mapping values retain selected path spellings, so separate aliases as well as
/// physical source identities. Missing legacy roots may cache an empty mapping;
/// their selected-path key is never treated as a verified SourceScope.
pub(crate) fn legacy_mapping_key(projects_root: &Path) -> Option<String> {
    let resolved = resolve_path_key(projects_root).ok()?;
    serde_json::to_string(&[
        "mapping-v2",
        "local",
        "claude",
        &resolved.key,
        projects_root.to_str()?,
    ])
    .ok()
}
