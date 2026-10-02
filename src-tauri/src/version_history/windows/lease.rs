//! Two permanent files, never deleted/replaced or upgraded shared-to-exclusive.
use super::{
    blocked,
    files::{ComponentName, FileIdentity, PinnedFile, PrivateDirectory},
    handle,
    security::CurrentUser,
    win_error,
};
use std::{ffi::OsStr, io, sync::Arc};
use windows::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows::Win32::{
    Storage::FileSystem::{
        LockFileEx, UnlockFileEx, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_WRITE_DATA, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        READ_CONTROL, SYNCHRONIZE,
    },
    System::IO::OVERLAPPED,
};

pub(crate) struct LeaseFiles {
    root: Arc<PrivateDirectory>,
    control: FileIdentity,
    lifetime: FileIdentity,
    _control_guard: PinnedFile,
    _lifetime_guard: PinnedFile,
}
impl LeaseFiles {
    pub(crate) fn open(root: Arc<PrivateDirectory>, user: &CurrentUser) -> io::Result<Self> {
        root.verify(user)?;
        let control = open_lock(&root, "control.lock", user)?;
        let lifetime = open_lock(&root, "lifetime.lock", user)?;
        Ok(Self {
            root,
            control: control.identity().clone(),
            lifetime: lifetime.identity().clone(),
            _control_guard: control,
            _lifetime_guard: lifetime,
        })
    }
    pub(crate) fn identities(&self) -> (&FileIdentity, &FileIdentity) {
        (&self.control, &self.lifetime)
    }
    fn lock(&self, name: &str, identity: &FileIdentity, exclusive: bool) -> io::Result<ByteLock> {
        let user = CurrentUser::capture()?;
        let file = open_lock(&self.root, name, &user)?;
        if file.identity() != identity {
            return Err(blocked("stable lease identity changed"));
        }
        let mut overlapped = OVERLAPPED::default();
        let mut flags = LOCKFILE_FAIL_IMMEDIATELY;
        if exclusive {
            flags |= LOCKFILE_EXCLUSIVE_LOCK;
        }
        unsafe {
            LockFileEx(handle(&file.file), flags, None, 1, 0, &mut overlapped)
                .map_err(win_error)?;
        }
        file.verify()?;
        Ok(ByteLock { file })
    }
    pub(crate) fn acquire_control(&self) -> io::Result<ControlLease> {
        Ok(ControlLease {
            lock: self.lock("control.lock", &self.control, true)?,
            root: self.root.directory().identity().clone(),
        })
    }
    fn check_control(&self, control: &ControlLease) -> io::Result<()> {
        control.lock.file.verify()?;
        if control.root != *self.root.directory().identity()
            || control.lock.file.identity() != &self.control
        {
            return Err(blocked("foreign control lease"));
        }
        Ok(())
    }
    pub(crate) fn acquire_shared(&self, control: &ControlLease) -> io::Result<SharedLease> {
        self.check_control(control)?;
        Ok(SharedLease(self.lock(
            "lifetime.lock",
            &self.lifetime,
            false,
        )?))
    }
    pub(crate) fn acquire_exclusive(&self, control: &ControlLease) -> io::Result<ExclusiveLease> {
        self.check_control(control)?;
        Ok(ExclusiveLease(self.lock(
            "lifetime.lock",
            &self.lifetime,
            true,
        )?))
    }
}
fn open_lock(root: &PrivateDirectory, name: &str, user: &CurrentUser) -> io::Result<PinnedFile> {
    root.verify(user)?;
    let name = ComponentName::new(OsStr::new(name))?;
    let security = user.descriptor(false)?;
    let file = root.directory().open_relative(
        &name,
        FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_OPEN_IF,
        false,
        Some(&security),
    )?;
    user.verify_private_file(handle(&file), false)?;
    PinnedFile::from_file(root.directory().clone(), name, file)
}
struct ByteLock {
    file: PinnedFile,
}
impl Drop for ByteLock {
    fn drop(&mut self) {
        let mut overlapped = OVERLAPPED::default();
        unsafe {
            let _ = UnlockFileEx(handle(&self.file.file), None, 1, 0, &mut overlapped);
        }
    }
}
pub(crate) struct ControlLease {
    lock: ByteLock,
    root: FileIdentity,
}
pub(crate) struct SharedLease(ByteLock);
pub(crate) struct ExclusiveLease(ByteLock);

impl SharedLease {
    pub(super) fn verify_root(&self, root: &PrivateDirectory) -> io::Result<()> {
        self.0.file.verify()?;
        root.directory().recheck()?;
        if self.0.file.parent.identity() != root.directory().identity() {
            return Err(blocked("foreign source lifetime lease"));
        }
        Ok(())
    }
}

impl ControlLease {
    /// A marker writer borrows this guard for its entire lifetime. A lock on a
    /// different recovery root cannot authorize publication here.
    pub(crate) fn verify_root(&self, root: &PrivateDirectory) -> io::Result<()> {
        self.lock.file.verify()?;
        root.directory().recheck()?;
        if &self.root != root.directory().identity()
            || self.lock.file.parent.identity() != root.directory().identity()
        {
            return Err(blocked("foreign control lease"));
        }
        Ok(())
    }
}

impl ExclusiveLease {
    pub(super) fn verify_root(&self, root: &PrivateDirectory) -> io::Result<()> {
        self.verify()?;
        root.directory().recheck()?;
        if self.0.file.parent.identity() != root.directory().identity() {
            return Err(blocked("foreign exclusive lifetime lease"));
        }
        Ok(())
    }
    pub(super) fn identity(&self) -> &FileIdentity {
        self.0.file.identity()
    }
    pub(super) fn verify(&self) -> io::Result<()> {
        self.0.file.verify()
    }
}
