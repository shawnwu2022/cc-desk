//! Windows local metadata containment. Backend directory must be private/trusted;
//! this is not a sandbox against another process running as the Host's user.
use crate::journal::JournalError;
use std::{fs::File, path::Path};
pub struct Guards {
    _files: Vec<File>,
}
#[cfg(windows)]
fn open_directory(p: &Path) -> std::io::Result<File> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows::{
        Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        core::PCWSTR,
    };
    let wide: Vec<u16> = p
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let h = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(|e| std::io::Error::from_raw_os_error((e.code().0 as u32 & 0xffff) as i32))?;
    Ok(unsafe { File::from_raw_handle(h.0) })
}
#[cfg(all(test, windows))]
mod diagnostics {
    #[test]
    fn directory_guard_supports_owned_temporary_database() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("journal.sqlite");
        let mut current = std::path::PathBuf::new();
        let components: Vec<_> = p.components().collect();
        for (i, c) in components.iter().enumerate() {
            current.push(c.as_os_str());
            if i == 0 || i == components.len() - 1 {
                continue;
            }
            match super::open_directory(&current) {
                Ok(_) => eprintln!("TEST_GUARD_DIR index={i} ok"),
                Err(e) => panic!("TEST_GUARD_DIR index={i} os={:?}", e.raw_os_error()),
            }
        }
        assert!(super::acquire(&p).is_ok());
    }
}
#[cfg(windows)]
pub fn acquire(path: &Path) -> Result<(Guards, bool), JournalError> {
    use std::{
        fs::OpenOptions,
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        },
        path::{Component, Prefix},
    };
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };
    const REPARSE: u32 = 0x400;
    const FLAGS: u32 = 0x00200000 | 0x02000000;
    fn open(p: &Path, write: bool, new: bool) -> Result<File, std::io::Error> {
        OpenOptions::new()
            .read(true)
            .write(write)
            .create_new(new)
            .share_mode(3)
            .custom_flags(FLAGS)
            .open(p)
    }
    fn check(f: &File, dir: bool) -> Result<(), JournalError> {
        let m = f.metadata().map_err(|_| JournalError::Storage)?;
        if m.file_attributes() & REPARSE != 0 || m.is_dir() != dir {
            return Err(JournalError::Bounds);
        }
        if !dir {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            unsafe { GetFileInformationByHandle(HANDLE(f.as_raw_handle()), &mut info) }
                .map_err(|_| JournalError::Storage)?;
            if info.nNumberOfLinks != 1 {
                return Err(JournalError::Bounds);
            }
        }
        Ok(())
    }
    let components: Vec<_> = path.components().collect();
    if components.len() < 3
        || components.len() > 64
        || !matches!(components[0],Component::Prefix(p) if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
        || !matches!(components[1], Component::RootDir)
    {
        return Err(JournalError::Bounds);
    }
    let mut files = Vec::with_capacity(components.len() + 3);
    let mut current = std::path::PathBuf::new();
    for (i, c) in components.iter().enumerate() {
        if let Component::Normal(s) = c {
            let text = s.to_str().ok_or(JournalError::Bounds)?;
            if text.ends_with(['.', ' ']) || text.contains(':') || text.contains('\0') {
                return Err(JournalError::Bounds);
            }
        } else if i > 1 {
            return Err(JournalError::Bounds);
        }
        current.push(c.as_os_str());
        if i == 1 {
            use std::os::windows::ffi::OsStrExt;
            let root: Vec<u16> = current
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            // WAL metadata is supported only on local fixed storage (DRIVE_FIXED=3).
            if unsafe {
                windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(
                    root.as_ptr(),
                ))
            } != 3
            {
                return Err(JournalError::Bounds);
            }
        }
        if i == 0 || i == components.len() - 1 {
            continue;
        }
        let f = open_directory(&current).map_err(|_| JournalError::Storage)?;
        check(&f, true)?;
        files.push(f);
    }
    let (db, created) = match open(path, true, true) {
        Ok(f) => (f, true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (
            open(path, false, false).map_err(|_| JournalError::Storage)?,
            false,
        ),
        Err(_) => return Err(JournalError::Storage),
    };
    check(&db, false)?;
    if db.metadata().map_err(|_| JournalError::Storage)?.len() > 64 * 1024 * 1024 {
        return Err(JournalError::Capacity);
    }
    if !created {
        use std::io::{Read, Seek, SeekFrom};
        let mut reader = db.try_clone().map_err(|_| JournalError::Storage)?;
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| JournalError::Schema)?;
        let mut h = [0u8; 100];
        reader
            .read_exact(&mut h)
            .map_err(|_| JournalError::Schema)?;
        if &h[..16] != b"SQLite format 3\0"
            || u32::from_be_bytes(h[68..72].try_into().unwrap()) != 0x43434452
        {
            return Err(JournalError::Schema);
        }
    }
    files.push(db);
    // Pin pre-existing sidecars too. Missing sidecars are created by SQLite in
    // the trusted backend directory; no untrusted path can be supplied remotely.
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match open(Path::new(&name), false, false) {
            Ok(f) => {
                check(&f, false)?;
                let limit = match suffix {
                    "-wal" => 16 * 1024 * 1024,
                    "-shm" => 1024 * 1024,
                    _ => 64 * 1024 * 1024,
                };
                if f.metadata().map_err(|_| JournalError::Storage)?.len() > limit {
                    return Err(JournalError::Capacity);
                }
                files.push(f);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(JournalError::Storage),
        }
    }
    Ok((Guards { _files: files }, created))
}
#[cfg(not(windows))]
pub fn acquire(_path: &Path) -> Result<(Guards, bool), JournalError> {
    Err(JournalError::UnsupportedPlatform)
}
#[cfg(all(test, not(windows)))]
mod unsupported {
    #[test]
    fn unsupported_platform_never_creates_database() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("must-not-create.sqlite");
        assert!(matches!(
            crate::journal::Journal::open(&p),
            Err(crate::journal::JournalError::UnsupportedPlatform)
        ));
        assert!(!p.exists());
    }
}
