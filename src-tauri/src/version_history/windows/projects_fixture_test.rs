#![allow(non_snake_case)]
//! 仅测试进程可安装的持有句柄状态仓库；不向正式恢复 API 增加覆盖权限。
use super::*;
use std::{io::Write, path::PathBuf, sync::OnceLock};
static STORE: OnceLock<FixtureStore> = OnceLock::new();
static REQUIRED: OnceLock<()> = OnceLock::new();

struct FixtureStore {
    root: Arc<Directory>,
    lock: PinnedFile,
    path: PathBuf,
}
fn name(value: &str) -> io::Result<ComponentName> {
    ComponentName::new(OsStr::new(value))
}
fn observe_worker(required: &OnceLock<()>, mode: Option<&OsStr>) -> bool {
    if mode.is_some() {
        required.get_or_init(|| ());
    }
    matches!(mode.and_then(OsStr::to_str), Some("native" | "no-native"))
}
pub(crate) fn active() -> bool {
    observe_worker(
        &REQUIRED,
        std::env::var_os("CC_DESK_STAGE11_WORKER").as_deref(),
    );
    STORE.get().is_some() || REQUIRED.get().is_some()
}
fn installed() -> io::Result<&'static FixtureStore> {
    STORE
        .get()
        .ok_or_else(|| blocked("isolated projects capability is not installed"))
}
impl FixtureStore {
    fn create(root: Arc<Directory>) -> io::Result<Self> {
        root.recheck()?;
        if !root.read_children(1)?.is_empty() {
            return Err(blocked("isolated projects directory must be empty"));
        }
        let lock_name = name("projects.json.lock")?;
        let lock = root.open_relative(
            &lock_name,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            false,
            None,
        )?;
        let lock = PinnedFile::from_file(root.clone(), lock_name, lock)?;
        let path = PathBuf::from(root.path()?);
        Ok(Self { root, lock, path })
    }
    fn check(&self, path: &Path, leaf: &str) -> io::Result<()> {
        self.root.recheck()?;
        self.lock.verify()?;
        if path != self.path.join(leaf) {
            return Err(blocked("path outside isolated projects capability"));
        }
        Ok(())
    }
    fn open_lock(&self) -> io::Result<File> {
        self.lock.verify()?;
        let held = self.root.open_relative(
            &name("projects.json.lock")?,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            false,
            None,
        )?;
        let held = PinnedFile::from_file(self.root.clone(), name("projects.json.lock")?, held)?;
        if held.identity() != self.lock.identity() {
            return Err(blocked("isolated lock identity changed"));
        }
        Ok(held.file)
    }
    fn read(&self) -> io::Result<Option<Vec<u8>>> {
        match self
            .root
            .open_file(name("projects.json")?, FileAccess::Read)
        {
            Ok(held) => {
                let bytes = held.read_prefix(4 * 1024 * 1024 + 1)?;
                if bytes.len() > 4 * 1024 * 1024 {
                    return Err(blocked("isolated projects size limit"));
                }
                Ok(Some(bytes))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        self.write_with(bytes, |_| Ok(()))
    }
    fn write_with(
        &self,
        bytes: &[u8],
        before_publish: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<()> {
        self.write_with_probes(bytes, before_publish, || Ok(()))
    }
    fn write_with_probes(
        &self,
        bytes: &[u8],
        before_publish: impl FnOnce(&Path) -> io::Result<()>,
        after_publish: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(blocked("isolated projects size limit"));
        }
        self.root.recheck()?;
        let temporary = name(&format!("projects-{}.tmp", uuid::Uuid::new_v4()))?;
        let file = self.root.open_relative(
            &temporary,
            FILE_READ_DATA
                | FILE_WRITE_DATA
                | FILE_READ_ATTRIBUTES
                | READ_CONTROL
                | DELETE
                | SYNCHRONIZE,
            FILE_SHARE_MODE(0),
            FILE_CREATE,
            false,
            None,
        )?;
        let mut held = PinnedFile::from_file(self.root.clone(), temporary, file)?;
        held.file.write_all(bytes)?;
        held.file.sync_all()?;
        held.verify()?;
        before_publish(Path::new(&held.path()?))?;
        held.verify()?;
        let destination = name("projects.json")?;
        let length = size_of::<FILE_RENAME_INFORMATION>() + destination.0.len() * 2;
        let mut buffer = vec![0usize; length.div_ceil(size_of::<usize>())];
        let mut status = IO_STATUS_BLOCK::default();
        // 只允许测试仓库固定叶子；源对象始终是本次 FILE_CREATE 后持有的原始句柄。
        unsafe {
            let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
            (*information).Anonymous.ReplaceIfExists = true;
            (*information).RootDirectory = self.root.raw();
            (*information).FileNameLength = (destination.0.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(
                destination.0.as_ptr(),
                (*information).FileName.as_mut_ptr(),
                destination.0.len(),
            );
            let result = NtSetInformationFile(
                handle(&held.file),
                &mut status,
                information.cast(),
                length as u32,
                FileRenameInformation,
            );
            if result.0 != 0 || status.Anonymous.Status.0 != 0 {
                return Err(io::Error::other(
                    "isolated atomic replace outcome unverified",
                ));
            }
        }
        held.name = destination;
        after_publish()?;
        held.verify()?;
        if held.read_prefix(bytes.len() + 1)? != bytes {
            return Err(blocked("isolated published content changed"));
        }
        Ok(())
    }
}
pub(crate) fn install(root: Arc<Directory>) -> io::Result<()> {
    if !observe_worker(
        &REQUIRED,
        std::env::var_os("CC_DESK_STAGE11_WORKER").as_deref(),
    ) {
        return Err(blocked(
            "only owned WebView worker may install projects fixture",
        ));
    }
    if STORE.get().is_some() {
        return Err(blocked("isolated projects capability already installed"));
    }
    STORE
        .set(FixtureStore::create(root)?)
        .map_err(|_| blocked("isolated projects capability already installed"))
}
pub(crate) fn paths() -> Option<io::Result<(PathBuf, PathBuf)>> {
    active().then(|| {
        let store = installed()?;
        store.root.recheck()?;
        store.lock.verify()?;
        Ok((
            store.path.join("projects.json"),
            store.path.join("projects.json.lock"),
        ))
    })
}
pub(crate) fn open_lock(path: &Path) -> Option<io::Result<File>> {
    active().then(|| {
        let store = installed()?;
        store.check(path, "projects.json.lock")?;
        store.open_lock()
    })
}
pub(crate) fn read_projects(path: &Path) -> Option<io::Result<Option<Vec<u8>>>> {
    active().then(|| {
        let store = installed()?;
        store.check(path, "projects.json")?;
        store.read()
    })
}
pub(crate) fn write_projects(path: &Path, bytes: &[u8]) -> Option<io::Result<()>> {
    active().then(|| {
        let store = installed()?;
        store.check(path, "projects.json")?;
        store.write(bytes)
    })
}
#[test]
fn IsolatedProjects_CapabilityRoundtripAndIndependentLock_001() {
    let temp = tempfile::tempdir().unwrap();
    let store = FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).unwrap();
    assert!(store
        .check(&temp.path().join("outside.json"), "projects.json")
        .is_err());
    let first = store.open_lock().unwrap();
    let second = store.open_lock().unwrap();
    first.try_lock().unwrap();
    assert!(second.try_lock().is_err());
    first.unlock().unwrap();
    second.try_lock().unwrap();
    second.unlock().unwrap();
    assert!(store.read().unwrap().is_none());
    store.write(b"one").unwrap();
    store.write(b"two").unwrap();
    assert_eq!(store.read().unwrap().unwrap(), b"two");
    assert!(std::fs::rename(temp.path(), temp.path().with_extension("moved")).is_err());
}
#[test]
fn IsolatedProjects_RejectsHardLinkedStateWithoutReading_002() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).unwrap();
    let target = outside.path().join("outside");
    std::fs::write(&target, b"unchanged").unwrap();
    std::fs::hard_link(&target, temp.path().join("projects.json")).unwrap();
    assert!(store.read().is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"unchanged");
}
#[test]
fn IsolatedProjects_PublicationCannotFollowReplacedLeaf_003() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).unwrap();
    let outside_file = outside.path().join("control");
    std::fs::write(&outside_file, b"outside unchanged").unwrap();
    store.write(b"old").unwrap();
    store
        .write_with(b"new", |temporary| {
            assert!(std::fs::rename(temporary, temporary.with_extension("stolen")).is_err());
            std::fs::rename(
                temp.path().join("projects.json"),
                temp.path().join("old.keep"),
            )?;
            std::fs::hard_link(&outside_file, temp.path().join("projects.json"))?;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.read().unwrap().unwrap(), b"new");
    assert_eq!(std::fs::read(outside_file).unwrap(), b"outside unchanged");
}
#[test]
fn IsolatedProjects_FailedPublicationNeverFallsBack_004() {
    let temp = tempfile::tempdir().unwrap();
    let store = FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).unwrap();
    store.write(b"old").unwrap();
    assert!(store
        .write_with(b"new", |_| Err(io::Error::other(
            "injected before publication"
        )))
        .is_err());
    assert_eq!(store.read().unwrap().unwrap(), b"old");
    assert!(store.root.read_children(16).unwrap().len() >= 3);
}
#[test]
fn IsolatedProjects_LockCannotBeReplacedOrAliased_005() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).unwrap();
    assert!(std::fs::rename(
        temp.path().join("projects.json.lock"),
        temp.path().join("old-lock")
    )
    .is_err());
    if std::fs::hard_link(
        temp.path().join("projects.json.lock"),
        outside.path().join("alias"),
    )
    .is_ok()
    {
        assert!(store.open_lock().is_err());
    }
}
#[test]
fn IsolatedProjects_ReparseLeavesCannotReachOutside_006() {
    use std::os::windows::process::CommandExt;
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .creation_flags(0x08000000)
            .output()
            .unwrap()
            .status
            .success()
    }
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).unwrap();
    let control = outside.path().join("control");
    std::fs::write(&control, b"outside unchanged").unwrap();
    assert!(junction(&temp.path().join("projects.json"), outside.path()));
    assert!(store.read().is_err());
    assert!(!junction(
        &temp.path().join("projects.json.lock"),
        outside.path()
    ));
    assert!(store
        .write_with(b"new", |temporary| {
            assert!(!junction(temporary, outside.path()));
            Err(io::Error::other("keep reparse fixture unchanged"))
        })
        .is_err());
    assert_eq!(std::fs::read(control).unwrap(), b"outside unchanged");
}

#[test]
fn IsolatedProjects_FailedInstallCannotClearRequiredMode_007() {
    let required = OnceLock::new();
    assert!(observe_worker(&required, Some(OsStr::new("no-native"))));
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("existing"), b"synthetic").unwrap();
    assert!(FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).is_err());
    assert!(!observe_worker(&required, None));
    assert!(required.get().is_some());
}
#[test]
fn IsolatedProjects_PublishedFailureDoesNotRollbackOrReplay_008() {
    let temp = tempfile::tempdir().unwrap();
    let store = FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).unwrap();
    store.write(b"old").unwrap();
    let issued = std::cell::Cell::new(0);
    assert!(store
        .write_with_probes(
            b"new",
            |_| {
                issued.set(issued.get() + 1);
                Ok(())
            },
            || Err(io::Error::other("injected after publication"))
        )
        .is_err());
    assert_eq!(issued.get(), 1);
    assert_eq!(store.read().unwrap().unwrap(), b"new");
    assert_eq!(store.root.read_children(16).unwrap().len(), 2);
}
#[test]
fn IsolatedProjects_ReparsePublicationCannotReachOutside_009() {
    use std::os::windows::process::CommandExt;
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = FixtureStore::create(Directory::open_absolute(temp.path()).unwrap()).unwrap();
    let control = outside.path().join("control");
    std::fs::write(&control, b"outside unchanged").unwrap();
    store.write(b"old").unwrap();
    let result = store.write_with(b"new", |_| {
        std::fs::rename(
            temp.path().join("projects.json"),
            temp.path().join("old.keep"),
        )?;
        assert!(std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(temp.path().join("projects.json"))
            .arg(outside.path())
            .creation_flags(0x08000000)
            .output()
            .unwrap()
            .status
            .success());
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(control).unwrap(), b"outside unchanged");
    assert_eq!(std::fs::read(temp.path().join("old.keep")).unwrap(), b"old");
    assert!(store.read().is_err());
}
