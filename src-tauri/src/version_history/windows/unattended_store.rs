//! 无人值守专用 NTFS 适配器；不共享历史或配对目录及 lease 实例。
//! 测试仅替换密封器，保留真实文件系统与锁检查。
#![allow(dead_code, reason = "分阶段独立适配器，尚未接入生产调用链")]
use super::{
    files::{ComponentName, Directory, FileIdentity, PinnedFile, PrivateDirectory},
    handle,
    lease::{ControlLease, LeaseFiles},
    security::CurrentUser,
};
#[cfg(test)]
use crate::remote_unattended_store::journal::RECORD_BYTES;
use crate::remote_unattended_store::journal::{Chain, CommitStamp, Record, MAX_JOURNAL};
use crate::remote_unattended_store::{Backend, Result, StoreError, Transaction, MAX_SEALED};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::sync::atomic::AtomicU8;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
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
const JOURNAL: &str = "commit.journal";
pub(crate) struct WindowsUnattendedBackend {
    root: Arc<PrivateDirectory>,
    leases: Arc<LeaseFiles>,
    poison: Arc<AtomicBool>,
    journal_identity: FileIdentity,
    fresh: bool,
    #[cfg(test)]
    journal_fault: Arc<AtomicU8>,
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
    #[cfg(test)]
    pub(crate) fn test_journal_fault(&self, fault: u8) {
        self.journal_fault.store(fault, Ordering::SeqCst);
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
        let control = leases.acquire_control().map_err(io_error)?;
        control.verify_root(&root).map_err(io_error)?;
        let poison = registry(root.directory().identity(), None)?;
        if poison.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        let journal = match open_journal(&root, &user, create) {
            Ok(journal) => journal,
            Err(_) => {
                poison.store(true, Ordering::Release);
                return Err(StoreError::Unknown);
            }
        };
        registry(root.directory().identity(), Some(journal.identity()))?;
        let journal_identity = journal.identity().clone();
        drop(journal);
        drop(control);
        Ok(Self {
            root,
            leases,
            poison,
            journal_identity,
            fresh: create,
            #[cfg(test)]
            journal_fault: Default::default(),
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
        if self.poison.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        #[cfg(test)]
        BEFORE_CONTROL.with_borrow_mut(|slot| {
            if let Some(action) = slot.take() {
                action()
            }
        });
        let user = CurrentUser::capture().map_err(io_error)?;
        self.root.verify(&user).map_err(io_error)?;
        let control = self.leases.acquire_control().map_err(io_error)?;
        control.verify_root(&self.root).map_err(io_error)?;
        if self.poison.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        let journal = match open_journal(&self.root, &user, false) {
            Ok(j) if j.identity() == &self.journal_identity => j,
            _ => {
                self.poison.store(true, Ordering::Release);
                return Err(StoreError::Unknown);
            }
        };
        Ok(UnattendedTransaction {
            journal,
            chain: None,
            poison: self.poison.clone(),
            fresh: self.fresh,
            inflight: false,
            #[cfg(test)]
            journal_fault: self.journal_fault.clone(),
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
    journal: PinnedFile,
    chain: Option<Chain>,
    poison: Arc<AtomicBool>,
    fresh: bool,
    inflight: bool,
    #[cfg(test)]
    journal_fault: Arc<AtomicU8>,
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
        PinnedFile::from_file(self.root.directory().clone(), name, file).map_err(io_error)
    }
    fn read_verified(&mut self) -> Result<Option<Vec<u8>>> {
        if self.poison.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        self.check()?;
        if self.observed {
            return Err(StoreError::Invalid);
        }
        self.observed = true;
        let bytes = read_bounded(&self.journal, MAX_JOURNAL)?;
        let chain = Chain::parse(&bytes, self.fresh)?;
        let file = match self.open_target() {
            Ok(file) => file,
            Err(StoreError::Missing) if chain.records == 0 && self.fresh => {
                self.chain = Some(chain);
                return Ok(None);
            }
            _ => return Err(StoreError::Unknown),
        };
        let blob = read(&file)?;
        if chain
            .last
            .as_ref()
            .is_none_or(|r| r.stamp.blob != <[u8; 32]>::from(Sha256::digest(&blob)))
        {
            return Err(StoreError::Unknown);
        }
        // 恢复证据需重新成功 flush，不把可见字节当持久提交；原 Unknown 不改判。
        file.file.sync_all().map_err(|_| StoreError::Unknown)?;
        self.journal
            .file
            .sync_all()
            .map_err(|_| StoreError::Unknown)?;
        file.verify().map_err(|_| StoreError::Unknown)?;
        self.journal.verify().map_err(|_| StoreError::Unknown)?;
        self.check()?;
        if self.poison.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        self.chain = Some(chain);
        self.old = Some(file);
        Ok(Some(blob))
    }
    fn append(&mut self, record: &Record) -> Result<()> {
        self.journal.verify().map_err(|_| StoreError::Unknown)?;
        #[cfg(test)]
        if !record.clean
            && self
                .journal_fault
                .compare_exchange(8, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            return Err(StoreError::Unknown);
        }
        let bytes = record.bytes();
        self.journal
            .file
            .seek(SeekFrom::End(0))
            .map_err(|_| StoreError::Unknown)?;
        #[cfg(test)]
        {
            let expected = if record.clean { 4 } else { 1 };
            if self
                .journal_fault
                .compare_exchange(expected, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                self.journal
                    .file
                    .write_all(&bytes[..RECORD_BYTES / 2])
                    .map_err(|_| StoreError::Unknown)?;
                return Err(StoreError::Unknown);
            }
        }
        self.journal
            .file
            .write_all(&bytes)
            .map_err(|_| StoreError::Unknown)?;
        #[cfg(test)]
        if self
            .journal_fault
            .compare_exchange(
                if record.clean { 5 } else { 2 },
                0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            return Err(StoreError::Unknown);
        }
        self.journal
            .file
            .sync_all()
            .map_err(|_| StoreError::Unknown)?;
        #[cfg(test)]
        if !record.clean
            && self
                .journal_fault
                .compare_exchange(3, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            return Err(StoreError::Unknown);
        }
        self.journal.verify().map_err(|_| StoreError::Unknown)?;
        Ok(())
    }
    fn publish_blob(
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
impl Transaction for UnattendedTransaction {
    fn stamp(&self) -> Option<&CommitStamp> {
        self.chain.as_ref()?.last.as_ref().map(|r| &r.stamp)
    }
    fn read(&mut self) -> Result<Option<Vec<u8>>> {
        let result = self.read_verified();
        if result.is_err() {
            self.poison.store(true, Ordering::Release)
        }
        result
    }
    fn commit(
        &mut self,
        bytes: &[u8],
        replacing: bool,
        stamp: &CommitStamp,
        authorize: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        if self.poison.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        if !self.observed
            || replacing != self.old.is_some()
            || stamp.blob != <[u8; 32]>::from(Sha256::digest(bytes))
        {
            return Err(StoreError::Invalid);
        }
        let pending = self
            .chain
            .as_ref()
            .ok_or(StoreError::Invalid)?
            .pending(stamp)?;
        authorize()?;
        // Pending 开始写入就是不确定窗口；之后任何错误都封闭此 NTFS 身份。
        self.inflight = true;
        let result: Result<()> = (|| {
            self.append(&pending)?;
            self.publish_blob(bytes, replacing, authorize)?;
            #[cfg(test)]
            if self
                .journal_fault
                .compare_exchange(9, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Err(StoreError::Unknown);
            }
            let clean = Record {
                clean: true,
                sequence: pending.sequence + 1,
                transaction: pending.transaction,
                stamp: pending.stamp.clone(),
                previous: Sha256::digest(pending.bytes()).into(),
            };
            self.append(&clean)?;
            #[cfg(test)]
            if self
                .journal_fault
                .compare_exchange(6, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Err(StoreError::Unknown);
            }
            // 成功线性化点为匹配 Clean 的成功同步；之后没有额外可失败的 I/O。
            Ok(())
        })();
        if result.is_err() {
            self.poison.store(true, Ordering::Release);
            return Err(StoreError::Unknown);
        }
        self.inflight = false;
        self.observed = false;
        Ok(())
    }
}
impl Drop for UnattendedTransaction {
    fn drop(&mut self) {
        if self.inflight {
            self.poison.store(true, Ordering::Release)
        }
    }
}
struct RegistryEntry {
    root: FileIdentity,
    journal: Option<FileIdentity>,
    poison: Arc<AtomicBool>,
}
fn registry(root: &FileIdentity, journal: Option<&FileIdentity>) -> Result<Arc<AtomicBool>> {
    static REGISTRY: OnceLock<Mutex<Vec<RegistryEntry>>> = OnceLock::new();
    let mut entries = REGISTRY
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|_| StoreError::Poisoned)?;
    if let Some(e) = entries.iter_mut().find(|e| &e.root == root) {
        if let Some(identity) = journal {
            if e.journal.as_ref().is_some_and(|old| old != identity) {
                e.poison.store(true, Ordering::Release)
            } else {
                e.journal = Some(identity.clone())
            }
        }
        if e.poison.load(Ordering::Acquire) {
            return Err(StoreError::Poisoned);
        }
        return Ok(e.poison.clone());
    }
    // 不淘汰已知失败；容量耗尽必须拒绝新身份。
    if entries.len() >= 1024 {
        return Err(StoreError::Capacity);
    }
    let poison = Arc::new(AtomicBool::new(false));
    entries.push(RegistryEntry {
        root: root.clone(),
        journal: journal.cloned(),
        poison: poison.clone(),
    });
    Ok(poison)
}
fn open_journal(
    root: &Arc<PrivateDirectory>,
    user: &CurrentUser,
    create: bool,
) -> Result<PinnedFile> {
    let name = ComponentName::new(OsStr::new(JOURNAL)).map_err(io_error)?;
    let descriptor = user.descriptor(false).map_err(io_error)?;
    let file = root
        .directory()
        .open_relative(
            &name,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ,
            if create { FILE_CREATE } else { FILE_OPEN },
            false,
            if create { Some(&descriptor) } else { None },
        )
        .map_err(io_error)?;
    user.verify_private_file(handle(&file), false)
        .map_err(io_error)?;
    let file = PinnedFile::from_file(root.directory().clone(), name, file).map_err(io_error)?;
    if create {
        file.file.sync_all().map_err(io_error)?
    }
    file.verify().map_err(io_error)?;
    Ok(file)
}
fn read_bounded(file: &PinnedFile, max: usize) -> Result<Vec<u8>> {
    file.verify().map_err(io_error)?;
    let mut reader = &file.file;
    reader.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut bytes = Vec::new();
    reader
        .take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    file.verify().map_err(io_error)?;
    if bytes.len() > max {
        return Err(StoreError::Unknown);
    }
    Ok(bytes)
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

#[cfg(test)]
thread_local! {static BEFORE_CONTROL:std::cell::RefCell<Option<Box<dyn FnOnce()>>>=std::cell::RefCell::new(None);}
#[cfg(test)]
pub(crate) fn test_before_control(action: impl FnOnce() + 'static) {
    BEFORE_CONTROL.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(Box::new(action))
    })
}
