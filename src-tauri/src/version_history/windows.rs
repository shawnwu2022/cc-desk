//! Windows recovery primitives, deliberately not wired into ordinary startup.
//! These guards describe real OS objects, not complete switch admission. The
//! coordinator must still establish installation scope, all exclusion roots,
//! journal intent and source quiescence before composing a SnapshotBoundary.
//! Operation-specific storage never supplies a generic directory-fsync adapter.
#[cfg(windows)]
pub(crate) mod context;
#[cfg(windows)]
pub(crate) mod coordinator;
#[cfg(windows)]
pub(crate) mod coordinator_evidence;
#[cfg(windows)]
pub(crate) mod durability;
#[cfg(windows)]
pub(crate) mod fence;
#[cfg(windows)]
pub(crate) mod files;
#[cfg(windows)]
pub(crate) mod install_admission;
#[cfg(windows)]
pub(crate) mod lease;
#[cfg(windows)]
pub(crate) mod manager_bundle;
#[cfg(windows)]
pub(crate) mod manager_handoff;
#[cfg(windows)]
pub(crate) mod manager_process;
#[cfg(windows)]
pub(crate) mod manager_ui;
#[cfg(windows)]
pub(crate) mod no_historical_launch;
#[cfg(windows)]
pub(crate) mod package;
#[cfg(windows)]
pub(crate) mod pre_context_abort;
#[cfg(windows)]
pub(crate) mod preinstall_return;
#[cfg(windows)]
pub(crate) mod process;
#[cfg(windows)]
pub(crate) mod recovery_space;
#[cfg(windows)]
pub(crate) mod reentry;
#[cfg(windows)]
pub(crate) mod registration_state;
#[cfg(windows)]
pub(crate) mod registry;
#[cfg(windows)]
pub(crate) mod return_boundary;
#[cfg(windows)]
pub(crate) mod return_checkpoint;
#[cfg(windows)]
pub(crate) mod scope;
#[cfg(windows)]
pub(crate) mod security;
#[cfg(windows)]
pub(crate) mod shortcuts;
#[cfg(windows)]
pub(crate) mod source_begin;
#[cfg(windows)]
pub(crate) mod source_boundary;
#[cfg(windows)]
pub(crate) mod source_failure;
#[cfg(windows)]
pub(crate) mod source_lifecycle;
#[cfg(windows)]
pub(crate) mod source_session;
#[cfg(windows)]
pub(crate) mod space;
#[cfg(windows)]
pub(crate) mod startup;
#[cfg(windows)]
pub(crate) mod webview;

#[cfg(windows)]
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;

#[cfg(windows)]
fn blocked(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
#[cfg(windows)]
fn win_error(error: windows_core::Error) -> io::Error {
    let code = error.code().0 as u32;
    if code & 0xffff0000 == 0x80070000 {
        io::Error::from_raw_os_error((code & 0xffff) as i32)
    } else {
        io::Error::other(error)
    }
}
#[cfg(windows)]
fn handle(value: &impl AsRawHandle) -> HANDLE {
    HANDLE(value.as_raw_handle())
}
#[cfg(windows)]
unsafe fn own(value: HANDLE) -> OwnedHandle {
    // SAFETY: callers only transfer successfully created, non-pseudo handles.
    unsafe { OwnedHandle::from_raw_handle(value.0) }
}
