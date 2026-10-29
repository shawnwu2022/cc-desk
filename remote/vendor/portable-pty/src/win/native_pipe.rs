//! CC Desk 限定资格 seam：不使用 ConPTY，不查询任何全局进程/线程。
use super::procthreadattr::ProcThreadAttributeList;
use crate::{CommandBuilder, JobSpawnError, JobSpawnPhase};
use std::{
    fs::File,
    io::{self, Read, Write},
    mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
    time::{Duration, Instant},
};
use winapi::{
    shared::{minwindef::TRUE, winerror::WAIT_TIMEOUT},
    um::{
        handleapi::SetHandleInformation,
        jobapi::IsProcessInJob,
        jobapi2::AssignProcessToJobObject,
        minwinbase::SECURITY_ATTRIBUTES,
        namedpipeapi::CreatePipe,
        processthreadsapi::{
            CreateProcessW, GetExitCodeProcess, ResumeThread, TerminateProcess, PROCESS_INFORMATION,
        },
        synchapi::WaitForSingleObject,
        winbase::{
            CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, DETACHED_PROCESS,
            EXTENDED_STARTUPINFO_PRESENT, STARTF_USESTDHANDLES, STARTUPINFOEXW, WAIT_OBJECT_0,
        },
        winnt::HANDLE,
    },
};
const QUALIFICATION: Duration = Duration::from_millis(500);
const CLEANUP: Duration = Duration::from_millis(500);
const INHERIT: u32 = 1;
fn preparation() -> JobSpawnError {
    JobSpawnError::KnownNotStarted {
        phase: JobSpawnPhase::Preparation,
    }
}
fn phase(stage: &'static str) {
    #[cfg(debug_assertions)]
    eprintln!("NATIVE_PIPE_PHASE={stage}");
    let _ = stage;
}
fn pair(parent_reads: bool) -> Result<(OwnedHandle, OwnedHandle), JobSpawnError> {
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: TRUE,
    };
    if unsafe { CreatePipe(&mut read, &mut write, &mut sa, 4096) } == 0 {
        return Err(preparation());
    }
    let read = unsafe { OwnedHandle::from_raw_handle(read.cast()) };
    let write = unsafe { OwnedHandle::from_raw_handle(write.cast()) };
    let (parent, child) = if parent_reads {
        (read, write)
    } else {
        (write, read)
    };
    if unsafe { SetHandleInformation(parent.as_raw_handle().cast(), INHERIT, 0) } == 0 {
        return Err(preparation());
    }
    Ok((parent, child))
}
fn root_exit(process: HANDLE) -> Result<Option<u32>, JobSpawnError> {
    match unsafe { WaitForSingleObject(process, 0) } {
        WAIT_OBJECT_0 => {
            let mut code = 0;
            if unsafe { GetExitCodeProcess(process, &mut code) } == 0 {
                return Err(JobSpawnError::OutcomeUnknown);
            }
            Ok(Some(code))
        }
        WAIT_TIMEOUT => Ok(None),
        _ => Err(JobSpawnError::OutcomeUnknown),
    }
}
struct Created {
    process: Option<OwnedHandle>,
    thread: OwnedHandle,
    active: bool,
    cleanup_end: Option<Instant>,
}
impl Created {
    fn reap(&mut self) -> bool {
        let now = Instant::now();
        let end = *self
            .cleanup_end
            .get_or_insert_with(|| now.checked_add(CLEANUP).unwrap_or(now));
        let process = self.process.as_ref().unwrap().as_raw_handle().cast();
        unsafe {
            TerminateProcess(process, 1);
        }
        while Instant::now() < end {
            if matches!(root_exit(process), Ok(Some(_))) && Instant::now() < end {
                self.active = false;
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }
}
impl Drop for Created {
    fn drop(&mut self) {
        if self.active {
            let _ = self.reap();
        }
    }
}
/// 同步匿名管道只委托 File；不假装满足 std Child* 的异步句柄前提。
pub struct NativePipeInput(File);
impl Write for NativePipeInput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
pub struct NativePipeOutput(File);
impl Read for NativePipeOutput {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}
/// 只持有本次 CreateProcessW 的确切 process 句柄；无 PID 收养/授权接口。
pub struct NativePipeChild {
    process: OwnedHandle,
    input: Option<NativePipeInput>,
    output: Option<NativePipeOutput>,
    error: Option<NativePipeOutput>,
}
impl NativePipeChild {
    pub fn take_stdin(&mut self) -> Option<NativePipeInput> {
        self.input.take()
    }
    pub fn take_stdout(&mut self) -> Option<NativePipeOutput> {
        self.output.take()
    }
    pub fn take_stderr(&mut self) -> Option<NativePipeOutput> {
        self.error.take()
    }
    pub fn try_wait_root(&self) -> Result<Option<u32>, JobSpawnError> {
        root_exit(self.process.as_raw_handle().cast())
    }
    pub fn wait_root_until(&self, end: Instant) -> Result<u32, JobSpawnError> {
        let process = self.process.as_raw_handle().cast();
        loop {
            if Instant::now() >= end {
                return Err(JobSpawnError::OutcomeUnknown);
            }
            if let Some(code) = root_exit(process)? {
                if Instant::now() < end {
                    return Ok(code);
                }
                return Err(JobSpawnError::OutcomeUnknown);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub fn stop_root_until(&self, end: Instant) -> Result<u32, JobSpawnError> {
        if !matches!(self.try_wait_root(), Ok(Some(_))) {
            unsafe {
                TerminateProcess(self.process.as_raw_handle().cast(), 1);
            }
        }
        self.wait_root_until(end)
    }
}
impl Drop for NativePipeChild {
    fn drop(&mut self) {
        unsafe {
            TerminateProcess(self.process.as_raw_handle().cast(), 1);
        }
    }
}
/// 后端 CommandBuilder 不是网络 DTO；此低层函数不证明应用 admission。
pub fn spawn_pipe_in_job(
    cmd: CommandBuilder,
    job: BorrowedHandle<'_>,
) -> Result<NativePipeChild, JobSpawnError> {
    phase("prepare");
    let program = cmd.get_argv().first().ok_or_else(preparation)?;
    let cwd = cmd.get_cwd().ok_or_else(preparation)?;
    if !Path::new(program).is_absolute()
        || !Path::new(cwd).is_absolute()
        || !Path::new(cwd).is_dir()
        || cmd.get_argv().len() > 65
    {
        return Err(preparation());
    }
    let expected_exe: Vec<u16> = program.encode_wide().chain(Some(0)).collect();
    let expected_cwd: Vec<u16> = cwd.encode_wide().chain(Some(0)).collect();
    let (mut exe, mut cmdline) = cmd.cmdline().map_err(|_| preparation())?;
    let actual_cwd = cmd.current_directory().ok_or_else(preparation)?;
    let mut env = cmd.environment_block();
    if env.len() == 1 {
        env.push(0);
    }
    if exe != expected_exe
        || actual_cwd != expected_cwd
        || cmdline.len() > 32767
        || env.len() > 32767
    {
        return Err(preparation());
    }
    let (input, child_input) = pair(false)?;
    let (output, child_output) = pair(true)?;
    let (error, child_error) = pair(true)?;
    phase("pipes_created");
    let mut handles = [
        child_input.as_raw_handle().cast(),
        child_output.as_raw_handle().cast(),
        child_error.as_raw_handle().cast(),
    ];
    let mut attrs = ProcThreadAttributeList::with_capacity(1).map_err(|_| preparation())?;
    attrs
        .set_pipe_handles(&mut handles)
        .map_err(|_| preparation())?;
    let mut si: STARTUPINFOEXW = unsafe { mem::zeroed() };
    si.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput = handles[0];
    si.StartupInfo.hStdOutput = handles[1];
    si.StartupInfo.hStdError = handles[2];
    si.lpAttributeList = attrs.as_mut_ptr();
    let mut pi: PROCESS_INFORMATION = unsafe { mem::zeroed() };
    phase("create_process_begin");
    let result = unsafe {
        CreateProcessW(
            exe.as_mut_ptr(),
            cmdline.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            TRUE,
            CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT
                | DETACHED_PROCESS,
            env.as_mut_ptr().cast(),
            actual_cwd.as_ptr(),
            &mut si.StartupInfo,
            &mut pi,
        )
    };
    if result == 0 {
        return Err(JobSpawnError::KnownNotStarted {
            phase: JobSpawnPhase::CreateRejected,
        });
    }
    // CreateProcessW 不可抢占；500ms 资格从实际返回/接管处开始，不重置。
    let started = Instant::now();
    let mut owned = Created {
        process: Some(unsafe { OwnedHandle::from_raw_handle(pi.hProcess.cast()) }),
        thread: unsafe { OwnedHandle::from_raw_handle(pi.hThread.cast()) },
        active: true,
        cleanup_end: None,
    };
    phase("create_process_returned");
    let end = started
        .checked_add(QUALIFICATION)
        .ok_or(JobSpawnError::OutcomeUnknown)?;
    let process = owned.process.as_ref().unwrap().as_raw_handle().cast();
    if Instant::now() >= end {
        return Err(JobSpawnError::OutcomeUnknown);
    }
    if unsafe { AssignProcessToJobObject(job.as_raw_handle().cast(), process) } == 0 {
        let reaped = owned.reap();
        return Err(if reaped && Instant::now() < end {
            JobSpawnError::KnownNotStarted {
                phase: JobSpawnPhase::AssignmentReaped,
            }
        } else {
            JobSpawnError::OutcomeUnknown
        });
    }
    phase("assigned");
    let mut member = 0;
    if unsafe { IsProcessInJob(process, job.as_raw_handle().cast(), &mut member) } == 0
        || member == 0
        || Instant::now() >= end
    {
        return Err(JobSpawnError::OutcomeUnknown);
    }
    if unsafe { ResumeThread(owned.thread.as_raw_handle().cast()) } != 1 {
        return Err(JobSpawnError::OutcomeUnknown);
    }
    phase("primary_resumed");
    if Instant::now() >= end {
        return Err(JobSpawnError::OutcomeUnknown);
    }
    owned.active = false;
    let child = NativePipeChild {
        process: owned.process.take().unwrap(),
        input: Some(NativePipeInput(File::from(input))),
        output: Some(NativePipeOutput(File::from(output))),
        error: Some(NativePipeOutput(File::from(error))),
    };
    phase("published");
    if Instant::now() >= end {
        let cleanup_end = Instant::now()
            .checked_add(CLEANUP)
            .unwrap_or_else(Instant::now);
        let _ = child.stop_root_until(cleanup_end);
        return Err(JobSpawnError::OutcomeUnknown);
    }
    Ok(child)
}
