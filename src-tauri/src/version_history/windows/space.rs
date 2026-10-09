//! Per-volume free-space admission from actual held inventories. This is an
//! observation, not an NTFS transaction or promise that unrelated users stop
//! consuming disk; every later copy phase performs its own fresh admission.
use super::{
    context::{HeldBundle, HeldContext},
    files::{Directory, PrivateDirectory},
    manager_process::launch_path,
    security::CurrentUser,
    win_error,
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        download::PreparedHandoff,
        journal::RootKind,
        payload_policy::PayloadAdmission,
        snapshot::{EntryType, ManifestEntry},
    },
};
use std::{os::windows::ffi::OsStrExt, sync::Arc};
use windows::{core::PCWSTR, Win32::Storage::FileSystem::GetDiskFreeSpaceExW};

const CONTROL_AND_ABORT_HEADROOM: u64 = 128 * 1024 * 1024;
fn bytes(entries: &[ManifestEntry]) -> Result<u64, SafeError> {
    entries.iter().try_fold(0u64, |sum, entry| {
        sum.checked_add(if entry.metadata.kind == EntryType::File {
            entry.metadata.size
        } else {
            0
        })
        .ok_or_else(|| error("HISTORY_CAPACITY"))
    })
}
fn total(parts: &[u64]) -> Result<u64, SafeError> {
    parts.iter().try_fold(0u64, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| error("HISTORY_CAPACITY"))
    })
}
fn available(directory: &Directory) -> Result<u64, SafeError> {
    directory
        .recheck()
        .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
    let path = launch_path(directory.raw()).map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
    let path: Vec<u16> = path.encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut available), None, None) }
        .map_err(win_error)
        .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
    directory
        .recheck()
        .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
    Ok(available)
}
pub(crate) struct SpaceAdmission {
    root: Arc<PrivateDirectory>,
    required: u64,
}
impl SpaceAdmission {
    pub(crate) fn source_handoff(
        root: Arc<PrivateDirectory>,
        installation: &Directory,
        bundle: &HeldBundle,
        transfer: &PreparedHandoff,
        payload: &PayloadAdmission,
    ) -> Result<Self, SafeError> {
        bundle
            .tree()
            .verify()
            .map_err(|_| error("HISTORY_SOURCE_CHANGED"))?;
        transfer.check()?;
        payload.verify_selection(transfer.selection())?;
        root.verify(&CurrentUser::capture().map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?)
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        installation
            .require_same_volume(root.directory())
            .map_err(|_| error("HISTORY_VOLUME_UNSUPPORTED"))?;
        let bundle_bytes = bytes(&bundle.tree().manifest().entries)?;
        // Complete manager copy + independent original backup + extra manager
        // image bounded by the full original bundle, package and target files.
        let required = total(&[
            bundle_bytes,
            bundle_bytes,
            bundle_bytes,
            transfer.selection().installer().size(),
            payload.installed_bytes(),
            CONTROL_AND_ABORT_HEADROOM,
        ])?;
        let admission = Self { root, required };
        admission.verify()?;
        Ok(admission)
    }
    pub(crate) fn context_copy(
        root: Arc<PrivateDirectory>,
        context: &HeldContext,
    ) -> Result<Self, SafeError> {
        context
            .verify_durable()
            .map_err(|_| error("HISTORY_SOURCE_CHANGED"))?;
        let required = total(&[
            bytes(&context.tree(RootKind::Desk).manifest().entries)?,
            bytes(&context.tree(RootKind::WebView).manifest().entries)?,
            CONTROL_AND_ABORT_HEADROOM,
        ])?;
        let admission = Self { root, required };
        admission.verify()?;
        Ok(admission)
    }
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        self.root
            .verify(&CurrentUser::capture().map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?)
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        if available(self.root.directory())? < self.required {
            return Err(error("HISTORY_INSUFFICIENT_SPACE"));
        }
        Ok(())
    }
}
