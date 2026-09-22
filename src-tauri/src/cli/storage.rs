//! Revision-checked workspace writes. Never writes native CLI configuration.
#![allow(dead_code)]

use super::profiles::{error, Profile};
use super::types::{SafeError, WireU64};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceDocument {
    pub(crate) schema_version: u32,
    pub(crate) revision: WireU64,
    pub(crate) profiles: BTreeMap<String, Profile>,
    #[serde(flatten)]
    pub(crate) extra: Map<String, Value>,
}

impl Default for WorkspaceDocument {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: WireU64::parse("0").expect("canonical zero"),
            profiles: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Patch {
    Create {
        profile: Profile,
    },
    Update {
        id: String,
        changes: Map<String, Value>,
    },
    Delete {
        id: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceRepository {
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStage {
    BeforeSync,
    BeforeReplace,
    AfterReplace,
}

impl WorkspaceRepository {
    pub(crate) fn open(path: PathBuf) -> Result<Self, SafeError> {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(SafeError::invalid("workspacePath"));
        }
        Ok(Self { path })
    }

    pub(crate) fn production() -> Result<Self, SafeError> {
        let home = dirs::home_dir().ok_or_else(|| error("HOME_UNAVAILABLE"))?;
        Self::open(home.join(".cc-box").join("cli-workspace.v1.json"))
    }

    fn lock(&self) -> Result<File, SafeError> {
        let parent = self.path.parent().ok_or_else(|| error("INVALID_PATH"))?;
        fs::create_dir_all(parent).map_err(|_| error("STORAGE_IO"))?;
        let name = self.path.file_name().ok_or_else(|| error("INVALID_PATH"))?;
        let mut lock_name = name.to_os_string();
        lock_name.push(".lock");
        let lock_path = parent.join(lock_name);
        check_regular_or_missing(&lock_path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(lock_path).map_err(|_| error("STORAGE_IO"))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(TryLockError::WouldBlock) => return Err(error("STORAGE_BUSY")),
                Err(TryLockError::Error(_)) => return Err(error("STORAGE_IO")),
            }
        }
    }

    pub(crate) fn read(&self) -> Result<WorkspaceDocument, SafeError> {
        let _lock = self.lock()?;
        self.read_locked()
    }

    pub(crate) fn get_profile(&self, id: &str) -> Result<Profile, SafeError> {
        self.read()?
            .profiles
            .get(id)
            .cloned()
            .ok_or_else(|| error("PROFILE_NOT_FOUND"))
    }

    fn read_locked(&self) -> Result<WorkspaceDocument, SafeError> {
        check_regular_or_missing(&self.path)?;
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WorkspaceDocument::default())
            }
            Err(_) => return Err(error("STORAGE_IO")),
        };
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error("STORAGE_IO"))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(error("WORKSPACE_TOO_LARGE"));
        }
        let raw: Value = serde_json::from_slice(&bytes).map_err(|_| error("WORKSPACE_INVALID"))?;
        if raw.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
            return Err(error("UNSUPPORTED_SCHEMA"));
        }
        let document: WorkspaceDocument =
            serde_json::from_value(raw).map_err(|_| error("WORKSPACE_INVALID"))?;
        for (id, profile) in &document.profiles {
            if id != &profile.id || profile.revision.get() > document.revision.get() {
                return Err(error("WORKSPACE_INVALID"));
            }
            profile.validate().map_err(|_| error("WORKSPACE_INVALID"))?;
        }
        Ok(document)
    }

    pub(crate) fn apply(
        &self,
        expected_revision: WireU64,
        patch: Patch,
    ) -> Result<WorkspaceDocument, SafeError> {
        self.apply_observed(expected_revision, patch, |_| Ok(()))
    }

    #[cfg(test)]
    pub(crate) fn apply_with_fault(
        &self,
        expected_revision: WireU64,
        patch: Patch,
        at: WriteStage,
    ) -> Result<WorkspaceDocument, SafeError> {
        self.apply_observed(expected_revision, patch, |stage| {
            if stage == at {
                Err(std::io::Error::other("synthetic fault"))
            } else {
                Ok(())
            }
        })
    }

    fn apply_observed(
        &self,
        expected_revision: WireU64,
        patch: Patch,
        observe: impl Fn(WriteStage) -> std::io::Result<()>,
    ) -> Result<WorkspaceDocument, SafeError> {
        let _lock = self.lock()?;
        let mut document = self.read_locked()?;
        if document.revision != expected_revision {
            return Err(error("REVISION_CONFLICT"));
        }
        let next = document
            .revision
            .get()
            .checked_add(1)
            .ok_or_else(|| error("REVISION_EXHAUSTED"))?;
        let next = WireU64::parse(&next.to_string())?;
        match patch {
            Patch::Create { mut profile } => {
                profile.validate()?;
                if profile.revision.get() != 0 {
                    return Err(SafeError::invalid("profile.revision"));
                }
                if document.profiles.contains_key(&profile.id) {
                    return Err(error("PROFILE_EXISTS"));
                }
                profile.revision = next;
                document.profiles.insert(profile.id.clone(), profile);
            }
            Patch::Update { id, changes } => {
                let old = document
                    .profiles
                    .get(&id)
                    .ok_or_else(|| error("PROFILE_NOT_FOUND"))?;
                let mut profile = old.patched(&changes)?;
                profile.revision = next;
                document.profiles.insert(id, profile);
            }
            Patch::Delete { id } => {
                if document.profiles.remove(&id).is_none() {
                    return Err(error("PROFILE_NOT_FOUND"));
                }
            }
        }
        document.revision = next;
        let bytes = serde_json::to_vec_pretty(&document).map_err(|_| error("SERIALIZE_FAILED"))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(error("WORKSPACE_TOO_LARGE"));
        }
        self.write_atomic(&bytes, observe)?;
        Ok(document)
    }

    fn write_atomic(
        &self,
        bytes: &[u8],
        observe: impl Fn(WriteStage) -> std::io::Result<()>,
    ) -> Result<(), SafeError> {
        let parent = self.path.parent().ok_or_else(|| error("INVALID_PATH"))?;
        let temporary = parent.join(format!(".cli-workspace-{}.tmp", Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|_| error("STORAGE_IO"))?;
        let _cleanup = TemporaryPath(temporary.clone());
        file.write_all(bytes).map_err(|_| error("STORAGE_IO"))?;
        observe(WriteStage::BeforeSync).map_err(|_| error("STORAGE_IO"))?;
        file.sync_all().map_err(|_| error("STORAGE_IO"))?;
        drop(file);
        observe(WriteStage::BeforeReplace).map_err(|_| error("STORAGE_IO"))?;
        check_regular_or_missing(&self.path)?;
        replace(&temporary, &self.path).map_err(|_| error("COMMIT_STATE_UNKNOWN"))?;
        observe(WriteStage::AfterReplace).map_err(|_| error("COMMIT_STATE_UNKNOWN"))?;
        #[cfg(unix)]
        File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| error("COMMIT_STATE_UNKNOWN"))?;
        Ok(())
    }
}

struct TemporaryPath(PathBuf);

impl Drop for TemporaryPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn check_regular_or_missing(path: &Path) -> Result<(), SafeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(error("UNSAFE_WORKSPACE_PATH")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(error("STORAGE_IO")),
    }
}

#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS};
    use windows_core::PCWSTR;
    if !to.exists() {
        return fs::rename(from, to);
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both NUL-terminated buffers live until the synchronous call returns.
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
    .map_err(std::io::Error::other)
}
