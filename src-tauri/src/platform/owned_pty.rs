//! Owned PTY handles with independent blocking domains, not a stop/drain policy.
#![allow(dead_code)] // Connected to staged D11; not exposed through live IPC yet.

use super::launch::ProcessLaunchSpec;
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use parking_lot::Mutex;
#[cfg(not(windows))]
use portable_pty::ChildKiller;
use portable_pty::{
    native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize,
};
use std::io::{self, Read, Write};
#[cfg(windows)]
use std::sync::atomic::{AtomicPtr, Ordering};

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "TerminateProcess"]
    fn terminate_process(handle: *mut std::ffi::c_void, exit_code: u32) -> i32;
}

struct ChildState {
    child: Box<dyn Child + Send + Sync>,
    status: Option<ExitStatus>,
}

/// Backend-only resource. Revoking a window does not drop it or kill its child.
/// Its lifecycle owner must wait/reap and decide when master closure is safe;
/// D14/D15 supply autonomous waiting, output draining and explicit stop policy.
/// In particular, releasing these handles is not a claim that output was parsed.
pub(crate) struct OwnedPty {
    master: Mutex<Box<dyn MasterPty + Send>>,
    reader: Mutex<Option<Box<dyn Read + Send>>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<ChildState>,
    #[cfg(not(windows))]
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    // Borrowed from the private child, which is never replaced or extracted.
    // AtomicPtr transports the immutable opaque Windows handle across threads;
    // it does not own it. Every use borrows Self and therefore pins its owner.
    #[cfg(windows)]
    process_handle: AtomicPtr<std::ffi::c_void>,
}

impl std::fmt::Debug for OwnedPty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwnedPty(<redacted>)")
    }
}

impl OwnedPty {
    pub(crate) fn spawn(spec: &ProcessLaunchSpec, size: PtySize) -> Result<Self, SafeError> {
        validate_size(size)?;
        let command = spec.command()?;
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|_| error("HOST_PTY_UNAVAILABLE"))?;
        Self::attach_and_spawn(pair, command)
    }

    /// Acquire both I/O handles before starting a child. There is no fallible
    /// reader/writer initialization after spawn succeeds, so an I/O setup failure
    /// cannot return Err while leaving a newly created child without an owner.
    pub(crate) fn attach_and_spawn(
        pair: PtyPair,
        command: CommandBuilder,
    ) -> Result<Self, SafeError> {
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|_| error("HOST_READER_UNAVAILABLE"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|_| error("HOST_WRITER_UNAVAILABLE"))?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|_| error("PROCESS_START_FAILED"))?;
        #[cfg(not(windows))]
        let killer = child.clone_killer();
        // portable-pty 0.8.1's WinChildKiller inverts TerminateProcess's BOOL.
        // Keep the already-owned native handle, not that incorrect wrapper.
        // Capturing it adds no fallible handle duplication after child creation.
        #[cfg(windows)]
        let process_handle =
            AtomicPtr::new(child.as_raw_handle().unwrap_or(std::ptr::null_mut()));
        // Do not retain the parent's slave endpoint and prevent stream EOF.
        drop(pair.slave);
        Ok(Self {
            master: Mutex::new(pair.master),
            reader: Mutex::new(Some(reader)),
            writer: Mutex::new(writer),
            child: Mutex::new(ChildState {
                child,
                status: None,
            }),
            #[cfg(not(windows))]
            killer: Mutex::new(killer),
            #[cfg(windows)]
            process_handle,
        })
    }

    pub(crate) fn take_reader(&self) -> Result<Box<dyn Read + Send>, SafeError> {
        self.reader
            .lock()
            .take()
            .ok_or_else(|| error("HOST_READER_ALREADY_TAKEN"))
    }

    pub(crate) fn with_writer<T>(
        &self,
        operation: impl FnOnce(&mut (dyn Write + Send)) -> io::Result<T>,
    ) -> io::Result<T> {
        operation(self.writer.lock().as_mut())
    }

    pub(crate) fn try_wait(&self) -> Result<Option<ExitStatus>, SafeError> {
        // Another thread may already be in blocking wait(). Do not block this
        // nonblocking observation behind that thread's child handle lock.
        let Some(mut state) = self.child.try_lock() else {
            return Ok(None);
        };
        if let Some(status) = &state.status {
            return Ok(Some(status.clone()));
        }
        let status = state
            .child
            .try_wait()
            .map_err(|_| error("PROCESS_WAIT_FAILED"))?;
        state.status = status.clone();
        Ok(status)
    }

    pub(crate) fn wait(&self) -> Result<ExitStatus, SafeError> {
        let mut state = self.child.lock();
        if let Some(status) = &state.status {
            return Ok(status.clone());
        }
        let status = state
            .child
            .wait()
            .map_err(|_| error("PROCESS_WAIT_FAILED"))?;
        state.status = Some(status.clone());
        Ok(status)
    }

    /// Signal only this retained child, never a process-name/PID search. Success
    /// means termination was accepted, not that wait/reap or output drain ended.
    /// Failure is not silently reclassified as success for an exited process.
    pub(crate) fn terminate_root(&self) -> Result<(), SafeError> {
        #[cfg(windows)]
        {
            let handle = self.process_handle.load(Ordering::Relaxed);
            if handle.is_null() {
                return Err(error("PROCESS_CONTROL_UNAVAILABLE"));
            }
            // SAFETY: the native Child retains this process handle for its whole
            // lifetime, including after wait(). The private child field is not
            // removed/replaced, and &self keeps it alive throughout this call.
            // No pointer is dereferenced here; Windows validates the opaque HANDLE.
            // Wait uses the same kernel object safely without taking our writer
            // or master lock. Do not replace the child without revisiting this.
            if unsafe { terminate_process(handle, 1) } == 0 {
                Err(error("PROCESS_TERMINATE_FAILED"))
            } else {
                Ok(())
            }
        }
        #[cfg(not(windows))]
        self.killer
            .lock()
            .kill()
            .map_err(|_| error("PROCESS_TERMINATE_FAILED"))
    }

    pub(crate) fn resize(&self, size: PtySize) -> Result<(), SafeError> {
        validate_size(size)?;
        self.master
            .lock()
            .resize(size)
            .map_err(|_| error("HOST_RESIZE_FAILED"))
    }
}

fn validate_size(size: PtySize) -> Result<(), SafeError> {
    if size.rows == 0 || size.cols == 0 {
        return Err(SafeError::invalid("terminalSize"));
    }
    Ok(())
}
