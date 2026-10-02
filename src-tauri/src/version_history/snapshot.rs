//! Complete, bounded context manifests; capture is read-only. Platform adapters
//! must enumerate through pinned no-follow handles and reject unsupported ACL,
//! reparse/alias and file-identity semantics. This core cannot bless an adapter.
use super::journal::{validate_digest, validate_id, JournalBinding, RootKind};
use super::maintenance::SnapshotBoundary;
use super::verified_package::sha256;
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{self, Read};

const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum EntryType { File, Directory, LinkOrReparse, Other }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum PermissionRecord {
    Unix { mode: u32 },
    /// Exact self-relative security descriptor + relevant DOS attributes, read
    /// and restored/verified by the Windows adapter, never interpreted as Unix.
    Windows { descriptor: Vec<u8>, attributes: u32 },
}
impl PermissionRecord {
    fn validate(&self) -> Result<(), SafeError> {
        match self {
            Self::Unix { mode } if *mode <= 0o177777 => Ok(()),
            Self::Windows { descriptor, .. } if !descriptor.is_empty() && descriptor.len() <= 65536 => Ok(()),
            _ => Err(error("HISTORY_PERMISSIONS_UNSUPPORTED")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryMetadata {
    /// Root directory uses ""; other paths use exact UTF-8 normal components.
    /// Unrepresentable native names must block the entire capture, not skip.
    pub(crate) path: String,
    pub(crate) kind: EntryType,
    pub(crate) size: u64,
    pub(crate) object_identity: String,
    pub(crate) link_count: u64,
    pub(crate) permissions: PermissionRecord,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootInventory {
    pub(crate) root: RootKind,
    /// Pinned root location/parent identity, including absent-root identity.
    pub(crate) location_identity: String,
    /// Backend discovery checks actual CLI config roots and project roots. This
    /// value is an adapter report, not an IPC or standalone authority field.
    pub(crate) overlaps_shared_data: bool,
    /// Empty means absent. An existing empty directory contains the "" entry.
    pub(crate) entries: Vec<EntryMetadata>,
}
impl RootInventory {
    #[cfg(test)]
    pub(crate) fn fixture(root: RootKind, identity: &str) -> Self {
        Self { root, location_identity: identity.into(), overlaps_shared_data: false, entries: vec![] }
    }
}

/// Backend adapter only. inventory() returns the COMPLETE tree (no filters);
/// open_file() rechecks the exact identity and no-link metadata on the retained
/// opened handle. Hold no-write/no-delete handles through every file read and
/// reject changed ancestors. All errors, including ACL/name/IO errors, propagate.
pub(crate) trait ContextReader {
    fn inventory(&mut self) -> io::Result<Vec<RootInventory>>;
    fn open_file(&mut self, root: RootKind, entry: &EntryMetadata) -> io::Result<Box<dyn Read>>;
}

#[derive(Clone, Copy)]
pub(crate) struct SnapshotLimits {
    pub(crate) max_entries: usize,
    pub(crate) max_bytes: u64,
    pub(crate) max_file_bytes: u64,
    pub(crate) max_depth: usize,
}
impl Default for SnapshotLimits {
    fn default() -> Self {
        Self { max_entries: 100_000, max_bytes: 16 * 1024 * 1024 * 1024, max_file_bytes: 8 * 1024 * 1024 * 1024, max_depth: 128 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestEntry {
    pub(crate) metadata: EntryMetadata,
    pub(crate) sha256: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootManifest {
    pub(crate) root: RootKind,
    pub(crate) location_identity: String,
    pub(crate) entries: Vec<ManifestEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotManifest {
    schema: u32,
    binding: JournalBinding,
    pub(crate) context_id: String,
    pub(crate) roots: Vec<RootManifest>,
}
impl SnapshotManifest {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, SafeError> {
        let bytes = serde_json::to_vec(self).map_err(|_| error("HISTORY_MANIFEST_INVALID"))?;
        if bytes.len() > MAX_MANIFEST_BYTES { return Err(error("HISTORY_SNAPSHOT_LIMIT")); }
        Ok(bytes)
    }
    pub(crate) fn digest(&self) -> Result<String, SafeError> { Ok(sha256(&self.encode()?)) }
    pub(crate) fn decode(bytes: &[u8], expected: &str, binding: &JournalBinding) -> Result<Self, SafeError> {
        validate_digest(expected)?;
        if bytes.len() > MAX_MANIFEST_BYTES || sha256(bytes) != expected { return Err(error("HISTORY_MANIFEST_CHANGED")); }
        let manifest: Self = serde_json::from_slice(bytes).map_err(|_| error("HISTORY_MANIFEST_INVALID"))?;
        binding.validate()?;
        if manifest.schema != 1 || &manifest.binding != binding || !binding.has_context(&manifest.context_id) {
            return Err(error("HISTORY_MANIFEST_CHANGED"));
        }
        // Validate all persisted data as well as fresh captures; a correct hash
        // of a malformed or foreign manifest never makes it a restore authority.
        let inventories: Vec<_> = manifest.roots.iter().map(|root| RootInventory {
            root: root.root, location_identity: root.location_identity.clone(), overlaps_shared_data: false,
            entries: root.entries.iter().map(|entry| entry.metadata.clone()).collect(),
        }).collect();
        validate_inventory(&inventories, SnapshotLimits::default())?;
        for root in &manifest.roots {
            for entry in &root.entries {
                match (&entry.metadata.kind, &entry.sha256) {
                    (EntryType::File, Some(digest)) => validate_digest(digest)?,
                    (EntryType::Directory, None) => (),
                    _ => return Err(error("HISTORY_MANIFEST_INVALID")),
                }
            }
        }
        Ok(manifest)
    }
}

fn io_error(_: io::Error) -> SafeError { error("HISTORY_SNAPSHOT_IO") }

pub(crate) fn capture_context(
    boundary: &SnapshotBoundary,
    context_id: &str,
    source: &mut dyn ContextReader,
    limits: SnapshotLimits,
) -> Result<SnapshotManifest, SafeError> {
    boundary.binding().validate()?;
    validate_id(context_id)?;
    if !boundary.binding().has_context(context_id) { return Err(error("HISTORY_CONTEXT_CHANGED")); }
    let mut before = source.inventory().map_err(io_error)?;
    sort_inventory(&mut before);
    validate_inventory(&before, limits)?;
    for root in &before {
        if boundary.root_identity(root.root) != Some(root.location_identity.as_str()) {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
    }
    let mut roots = Vec::new();
    for root in &before {
        let mut entries = Vec::new();
        for metadata in &root.entries {
            let digest = if metadata.kind == EntryType::File {
                let mut reader = source.open_file(root.root, metadata).map_err(io_error)?;
                let mut remaining = metadata.size;
                let mut hasher = Sha256::new();
                let mut buffer = [0; 64 * 1024];
                while remaining != 0 {
                    let size = remaining.min(buffer.len() as u64) as usize;
                    let count = reader.read(&mut buffer[..size]).map_err(io_error)?;
                    if count == 0 { return Err(error("HISTORY_CONTEXT_CHANGED")); }
                    hasher.update(&buffer[..count]);
                    remaining -= count as u64;
                }
                if reader.read(&mut buffer[..1]).map_err(io_error)? != 0 { return Err(error("HISTORY_CONTEXT_CHANGED")); }
                Some(format!("{:x}", hasher.finalize()))
            } else { None };
            entries.push(ManifestEntry { metadata: metadata.clone(), sha256: digest });
        }
        roots.push(RootManifest { root: root.root, location_identity: root.location_identity.clone(), entries });
    }
    let mut after = source.inventory().map_err(io_error)?;
    sort_inventory(&mut after);
    if before != after { return Err(error("HISTORY_CONTEXT_CHANGED")); }
    let manifest = SnapshotManifest { schema: 1, binding: boundary.binding().clone(), context_id: context_id.into(), roots };
    manifest.encode()?;
    Ok(manifest)
}

/// Exact same-context validation, including original file identities. A copied
/// tree has NEW identities: Task4 must create a separate retained-copy manifest
/// and compare content/permissions, never pretend this validates a moved target.
pub(crate) fn verify_context(
    boundary: &SnapshotBoundary,
    expected: &SnapshotManifest,
    source: &mut dyn ContextReader,
    limits: SnapshotLimits,
) -> Result<(), SafeError> {
    if &expected.binding != boundary.binding() { return Err(error("HISTORY_CONTEXT_CHANGED")); }
    let actual = capture_context(boundary, &expected.context_id, source, limits)?;
    if actual != *expected { return Err(error("HISTORY_CONTEXT_CHANGED")); }
    Ok(())
}

fn sort_inventory(roots: &mut [RootInventory]) {
    roots.sort_by_key(|root| root.root);
    for root in roots { root.entries.sort_by(|left, right| left.path.cmp(&right.path)); }
}
fn validate_inventory(roots: &[RootInventory], limits: SnapshotLimits) -> Result<(), SafeError> {
    if roots.len() != 2 || roots[0].root != RootKind::Desk || roots[1].root != RootKind::WebView
        || roots[0].location_identity == roots[1].location_identity {
        return Err(error("HISTORY_ROOT_CHANGED"));
    }
    let mut total = 0u64;
    let mut count = 0usize;
    for root in roots {
        if root.overlaps_shared_data { return Err(error("HISTORY_SHARED_ROOT_OVERLAP")); }
        if root.location_identity.is_empty() || root.location_identity.len() > 4096 { return Err(error("HISTORY_ROOT_CHANGED")); }
        let mut paths = BTreeSet::new();
        let mut directories = BTreeSet::new();
        let mut identities = BTreeSet::new();
        for entry in &root.entries {
            count = count.checked_add(1).ok_or_else(|| error("HISTORY_SNAPSHOT_LIMIT"))?;
            if count > limits.max_entries { return Err(error("HISTORY_SNAPSHOT_LIMIT")); }
            validate_path(&entry.path, limits.max_depth)?;
            if !paths.insert(entry.path.to_ascii_lowercase()) || entry.object_identity.is_empty()
                || entry.object_identity.len() > 4096 || !identities.insert(&entry.object_identity) {
                return Err(error("HISTORY_UNSAFE_TREE"));
            }
            entry.permissions.validate()?;
            match entry.kind {
                EntryType::File if !entry.path.is_empty() && entry.link_count == 1 => {
                    total = total.checked_add(entry.size).ok_or_else(|| error("HISTORY_SNAPSHOT_LIMIT"))?;
                    if total > limits.max_bytes || entry.size > limits.max_file_bytes { return Err(error("HISTORY_SNAPSHOT_LIMIT")); }
                }
                EntryType::Directory if entry.size == 0 => { directories.insert(entry.path.as_str()); }
                _ => return Err(error("HISTORY_UNSAFE_TREE")),
            }
        }
        if !root.entries.is_empty() && !directories.contains("") { return Err(error("HISTORY_UNSAFE_TREE")); }
        for entry in &root.entries {
            if !entry.path.is_empty() {
                let parent = entry.path.rsplit_once('/').map_or("", |(parent, _)| parent);
                if !directories.contains(parent) { return Err(error("HISTORY_UNSAFE_TREE")); }
            }
        }
    }
    Ok(())
}
fn validate_path(path: &str, max_depth: usize) -> Result<(), SafeError> {
    if path.is_empty() { return Ok(()); }
    if path.len() > 32768 || path.chars().any(char::is_control) || path.contains(['\\', ':']) {
        return Err(error("HISTORY_UNSAFE_TREE"));
    }
    let components: Vec<_> = path.split('/').collect();
    if components.len() > max_depth || components.iter().any(|part| {
        part.is_empty() || *part == "." || *part == ".." || part.ends_with(['.', ' '])
    }) { return Err(error("HISTORY_UNSAFE_TREE")); }
    Ok(())
}
