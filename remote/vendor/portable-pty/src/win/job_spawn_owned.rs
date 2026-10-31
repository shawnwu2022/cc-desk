//! Original CreateProcess handles retained on typed Job-launch failure.
use crate::JobSpawnError;
use filedescriptor::OwnedHandle;
use std::os::windows::io::AsRawHandle;
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::winbase::WAIT_OBJECT_0;
use winapi::um::{processthreadsapi::TerminateProcess, synchapi::WaitForSingleObject};

#[must_use = "Retain created-root ownership until exact cleanup is observed"]
pub struct OwnedJobSpawnFailure {
    pub(crate) classification: JobSpawnError,
    pub(crate) pending: Option<PendingJobChild>,
}
impl std::fmt::Debug for OwnedJobSpawnFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedJobSpawnFailure")
            .field("classification", &self.classification)
            .field("created", &self.pending.is_some())
            .finish()
    }
}
impl OwnedJobSpawnFailure {
    pub fn classification(&self) -> JobSpawnError {
        self.classification
    }
    pub fn has_created_child(&self) -> bool {
        self.pending.is_some()
    }
    pub fn into_parts(self) -> (JobSpawnError, Option<PendingJobChild>) {
        (self.classification, self.pending)
    }
    pub(crate) fn before_create(classification: JobSpawnError) -> Self {
        assert!(
            matches!(
                classification,
                JobSpawnError::KnownNotStarted {
                    phase: crate::JobSpawnPhase::Preparation | crate::JobSpawnPhase::CreateRejected
                }
            ),
            "created-root errors require original ownership"
        );
        Self {
            classification,
            pending: None,
        }
    }
    pub(super) fn after_create(classification: JobSpawnError, pending: PendingJobChild) -> Self {
        Self {
            classification,
            pending: Some(pending),
        }
    }
}

/// No resume or raw-handle escape API. Only this original root may be observed/stopped.
#[must_use = "This exact root may still be alive, including outside the Job"]
pub struct PendingJobChild {
    pub(super) proc: Option<OwnedHandle>,
    pub(super) thread: OwnedHandle,
    pub(super) cleanup: bool,
    pub(super) assigned: bool,
    pub(super) resume_attempted: bool,
}
impl std::fmt::Debug for PendingJobChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingJobChild")
            .field("assigned", &self.assigned)
            .field("resume_attempted", &self.resume_attempted)
            .finish()
    }
}
impl PendingJobChild {
    pub(super) fn resume_once(mut self) -> Result<Self, OwnedJobSpawnFailure> {
        self.resume_attempted = true;
        // Only the original primary thread is used. Anything other than exactly
        // one cannot prove a normal first resume, even if cleanup later succeeds.
        if unsafe {
            winapi::um::processthreadsapi::ResumeThread(self.thread.as_raw_handle().cast())
        } != 1
        {
            let _ = self.reap();
            return Err(OwnedJobSpawnFailure::after_create(
                JobSpawnError::OutcomeUnknown,
                self,
            ));
        }
        self.cleanup = false;
        Ok(self)
    }
    pub fn assigned_to_job(&self) -> bool {
        self.assigned
    }
    pub fn resume_was_attempted(&self) -> bool {
        self.resume_attempted
    }
    pub fn try_wait_root(&mut self) -> Result<bool, JobSpawnError> {
        let root = self.proc.as_ref().ok_or(JobSpawnError::OutcomeUnknown)?;
        match unsafe { WaitForSingleObject(root.as_raw_handle().cast(), 0) } {
            WAIT_OBJECT_0 => {
                self.cleanup = false;
                Ok(true)
            }
            WAIT_TIMEOUT => Ok(false),
            _ => Err(JobSpawnError::OutcomeUnknown),
        }
    }
    pub fn request_root_termination(&self) -> Result<(), JobSpawnError> {
        let root = self.proc.as_ref().ok_or(JobSpawnError::OutcomeUnknown)?;
        if unsafe { TerminateProcess(root.as_raw_handle().cast(), 1) } == 0 {
            Err(JobSpawnError::OutcomeUnknown)
        } else {
            Ok(())
        }
    }
    pub(super) fn reap(&mut self) -> bool {
        let root = self.proc.as_ref().expect("original created process");
        let killed = unsafe { TerminateProcess(root.as_raw_handle().cast(), 1) } != 0;
        let exited =
            unsafe { WaitForSingleObject(root.as_raw_handle().cast(), 500) } == WAIT_OBJECT_0;
        if exited {
            self.cleanup = false;
        }
        killed && exited
    }
}
impl Drop for PendingJobChild {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = self.reap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandBuilder;
    use std::os::windows::io::FromRawHandle;
    use std::{
        mem, ptr,
        time::{Duration, Instant},
    };
    use winapi::um::handleapi::GetHandleInformation;
    use winapi::um::processthreadsapi::{CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW};
    use winapi::um::winbase::{CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT};

    /// Real suspended root/original handles, passed through the same Unknown
    /// constructor used by ResumeThread/reap failure. No fabricated OS denial.
    fn suspended_self() -> PendingJobChild {
        let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
        command.arg("--list");
        command.env_clear();
        command.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
        let (mut exe, mut line) = command.cmdline().unwrap();
        let mut environment = command.environment_block();
        let mut startup: STARTUPINFOW = unsafe { mem::zeroed() };
        startup.cb = mem::size_of::<STARTUPINFOW>() as u32;
        let mut info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
        assert_ne!(
            unsafe {
                CreateProcessW(
                    exe.as_mut_ptr(),
                    line.as_mut_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                    CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                    environment.as_mut_ptr().cast(),
                    ptr::null(),
                    &mut startup,
                    &mut info,
                )
            },
            0
        );
        // Acquire RAII ownership immediately; this root is never resumed.
        PendingJobChild {
            proc: Some(unsafe { OwnedHandle::from_raw_handle(info.hProcess.cast()) }),
            thread: unsafe { OwnedHandle::from_raw_handle(info.hThread.cast()) },
            cleanup: true,
            assigned: false,
            resume_attempted: false,
        }
    }
    #[test]
    #[allow(non_snake_case)]
    fn OwnedJob_UnknownEnvelopeRetainsRealOriginalHandles_003() {
        let pending = suspended_self();
        let failure = OwnedJobSpawnFailure::after_create(JobSpawnError::OutcomeUnknown, pending);
        assert_eq!(failure.classification(), JobSpawnError::OutcomeUnknown);
        let (_, pending) = failure.into_parts();
        let mut pending = pending.unwrap();
        let mut flags = 0;
        assert_ne!(
            unsafe {
                GetHandleInformation(
                    pending.proc.as_ref().unwrap().as_raw_handle().cast(),
                    &mut flags,
                )
            },
            0
        );
        assert_ne!(
            unsafe { GetHandleInformation(pending.thread.as_raw_handle().cast(), &mut flags) },
            0
        );
        assert!(!pending.try_wait_root().unwrap());
        pending.request_root_termination().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !pending.try_wait_root().unwrap() {
            assert!(Instant::now() < until, "self fixture root did not signal");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!pending.cleanup);
    }

    #[test]
    #[allow(non_snake_case)]
    fn OwnedJob_RealUnexpectedResumeCountReturnsOwnedUnknown_006() {
        use winapi::um::jobapi2::{
            AssignProcessToJobObject, CreateJobObjectW, QueryInformationJobObject,
            SetInformationJobObject,
        };
        use winapi::um::winnt::{
            JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        let mut root = suspended_self();
        let raw = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        assert!(!raw.is_null());
        let job = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        assert_ne!(
            unsafe {
                SetInformationJobObject(
                    raw,
                    JobObjectExtendedLimitInformation,
                    &mut limits as *mut _ as _,
                    mem::size_of_val(&limits) as u32,
                )
            },
            0
        );
        assert_ne!(
            unsafe {
                AssignProcessToJobObject(raw, root.proc.as_ref().unwrap().as_raw_handle().cast())
            },
            0
        );
        root.assigned = true;
        // Own fixed suspended fixture only: increase count to 2, so the REAL
        // production ResumeThread path sees 2 and leaves it suspended at 1.
        assert_eq!(
            unsafe {
                winapi::um::processthreadsapi::SuspendThread(root.thread.as_raw_handle().cast())
            },
            1
        );
        let failure = root
            .resume_once()
            .expect_err("count two cannot become a successful launch");
        assert_eq!(failure.classification(), JobSpawnError::OutcomeUnknown);
        let (_, pending) = failure.into_parts();
        let mut root = pending.unwrap();
        assert!(root.assigned_to_job());
        assert!(root.resume_was_attempted());
        assert!(root.try_wait_root().unwrap());
        let mut information: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { mem::zeroed() };
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    job.as_raw_handle().cast(),
                    JobObjectBasicAccountingInformation,
                    &mut information as *mut _ as _,
                    mem::size_of_val(&information) as u32,
                    ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(information.ActiveProcesses, 0);
        let mut flags = 0;
        assert_ne!(
            unsafe { GetHandleInformation(root.thread.as_raw_handle().cast(), &mut flags) },
            0
        );
    }
}
