//! Pairing-only NTFS adapter. Never shares the history root, leases or replacement
//! policy. Tests substitute the sealer, not these filesystem and locking guards.
use super::{
    files::{ComponentName, Directory, FileAccess, PinnedFile, PrivateDirectory},
    handle,
    lease::{ControlLease, LeaseFiles},
    security::CurrentUser,
};
use crate::remote_native_pairing_store::{Backend, Result, StoreError, Transaction, MAX_SEALED};
use std::{
    ffi::OsStr,
    io::{self, Read, Seek, SeekFrom, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use windows::{
    Wdk::Storage::FileSystem::{FILE_CREATE, FILE_OPEN},
    Win32::Storage::FileSystem::{
        ReplaceFileW, DELETE, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_MODE,
        FILE_SHARE_READ, FILE_WRITE_DATA, READ_CONTROL, REPLACE_FILE_FLAGS, SYNCHRONIZE,
    },
};
use windows_core::PCWSTR;
const DIRECTORY: &str = "remote-pairings-v1";
const FILE: &str = "devices.dpapi";
pub(crate) struct WindowsPairingBackend {
    root: Arc<PrivateDirectory>,
    leases: Arc<LeaseFiles>,
    #[cfg(test)]
    fail_after_publish: Arc<std::sync::atomic::AtomicBool>,
}
impl WindowsPairingBackend {
    /// Explicit setup only. Missing directories are never auto-created by open/load.
    pub(crate) fn create_new(parent: &Path) -> Result<Self> {
        Self::open_impl(parent, true)
    }
    pub(crate) fn open_existing(parent: &Path) -> Result<Self> {
        Self::open_impl(parent, false)
    }
    #[cfg(test)]
    pub(crate) fn test_fail_after_publish(&self) {
        self.fail_after_publish
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    fn open_impl(parent: &Path, create: bool) -> Result<Self> {
        let user = CurrentUser::capture().map_err(io_error)?;
        let parent = Directory::open_absolute(parent).map_err(io_error)?;
        let name = ComponentName::new(OsStr::new(DIRECTORY)).map_err(io_error)?;
        let root = Arc::new(
            if create {
                PrivateDirectory::create_new(parent, name, &user)
            } else {
                PrivateDirectory::open_existing(parent, name, &user)
            }
            .map_err(io_error)?,
        );
        let leases = Arc::new(LeaseFiles::open(root.clone(), &user).map_err(io_error)?);
        Ok(Self {
            root,
            leases,
            #[cfg(test)]
            fail_after_publish: Default::default(),
        })
    }
}
impl Backend for WindowsPairingBackend {
    type Guard = PairingTransaction;
    fn target_label(&self) -> Result<String> {
        self.root
            .verify(&CurrentUser::capture().map_err(io_error)?)
            .map_err(io_error)?;
        self.root
            .directory()
            .path()
            .map_err(io_error)?
            .into_string()
            .map_err(|_| StoreError::Invalid)
    }
    fn begin(&self) -> Result<Self::Guard> {
        let user = CurrentUser::capture().map_err(io_error)?;
        self.root.verify(&user).map_err(io_error)?;
        let control = self.leases.acquire_control().map_err(io_error)?;
        control.verify_root(&self.root).map_err(io_error)?;
        Ok(PairingTransaction {
            root: self.root.clone(),
            _leases: self.leases.clone(),
            control,
            user,
            old: None,
            observed: false,
            #[cfg(test)]
            fail_after_publish: self.fail_after_publish.clone(),
        })
    }
}
pub(crate) struct PairingTransaction {
    root: Arc<PrivateDirectory>,
    _leases: Arc<LeaseFiles>,
    control: ControlLease,
    user: CurrentUser,
    old: Option<PinnedFile>,
    observed: bool,
    #[cfg(test)]
    fail_after_publish: Arc<std::sync::atomic::AtomicBool>,
}
impl PairingTransaction {
    fn check(&self) -> Result<()> {
        self.root.verify(&self.user).map_err(io_error)?;
        self.control.verify_root(&self.root).map_err(io_error)
    }
    fn open_target(&self) -> Result<PinnedFile> {
        let file = self
            .root
            .directory()
            .open_file(
                ComponentName::new(OsStr::new(FILE)).map_err(io_error)?,
                FileAccess::Read,
            )
            .map_err(io_error)?;
        self.user
            .verify_private_file(handle(&file.file), false)
            .map_err(io_error)?;
        Ok(file)
    }
}
impl Transaction for PairingTransaction {
    fn read(&mut self) -> Result<Option<Vec<u8>>> {
        self.check()?;
        if self.observed {
            return Err(StoreError::Invalid);
        }
        self.observed = true;
        let file = match self.open_target() {
            Ok(file) => file,
            Err(StoreError::Missing) => return Ok(None),
            Err(e) => return Err(e),
        };
        let bytes = read(&file)?;
        self.old = Some(file);
        Ok(Some(bytes))
    }
    fn commit(
        &mut self,
        bytes: &[u8],
        replacing: bool,
        authorize: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        self.check()?;
        if !self.observed
            || replacing != self.old.is_some()
            || bytes.is_empty()
            || bytes.len() > MAX_SEALED
        {
            return Err(StoreError::Invalid);
        }
        let name = ComponentName::new(OsStr::new(&format!(
            "pending-{}.dpapi",
            uuid::Uuid::new_v4()
        )))
        .map_err(io_error)?;
        let descriptor = self.user.descriptor(false).map_err(io_error)?;
        let file = self
            .root
            .directory()
            .open_relative(
                &name,
                FILE_READ_DATA
                    | FILE_WRITE_DATA
                    | FILE_READ_ATTRIBUTES
                    | READ_CONTROL
                    | DELETE
                    | SYNCHRONIZE,
                FILE_SHARE_MODE(0),
                FILE_CREATE,
                false,
                Some(&descriptor),
            )
            .map_err(io_error)?;
        self.user
            .verify_private_file(handle(&file), false)
            .map_err(io_error)?;
        let mut staged =
            PinnedFile::from_file(self.root.directory().clone(), name, file).map_err(io_error)?;
        staged.file.write_all(bytes).map_err(io_error)?;
        staged.file.sync_all().map_err(io_error)?;
        staged.verify().map_err(io_error)?;
        self.check()?;
        if let Some(old) = &self.old {
            old.verify().map_err(io_error)?;
            self.user
                .verify_private_file(handle(&old.file), false)
                .map_err(io_error)?;
        }
        let identity = staged.identity().clone();
        let from = staged.path().map_err(io_error)?;
        let to = PathBuf::from(self.root.directory().path().map_err(io_error)?).join(FILE);
        let from: Vec<u16> = from.encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
        // Recheck the original native permission after staging+sync, immediately
        // before publication. Old read handle remains pinned during this check.
        authorize()?;
        if replacing {
            // PinnedFile intentionally does not share DELETE. Release target and
            // staged handles only while the original private parent+lock are held.
            drop(self.old.take());
            drop(staged);
            authorize()?;
            unsafe {
                ReplaceFileW(
                    PCWSTR(to.as_ptr()),
                    PCWSTR(from.as_ptr()),
                    PCWSTR::null(),
                    REPLACE_FILE_FLAGS(0),
                    None,
                    None,
                )
            }
            .map_err(|_| StoreError::Unknown)?;
        } else {
            staged.publish_pairing_new(
                self.root.directory().clone(),
                ComponentName::new(OsStr::new(FILE)).map_err(|_| StoreError::Invalid)?,
                authorize,
            )?;
            drop(staged);
        }
        #[cfg(test)]
        if self
            .fail_after_publish
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(StoreError::Unknown);
        }
        // From publication attempt onward every failure is Unknown. Never repair,
        // replay, fall back to a temporary file or pretend an old blob is current.
        let verify = || -> Result<()> {
            self.check()?;
            let name = ComponentName::new(OsStr::new(FILE)).map_err(io_error)?;
            let file = self
                .root
                .directory()
                .open_relative(
                    &name,
                    FILE_READ_DATA
                        | FILE_WRITE_DATA
                        | FILE_READ_ATTRIBUTES
                        | READ_CONTROL
                        | SYNCHRONIZE,
                    FILE_SHARE_READ,
                    FILE_OPEN,
                    false,
                    None,
                )
                .map_err(io_error)?;
            self.user
                .verify_private_file(handle(&file), false)
                .map_err(io_error)?;
            let file = PinnedFile::from_file(self.root.directory().clone(), name, file)
                .map_err(io_error)?;
            if file.identity() != &identity || read(&file)? != bytes {
                return Err(StoreError::Invalid);
            }
            file.file.sync_all().map_err(io_error)?;
            file.verify().map_err(io_error)?;
            self.check()
        };
        verify().map_err(|_| StoreError::Unknown)
    }
}
fn read(file: &PinnedFile) -> Result<Vec<u8>> {
    file.verify().map_err(io_error)?;
    let mut reader = &file.file;
    reader.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut bytes = Vec::new();
    reader
        .take((MAX_SEALED + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    file.verify().map_err(io_error)?;
    if bytes.len() > MAX_SEALED {
        return Err(StoreError::Invalid);
    }
    Ok(bytes)
}
fn io_error(error: io::Error) -> StoreError {
    #[cfg(test)]
    eprintln!("TEST_ONLY pairing filesystem rejection: {error:?}");
    match error.kind() {
        io::ErrorKind::NotFound => StoreError::Missing,
        io::ErrorKind::WouldBlock => StoreError::Busy,
        _ if error.raw_os_error() == Some(33) => StoreError::Busy,
        _ => StoreError::Io,
    }
}
