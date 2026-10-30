use crate::RuntimeError;
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::Child,
    time::{Duration, Instant},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{ERROR_NO_MORE_FILES, HANDLE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{GetProcessIdOfThread, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    },
};
pub(crate) struct Job(OwnedHandle);
#[cfg(test)]
thread_local! {static LAST_RESUME_COUNT: std::cell::Cell<Option<u32>> = const {std::cell::Cell::new(None)};}
#[cfg(test)]
pub(crate) fn take_resume_count_for_test() -> Option<u32> {
    LAST_RESUME_COUNT.with(std::cell::Cell::take)
}
fn own(handle: HANDLE) -> OwnedHandle {
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}
impl Job {
    #[cfg(feature = "conpty-qualification")]
    pub(crate) fn active_processes(&self) -> Result<u32, RuntimeError> {
        use windows::Win32::System::JobObjects::{
            JobObjectBasicAccountingInformation, QueryInformationJobObject,
        };
        read_activity(
            HANDLE(self.0.as_raw_handle()),
            |handle, information| unsafe {
                QueryInformationJobObject(
                Some(handle),
                JobObjectBasicAccountingInformation,
                (information as *mut windows::Win32::System::JobObjects::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(information) as u32,
                None,
            )
            },
        )
    }
    #[cfg(feature = "conpty-qualification")]
    #[allow(dead_code)] // Source-reused only by the test=false qualification worker.
    pub(crate) fn borrowed(&self) -> std::os::windows::io::BorrowedHandle<'_> {
        use std::os::windows::io::AsHandle;
        self.0.as_handle()
    }
    pub(crate) fn new() -> Result<Self, RuntimeError> {
        // No name, inheritable security attributes or breakaway flag.
        let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|_| RuntimeError::JobSetupFailed)?;
        let job = Self(own(raw));
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                HANDLE(job.0.as_raw_handle()),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        }
        .map_err(|_| RuntimeError::JobSetupFailed)?;
        Ok(job)
    }
    pub(crate) fn assign(&self, child: &Child) -> Result<(), RuntimeError> {
        unsafe {
            AssignProcessToJobObject(
                HANDLE(self.0.as_raw_handle()),
                HANDLE(child.as_raw_handle()),
            )
        }
        .map_err(|_| RuntimeError::AssignmentFailedReaped)
    }
    pub(crate) fn resume(&self, child: &Child) -> Result<(), RuntimeError> {
        let end = Instant::now() + Duration::from_millis(500);
        let snapshot = own(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
            .map_err(|_| RuntimeError::OutcomeUnknown)?);
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        unsafe { Thread32First(HANDLE(snapshot.as_raw_handle()), &mut entry) }
            .map_err(|_| RuntimeError::OutcomeUnknown)?;
        let mut scanned = 0;
        let mut thread_id = None;
        loop {
            scanned += 1;
            if scanned > 65536 || Instant::now() >= end {
                return Err(RuntimeError::OutcomeUnknown);
            }
            if entry.th32OwnerProcessID == child.id()
                && thread_id.replace(entry.th32ThreadID).is_some()
            {
                #[cfg(test)]
                eprintln!("TEST_ONLY_RESUME_FAILURE=multiple-owned-threads");
                return Err(RuntimeError::OutcomeUnknown);
            }
            if let Err(error) =
                unsafe { Thread32Next(HANDLE(snapshot.as_raw_handle()), &mut entry) }
            {
                if error.code() != windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
                    return Err(RuntimeError::OutcomeUnknown);
                }
                break;
            }
        }
        let thread = own(unsafe {
            OpenThread(
                THREAD_SUSPEND_RESUME
                    | windows::Win32::System::Threading::THREAD_QUERY_LIMITED_INFORMATION,
                false,
                thread_id.ok_or(RuntimeError::OutcomeUnknown)?,
            )
        }
        .map_err(|_| RuntimeError::OutcomeUnknown)?);
        if unsafe { GetProcessIdOfThread(HANDLE(thread.as_raw_handle())) } != child.id() {
            return Err(RuntimeError::OutcomeUnknown);
        }
        // A newly-created suspended primary thread must have exactly one suspend count.
        if Instant::now() >= end {
            return Err(RuntimeError::OutcomeUnknown);
        }
        let previous = unsafe { ResumeThread(HANDLE(thread.as_raw_handle())) };
        #[cfg(test)]
        LAST_RESUME_COUNT.with(|n| n.set(Some(previous)));
        if previous != 1 {
            return Err(RuntimeError::OutcomeUnknown);
        }
        Ok(())
    }
}

#[cfg(feature = "conpty-qualification")]
fn read_activity(
    handle: HANDLE,
    query: impl FnOnce(
        HANDLE,
        &mut windows::Win32::System::JobObjects::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    ) -> windows::core::Result<()>,
) -> Result<u32, RuntimeError> {
    let mut information =
        windows::Win32::System::JobObjects::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    query(handle, &mut information).map_err(|_| RuntimeError::OutcomeUnknown)?;
    Ok(information.ActiveProcesses)
}

#[cfg(all(test, feature = "conpty-qualification"))]
pub(crate) fn non_job_for_activity_test() -> Job {
    let event =
        unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }
            .unwrap();
    Job(own(event))
}

#[cfg(all(test, feature = "conpty-qualification"))]
mod activity_tests {
    use super::*;
    use windows::Win32::System::JobObjects::{
        JobObjectExtendedLimitInformation, QueryInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    };
    #[test]
    fn job_activity_empty_query_preserves_owned_handle_and_limits() {
        let job = Job::new().unwrap();
        let handle = job.0.as_raw_handle();
        let mut before = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(HANDLE(handle)),
                JobObjectExtendedLimitInformation,
                (&mut before as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&before) as u32,
                None,
            )
        }
        .unwrap();
        assert_eq!(job.active_processes(), Ok(0));
        assert_eq!(job.0.as_raw_handle(), handle);
        let mut after = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(HANDLE(handle)),
                JobObjectExtendedLimitInformation,
                (&mut after as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&after) as u32,
                None,
            )
        }
        .unwrap();
        assert_eq!(
            before.BasicLimitInformation.LimitFlags,
            after.BasicLimitInformation.LimitFlags
        );
        assert_eq!(
            after.BasicLimitInformation.LimitFlags,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        );
    }
    #[test]
    fn job_activity_wrong_owned_object_is_unknown_not_zero() {
        // A live self-owned event induces a real native query error without any
        // invalid/dangling handle or external process. Only tests can forge Job.
        let wrong_kind = non_job_for_activity_test();
        assert_eq!(
            wrong_kind.active_processes(),
            Err(RuntimeError::OutcomeUnknown)
        );
    }
    #[test]
    fn job_activity_adapter_keeps_active_count_not_total_or_default() {
        let job = Job::new().unwrap();
        let handle = HANDLE(job.0.as_raw_handle());
        for active in [0, 3, u32::MAX] {
            let actual = read_activity(handle, |queried, information| {
                assert_eq!(queried, handle);
                information.ActiveProcesses = active;
                information.TotalProcesses = 27;
                Ok(())
            });
            assert_eq!(actual, Ok(active));
        }
    }
    #[test]
    fn job_activity_adapter_native_error_is_unknown() {
        let job = Job::new().unwrap();
        let handle = HANDLE(job.0.as_raw_handle());
        let actual = read_activity(handle, |queried, _| {
            assert_eq!(queried, handle);
            Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                0x80070005u32 as i32,
            )))
        });
        assert_eq!(actual, Err(RuntimeError::OutcomeUnknown));
    }
}
