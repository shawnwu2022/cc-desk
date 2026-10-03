//! Exact publisher-verified bytes and their retained file object. No wire constructor,
//! serializable path, install API, or authenticated payload-identity claim lives here.
use super::catalog::SelectionMetadata;
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use base64::{engine::general_purpose::STANDARD, Engine};
use cap_std::fs::{Dir, DirBuilder, OpenOptions};
use minisign_verify::{PublicKey, Signature};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;

pub(super) const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
pub(super) const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;
const PACKAGE_FILE: &str = "package.bin";
const SIGNATURE_FILE: &str = "signature.bin";

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Matches the locked updater's base64(text) envelope, with stricter bounds and
/// complete text consumption. Only one transport line ending is tolerated.
fn envelope(input: &[u8], maximum: usize) -> Result<String, SafeError> {
    if input.is_empty() || input.len() > maximum {
        return Err(error("HISTORY_SIGNATURE_INVALID"));
    }
    let text = std::str::from_utf8(input).map_err(|_| error("HISTORY_SIGNATURE_INVALID"))?;
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    let decoded = STANDARD
        .decode(text)
        .map_err(|_| error("HISTORY_SIGNATURE_INVALID"))?;
    if decoded.len() > maximum || !decoded.is_ascii() {
        return Err(error("HISTORY_SIGNATURE_INVALID"));
    }
    String::from_utf8(decoded).map_err(|_| error("HISTORY_SIGNATURE_INVALID"))
}

pub(super) struct PublisherKey(PublicKey);
impl PublisherKey {
    pub(super) fn production() -> Result<Self, SafeError> {
        Self::decode(&super::policy::trusted_public_key())
    }
    fn decode(encoded: &str) -> Result<Self, SafeError> {
        let text = envelope(encoded.as_bytes(), 2048)?;
        let lines: Vec<_> = text.lines().collect();
        if lines.len() != 2 || !lines[0].starts_with("untrusted comment: ") || lines[1].len() != 56
        {
            return Err(error("HISTORY_SIGNATURE_INVALID"));
        }
        PublicKey::decode(&text)
            .map(Self)
            .map_err(|_| error("HISTORY_SIGNATURE_INVALID"))
    }
    pub(super) fn verify(
        &self,
        bytes: &[u8],
        signature: &[u8],
        digest: &str,
        size: u64,
    ) -> Result<(), SafeError> {
        if bytes.len() as u64 != size || size == 0 || size > MAX_PACKAGE_BYTES {
            return Err(error("HISTORY_SIZE_MISMATCH"));
        }
        if digest.len() != 64 || sha256(bytes) != digest {
            return Err(error("HISTORY_DIGEST_MISMATCH"));
        }
        let text = envelope(signature, MAX_SIGNATURE_BYTES as usize)?;
        let lines: Vec<_> = text.lines().collect();
        if lines.len() != 4
            || !lines[0].starts_with("untrusted comment: ")
            || lines[1].len() != 100
            || !lines[2].starts_with("trusted comment: ")
            || lines[3].len() != 88
        {
            return Err(error("HISTORY_SIGNATURE_INVALID"));
        }
        let signature = Signature::decode(&text).map_err(|_| error("HISTORY_SIGNATURE_INVALID"))?;
        // Exact locked updater 2.10.1 semantics: legacy Ed25519 and current
        // prehashed signatures are accepted; neither advertised version nor
        // trusted filename comments are treated as authenticated payload identity.
        self.0
            .verify(bytes, &signature, true)
            .map_err(|_| error("HISTORY_SIGNATURE_INVALID"))
    }
    #[cfg(test)]
    pub(super) fn fixture(encoded: &str) -> Result<Self, SafeError> {
        Self::decode(encoded)
    }
}
#[cfg(test)]
pub(crate) fn verify_fixture_payload(
    bytes: &[u8],
    signature: &[u8],
    encoded_key: &str,
    digest: &str,
    size: u64,
) -> Result<(), SafeError> {
    PublisherKey::fixture(encoded_key)?.verify(bytes, signature, digest, size)
}

/// Must be given a manager-owned, secured private directory capability. Task3
/// supplies and verifies its Windows owner ACL and pinned directory identity.
/// No frontend path is accepted, and no ambient path open occurs in this module.
pub(super) struct PrivatePackageStore {
    parent: Dir,
    root: Option<Dir>,
    name: String,
}
impl PrivatePackageStore {
    pub(super) fn new(parent: Dir) -> Result<Arc<Self>, SafeError> {
        let name = format!("history-{}", uuid::Uuid::new_v4().simple());
        private_directory(&parent, &name)?;
        let root = match parent.open_dir(&name) {
            Ok(root) => root,
            Err(_) => {
                let _ = parent.remove_dir(&name);
                return Err(error("HISTORY_STORAGE_UNAVAILABLE"));
            }
        };
        Ok(Arc::new(Self {
            parent,
            root: Some(root),
            name,
        }))
    }
    pub(super) fn transaction(self: &Arc<Self>, id: &str) -> Result<TransactionStorage, SafeError> {
        let root = self
            .root
            .as_ref()
            .ok_or_else(|| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        private_directory(root, id)?;
        let dir = match root.open_dir(id) {
            Ok(dir) => dir,
            Err(_) => {
                let _ = root.remove_dir(id);
                return Err(error("HISTORY_STORAGE_UNAVAILABLE"));
            }
        };
        Ok(TransactionStorage {
            store: self.clone(),
            dir: Some(dir),
            id: id.to_owned(),
        })
    }
}
impl Drop for PrivatePackageStore {
    fn drop(&mut self) {
        drop(self.root.take());
        // Non-recursive removal only: never traverse or erase an unexpected file.
        let _ = self.parent.remove_dir(&self.name);
    }
}
fn private_directory(parent: &Dir, name: &str) -> Result<(), SafeError> {
    #[allow(unused_mut)] // The mode extension is Unix-only.
    let mut options = DirBuilder::new();
    #[cfg(unix)]
    {
        use cap_std::fs::DirBuilderExt;
        options.mode(0o700);
    }
    parent
        .create_dir_with(name, &options)
        .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))
}

pub(super) struct TransactionStorage {
    store: Arc<PrivatePackageStore>,
    dir: Option<Dir>,
    id: String,
}
impl TransactionStorage {
    pub(super) fn create_package(&self) -> Result<File, SafeError> {
        self.create_file(PACKAGE_FILE)
    }
    fn create_file(&self, name: &str) -> Result<File, SafeError> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        self.dir
            .as_ref()
            .ok_or_else(|| error("HISTORY_STORAGE_UNAVAILABLE"))?
            .open_with(name, &options)
            .map(|file| file.into_std())
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))
    }
    fn signature(&self, bytes: &[u8]) -> Result<(), SafeError> {
        let mut file = self.create_file(SIGNATURE_FILE)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))
    }
    fn seal(&self, file: File) -> Result<(File, String), SafeError> {
        file.sync_all()
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        let expected = file_identity(&file)?;
        drop(file);
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.share_mode(windows::Win32::Storage::FileSystem::FILE_SHARE_READ.0);
        }
        let dir = self
            .dir
            .as_ref()
            .ok_or_else(|| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        let metadata = dir
            .symlink_metadata(PACKAGE_FILE)
            .map_err(|_| error("HISTORY_PACKAGE_CHANGED"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        // Reopen relative to the retained transaction directory, then prove the
        // same file object before publishing. On Windows this read-only handle
        // denies future write/delete opens and is held through every consumer.
        let file = dir
            .open_with(PACKAGE_FILE, &options)
            .map_err(|_| error("HISTORY_PACKAGE_CHANGED"))?
            .into_std();
        if file_identity(&file)? != expected {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        Ok((file, expected))
    }
}
impl Drop for TransactionStorage {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            // All package file guards have already been released by their owner.
            // Only our fixed private names are removed; user data is not touched.
            let _ = dir.remove_file(PACKAGE_FILE);
            let _ = dir.remove_file(SIGNATURE_FILE);
            drop(dir);
        }
        if let Some(root) = self.store.root.as_ref() {
            let _ = root.remove_dir(&self.id);
        }
    }
}

pub(super) struct DownloadedPayload {
    pub(super) selection: SelectionMetadata,
    pub(super) bytes: Vec<u8>,
    pub(super) signature: Vec<u8>,
}

pub(crate) struct VerifiedPackage {
    selection: SelectionMetadata,
    bytes: Box<[u8]>,
    signature: Box<[u8]>,
    publisher: Arc<PublisherKey>,
    file: Mutex<Option<File>>,
    file_identity: String,
    _storage: TransactionStorage,
}
impl std::fmt::Debug for VerifiedPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedPackage(<private; publisher verified; payload identity unverified>)")
    }
}
impl VerifiedPackage {
    pub(super) fn finish(
        payload: DownloadedPayload,
        publisher: Arc<PublisherKey>,
        storage: TransactionStorage,
        file: File,
        check: &dyn Fn() -> Result<(), SafeError>,
    ) -> Result<Self, SafeError> {
        let DownloadedPayload {
            selection,
            bytes,
            signature,
        } = payload;
        check()?;
        if signature.len() as u64 != selection.signature().size()
            || sha256(&signature) != selection.signature().sha256()
        {
            return Err(error("HISTORY_DIGEST_MISMATCH"));
        }
        publisher.verify(
            &bytes,
            &signature,
            selection.installer().sha256(),
            selection.installer().size(),
        )?;
        check()?;
        storage.signature(&signature)?;
        let (file, file_identity) = storage.seal(file)?;
        let package = Self {
            selection,
            bytes: bytes.into_boxed_slice(),
            signature: signature.into_boxed_slice(),
            publisher,
            file: Mutex::new(Some(file)),
            file_identity,
            _storage: storage,
        };
        package.revalidate(check)?;
        Ok(package)
    }
    pub(crate) fn selection(&self) -> &SelectionMetadata {
        &self.selection
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn sha256(&self) -> &str {
        self.selection.installer().sha256()
    }
    pub(crate) fn size(&self) -> u64 {
        self.selection.installer().size()
    }
    pub(super) fn signature(&self) -> &[u8] {
        &self.signature
    }
    pub(super) fn retained_identity(&self) -> &str {
        &self.file_identity
    }
    /// Requires a later reviewed manifest keyed by this exact installer digest.
    pub(crate) fn payload_identity_authenticated(&self) -> bool {
        false
    }
    pub(super) fn revalidate(
        &self,
        check: &dyn Fn() -> Result<(), SafeError>,
    ) -> Result<(), SafeError> {
        check()?;
        let mut guard = self.file.lock();
        let file = guard
            .as_mut()
            .ok_or_else(|| error("HISTORY_PACKAGE_CHANGED"))?;
        if file_identity(file)? != self.file_identity
            || file
                .metadata()
                .map_err(|_| error("HISTORY_PACKAGE_CHANGED"))?
                .len()
                != self.size()
        {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| error("HISTORY_PACKAGE_CHANGED"))?;
        let mut buffer = [0u8; 64 * 1024];
        let mut offset = 0;
        loop {
            check()?;
            let count = file
                .read(&mut buffer)
                .map_err(|_| error("HISTORY_PACKAGE_CHANGED"))?;
            check()?;
            if count == 0 {
                break;
            }
            if self.bytes.get(offset..offset + count) != Some(&buffer[..count]) {
                return Err(error("HISTORY_PACKAGE_CHANGED"));
            }
            offset += count;
        }
        if offset != self.bytes.len() || file_identity(file)? != self.file_identity {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        self.publisher
            .verify(&self.bytes, &self.signature, self.sha256(), self.size())?;
        check()
    }
}
impl Drop for VerifiedPackage {
    fn drop(&mut self) {
        // Explicit close precedes TransactionStorage's cleanup on Windows.
        drop(self.file.get_mut().take());
    }
}

fn file_identity(file: &File) -> Result<String, SafeError> {
    let metadata = file
        .metadata()
        .map_err(|_| error("HISTORY_PACKAGE_CHANGED"))?;
    if !metadata.is_file() {
        return Err(error("HISTORY_PACKAGE_CHANGED"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // The retained file handle and output buffer are live for this call.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| error("HISTORY_PACKAGE_CHANGED"))?;
        if (info.nFileIndexHigh == 0 && info.nFileIndexLow == 0)
            || info.nNumberOfLinks != 1
            || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        {
            return Err(error("HISTORY_PACKAGE_CHANGED"));
        }
        Ok(format!(
            "windows:{}:{}:{}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(error("HISTORY_PLATFORM_UNSUPPORTED"))
    }
}
