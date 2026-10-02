//! Complete private manager material. The extra manager executable is a new
//! independent object in the final inventory, never an unchanged-copy claim.
use super::{
    blocked,
    context::{
        ContextJournal, HeldBundle, HeldTree, PrivateCopyManifest, PrivateTreeCopy, TreeManifest,
    },
    files::{ComponentName, FileAccess, FileIdentity, PinnedFile, PrivateDirectory},
    handle,
    scope::RegisteredInstallation,
    security::CurrentUser,
    win_error,
};
use crate::version_history::{
    journal::{validate_digest, validate_id},
    snapshot::{EntryType, SnapshotLimits},
    verified_package::sha256,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::Arc,
};
use windows::{
    Wdk::Storage::FileSystem::FILE_CREATE,
    Win32::Storage::FileSystem::{
        FlushFileBuffers, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_WRITE_DATA,
        READ_CONTROL, SYNCHRONIZE,
    },
};

pub(crate) const MANAGER_BASENAME: &str = "cc-desk-version-manager.exe";
const BUNDLE_DIRECTORY: &str = "bundle";
const BUNDLE_RECORD: &str = "manager-bundle.json";
const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn name(value: &str) -> io::Result<ComponentName> {
    ComponentName::new(OsStr::new(value))
}
pub(super) fn valid_transaction(transaction: &str) -> io::Result<()> {
    validate_id(transaction).map_err(|_| blocked("invalid manager transaction"))
}

/// A protected reference is an observation, not a file capability. Reopening
/// requires the independently admitted data root and all exact object checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagerRecordReference {
    identity: FileIdentity,
    size: u64,
    digest: String,
}
impl ManagerRecordReference {
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
    fn validate(&self) -> io::Result<()> {
        validate_digest(&self.digest).map_err(|_| blocked("invalid manager record reference"))?;
        if self.size == 0 || self.size > MAX_RECORD_BYTES as u64 {
            return Err(blocked("manager record exceeds capacity"));
        }
        Ok(())
    }
}

/// Read-only before cross-process publication. A writable receipt guard would
/// prevent the child's ordinary no-write-sharing read from succeeding.
pub(super) struct ManagerRecord {
    root: Arc<PrivateDirectory>,
    file: PinnedFile,
    reference: ManagerRecordReference,
    bytes: Vec<u8>,
}
impl ManagerRecord {
    pub(super) fn create<T: Serialize>(
        root: Arc<PrivateDirectory>,
        filename: &str,
        value: &T,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let bytes = serde_json::to_vec(value)?;
        if bytes.is_empty() || bytes.len() > MAX_RECORD_BYTES {
            return Err(blocked("manager record exceeds capacity"));
        }
        root.verify(user)?;
        let filename = name(filename)?;
        let security = user.descriptor(false)?;
        let file = root.directory().open_relative(
            &filename,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ,
            FILE_CREATE,
            false,
            Some(&security),
        )?;
        user.verify_private_file(handle(&file), false)?;
        let file = PinnedFile::from_file(root.directory().clone(), filename.clone(), file)?;
        let mut writer = &file.file;
        writer.write_all(&bytes)?;
        unsafe { FlushFileBuffers(handle(&file.file)) }.map_err(win_error)?;
        let reference = ManagerRecordReference {
            identity: file.identity().clone(),
            size: bytes.len() as u64,
            digest: sha256(&bytes),
        };
        if file.digest()? != reference.digest {
            return Err(blocked("manager record readback differs"));
        }
        // No overwrite/cleanup on any failure, including this re-admission gap.
        drop(file);
        Self::open(
            root,
            &filename
                .os_string()
                .into_string()
                .map_err(|_| blocked("invalid record name"))?,
            &reference,
            user,
        )
    }
    pub(super) fn open(
        root: Arc<PrivateDirectory>,
        filename: &str,
        reference: &ManagerRecordReference,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        reference.validate()?;
        root.verify(user)?;
        let file = root
            .directory()
            .open_file(name(filename)?, FileAccess::Read)?;
        let mut reader = &file.file;
        reader.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        reader.take(reference.size + 1).read_to_end(&mut bytes)?;
        let record = Self {
            root,
            file,
            reference: reference.clone(),
            bytes,
        };
        record.verify(user)?;
        Ok(record)
    }
    /// Only for a response to this exact already retained child/admission.
    /// The caller must immediately compare the full typed response bindings.
    pub(super) fn observe(
        root: Arc<PrivateDirectory>,
        filename: &str,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        root.verify(user)?;
        let file = root
            .directory()
            .open_file(name(filename)?, FileAccess::Read)?;
        let size = file.file.metadata()?.len();
        if size == 0 || size > MAX_RECORD_BYTES as u64 {
            return Err(blocked("invalid manager response size"));
        }
        let reference = ManagerRecordReference {
            identity: file.identity().clone(),
            size,
            digest: file.digest()?,
        };
        drop(file);
        Self::open(root, filename, &reference, user)
    }
    pub(super) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.reference.validate()?;
        self.root.verify(user)?;
        self.file.verify()?;
        user.verify_private_file(handle(&self.file.file), false)?;
        if self.file.identity() != &self.reference.identity
            || self.file.file.metadata()?.len() != self.reference.size
            || self.bytes.len() as u64 != self.reference.size
            || sha256(&self.bytes) != self.reference.digest
            || self.file.digest()? != self.reference.digest
        {
            return Err(blocked("manager record changed"));
        }
        Ok(())
    }
    pub(super) fn decode<T: serde::de::DeserializeOwned>(
        &self,
        user: &CurrentUser,
    ) -> io::Result<T> {
        self.verify(user)?;
        serde_json::from_slice(&self.bytes).map_err(io::Error::other)
    }
    pub(super) fn reference(&self) -> &ManagerRecordReference {
        &self.reference
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleBinding {
    schema: u32,
    transaction: String,
    data_root: FileIdentity,
    bundle_root: FileIdentity,
    source_bundle: String,
    source_image: String,
    base: PrivateCopyManifest,
    complete: TreeManifest,
}

pub(crate) struct ManagerBundle {
    data_root: Arc<PrivateDirectory>,
    root: Arc<PrivateDirectory>,
    tree: HeldTree,
    image: PinnedFile,
    record: ManagerRecord,
    binding: BundleBinding,
}
impl ManagerBundle {
    pub(crate) fn prepare(
        installation: &RegisteredInstallation,
        source: &HeldBundle,
        data_root: Arc<PrivateDirectory>,
        transaction: &str,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        valid_transaction(transaction)?;
        installation
            .recheck()
            .map_err(|_| blocked("registered source changed"))?;
        let manager = Self::prepare_copy(
            source.tree(),
            installation.image(),
            source.manifest().logical_digest()?,
            data_root,
            transaction,
            user,
            journal,
        )?;
        installation
            .recheck()
            .map_err(|_| blocked("registered source changed during manager copy"))?;
        source.tree().verify()?;
        Ok(manager)
    }
    fn prepare_copy(
        source: &HeldTree,
        source_file: &PinnedFile,
        source_bundle: String,
        data_root: Arc<PrivateDirectory>,
        transaction: &str,
        user: &CurrentUser,
        journal: &mut ContextJournal<'_>,
    ) -> io::Result<Self> {
        valid_transaction(transaction)?;
        journal.verify_transaction(transaction)?;
        source.verify()?;
        // The final protected record contains the original, base copy and
        // augmented complete tree. Refuse capacity before creating any copy.
        let source_bytes = source.manifest().encode()?;
        if source_bytes
            .len()
            .checked_mul(3)
            .and_then(|size| size.checked_add(source.manifest().entries.len().saturating_mul(2048)))
            .and_then(|size| size.checked_add(8192))
            .is_none_or(|size| size > MAX_RECORD_BYTES)
        {
            return Err(blocked("complete manager mapping exceeds capacity"));
        }
        let source_image = source_file
            .name
            .os_string()
            .into_string()
            .map_err(|_| blocked("unrepresentable source image"))?;
        if source
            .manifest()
            .entries
            .iter()
            .any(|entry| entry.metadata.path.eq_ignore_ascii_case(MANAGER_BASENAME))
        {
            return Err(blocked("source conflicts with distinct manager image"));
        }
        let original = source
            .manifest()
            .entries
            .iter()
            .find(|entry| entry.metadata.path == source_image)
            .ok_or_else(|| blocked("source image absent from complete bundle"))?;
        if original.metadata.kind != EntryType::File
            || original.sha256.as_deref() != Some(source_file.digest()?.as_str())
            || original.metadata.object_identity
                != sha256(&serde_json::to_vec(source_file.identity())?)
        {
            return Err(blocked("source image differs from bundle"));
        }
        let mut copy = PrivateTreeCopy::new(data_root.clone(), name(BUNDLE_DIRECTORY)?);
        copy.copy_from(source, user, journal)?;
        let base = copy.manifest()?.clone();
        // Context owns the separately planned extra-file effect and read-only
        // sealing. Its old unchanged-copy manifest is consumed at this point.
        let (root, tree) =
            copy.into_manager_bundle(source_file, name(MANAGER_BASENAME)?, user, journal)?;
        let image = root
            .directory()
            .open_file(name(MANAGER_BASENAME)?, FileAccess::Read)?;
        let binding = BundleBinding {
            schema: 1,
            transaction: transaction.into(),
            data_root: data_root.directory().identity().clone(),
            bundle_root: root.directory().identity().clone(),
            source_bundle,
            source_image,
            base,
            complete: tree.manifest().clone(),
        };
        validate_mapping(&binding)?;
        source.verify()?;
        let record = ManagerRecord::create(data_root.clone(), BUNDLE_RECORD, &binding, user)?;
        let manager = Self {
            data_root,
            root,
            tree,
            image,
            record,
            binding,
        };
        manager.verify(user)?;
        Ok(manager)
    }
    pub(crate) fn reopen(
        data_root: Arc<PrivateDirectory>,
        transaction: &str,
        expected: &ManagerRecordReference,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        valid_transaction(transaction)?;
        let record = ManagerRecord::open(data_root.clone(), BUNDLE_RECORD, expected, user)?;
        let binding: BundleBinding = record.decode(user)?;
        if binding.transaction != transaction
            || binding.data_root != *data_root.directory().identity()
        {
            return Err(blocked("manager bundle belongs to another transaction"));
        }
        validate_mapping(&binding)?;
        let root = Arc::new(PrivateDirectory::open_existing(
            data_root.directory().clone(),
            name(BUNDLE_DIRECTORY)?,
            user,
        )?);
        if binding.bundle_root != *root.directory().identity() {
            return Err(blocked("manager bundle root changed"));
        }
        let tree =
            HeldTree::capture_private(root.directory().clone(), SnapshotLimits::default(), user)?;
        let image = root
            .directory()
            .open_file(name(MANAGER_BASENAME)?, FileAccess::Read)?;
        let manager = Self {
            data_root,
            root,
            tree,
            image,
            record,
            binding,
        };
        manager.verify(user)?;
        Ok(manager)
    }
    pub(crate) fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.data_root.verify(user)?;
        self.root.verify(user)?;
        self.record.verify(user)?;
        self.tree.verify()?;
        self.image.verify()?;
        user.verify_private_file(handle(&self.image.file), false)?;
        validate_mapping(&self.binding)?;
        if self.tree.manifest() != &self.binding.complete
            || self.binding.bundle_root != *self.root.directory().identity()
            || self.binding.data_root != *self.data_root.directory().identity()
        {
            return Err(blocked("complete manager bundle changed"));
        }
        let image = self
            .binding
            .complete
            .entries
            .iter()
            .find(|entry| entry.metadata.path == MANAGER_BASENAME)
            .ok_or_else(|| blocked("manager entrypoint missing"))?;
        if image.sha256.as_deref() != Some(self.image.digest()?.as_str())
            || image.metadata.object_identity != sha256(&serde_json::to_vec(self.image.identity())?)
        {
            return Err(blocked("manager image changed"));
        }
        Ok(())
    }
    pub(crate) fn reference(&self) -> &ManagerRecordReference {
        self.record.reference()
    }
    pub(crate) fn source_bundle(&self) -> &str {
        &self.binding.source_bundle
    }
    pub(super) fn image(&self) -> &PinnedFile {
        &self.image
    }
    pub(super) fn verify_source_image(&self, image: &PinnedFile) -> io::Result<()> {
        image.verify()?;
        let source = self
            .binding
            .base
            .source
            .entries
            .iter()
            .find(|entry| entry.metadata.path == self.binding.source_image)
            .ok_or_else(|| blocked("source entrypoint missing"))?;
        if source.metadata.object_identity != sha256(&serde_json::to_vec(image.identity())?)
            || source.sha256.as_deref() != Some(image.digest()?.as_str())
        {
            return Err(blocked(
                "source entrypoint differs from retained manager copy",
            ));
        }
        Ok(())
    }
    pub(super) fn data_root(&self) -> &Arc<PrivateDirectory> {
        &self.data_root
    }
    pub(super) fn transaction(&self) -> &str {
        &self.binding.transaction
    }
    pub(super) fn reopen_image(&self, user: &CurrentUser) -> io::Result<PinnedFile> {
        self.verify(user)?;
        let image = self
            .root
            .directory()
            .open_file(name(MANAGER_BASENAME)?, FileAccess::Read)?;
        if image.identity() != self.image.identity() {
            return Err(blocked("manager entrypoint replaced"));
        }
        Ok(image)
    }
}

fn validate_mapping(binding: &BundleBinding) -> io::Result<()> {
    valid_transaction(&binding.transaction)?;
    validate_digest(&binding.source_bundle)
        .map_err(|_| blocked("invalid original bundle digest"))?;
    if binding.schema != 1
        || binding.base.source.entries.len() != binding.base.copy.entries.len()
        || binding.complete.entries.len() != binding.base.copy.entries.len() + 1
        || binding.complete.location_identity != binding.base.copy.location_identity
    {
        return Err(blocked("complete manager mapping differs"));
    }
    let mut source_ids = std::collections::BTreeSet::new();
    for entry in &binding.base.source.entries {
        source_ids.insert(&entry.metadata.object_identity);
    }
    let logical_entries: Vec<_> = binding
        .base
        .source
        .entries
        .iter()
        .map(|entry| {
            (
                &entry.metadata.path,
                entry.metadata.kind,
                entry.metadata.size,
                entry.metadata.link_count,
                &entry.metadata.permissions,
                &entry.sha256,
            )
        })
        .collect();
    if source_ids.len() != binding.base.source.entries.len()
        || sha256(&serde_json::to_vec(&(
            1u32,
            &binding.source_image,
            logical_entries,
        ))?) != binding.source_bundle
    {
        return Err(blocked("original complete bundle identity differs"));
    }
    let complete: std::collections::BTreeMap<_, _> = binding
        .complete
        .entries
        .iter()
        .map(|entry| (entry.metadata.path.as_str(), entry))
        .collect();
    if complete.len() != binding.complete.entries.len() {
        return Err(blocked("duplicate complete manager entry"));
    }
    let mut copy_ids = std::collections::BTreeSet::new();
    for (source, copy) in binding
        .base
        .source
        .entries
        .iter()
        .zip(&binding.base.copy.entries)
    {
        if source.metadata.path != copy.metadata.path
            || source.metadata.kind != copy.metadata.kind
            || source.metadata.size != copy.metadata.size
            || source.sha256 != copy.sha256
            || source_ids.contains(&copy.metadata.object_identity)
            || !copy_ids.insert(&copy.metadata.object_identity)
            || complete.get(copy.metadata.path.as_str()).copied() != Some(copy)
        {
            return Err(blocked(
                "manager did not retain the entire independent source copy",
            ));
        }
    }
    let original = binding
        .base
        .source
        .entries
        .iter()
        .find(|entry| entry.metadata.path == binding.source_image)
        .ok_or_else(|| blocked("original manager source image missing"))?;
    let extra = binding
        .complete
        .entries
        .iter()
        .find(|entry| entry.metadata.path == MANAGER_BASENAME)
        .ok_or_else(|| blocked("distinct manager executable missing"))?;
    if original.metadata.kind != EntryType::File
        || extra.metadata.kind != EntryType::File
        || original.sha256 != extra.sha256
        || original.metadata.size != extra.metadata.size
        || source_ids.contains(&extra.metadata.object_identity)
        || copy_ids.contains(&extra.metadata.object_identity)
        || binding
            .base
            .copy
            .entries
            .iter()
            .any(|entry| entry.metadata.path.eq_ignore_ascii_case(MANAGER_BASENAME))
    {
        return Err(blocked(
            "additional manager image is not an independent exact copy",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/version_history_manager_bundle_windows.rs"]
#[allow(non_snake_case)]
mod tests;
