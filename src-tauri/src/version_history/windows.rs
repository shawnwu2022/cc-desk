//! Windows recovery primitives, deliberately not wired into ordinary startup.
//! These guards describe real OS objects, not complete switch admission. The
//! coordinator must still establish installation scope, all exclusion roots,
//! journal intent and source quiescence before composing a SnapshotBoundary.
//! In particular there is no DirectoryDurability or SnapshotBoundary factory.
#[cfg(windows)]
pub(crate) mod durability;
#[cfg(windows)]
pub(crate) mod fence;
#[cfg(windows)]
pub(crate) mod files;
#[cfg(windows)]
pub(crate) mod lease;
#[cfg(windows)]
pub(crate) mod process;
#[cfg(windows)]
pub(crate) mod registry;
#[cfg(windows)]
pub(crate) mod security;
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
