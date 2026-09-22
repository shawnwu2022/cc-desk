//! Backend path identity. Display spelling is never an authorization or deduplication key.
use super::profiles::error;
use super::types::SafeError;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct ResolvedPathKey {
    pub(crate) key: String,
    pub(crate) selected_path: PathBuf,
    pub(crate) canonical_path: Option<PathBuf>,
}

pub(crate) fn validate_selected_path(path: &Path) -> Result<(), SafeError> {
    let text = path
        .to_str()
        .ok_or_else(|| SafeError::invalid("selectedPath"))?;
    if !path.is_absolute() || text.len() > 32768 || text.contains('\0') {
        return Err(SafeError::invalid("selectedPath"));
    }
    Ok(())
}

pub(crate) fn resolve_path_key(path: &Path) -> Result<ResolvedPathKey, SafeError> {
    validate_selected_path(path)?;
    let selected_path = path.to_path_buf();
    let metadata = match fs::metadata(path) {
        Ok(value) if !value.is_dir() => return Err(error("PROJECT_NOT_DIRECTORY")),
        Ok(value) => Some(value),
        // Unavailable paths remain explicit records; no lexical alias guesses.
        Err(_) => None,
    };
    let identity = metadata
        .as_ref()
        .and_then(|value| directory_identity(path, value));
    let canonical_path = identity.as_ref().and_then(|key| {
        let canonical = fs::canonicalize(path).ok()?;
        validate_selected_path(&canonical).ok()?;
        let metadata = fs::metadata(&canonical).ok()?;
        if !metadata.is_dir() || directory_identity(&canonical, &metadata).as_ref() != Some(key) {
            return None;
        }
        Some(canonical)
    });
    let key = identity.unwrap_or_else(|| {
        serde_json::to_string(&["local", "selected", path.to_str().expect("validated UTF-8")])
            .expect("string array serialization")
    });
    Ok(ResolvedPathKey {
        key,
        selected_path,
        canonical_path,
    })
}

pub(crate) fn is_verified_key(key: &str) -> bool {
    key.starts_with("local:unix:") || key.starts_with("local:windows:")
}

#[cfg(unix)]
fn directory_identity(_path: &Path, metadata: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    Some(format!("local:unix:{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn directory_identity(path: &Path, _metadata: &fs::Metadata) -> Option<String> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let file = OpenOptions::new()
        .read(true)
        .access_mode(0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_dir() {
        return None;
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // The directory handle and writable information buffer live across this synchronous call.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok()?;
    if info.nFileIndexHigh == 0 && info.nFileIndexLow == 0 {
        return None;
    }
    Some(format!(
        "local:windows:{}:{}:{}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    ))
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_path: &Path, _metadata: &fs::Metadata) -> Option<String> {
    None
}
