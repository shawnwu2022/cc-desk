//! Durable manager-owned package material. A verified private copy preserves the
//! exact source selection/signature and file identities, but is not installation
//! or payload admission. No path, URL, PID or key is accepted from the frontend.
use super::{
    files::{ComponentName, FileAccess, FileIdentity, PinnedFile, PrivateDirectory},
    handle,
    security::CurrentUser,
};
use crate::cli::{profiles::error, types::SafeError};
use crate::version_history::{
    catalog::{
        inspect_retained_observation, CatalogService, RetainedSelectionDiagnostic,
        SelectionMetadata, MAX_CATALOG_BYTES,
    },
    download::PreparedHandoff,
    journal::validate_id,
    verified_package::{sha256, PublisherKey, MAX_PACKAGE_BYTES, MAX_SIGNATURE_BYTES},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    io::{Read, Seek, SeekFrom, Write},
    sync::Arc,
};
use windows::{
    Wdk::Storage::FileSystem::FILE_CREATE,
    Win32::Storage::FileSystem::{
        FlushFileBuffers, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_WRITE_DATA,
        READ_CONTROL, SYNCHRONIZE,
    },
};

const PACKAGE: &str = "official-installer.exe";
const SIGNATURE: &str = "official-signature.bin";
const OBSERVATION: &str = "official-selection.json";
const BINDING: &str = "package-transfer.json";
const MAX_BINDING: usize = 4096;
fn storage(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_STORAGE_UNAVAILABLE")
}
fn changed(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_PACKAGE_CHANGED")
}
fn name(value: &str) -> Result<ComponentName, SafeError> {
    ComponentName::new(OsStr::new(value)).map_err(storage)
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ObjectBinding {
    identity: FileIdentity,
    size: u64,
    digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransferBinding {
    schema: u32,
    transaction_id: String,
    preparation_id: String,
    source_identity: String,
    package: ObjectBinding,
    signature: ObjectBinding,
    observation: ObjectBinding,
}

/// All guards are read-only before another process is asked to reopen them.
/// The writable handle is flushed/read back, then the read guard must identify
/// the SAME object. The sealed handle denies future write/delete sharing.
struct SealedFile {
    file: PinnedFile,
    binding: ObjectBinding,
}
impl SealedFile {
    fn create(
        root: &Arc<PrivateDirectory>,
        filename: &str,
        bytes: &[u8],
        limit: usize,
        user: &CurrentUser,
        check: &dyn Fn() -> Result<(), SafeError>,
    ) -> Result<Self, SafeError> {
        if bytes.is_empty() || bytes.len() > limit {
            return Err(error("HISTORY_CAPACITY"));
        }
        check()?;
        root.verify(user).map_err(storage)?;
        let filename = name(filename)?;
        let security = user.descriptor(false).map_err(storage)?;
        let file = root
            .directory()
            .open_relative(
                &filename,
                FILE_READ_DATA
                    | FILE_WRITE_DATA
                    | FILE_READ_ATTRIBUTES
                    | READ_CONTROL
                    | SYNCHRONIZE,
                FILE_SHARE_READ,
                FILE_CREATE,
                false,
                Some(&security),
            )
            .map_err(storage)?;
        user.verify_private_file(handle(&file), false)
            .map_err(storage)?;
        let file = PinnedFile::from_file(root.directory().clone(), filename.clone(), file)
            .map_err(storage)?;
        // Any error retains the exact partial object; there is no overwrite,
        // deletion or implicit retry, even for an interrupted preparation.
        let mut writer = &file.file;
        for chunk in bytes.chunks(64 * 1024) {
            check()?;
            writer.write_all(chunk).map_err(storage)?;
        }
        check()?;
        unsafe { FlushFileBuffers(handle(&file.file)) }.map_err(storage)?;
        let binding = ObjectBinding {
            identity: file.identity().clone(),
            size: bytes.len() as u64,
            digest: sha256(bytes),
        };
        if file.digest().map_err(changed)? != binding.digest {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        drop(file);
        let sealed = Self::open(root, filename, binding, limit, user)?;
        check()?;
        Ok(sealed)
    }
    fn open(
        root: &Arc<PrivateDirectory>,
        filename: ComponentName,
        binding: ObjectBinding,
        limit: usize,
        user: &CurrentUser,
    ) -> Result<Self, SafeError> {
        root.verify(user).map_err(storage)?;
        if binding.size == 0 || binding.size > limit as u64 {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        let file = root
            .directory()
            .open_file(filename, FileAccess::Read)
            .map_err(changed)?;
        user.verify_private_file(handle(&file.file), false)
            .map_err(changed)?;
        let result = Self { file, binding };
        result.verify(user)?;
        Ok(result)
    }
    fn verify(&self, user: &CurrentUser) -> Result<(), SafeError> {
        self.file.verify().map_err(changed)?;
        user.verify_private_file(handle(&self.file.file), false)
            .map_err(changed)?;
        if self.file.identity() != &self.binding.identity
            || self.file.file.metadata().map_err(changed)?.len() != self.binding.size
            || self.file.digest().map_err(changed)? != self.binding.digest
        {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        Ok(())
    }
    fn bytes(&self, user: &CurrentUser) -> Result<Vec<u8>, SafeError> {
        self.verify(user)?;
        let mut source = &self.file.file;
        source.seek(SeekFrom::Start(0)).map_err(changed)?;
        let mut bytes = Vec::with_capacity(self.binding.size as usize);
        source
            .take(self.binding.size + 1)
            .read_to_end(&mut bytes)
            .map_err(changed)?;
        if bytes.len() as u64 != self.binding.size || sha256(&bytes) != self.binding.digest {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        self.verify(user)?;
        Ok(bytes)
    }
}
struct RetainedFiles {
    package: SealedFile,
    signature: SealedFile,
    observation: SealedFile,
    record: SealedFile,
}
pub(crate) struct RetainedPackage {
    root: Arc<PrivateDirectory>,
    binding: TransferBinding,
    selection: SelectionMetadata,
    files: Mutex<RetainedFiles>,
}
impl RetainedPackage {
    pub(crate) fn retain(
        transfer: &PreparedHandoff,
        root: Arc<PrivateDirectory>,
    ) -> Result<Self, SafeError> {
        let user = CurrentUser::capture().map_err(storage)?;
        root.verify(&user).map_err(storage)?;
        transfer.with_material(
            |bytes, signature_bytes, observation_bytes, source_identity| {
                let check = || transfer.check();
                let package = SealedFile::create(
                    &root,
                    PACKAGE,
                    bytes,
                    MAX_PACKAGE_BYTES as usize,
                    &user,
                    &check,
                )?;
                let signature = SealedFile::create(
                    &root,
                    SIGNATURE,
                    signature_bytes,
                    MAX_SIGNATURE_BYTES as usize,
                    &user,
                    &check,
                )?;
                let observation = SealedFile::create(
                    &root,
                    OBSERVATION,
                    observation_bytes,
                    MAX_CATALOG_BYTES,
                    &user,
                    &check,
                )?;
                let binding = TransferBinding {
                    schema: 1,
                    transaction_id: transfer.transaction_id().into(),
                    preparation_id: transfer.preparation_id().into(),
                    source_identity: source_identity.into(),
                    package: package.binding.clone(),
                    signature: signature.binding.clone(),
                    observation: observation.binding.clone(),
                };
                let encoded = serde_json::to_vec(&binding).map_err(storage)?;
                let record =
                    SealedFile::create(&root, BINDING, &encoded, MAX_BINDING, &user, &check)?;
                let result = Self {
                    root: root.clone(),
                    binding,
                    selection: transfer.selection().clone(),
                    files: Mutex::new(RetainedFiles {
                        package,
                        signature,
                        observation,
                        record,
                    }),
                };
                result.verify_transfer(transfer)?;
                Ok(result)
            },
        )
    }
    pub(crate) fn verify_transfer(&self, transfer: &PreparedHandoff) -> Result<(), SafeError> {
        if self.binding.transaction_id != transfer.transaction_id()
            || self.binding.preparation_id != transfer.preparation_id()
            || &self.selection != transfer.selection()
        {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let user = CurrentUser::capture().map_err(storage)?;
        self.root.verify(&user).map_err(storage)?;
        transfer.with_material(|bytes, signature, observation, identity| {
            let files = self.files.lock();
            if files.package.bytes(&user)? != bytes
                || files.signature.bytes(&user)? != signature
                || files.observation.bytes(&user)? != observation
                || self.binding.source_identity != identity
                || files.record.bytes(&user)?
                    != serde_json::to_vec(&self.binding).map_err(storage)?
            {
                return Err(error("HISTORY_PACKAGE_CHANGED"));
            }
            Ok(())
        })
    }
    pub(crate) fn record_digest(&self) -> String {
        self.files.lock().record.binding.digest.clone()
    }
    pub(crate) fn transaction_id(&self) -> &str {
        &self.binding.transaction_id
    }
    pub(crate) fn selection(&self) -> &SelectionMetadata {
        &self.selection
    }
    pub(crate) fn root_identity(&self) -> &FileIdentity {
        self.root.directory().identity()
    }
    /// Reopen only this exact sealed installer object. The returned handle is
    /// still package evidence; the coordinator must separately admit scope,
    /// journal intent, suspended process ownership and all live execution guards.
    pub(crate) fn installer_image(&self) -> Result<PinnedFile, SafeError> {
        self.verify_retained()?;
        let image = {
            let files = self.files.lock();
            let expected = &files.package;
            let image = self
                .root
                .directory()
                .open_file(expected.file.name.clone(), FileAccess::Read)
                .map_err(changed)?;
            if image.identity() != &expected.binding.identity
                || image.file.metadata().map_err(changed)?.len() != expected.binding.size
                || image.digest().map_err(changed)? != expected.binding.digest
            {
                return Err(error("HISTORY_PACKAGE_CHANGED"));
            }
            image
        };
        self.verify_retained()?;
        image.verify().map_err(changed)?;
        Ok(image)
    }
    /// Revalidates the exact retained objects/bytes used at transfer. Official
    /// metadata and publisher authentication are freshly re-established by
    /// reopen; this never changes package identity into installation authority.
    pub(crate) fn verify_retained(&self) -> Result<(), SafeError> {
        let user = CurrentUser::capture().map_err(storage)?;
        self.root.verify(&user).map_err(storage)?;
        let files = self.files.lock();
        files.package.verify(&user)?;
        files.signature.verify(&user)?;
        files.observation.verify(&user)?;
        if files.package.binding != self.binding.package
            || files.signature.binding != self.binding.signature
            || files.observation.binding != self.binding.observation
            || files.record.bytes(&user)? != serde_json::to_vec(&self.binding).map_err(storage)?
        {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        Ok(())
    }
    /// Only a validated transaction's protected record digest may be supplied.
    /// The selected release is re-fetched and its complete observation compared;
    /// every reopened file must retain the exact copied object identity.
    pub(crate) fn reopen(
        root: Arc<PrivateDirectory>,
        transaction_id: &str,
        expected_record_digest: &str,
        catalog: &CatalogService,
    ) -> Result<Self, SafeError> {
        let (binding, files) = reopen_files(&root, transaction_id, expected_record_digest)?;
        let user = CurrentUser::capture().map_err(storage)?;
        let RetainedFiles {
            package,
            signature,
            observation,
            record,
        } = files;
        let selection = catalog.revalidate_retained_observation(&observation.bytes(&user)?)?;
        let signature_bytes = signature.bytes(&user)?;
        if signature_bytes.len() as u64 != selection.signature().size()
            || sha256(&signature_bytes) != selection.signature().sha256()
        {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        PublisherKey::production()?.verify(
            &package.bytes(&user)?,
            &signature_bytes,
            selection.installer().sha256(),
            selection.installer().size(),
        )?;
        Ok(Self {
            root,
            binding,
            selection,
            files: Mutex::new(RetainedFiles {
                package,
                signature,
                observation,
                record,
            }),
        })
    }
}

/// Opens exact retained objects only. Neither a stored digest nor successful
/// local byte inspection grants fresh selection or installation authority.
fn reopen_files(
    root: &Arc<PrivateDirectory>,
    transaction_id: &str,
    expected_record_digest: &str,
) -> Result<(TransferBinding, RetainedFiles), SafeError> {
    validate_id(transaction_id)?;
    crate::version_history::journal::validate_digest(expected_record_digest)?;
    let user = CurrentUser::capture().map_err(storage)?;
    user.require_unelevated().map_err(storage)?;
    root.verify(&user).map_err(storage)?;
    let file = root
        .directory()
        .open_file(name(BINDING)?, FileAccess::Read)
        .map_err(changed)?;
    let record_binding = ObjectBinding {
        identity: file.identity().clone(),
        size: file.file.metadata().map_err(changed)?.len(),
        digest: expected_record_digest.into(),
    };
    if record_binding.size == 0 || record_binding.size > MAX_BINDING as u64 {
        return Err(error("HISTORY_PACKAGE_CHANGED"));
    }
    let record = SealedFile {
        file,
        binding: record_binding,
    };
    let binding: TransferBinding =
        serde_json::from_slice(&record.bytes(&user)?).map_err(changed)?;
    if binding.schema != 1
        || binding.transaction_id != transaction_id
        || binding.preparation_id.len() != 32
        || !binding
            .preparation_id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || binding.source_identity.is_empty()
        || binding.source_identity.len() > 256
    {
        return Err(error("HISTORY_HANDOFF_CHANGED"));
    }
    let package = SealedFile::open(
        root,
        name(PACKAGE)?,
        binding.package.clone(),
        MAX_PACKAGE_BYTES as usize,
        &user,
    )?;
    let signature = SealedFile::open(
        root,
        name(SIGNATURE)?,
        binding.signature.clone(),
        MAX_SIGNATURE_BYTES as usize,
        &user,
    )?;
    let observation = SealedFile::open(
        root,
        name(OBSERVATION)?,
        binding.observation.clone(),
        MAX_CATALOG_BYTES,
        &user,
    )?;
    Ok((
        binding,
        RetainedFiles {
            package,
            signature,
            observation,
            record,
        },
    ))
}

/// Read-only offline package inspection for the restarted manager. This owner
/// deliberately has no installer-image or SelectionMetadata accessor.
pub(crate) struct LocalPackageInspection {
    root: Arc<PrivateDirectory>,
    binding: TransferBinding,
    diagnostic: RetainedSelectionDiagnostic,
    files: Mutex<RetainedFiles>,
}
impl LocalPackageInspection {
    pub(crate) fn open(
        root: Arc<PrivateDirectory>,
        transaction_id: &str,
        expected_record_digest: &str,
    ) -> Result<Self, SafeError> {
        let (binding, files) = reopen_files(&root, transaction_id, expected_record_digest)?;
        let user = CurrentUser::capture().map_err(storage)?;
        let diagnostic = inspect_retained_observation(&files.observation.bytes(&user)?)?;
        let signature = files.signature.bytes(&user)?;
        if signature.len() as u64 != diagnostic.signature_size()
            || sha256(&signature) != diagnostic.signature_digest()
        {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        // Reestablish publisher authenticity from actual retained bytes, even
        // when the remote release is unavailable. This mints no install owner.
        PublisherKey::production()?.verify(
            &files.package.bytes(&user)?,
            &signature,
            diagnostic.installer_digest(),
            diagnostic.installer_size(),
        )?;
        let inspection = Self {
            root,
            binding,
            diagnostic,
            files: Mutex::new(files),
        };
        inspection.verify()?;
        Ok(inspection)
    }
    pub(crate) fn diagnostic(&self) -> &RetainedSelectionDiagnostic {
        &self.diagnostic
    }
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        let user = CurrentUser::capture().map_err(storage)?;
        self.root.verify(&user).map_err(storage)?;
        let files = self.files.lock();
        files.package.verify(&user)?;
        files.signature.verify(&user)?;
        files.observation.verify(&user)?;
        if files.package.binding != self.binding.package
            || files.signature.binding != self.binding.signature
            || files.observation.binding != self.binding.observation
            || files.record.bytes(&user)? != serde_json::to_vec(&self.binding).map_err(storage)?
            || files.package.binding.digest != self.diagnostic.installer_digest()
            || files.package.binding.size != self.diagnostic.installer_size()
            || inspect_retained_observation(&files.observation.bytes(&user)?)? != self.diagnostic
        {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        Ok(())
    }
}
