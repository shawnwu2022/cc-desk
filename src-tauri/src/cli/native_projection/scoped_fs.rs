//! Read-only directory capabilities. Ambient paths are accepted only at backend root admission.
//! Every descendant open is relative to the retained cap-std handle, never path.join + fs::read.
use cap_std::fs::{Dir, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) type ReadResult<T> = Result<T, &'static str>;
type HistoryObserver<'a> = dyn FnMut(&[u8]) -> ReadResult<bool> + 'a;
pub(crate) type TitleTail = (Vec<u8>, Vec<u8>, bool);
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub file_bytes: usize,
    pub total_bytes: usize,
    pub entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            file_bytes: 2 * 1024 * 1024,
            total_bytes: 16 * 1024 * 1024,
            entries: 4096,
        }
    }
}
pub(crate) struct Budget {
    file_bytes: usize,
    bytes_left: usize,
    entries_left: usize,
    started: Instant,
    history_metadata_incomplete: bool,
    history_read_failures: std::collections::BTreeSet<&'static str>,
}
impl Budget {
    pub(crate) fn new(limits: Limits) -> Self {
        let maximum = Limits::default();
        Self {
            file_bytes: limits.file_bytes.min(maximum.file_bytes),
            bytes_left: limits.total_bytes.min(maximum.total_bytes),
            entries_left: limits.entries.min(maximum.entries),
            started: Instant::now(),
            history_metadata_incomplete: false,
            history_read_failures: Default::default(),
        }
    }
    pub(crate) fn checkpoint(&self) -> ReadResult<()> {
        // Cooperative deadline between operations; not a promise to interrupt OS filesystem I/O.
        if self.started.elapsed() > Duration::from_secs(5) {
            return Err("SOURCE_BUDGET_EXCEEDED");
        }
        Ok(())
    }
    pub(crate) fn history_metadata_incomplete(&self) -> bool {
        self.history_metadata_incomplete
    }
    pub(crate) fn unobserved_history_association(&mut self) {
        self.history_metadata_incomplete = true;
    }
    pub(crate) fn optional_title_available(&self, header_bytes: usize) -> bool {
        self.started.elapsed() < Duration::from_secs(4)
            && self.entries_left > 0
            && header_bytes.saturating_add(4096) <= self.bytes_left
            && header_bytes.saturating_mul(2).saturating_add(4096) <= self.file_bytes
    }
    pub(crate) fn history_read_failures(&self) -> Vec<&'static str> {
        self.history_read_failures.iter().copied().collect()
    }
    // Only content/read failures of an individual history entry are isolatable.
    // Scope, identity, permission, links, ambiguity and aggregate limits still fail closed.
    pub(crate) fn omit_history_entry(&mut self, code: &'static str) -> bool {
        if (code == "SOURCE_TOO_LARGE" && self.bytes_left == 0)
            || !matches!(
                code,
                "SOURCE_UNSUPPORTED"
                    | "SOURCE_INVALID"
                    | "SOURCE_INVALID_TEXT"
                    | "SOURCE_TOO_LARGE"
                    | "SOURCE_READ_FAILED"
            )
        {
            return false;
        }
        self.history_metadata_incomplete = true;
        self.history_read_failures.insert(code);
        true
    }
    fn entry(&mut self) -> ReadResult<()> {
        self.checkpoint()?;
        self.entries_left = self
            .entries_left
            .checked_sub(1)
            .ok_or("SOURCE_TOO_MANY_ENTRIES")?;
        Ok(())
    }
}
pub(crate) struct Root {
    dir: Dir,
    selected: PathBuf,
    identity: String,
}
impl std::fmt::Debug for Root {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Root(<redacted>)")
    }
}
pub(crate) struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
}
impl Root {
    pub(crate) fn open(path: &Path) -> ReadResult<Self> {
        let text = path.to_str().ok_or("SOURCE_INVALID_TEXT")?;
        if !path.is_absolute() || text.len() > 32768 || text.contains('\0') {
            return Err("SCOPE_UNKNOWN");
        }
        let dir = Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|_| "SCOPE_UNKNOWN")?;
        let identity = directory_key(&dir)?;
        let root = Self {
            dir,
            selected: path.to_owned(),
            identity,
        };
        root.current()?;
        Ok(root)
    }
    pub(crate) fn key(&self) -> &str {
        &self.identity
    }
    pub(crate) fn descendant(&self, path: &Path) -> ReadResult<String> {
        let path = path
            .strip_prefix(&self.selected)
            .map_err(|_| "SOURCE_PATH_REJECTED")?;
        let components = path
            .components()
            .map(|part| match part {
                Component::Normal(name) => name.to_str().ok_or("SOURCE_INVALID_TEXT"),
                _ => Err("SOURCE_PATH_REJECTED"),
            })
            .collect::<ReadResult<Vec<_>>>()?;
        let relative_path = components.join("/");
        relative(Path::new(&relative_path), false)?;
        Ok(relative_path)
    }

    pub(crate) fn current(&self) -> ReadResult<()> {
        let now = Dir::open_ambient_dir(&self.selected, cap_std::ambient_authority())
            .map_err(|_| "SOURCE_CHANGED")?;
        if directory_key(&now)? != self.identity {
            return Err("SOURCE_CHANGED");
        }
        Ok(())
    }
    pub(crate) fn read(&self, path: &Path, budget: &mut Budget) -> ReadResult<Option<Vec<u8>>> {
        Ok(self
            .read_bounded(path, budget, false, 64 * 1024, None)?
            .map(|(bytes, _)| bytes))
    }
    #[cfg(test)]
    pub(crate) fn history_prefix(
        &self,
        path: &Path,
        budget: &mut Budget,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        self.read_bounded(path, budget, true, 64 * 1024, None)
    }
    pub(crate) fn history_header(
        &self,
        path: &Path,
        budget: &mut Budget,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        // Codex stores identity/cwd in its first complete session_meta record.
        // Sample a bounded title only when it is in the same observed chunks.
        self.read_bounded(path, budget, true, 4 * 1024, None)
    }
    pub(crate) fn history_until(
        &self,
        path: &Path,
        budget: &mut Budget,
        observe: &mut HistoryObserver<'_>,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        // The observer sees charged bytes from one held file. It may ask for
        // another chunk but cannot expand the original file/aggregate caps.
        self.read_bounded(path, budget, true, 4 * 1024, Some(observe))
    }
    pub(crate) fn history_title_tail(
        &self,
        path: &Path,
        header_bytes: usize,
        budget: &mut Budget,
    ) -> ReadResult<Option<TitleTail>> {
        if !budget.optional_title_available(header_bytes) {
            return Ok(None);
        }
        relative(path, false)?;
        budget.entry()?;
        self.current()?;
        let optional_read_error = |error: io::Error| {
            if error.kind() == io::ErrorKind::NotFound {
                "SOURCE_CHANGED"
            } else {
                read_error(error)
            }
        };
        let metadata = self
            .dir
            .symlink_metadata(path)
            .map_err(optional_read_error)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = self
            .dir
            .open_with(path, &options)
            .map_err(optional_read_error)?;
        let before = file.metadata().map_err(read_error)?;
        if !before.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }
        if before.len() < header_bytes as u64 {
            return Err("SOURCE_CHANGED");
        }
        let mut header = Vec::new();
        let read = (&file).take(header_bytes as u64).read_to_end(&mut header);
        budget.bytes_left = budget.bytes_left.saturating_sub(header.len());
        read.map_err(read_error)?;
        budget.checkpoint()?;
        let tail_bytes = before.len().min(4096);
        (&file)
            .seek(SeekFrom::End(-(tail_bytes as i64)))
            .map_err(read_error)?;
        let mut tail = Vec::new();
        let read = (&file).take(tail_bytes).read_to_end(&mut tail);
        budget.bytes_left = budget.bytes_left.saturating_sub(tail.len());
        read.map_err(read_error)?;
        let after = file.metadata().map_err(read_error)?;
        if before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
            || header.len() != header_bytes
            || tail.len() as u64 != tail_bytes
        {
            return Err("SOURCE_CHANGED");
        }
        budget.checkpoint()?;
        self.current()?;
        Ok(Some((header, tail, before.len() > tail_bytes)))
    }
    fn read_bounded(
        &self,
        path: &Path,
        budget: &mut Budget,
        history_prefix: bool,
        chunk_bytes: usize,
        mut observe: Option<&mut HistoryObserver<'_>>,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        relative(path, false)?;
        budget.entry()?;
        self.current()?;
        let metadata = match self.dir.symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(read_error(e)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let cap = budget.file_bytes.min(budget.bytes_left);
        if (history_prefix && cap == 0) || (!history_prefix && metadata.len() > cap as u64) {
            return Err("SOURCE_TOO_LARGE");
        }
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
        if !before.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }
        if !history_prefix && before.len() > cap as u64 {
            return Err("SOURCE_TOO_LARGE");
        }
        let mut bytes = Vec::new();
        let mut limit = if history_prefix {
            cap.min(chunk_bytes)
        } else {
            cap + 1
        };
        let read = (&file).take(limit as u64).read_to_end(&mut bytes);
        // A failed read can already have consumed bytes. Debit before returning
        // so an omitted history entry cannot reset the aggregate I/O budget.
        budget.bytes_left = budget.bytes_left.saturating_sub(bytes.len());
        read.map_err(read_error)?;
        // Codex needs a complete first record; Claude may need to pass leading
        // metadata before observing a complete supported cwd record.
        let mut complete_record = if let Some(observe) = observe.as_mut() {
            observe(&bytes)?
        } else {
            history_prefix && bytes.contains(&b'\n')
        };
        while history_prefix && !complete_record && bytes.len() == limit && limit < cap {
            budget.checkpoint()?;
            let next = (cap - limit).min(chunk_bytes);
            let previous = bytes.len();
            let read = (&file).take(next as u64).read_to_end(&mut bytes);
            budget.bytes_left = budget.bytes_left.saturating_sub(bytes.len() - previous);
            read.map_err(read_error)?;
            // The earlier chunks have already been checked. A long first record
            // must not rescan the entire accumulated prefix on every extension.
            complete_record = if let Some(observe) = observe.as_mut() {
                observe(&bytes)?
            } else {
                bytes[previous..].contains(&b'\n')
            };
            limit += next;
        }
        if bytes.len() > cap {
            return Err("SOURCE_TOO_LARGE");
        }
        let after = file.metadata().map_err(read_error)?;
        let expected = if history_prefix {
            after.len().min(limit as u64)
        } else {
            after.len()
        };
        if before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
            || bytes.len() as u64 != expected
        {
            return Err("SOURCE_CHANGED");
        }
        let incomplete = bytes.len() as u64 != after.len();
        budget.history_metadata_incomplete |= history_prefix && incomplete;
        budget.checkpoint()?;
        self.current()?;
        Ok(Some((bytes, incomplete)))
    }
    pub(crate) fn list(&self, path: &Path, budget: &mut Budget) -> ReadResult<Vec<Entry>> {
        relative(path, true)?;
        budget.checkpoint()?;
        self.current()?;
        let path = if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        };
        let entries = match self.dir.read_dir(path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(read_error(e)),
        };
        let mut result = Vec::new();
        for entry in entries {
            budget.entry()?;
            let entry = entry.map_err(read_error)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "SOURCE_INVALID_TEXT")?;
            relative(Path::new(&name), false)?;
            let kind = entry.file_type().map_err(read_error)?;
            result.push(Entry {
                name,
                is_dir: kind.is_dir() && !kind.is_symlink(),
                is_file: kind.is_file() && !kind.is_symlink(),
            });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        self.current()?;
        Ok(result)
    }
}
fn relative(path: &Path, allow_empty: bool) -> ReadResult<()> {
    let text = path.to_str().ok_or("SOURCE_INVALID_TEXT")?;
    if text.len() > 4096
        || text.contains(['\0', ':', '\\'])
        || (!allow_empty && text.is_empty())
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
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
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // The owned directory handle and output buffer remain live for this synchronous call.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| "SCOPE_UNKNOWN")?;
        if info.nFileIndexHigh == 0 && info.nFileIndexLow == 0 {
            return Err("SCOPE_UNKNOWN");
        }
        Ok(format!(
            "local:windows:{}:{}:{}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err("SOURCE_UNSUPPORTED")
    }
}
#[cfg(test)]
#[path = "../../tests/native_cli_scoped_fs.rs"]
mod tests;
