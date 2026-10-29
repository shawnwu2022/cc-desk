//! Default-off backend PTY custody, not authorization or full tree/IO retirement.
pub use crate::native_pipe::BackendPipeLaunch as BackendPtyLaunch;
use crate::pinned;
pub use portable_pty::{JobSpawnError as NativePtyError, JobSpawnPhase as NativePtyPhase};
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{GetExitCodeProcess, TerminateProcess, WaitForSingleObject},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePtySize {
    rows: u16,
    cols: u16,
}
// Current TerminalCore bounds (terminal/src/emulator.rs); shared-schema gate remains separate.
pub const MAX_PTY_ROWS: u16 = 100;
pub const MAX_PTY_COLS: u16 = 240;
impl NativePtySize {
    pub fn new(rows: u16, cols: u16) -> Result<Self, NativePtyError> {
        if rows == 0 || rows > MAX_PTY_ROWS || cols == 0 || cols > MAX_PTY_COLS {
            return Err(preparation());
        }
        Ok(Self { rows, cols })
    }
}
pub struct NativeOwnedPtyRun {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    job: Option<crate::windows_job::Job>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    reader: Option<Box<dyn Read + Send>>,
    writer: Option<Box<dyn Write + Send>>,
}
fn preparation() -> NativePtyError {
    NativePtyError::KnownNotStarted {
        phase: NativePtyPhase::Preparation,
    }
}
impl NativeOwnedPtyRun {
    /// No vendor PTY allocation/lazy resolver may run until app-local pinning succeeds.
    /// Error classification concerns the target root, not proof of OpenConsole quiescence.
    pub fn launch(
        recipe: BackendPtyLaunch,
        size: NativePtySize,
        end: Instant,
    ) -> Result<Self, NativePtyError> {
        if size.rows == 0 || size.cols == 0 || Instant::now() >= end {
            return Err(preparation());
        }
        pinned::initialize().map_err(|_| preparation())?;
        if Instant::now() >= end {
            return Err(preparation());
        }
        let job = crate::windows_job::Job::new().map_err(|_| preparation())?;
        // openpty may create OpenConsole before the target. Failures here remain unknown.
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|_| NativePtyError::OutcomeUnknown)?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|_| NativePtyError::OutcomeUnknown)?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|_| NativePtyError::OutcomeUnknown)?;
        if Instant::now() >= end {
            return Err(NativePtyError::OutcomeUnknown);
        }
        let child = pair
            .slave
            .spawn_command_in_job(recipe.into_command(), job.borrowed())?;
        drop(pair.slave);
        let mut run = Self {
            child,
            job: Some(job),
            _master: pair.master,
            reader: Some(reader),
            writer: Some(writer),
        };
        if Instant::now() >= end {
            run.request_stop();
            return Err(NativePtyError::OutcomeUnknown);
        }
        Ok(run)
    }
    pub fn take_reader(&mut self) -> Option<Box<dyn Read + Send>> {
        self.reader.take()
    }
    pub fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        self.writer.take()
    }
    fn root_handle(&self) -> Result<HANDLE, NativePtyError> {
        self.child
            .as_raw_handle()
            .map(HANDLE)
            .ok_or(NativePtyError::OutcomeUnknown)
    }
    /// Exact retained root signal only; EOF, a kill request and tree state are distinct.
    pub fn try_wait_root(&self) -> Result<Option<crate::NativeRootExitOnly>, NativePtyError> {
        let handle = self.root_handle()?;
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_OBJECT_0 => {
                let mut exit_code = 0;
                unsafe { GetExitCodeProcess(handle, &mut exit_code) }
                    .map_err(|_| NativePtyError::OutcomeUnknown)?;
                Ok(Some(crate::NativeRootExitOnly { exit_code }))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(NativePtyError::OutcomeUnknown),
        }
    }
    fn request_stop(&mut self) {
        drop(self.job.take());
        if let Ok(handle) = self.root_handle() {
            let _ = unsafe { TerminateProcess(handle, 1) };
        }
    }
    /// Root control never acquires the transferred writer or master. Cannot preempt Win32 calls.
    pub fn stop_root_until(
        &mut self,
        end: Instant,
    ) -> Result<crate::NativeRootExitOnly, NativePtyError> {
        self.request_stop();
        while Instant::now() < end {
            if let Some(root) = self.try_wait_root()? {
                if Instant::now() < end {
                    return Ok(root);
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Err(NativePtyError::OutcomeUnknown)
    }
}
impl Drop for NativeOwnedPtyRun {
    fn drop(&mut self) {
        self.request_stop(); /* no wait/IO join claim; master Drop may block on Windows10 */
    }
}
