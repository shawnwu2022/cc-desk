//! Read-only directory capabilities. Ambient paths are accepted only at backend root admission.
//! Every descendant open is relative to the retained cap-std handle, never path.join + fs::read.
use cap_std::fs::{Dir, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) type ReadResult<T> = Result<T, &'static str>;
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub file_bytes: usize,
    pub total_bytes: usize,
    pub entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self { file_bytes: 2 * 1024 * 1024, total_bytes: 16 * 1024 * 1024, entries: 4096 }
    }
}
pub(crate) struct Budget {
    file_bytes: usize,
    bytes_left: usize,
    entries_left: usize,
    started: Instant,
}
impl Budget {
    pub(crate) fn new(limits: Limits) -> Self {
        let maximum = Limits::default();
        Self {
            file_bytes: limits.file_bytes.min(maximum.file_bytes),
            bytes_left: limits.total_bytes.min(maximum.total_bytes),
            entries_left: limits.entries.min(maximum.entries),
            started: Instant::now(),
        }
    }
    pub(crate) fn checkpoint(&self) -> ReadResult<()> {
        // Cooperative deadline between operations; not a promise to interrupt OS filesystem I/O.
        if self.started.elapsed() > Duration::from_secs(5) { return Err("SOURCE_BUDGET_EXCEEDED"); }
        Ok(())
    }
    fn entry(&mut self) -> ReadResult<()> {
        self.checkpoint()?;
        self.entries_left = self.entries_left.checked_sub(1).ok_or("SOURCE_TOO_MANY_ENTRIES")?;
        Ok(())
    }
}
pub(crate) struct Root {
    dir: Dir,
    selected: PathBuf,
    identity: String,
}
impl std::fmt::Debug for Root {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Root(<redacted>)") }
}
pub(crate) struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
}
impl Root {
    pub(crate) fn open(path: &Path) -> ReadResult<Self> {
        let text = path.to_str().ok_or("SOURCE_INVALID_TEXT")?;
        if !path.is_absolute() || text.len() > 32768 || text.contains('\0') { return Err("SCOPE_UNKNOWN"); }
        let dir = Dir::open_ambient_dir(path, cap_std::ambient_authority()).map_err(|_| "SCOPE_UNKNOWN")?;
        let identity = directory_key(&dir)?;
        let root = Self { dir, selected: path.to_owned(), identity };
        root.current()?;
        Ok(root)
    }
    pub(crate) fn key(&self) -> &str { &self.identity }
    pub(crate) fn current(&self) -> ReadResult<()> {
        let now = Dir::open_ambient_dir(&self.selected, cap_std::ambient_authority()).map_err(|_| "SOURCE_CHANGED")?;
        if directory_key(&now)? != self.identity { return Err("SOURCE_CHANGED"); }
        Ok(())
    }
    pub(crate) fn read(&self, path: &Path, budget: &mut Budget) -> ReadResult<Option<Vec<u8>>> {
        relative(path, false)?;
        budget.entry()?;
        self.current()?;
        let metadata = match self.dir.symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(read_error(e)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() { return Err("SOURCE_NOT_REGULAR"); }
        let cap = budget.file_bytes.min(budget.bytes_left);
        if metadata.len() > cap as u64 { return Err("SOURCE_TOO_LARGE"); }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            // A file swapped for a FIFO must not indefinitely block the reader.
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = self.dir.open_with(path, &options).map_err(read_error)?;
        let before = file.metadata().map_err(read_error)?;
        if !before.is_file() { return Err("SOURCE_NOT_REGULAR"); }
        if before.len() > cap as u64 { return Err("SOURCE_TOO_LARGE"); }
        let mut bytes = Vec::new();
        (&file).take(cap as u64 + 1).read_to_end(&mut bytes).map_err(read_error)?;
        if bytes.len() > cap { return Err("SOURCE_TOO_LARGE"); }
        let after = file.metadata().map_err(read_error)?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() || bytes.len() as u64 != after.len() {
            return Err("SOURCE_CHANGED");
        }
        budget.bytes_left -= bytes.len();
        budget.checkpoint()?;
        self.current()?;
        Ok(Some(bytes))
    }
    pub(crate) fn list(&self, path: &Path, budget: &mut Budget) -> ReadResult<Vec<Entry>> {
        relative(path, true)?;
        budget.checkpoint()?;
        self.current()?;
        let path = if path.as_os_str().is_empty() { Path::new(".") } else { path };
        let entries = match self.dir.read_dir(path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(read_error(e)),
        };
        let mut result = Vec::new();
        for entry in entries {
            budget.entry()?;
            let entry = entry.map_err(read_error)?;
            let name = entry.file_name().into_string().map_err(|_| "SOURCE_INVALID_TEXT")?;
            relative(Path::new(&name), false)?;
            let kind = entry.file_type().map_err(read_error)?;
            result.push(Entry { name, is_dir: kind.is_dir() && !kind.is_symlink(), is_file: kind.is_file() && !kind.is_symlink() });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        self.current()?;
        Ok(result)
    }
}
fn relative(path: &Path, allow_empty: bool) -> ReadResult<()> {
    let text = path.to_str().ok_or("SOURCE_INVALID_TEXT")?;
    if text.len() > 4096 || text.contains(['\0', ':', '\\']) || (!allow_empty && text.is_empty())
        || path.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("SOURCE_PATH_REJECTED");
    }
    Ok(())
}
fn read_error(error: io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "SOURCE_READ_FORBIDDEN",
        _ => "SOURCE_READ_FAILED",
    }
}
fn directory_key(dir: &Dir) -> ReadResult<String> {
    let file = dir.try_clone().map_err(read_error)?.into_std_file();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata().map_err(read_error)?;
        Ok(format!("local:unix:{}:{}", m.dev(), m.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // The owned directory handle and output buffer remain live for this synchronous call.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.map_err(|_| "SCOPE_UNKNOWN")?;
        if info.nFileIndexHigh == 0 && info.nFileIndexLow == 0 { return Err("SCOPE_UNKNOWN"); }
        Ok(format!("local:windows:{}:{}:{}", info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
    }
    #[cfg(not(any(unix, windows)))]
    { let _ = file; Err("SOURCE_UNSUPPORTED") }
}
#[cfg(test)]
#[path = "../../tests/native_cli_scoped_fs.rs"]
mod tests;
