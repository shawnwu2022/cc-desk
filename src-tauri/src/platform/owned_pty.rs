//! Owned PTY handles with independent blocking domains, not a stop/drain policy.
#![allow(dead_code)] // Connected to staged D11; not exposed through live IPC yet.

use super::admitted_child::AdmittedChild;
use super::launch::ProcessLaunchSpec;
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use crate::version_history::maintenance::{PreparingStart, StartTicket};
use parking_lot::Mutex;
#[cfg(not(windows))]
use portable_pty::ChildKiller;
use portable_pty::{native_pty_system, CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize};
use std::io::{self, Read, Write};
#[cfg(windows)]
use std::sync::atomic::{AtomicPtr, Ordering};
#[cfg(any(test, feature = "remote-host"))]
use std::sync::{
    atomic::{AtomicU8, Ordering as DropOrdering},
    Arc,
};

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "TerminateProcess"]
    fn terminate_process(handle: *mut std::ffi::c_void, exit_code: u32) -> i32;
}

/// Backend-only resource. Revoking a window does not drop it or kill its child.
/// Its lifecycle owner must wait/reap and decide when master closure is safe;
/// D14/D15 supply autonomous waiting, output draining and explicit stop policy.
/// In particular, releasing these handles is not a claim that output was parsed.
pub(crate) struct OwnedPty {
    #[cfg(any(test, feature = "remote-host"))]
    io_drop: IoDropWitness,
    mutation: Mutex<PtyMutation>,
    #[cfg(test)]
    mutation_barrier: Mutex<Option<mutation_test::Hook>>,
    reader: Mutex<Option<Box<dyn Read + Send>>>,
    child: Mutex<AdmittedChild>,
    #[cfg(not(windows))]
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    // Borrowed from the private child, which is never replaced or extracted.
    // AtomicPtr transports the immutable opaque Windows handle across threads;
    // it does not own it. Every use borrows Self and therefore pins its owner.
    #[cfg(windows)]
    process_handle: AtomicPtr<std::ffi::c_void>,
}

/// Rust-owner destructor return only, not kernel close or process/IO retirement.
#[cfg(any(test, feature = "remote-host"))]
#[derive(Clone, Default)]
pub(crate) struct IoDropWitness(Arc<AtomicU8>);
#[cfg(any(test, feature = "remote-host"))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IoDropState {
    Pending,
    Returned,
    Unknown,
}
#[cfg(any(test, feature = "remote-host"))]
impl IoDropWitness {
    pub(crate) fn observe(&self) -> IoDropState {
        let bits = self.0.load(DropOrdering::Acquire);
        let returned = bits & 7;
        let failed = (bits >> 3) & 7;
        if (returned | failed) != 7 {
            IoDropState::Pending
        } else if failed != 0 {
            IoDropState::Unknown
        } else {
            IoDropState::Returned
        }
    }
    pub(crate) fn all_returned(&self) -> bool {
        self.0.load(DropOrdering::Acquire) == 7
    }
}

#[cfg(any(test, feature = "remote-host"))]
struct TrackedDrop<T> {
    inner: Option<T>,
    witness: IoDropWitness,
    bit: u8,
}
#[cfg(any(test, feature = "remote-host"))]
impl<T> TrackedDrop<T> {
    fn new(inner: T, witness: IoDropWitness, bit: u8) -> Self {
        Self {
            inner: Some(inner),
            witness,
            bit,
        }
    }
}
#[cfg(any(test, feature = "remote-host"))]
impl<T> std::ops::Deref for TrackedDrop<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner
            .as_ref()
            .expect("original I/O owner exists until Drop")
    }
}
#[cfg(any(test, feature = "remote-host"))]
impl<T> std::ops::DerefMut for TrackedDrop<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner
            .as_mut()
            .expect("original I/O owner exists until Drop")
    }
}
#[cfg(any(test, feature = "remote-host"))]
impl<T: Read> Read for TrackedDrop<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.inner.as_mut().expect("original reader").read(bytes)
    }
}
// This guard records FAILURE on unwind, never success from a later field Drop.
#[cfg(any(test, feature = "remote-host"))]
struct DropFailure<'a> {
    witness: &'a IoDropWitness,
    bit: u8,
    returned: bool,
}
#[cfg(any(test, feature = "remote-host"))]
impl Drop for DropFailure<'_> {
    fn drop(&mut self) {
        if !self.returned {
            self.witness
                .0
                .fetch_or(self.bit << 3, DropOrdering::Release);
        }
    }
}
#[cfg(any(test, feature = "remote-host"))]
impl<T> Drop for TrackedDrop<T> {
    fn drop(&mut self) {
        let mut failure = DropFailure {
            witness: &self.witness,
            bit: self.bit,
            returned: false,
        };
        drop(self.inner.take());
        failure.returned = true;
        self.witness.0.fetch_or(self.bit, DropOrdering::Release);
    }
}
#[cfg(any(test, feature = "remote-host"))]
type IoOwner<T> = TrackedDrop<T>;
#[cfg(not(any(test, feature = "remote-host")))]
type IoOwner<T> = T;

// The original handles stay in one per-PTY mutation owner. A complete user or
// protocol frame and native resize cannot run concurrently. Reader and root
// control deliberately do not acquire this potentially blocking region.
struct PtyMutation {
    master: IoOwner<Box<dyn MasterPty + Send>>,
    writer: IoOwner<Box<dyn Write + Send>>,
}

/// The same original IO owners, prepared before any process-creation call.
/// Slave is first so an abandoned preparation releases it before master.
pub(crate) struct PreparedPtyIo {
    slave: Option<Box<dyn portable_pty::SlavePty + Send>>,
    master: IoOwner<Box<dyn MasterPty + Send>>,
    writer: IoOwner<Box<dyn Write + Send>>,
    reader: Option<Box<dyn Read + Send>>,
    #[cfg(any(test, feature = "remote-host"))]
    io_drop: IoDropWitness,
}
impl PreparedPtyIo {
    pub(crate) fn new(pair: PtyPair) -> Result<Self, SafeError> {
        #[cfg(any(test, feature = "remote-host"))]
        let io_drop = IoDropWitness::default();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|_| error("HOST_READER_UNAVAILABLE"))?;
        #[cfg(any(test, feature = "remote-host"))]
        let reader: Box<dyn Read + Send> = Box::new(TrackedDrop::new(reader, io_drop.clone(), 1));
        let writer = pair
            .master
            .take_writer()
            .map_err(|_| error("HOST_WRITER_UNAVAILABLE"))?;
        #[cfg(any(test, feature = "remote-host"))]
        let writer = TrackedDrop::new(writer, io_drop.clone(), 2);
        #[cfg(any(test, feature = "remote-host"))]
        let master = TrackedDrop::new(pair.master, io_drop.clone(), 4);
        #[cfg(not(any(test, feature = "remote-host")))]
        let master = pair.master;
        Ok(Self {
            slave: Some(pair.slave),
            master,
            writer,
            reader: Some(reader),
            #[cfg(any(test, feature = "remote-host"))]
            io_drop,
        })
    }
    pub(crate) fn slave(&self) -> &(dyn portable_pty::SlavePty + Send) {
        self.slave
            .as_deref()
            .expect("original slave before creation")
    }
    #[cfg(all(windows, any(test, feature = "remote-host")))]
    pub(crate) fn close_slave(&mut self) {
        drop(self.slave.take());
    }
    #[cfg(all(windows, any(test, feature = "remote-host")))]
    pub(crate) fn take_reader(&mut self) -> Result<Box<dyn Read + Send>, SafeError> {
        self.reader
            .take()
            .ok_or_else(|| error("HOST_READER_ALREADY_TAKEN"))
    }
    #[cfg(all(windows, any(test, feature = "remote-host")))]
    pub(crate) fn io_drop_witness(&self) -> IoDropWitness {
        self.io_drop.clone()
    }
    pub(crate) fn finish(mut self, child: AdmittedChild) -> OwnedPty {
        #[cfg(not(windows))]
        let killer = child.clone_killer();
        #[cfg(windows)]
        let process_handle = AtomicPtr::new(child.raw_handle());
        drop(self.slave.take());
        OwnedPty {
            #[cfg(any(test, feature = "remote-host"))]
            io_drop: self.io_drop,
            mutation: Mutex::new(PtyMutation {
                master: self.master,
                writer: self.writer,
            }),
            #[cfg(test)]
            mutation_barrier: Mutex::new(None),
            reader: Mutex::new(self.reader),
            child: Mutex::new(child),
            #[cfg(not(windows))]
            killer: Mutex::new(killer),
            #[cfg(windows)]
            process_handle,
        }
    }
}

impl std::fmt::Debug for OwnedPty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwnedPty(<redacted>)")
    }
}

impl OwnedPty {
    #[cfg(test)]
    pub(crate) fn spawn(spec: &ProcessLaunchSpec, size: PtySize) -> Result<Self, SafeError> {
        let ticket = crate::version_history::maintenance::AdmissionGate::new()
            .begin_start(crate::version_history::maintenance::RuntimeKind::Native)?;
        Self::spawn_admitted(spec, size, ticket)
    }

    pub(crate) fn spawn_admitted(
        spec: &ProcessLaunchSpec,
        size: PtySize,
        ticket: StartTicket,
    ) -> Result<Self, SafeError> {
        let admission = ticket.preparing();
        validate_size(size)?;
        let command = spec.command()?;
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|_| error("HOST_PTY_UNAVAILABLE"))?;
        Self::attach_admitted(pair, command, admission, true)
    }

    /// Acquire both I/O handles before starting a child. There is no fallible
    /// reader/writer initialization after spawn succeeds, so an I/O setup failure
    /// cannot return Err while leaving a newly created child without an owner.
    #[cfg(test)]
    pub(crate) fn attach_and_spawn(
        pair: PtyPair,
        command: CommandBuilder,
    ) -> Result<Self, SafeError> {
        let admission = crate::version_history::maintenance::AdmissionGate::new()
            .begin_start(crate::version_history::maintenance::RuntimeKind::Native)?
            .preparing();
        Self::attach_admitted(pair, command, admission, false)
    }

    fn attach_admitted(
        pair: PtyPair,
        command: CommandBuilder,
        mut admission: PreparingStart,
        native: bool,
    ) -> Result<Self, SafeError> {
        let prepared = PreparedPtyIo::new(pair)?;
        let ticket = admission.begin_creation();
        let child = if native {
            AdmittedChild::spawn_native(prepared.slave(), command, ticket)
                .map_err(|_| error("PROCESS_START_FAILED"))?
        } else {
            // Arbitrary SlavePty adapters have no audited no-child-on-Err contract.
            let child = prepared
                .slave()
                .spawn_command(command)
                .map_err(|_| error("PROCESS_START_FAILED"))?;
            AdmittedChild::created(child, ticket)
        };
        Ok(prepared.finish(child))
    }

    pub(crate) fn take_reader(&self) -> Result<Box<dyn Read + Send>, SafeError> {
        self.reader
            .lock()
            .take()
            .ok_or_else(|| error("HOST_READER_ALREADY_TAKEN"))
    }
    #[cfg(all(windows, any(test, feature = "remote-host")))]
    pub(crate) fn observe_exact_root(&self) -> io::Result<bool> {
        let Some(mut child) = self.child.try_lock() else {
            return Ok(false);
        };
        child.observe_exact_root()
    }
    #[cfg(any(test, feature = "remote-host"))]
    pub(crate) fn io_drop_witness(&self) -> IoDropWitness {
        self.io_drop.clone()
    }

    pub(crate) fn with_writer<T>(
        &self,
        operation: impl FnOnce(&mut (dyn Write + Send)) -> io::Result<T>,
    ) -> io::Result<T> {
        #[cfg(test)]
        mutation_test::before_lock(self);
        operation(self.mutation.lock().writer.as_mut())
    }

    pub(crate) fn try_wait(&self) -> Result<Option<ExitStatus>, SafeError> {
        // Another thread may already be in blocking wait(). Do not block this
        // nonblocking observation behind that thread's child handle lock.
        let Some(mut state) = self.child.try_lock() else {
            return Ok(None);
        };
        state.try_wait().map_err(|_| error("PROCESS_WAIT_FAILED"))
    }

    pub(crate) fn wait(&self) -> Result<ExitStatus, SafeError> {
        self.child
            .lock()
            .wait()
            .map_err(|_| error("PROCESS_WAIT_FAILED"))
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
        self.resize_checked(size, || Ok(()))
    }

    /// Revalidate a caller after waiting for the mutation lock, immediately before
    /// the native effect. Internal lifecycle users use resize with no wire owner.
    pub(crate) fn resize_checked(
        &self,
        size: PtySize,
        authorize: impl FnOnce() -> Result<(), SafeError>,
    ) -> Result<(), SafeError> {
        validate_size(size)?;
        #[cfg(test)]
        mutation_test::before_lock(self);
        let mutation = self.mutation.lock();
        authorize()?;
        mutation
            .master
            .resize(size)
            .map_err(|_| error("HOST_RESIZE_FAILED"))
    }
    /// Read the adapter-reported size under the SAME original mutation region.
    /// ConPTY caches this value; independent OS evidence requires child observation.
    /// A failed report after resize is an unknown effect.
    #[cfg(feature = "remote-host")]
    pub(crate) fn resize_checked_reported(
        &self,
        size: PtySize,
        authorize: impl FnOnce() -> Result<(), SafeError>,
    ) -> Result<PtySize, SafeError> {
        validate_size(size)?;
        #[cfg(test)]
        mutation_test::before_lock(self);
        let mutation = self.mutation.lock();
        authorize()?;
        mutation
            .master
            .resize(size)
            .map_err(|_| error("HOST_RESIZE_FAILED"))?;
        mutation
            .master
            .get_size()
            .map_err(|_| error("HOST_RESIZE_OBSERVATION_UNKNOWN"))
    }
}

fn validate_size(size: PtySize) -> Result<(), SafeError> {
    if size.rows == 0 || size.cols == 0 {
        return Err(SafeError::invalid("terminalSize"));
    }
    Ok(())
}

// Native test preconditions live outside production classes and do not expose
// process handles or synchronization operations in application builds.
#[cfg(all(test, windows))]
#[path = "../tests/native_cli_owned_pty_barrier.rs"]
pub(crate) mod test_barrier;

#[cfg(test)]
#[path = "../tests/native_cli_mutation_barrier.rs"]
pub(crate) mod mutation_test;

#[cfg(test)]
#[path = "../tests/native_cli_io_drop.rs"]
mod io_drop_tests;
