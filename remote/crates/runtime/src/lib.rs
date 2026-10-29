//! Backend-only process ownership primitives; no authorization or network entry.
use std::process::Command;
mod owned_run;
#[cfg(windows)]
mod windows_job;
pub use owned_run::{OwnedRun, RootExitOnly};
pub struct BackendLaunch {
    command: Command,
}
impl BackendLaunch {
    pub fn from_backend_command(command: Command) -> Self {
        Self { command }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Unsupported,
    JobSetupFailed,
    SpawnFailed,
    AssignmentFailedReaped,
    OutcomeUnknown,
}
pub struct OwnedJob;
impl OwnedJob {
    pub fn launch(launch: BackendLaunch) -> Result<OwnedRun, RuntimeError> {
        owned_run::launch(launch)
    }
}
#[cfg(all(test, windows))]
fn launch_rejected_for_test(launch: BackendLaunch) -> Result<OwnedRun, RuntimeError> {
    use std::os::windows::io::AsRawHandle;
    owned_run::launch_with(launch, |_job, child| {
        // Actual OS failure using only the child we just created suspended.
        let rejected = unsafe {
            windows::Win32::System::JobObjects::AssignProcessToJobObject(
                windows::Win32::Foundation::HANDLE::default(),
                windows::Win32::Foundation::HANDLE(child.as_raw_handle()),
            )
        };
        assert!(rejected.is_err());
        Err(RuntimeError::AssignmentFailedReaped)
    })
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    #[allow(non_snake_case)]
    fn Host_AssignFailureNeverExecutes_001() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("must-not-execute");
        let fixture = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("owned-job-fixture.exe");
        assert!(
            fixture.is_file(),
            "fixture missing from current Cargo build"
        );
        let mut c = Command::new(fixture);
        c.arg("--marker").arg(&marker).env_clear();
        assert!(matches!(
            launch_rejected_for_test(BackendLaunch::from_backend_command(c)),
            Err(RuntimeError::AssignmentFailedReaped)
        ));
        assert!(!marker.exists());
    }
    #[test]
    #[allow(non_snake_case)]
    fn Host_AlreadyRunningResumeOutcomeUnknown_001() {
        use std::{
            os::windows::io::{AsRawHandle, BorrowedHandle},
            time::{Duration, Instant},
        };
        use windows::Win32::{
            Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::Threading::WaitForSingleObject,
        };
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("executed-before-second-resume");
        let fixture = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("owned-job-fixture.exe");
        let mut command = Command::new(fixture);
        command.arg("--marker-alive").arg(&marker).env_clear();
        let mut observed = None;
        let result = owned_run::launch_with(
            BackendLaunch::from_backend_command(command),
            |job, child| {
                observed = Some(
                    unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) }
                        .try_clone_to_owned()
                        .unwrap(),
                );
                job.assign(child)?;
                job.resume(child)?;
                let deadline = Instant::now() + Duration::from_secs(2);
                while std::fs::read(&marker).ok().as_deref() != Some(b"executed") {
                    assert!(Instant::now() < deadline, "owned fixture did not execute");
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(
                    unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), 0) },
                    WAIT_TIMEOUT
                );
                Ok(())
            },
        );
        eprintln!(
            "TEST_ONLY_LAST_RESUME_COUNT={:?} MARKER_EXECUTED=true",
            crate::windows_job::take_resume_count_for_test()
        );
        assert_eq!(std::fs::read(&marker).unwrap(), b"executed");
        let handle = observed.unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 2000) },
            WAIT_OBJECT_0,
            "owned child must be terminated"
        );
        assert!(
            matches!(result, Err(RuntimeError::OutcomeUnknown)),
            "execution happened; cleanup must not imply no effects"
        );
    }
}
