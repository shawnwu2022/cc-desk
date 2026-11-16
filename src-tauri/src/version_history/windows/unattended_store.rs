//! 无人值守专用 NTFS 适配器；不共享历史或配对目录及 lease 实例。
//! 测试仅替换密封器，保留真实文件系统与锁检查。
#![allow(dead_code, reason = "分阶段独立适配器，尚未接入生产调用链")]
use super::{
    files::{ComponentName, Directory, FileAccess, PinnedFile, PrivateDirectory},
    handle,
    lease::{ControlLease, LeaseFiles},
    security::CurrentUser,
};
use crate::remote_unattended_store::{Backend, Result, StoreError, Transaction, MAX_SEALED};
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
const DIRECTORY: &str = "remote-unattended-v1";
const FILE: &str = "delegation.dpapi";
pub(crate) struct WindowsUnattendedBackend {
    root: Arc<PrivateDirectory>,
    leases: Arc<LeaseFiles>,
    #[cfg(test)]
    fail_after_publish: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(test)]
    fail_before_publish: Arc<std::sync::atomic::AtomicBool>,
}
impl WindowsUnattendedBackend {
    /// 仅显式初始化创建；打开和加载绝不自动补建缺失目录。
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
    #[cfg(test)]
    pub(crate) fn test_fail_before_publish(&self) {
        self.fail_before_publish
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
        let leases = Arc::new(
            if create {
                LeaseFiles::open(root.clone(), &user)
            } else {
                LeaseFiles::open_existing(root.clone(), &user)
            }
            .map_err(io_error)?,
        );
        Ok(Self {
            root,
            leases,
            #[cfg(test)]
            fail_after_publish: Default::default(),
            #[cfg(test)]
            fail_before_publish: Default::default(),
        })
    }
}
impl Backend for WindowsUnattendedBackend {
    type Guard = UnattendedTransaction;
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
        Ok(UnattendedTransaction {
            root: self.root.clone(),
            _leases: self.leases.clone(),
            control,
            user,
            old: None,
            observed: false,
            #[cfg(test)]
            fail_after_publish: self.fail_after_publish.clone(),
            #[cfg(test)]
            fail_before_publish: self.fail_before_publish.clone(),
        })
    }
}
pub(crate) struct UnattendedTransaction {
    root: Arc<PrivateDirectory>,
    _leases: Arc<LeaseFiles>,
    control: ControlLease,
    user: CurrentUser,
    old: Option<PinnedFile>,
    observed: bool,
    #[cfg(test)]
    fail_after_publish: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(test)]
    fail_before_publish: Arc<std::sync::atomic::AtomicBool>,
}
impl UnattendedTransaction {
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
impl Transaction for UnattendedTransaction {
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
        // staging 与 sync 后，在实际发布之前再次检查原生许可。
        // 检查期间仍固定旧读取句柄。
        authorize()?;
        #[cfg(test)]
        if self
            .fail_before_publish
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(StoreError::Io);
        }
        if replacing {
            // PinnedFile 不共享 DELETE；仅在原私有父目录和锁仍持有时
            // 释放目标及 staging 句柄。
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
            staged.publish_unattended_new(
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
        // 一旦尝试发布，后续失败均为 Unknown。不得修复、重放、
        // 回退临时文件或声称旧文件就是当前版本。
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
    eprintln!("TEST_ONLY unattended filesystem rejection: {error:?}");
    match error.kind() {
        io::ErrorKind::NotFound => StoreError::Missing,
        io::ErrorKind::WouldBlock => StoreError::Busy,
        _ if error.raw_os_error() == Some(33) => StoreError::Busy,
        _ => StoreError::Io,
    }
}
