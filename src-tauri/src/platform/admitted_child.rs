//! Exact direct-child maintenance evidence shared by Native and Legacy PTYs.
use crate::version_history::maintenance::{OwnedChildTicket, StartTicket};
use portable_pty::{Child, ChildKiller, CommandBuilder, ExitStatus, SlavePty};
use std::io;

pub(crate) struct AdmittedChild {
    child: Box<dyn Child + Send + Sync>,
    ticket: Option<OwnedChildTicket>,
    status: Option<ExitStatus>,
}

impl AdmittedChild {
    pub(crate) fn created(child: Box<dyn Child + Send + Sync>, ticket: StartTicket) -> Self {
        Self {
            child,
            ticket: Some(ticket.child_created()),
            status: None,
        }
    }

    /// Only call with native_pty_system's pinned portable-pty 0.8.1 slave.
    /// Its Unix Command::spawn Err creates no child; ConPTY has no fallible
    /// Result step after successful CreateProcessW. A panic remains unknown.
    pub(crate) fn spawn_native(
        slave: &dyn SlavePty,
        command: CommandBuilder,
        ticket: StartTicket,
    ) -> anyhow::Result<Self> {
        match slave.spawn_command(command) {
            Ok(child) => Ok(Self::created(child, ticket)),
            Err(error) => {
                ticket.no_child_created();
                Err(error)
            }
        }
    }

    pub(crate) fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        self.child.clone_killer()
    }
    pub(crate) fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    #[cfg(windows)]
    pub(crate) fn raw_handle(&self) -> *mut std::ffi::c_void {
        self.child.as_raw_handle().unwrap_or(std::ptr::null_mut())
    }

    fn settle_terminal(&mut self, blocking: bool) {
        #[cfg(windows)]
        {
            let handle = self.raw_handle();
            if handle.is_null() {
                return;
            }
            // SAFETY: the private child owns this exact handle throughout the
            // call. It is never reopened by PID, replaced, or extracted.
            let result =
                unsafe { wait_for_single_object(handle, if blocking { u32::MAX } else { 0 }) };
            if result != 0 {
                return; // Only WAIT_OBJECT_0 is terminal proof.
            }
        }
        #[cfg(not(windows))]
        let _ = blocking; // Successful std::process::Child wait/try_wait reaps.
        if let Some(ticket) = self.ticket.take() {
            ticket.reaped();
        }
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none() {
            self.status = self.child.try_wait()?;
        }
        if self.status.is_some() {
            self.settle_terminal(false);
        }
        Ok(self.status.clone())
    }

    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = match &self.status {
            Some(status) => status.clone(),
            None => self.child.wait()?,
        };
        self.status = Some(status.clone());
        // Windows portable-pty's cached code and unchecked internal wait are
        // observations only; independently require the retained handle signal.
        self.settle_terminal(true);
        Ok(status)
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "WaitForSingleObject"]
    fn wait_for_single_object(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
}

#[cfg(test)]
#[path = "../tests/version_history_child.rs"]
mod tests;
