use crate::{BackendLaunch, RuntimeError};
use std::{
    process::{Child, ChildStdout, ExitStatus},
    time::{Duration, Instant},
};
pub struct OwnedRun {
    child: Child,
    #[cfg(windows)]
    job: Option<crate::windows_job::Job>,
}
/// Confirms only root exit, never descendant quiescence or old IO completion.
#[derive(Clone, Copy, Debug)]
pub struct RootExitOnly {
    pub root_status: ExitStatus,
}
impl OwnedRun {
    pub fn try_wait(&mut self) -> Result<Option<RootExitOnly>, RuntimeError> {
        self.child
            .try_wait()
            .map(|status| status.map(|root_status| RootExitOnly { root_status }))
            .map_err(|_| RuntimeError::OutcomeUnknown)
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
    /// Requests owned-tree termination; success confirms root exit only.
    /// Unknown does not authorize retry/relaunch or generation replacement.
    pub fn stop(&mut self, deadline: Instant) -> Result<RootExitOnly, RuntimeError> {
        #[cfg(windows)]
        drop(self.job.take());
        let _ = self.child.kill();
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(RuntimeError::OutcomeUnknown);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for OwnedRun {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.job.take());
        let _ = self.child.kill();
        // Drop does not claim a reap or block on unread pipes. Confirm through stop/wait.
    }
}
#[cfg(windows)]
pub(crate) fn launch(launch: BackendLaunch) -> Result<OwnedRun, RuntimeError> {
    launch_with(launch, |job, child| job.assign(child))
}
#[cfg(not(windows))]
pub(crate) fn launch(_launch: BackendLaunch) -> Result<OwnedRun, RuntimeError> {
    Err(RuntimeError::Unsupported)
}
#[cfg(windows)]
pub(crate) fn launch_with(
    mut launch: BackendLaunch,
    assign: impl FnOnce(&crate::windows_job::Job, &Child) -> Result<(), RuntimeError>,
) -> Result<OwnedRun, RuntimeError> {
    use std::os::windows::process::CommandExt;
    let job = crate::windows_job::Job::new()?;
    launch.command.creation_flags(0x08000000 | 0x00000004);
    let child = launch
        .command
        .spawn()
        .map_err(|_| RuntimeError::SpawnFailed)?;
    let mut run = OwnedRun {
        child,
        job: Some(job),
    };
    if assign(run.job.as_ref().unwrap(), &run.child).is_err() {
        return match run.stop(Instant::now() + Duration::from_secs(2)) {
            Ok(_) => Err(RuntimeError::AssignmentFailedReaped),
            Err(_) => Err(RuntimeError::OutcomeUnknown),
        };
    }
    if run.job.as_ref().unwrap().resume(&run.child).is_err() {
        // Cleanup cannot undo side effects if execution may already have begun.
        let _ = run.stop(Instant::now() + Duration::from_secs(2));
        return Err(RuntimeError::OutcomeUnknown);
    }
    Ok(run)
}
