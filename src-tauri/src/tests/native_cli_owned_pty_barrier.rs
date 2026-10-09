//! Kernel-state barrier for the real Windows termination-failure fixture.
use super::OwnedPty;
use std::sync::atomic::Ordering;

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "WaitForSingleObject"]
    fn wait_for_single_object(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
}

/// Return the real native wait result, including failures/timeouts. No polling
/// of PID/name, artificial delays, retries of termination or assertion waiver.
pub(crate) fn wait_for_exit(process: &OwnedPty) -> u32 {
    let handle = process.process_handle.load(Ordering::Relaxed);
    // SAFETY: &OwnedPty pins its private child and the immutable borrowed HANDLE.
    // No handle is closed or replaced here. The wait has a 15-second deadline.
    unsafe { wait_for_single_object(handle, 15_000) }
}
