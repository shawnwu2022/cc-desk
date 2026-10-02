//! Operation-specific local NTFS receipts. Deliberately does not implement
//! DirectoryDurability: directory FlushFileBuffers is not a portable fsync.
use super::{
    blocked,
    files::{ComponentName, FileAccess, FileIdentity, PinnedFile, PrivateDirectory},
    handle,
    security::CurrentUser,
    win_error,
};
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    sync::Arc,
};
use windows::Wdk::Storage::FileSystem::FILE_CREATE;
use windows::Win32::Storage::FileSystem::{
    FlushFileBuffers, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_WRITE_DATA,
    READ_CONTROL, SYNCHRONIZE,
};

const MAX_RECEIPT_BYTES: usize = 1024 * 1024;
/// The same write-through, no-delete/no-external-write handle survives flush,
/// readback and subsequent use. This is one immutable record, not a transaction
/// commit or evidence that unrelated metadata was durably persisted.
pub(crate) struct DurableRecord {
    file: PinnedFile,
    bytes: Vec<u8>,
    digest: String,
    _root: Arc<PrivateDirectory>,
}
impl DurableRecord {
    pub(crate) fn create(
        root: Arc<PrivateDirectory>,
        name: ComponentName,
        bytes: &[u8],
        user: &CurrentUser,
    ) -> io::Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_RECEIPT_BYTES {
            return Err(blocked("unsupported receipt size"));
        }
        root.verify(user)?;
        let security = user.descriptor(false)?;
        let mut file = root.directory().open_relative(
            &name,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ,
            FILE_CREATE,
            false,
            Some(&security),
        )?;
        user.verify_private_file(handle(&file), false)?;
        // On any failure the partial file remains for reconciliation. Never
        // truncate, delete, overwrite or silently retry a failed record write.
        file.write_all(bytes)?;
        unsafe {
            FlushFileBuffers(handle(&file)).map_err(win_error)?;
        }
        let file = PinnedFile::from_file(root.directory().clone(), name, file)?;
        let result = Self {
            file,
            bytes: bytes.to_vec(),
            digest: crate::version_history::verified_package::sha256(bytes),
            _root: root,
        };
        result.verify()?;
        Ok(result)
    }
    /// Reopen only through the secured retained root and the expected digest
    /// from the validated journal. Existence alone never supplies authority.
    pub(crate) fn open(
        root: Arc<PrivateDirectory>,
        name: ComponentName,
        expected_digest: &str,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        root.verify(user)?;
        let file = root.directory().open_file(name, FileAccess::Read)?;
        user.verify_private_file(handle(&file.file), false)?;
        let mut source = &file.file;
        source.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        source
            .take((MAX_RECEIPT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty()
            || bytes.len() > MAX_RECEIPT_BYTES
            || crate::version_history::verified_package::sha256(&bytes) != expected_digest
        {
            return Err(blocked("persisted receipt differs from the journal"));
        }
        let result = Self {
            file,
            bytes,
            digest: expected_digest.into(),
            _root: root,
        };
        result.verify()?;
        Ok(result)
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.file.verify()?;
        let mut file = &self.file.file;
        file.seek(SeekFrom::Start(0))?;
        let mut actual = Vec::new();
        file.take((MAX_RECEIPT_BYTES + 1) as u64)
            .read_to_end(&mut actual)?;
        if actual != self.bytes || self.file.digest()? != self.digest {
            return Err(blocked("durable receipt changed"));
        }
        Ok(())
    }
    pub(crate) fn verify_after_parent_rename(&self) -> io::Result<()> {
        self.verify()
    }
    pub(super) fn root_identity(&self) -> &FileIdentity {
        self._root.directory().identity()
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
}

/// Authoritative write completion must be proved separately, before this
/// post-exit flush. A successful flush cannot repair an interrupted truncation.
pub(crate) fn flush_held_file(file: &PinnedFile, expected_digest: &str) -> io::Result<()> {
    file.verify()?;
    unsafe {
        FlushFileBuffers(handle(&file.file)).map_err(win_error)?;
    }
    if file.digest()? != expected_digest {
        return Err(blocked("flushed file content differs"));
    }
    Ok(())
}
