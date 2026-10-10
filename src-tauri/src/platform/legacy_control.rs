//! Independent control of the exact retained legacy child.
//! portable-pty 0.8.1's Windows cloned killer inverts TerminateProcess's BOOL.
use super::admitted_child::AdmittedChild;
#[cfg(not(windows))]
use portable_pty::ChildKiller;
use std::io;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, BorrowedHandle, OwnedHandle};

pub(crate) struct LegacyProcessControl {
    #[cfg(windows)]
    handle: OwnedHandle,
    #[cfg(not(windows))]
    killer: Box<dyn ChildKiller + Send + Sync>,
}

impl LegacyProcessControl {
    pub(crate) fn capture(child: &AdmittedChild) -> io::Result<Self> {
        #[cfg(windows)]
        {
            let raw = child.raw_handle();
            if raw.is_null() || raw as isize == -1 {
                return Err(io::Error::other("owned child control handle unavailable"));
            }
            // SAFETY: child keeps its private process handle alive throughout
            // this synchronous duplication. The resulting owner is independent
            // of the waiter and never reopens a potentially reused process ID.
            let handle = unsafe { BorrowedHandle::borrow_raw(raw) }.try_clone_to_owned()?;
            Ok(Self { handle })
        }
        #[cfg(not(windows))]
        Ok(Self {
            killer: child.clone_killer(),
        })
    }

    pub(crate) fn kill(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        {
            let handle = self.handle.as_raw_handle();
            // A signalled retained handle proves this exact child already ended.
            if unsafe { wait_for_single_object(handle, 0) } == 0 {
                return Ok(());
            }
            // Windows BOOL: nonzero is accepted; zero is failure. Do not use
            // portable-pty's inverted WinChildKiller result or blanket-swallow it.
            if unsafe { terminate_process(handle, 1) } == 0 {
                let failure = io::Error::last_os_error();
                if unsafe { wait_for_single_object(handle, 0) } == 0 {
                    return Ok(());
                }
                return Err(failure);
            }
            // Acceptance alone is not exit proof. Keep the row until the owned
            // process object is signalled, independently of PTY output draining.
            match unsafe { wait_for_single_object(handle, 5000) } {
                0 => Ok(()),
                258 => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned child stop unconfirmed",
                )),
                _ => Err(io::Error::last_os_error()),
            }
        }
        #[cfg(not(windows))]
        self.killer.kill()
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "TerminateProcess"]
    fn terminate_process(handle: *mut std::ffi::c_void, exit_code: u32) -> i32;
    #[link_name = "WaitForSingleObject"]
    fn wait_for_single_object(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
}
