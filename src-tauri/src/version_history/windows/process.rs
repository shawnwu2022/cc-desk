//! Explicit suspended creation, creation-time job membership and immutable
//! receipts. No shell, post-create assignment, PID-name scan or automatic replay.
use super::{
    blocked,
    durability::DurableRecord,
    files::{
        validate_absolute, ComponentName, Directory, FileAccess, FileIdentity, PinnedFile,
        PrivateDirectory,
    },
    handle,
    lease::ExclusiveLease,
    own,
    security::CurrentUser,
    win_error,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    io,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::OwnedHandle,
    },
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use windows::Win32::{
    Foundation::{
        GetLastError, SetLastError, ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
        FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        JobObjects::{
            CreateJobObjectW, IsProcessInJob, JobObjectBasicAccountingInformation,
            JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, OpenJobObjectW,
            QueryInformationJobObject, SetInformationJobObject,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcessId, GetExitCodeProcess,
            GetProcessId, GetProcessTimes, InitializeProcThreadAttributeList, OpenProcess,
            QueryFullProcessImageNameW, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
            WaitForSingleObject, CREATE_BREAKAWAY_FROM_JOB, CREATE_SUSPENDED,
            CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST,
            PROCESS_ACCESS_RIGHTS, PROCESS_INFORMATION, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTUPINFOEXW,
        },
    },
};
use windows_core::{BOOL, PCWSTR, PWSTR};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum JobKind {
    Installer,
    HistoricalApplication,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProcessIdentity {
    pid: u32,
    created: u64,
    session: u32,
    image: FileIdentity,
    image_digest: String,
}
impl ProcessIdentity {
    pub(super) fn session(&self) -> u32 {
        self.session
    }
    pub(super) fn validate(&self) -> io::Result<()> {
        if self.pid == 0
            || self.created == 0
            || self.image_digest.len() != 64
            || !self
                .image_digest
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(blocked("invalid persisted process identity"));
        }
        Ok(())
    }
}
pub(crate) struct ExactProcess {
    process: OwnedHandle,
    identity: ProcessIdentity,
    _image: PinnedFile,
}
impl ExactProcess {
    /// Revalidate a scope-side image against this exact retained process. A
    /// pathname or equal bytes at a different file object are not sufficient.
    pub(crate) fn verify_held_image(&self, image: &PinnedFile) -> io::Result<()> {
        self._image.verify()?;
        image.verify()?;
        if identity(handle(&self.process), image)? != self.identity {
            return Err(blocked("held source image differs from the exact process"));
        }
        Ok(())
    }
    pub(crate) fn verify_current_user(&self, user: &CurrentUser) -> io::Result<()> {
        user.verify_process_user(handle(&self.process))?;
        self.verify_held_image(&self._image)
    }
    /// Locator for registered-source re-admission, derived from this exact
    /// retained image. The locator alone never authorizes opening or launch.
    pub(crate) fn observed_image_path(&self) -> io::Result<std::path::PathBuf> {
        self.verify_held_image(&self._image)?;
        let path = super::manager_process::launch_path(handle(&self._image.file))?;
        self.verify_held_image(&self._image)?;
        Ok(path.into())
    }
    pub(super) fn from_created(process: OwnedHandle, image: PinnedFile) -> io::Result<Self> {
        let identity = identity(handle(&process), &image)?;
        Ok(Self {
            process,
            identity,
            _image: image,
        })
    }
    /// A PID is accepted only from a live backend OS/COM observation, never a
    /// frontend field or executable-name search. Reopen uses recorded identity.
    pub(crate) fn capture_observed(pid: u32) -> io::Result<Self> {
        Self::capture_with_access(pid, PROCESS_ACCESS_RIGHTS(0))
    }
    fn capture_with_access(pid: u32, extra: PROCESS_ACCESS_RIGHTS) -> io::Result<Self> {
        if pid == 0 {
            return Err(blocked("missing observed process"));
        }
        let process = unsafe {
            own(OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | extra,
                false,
                pid,
            )
            .map_err(win_error)?)
        };
        let image = process_image(handle(&process))?;
        Self::from_created(process, image)
    }
    pub(crate) fn reopen(expected: &ProcessIdentity) -> io::Result<Self> {
        let process = Self::capture_observed(expected.pid)?;
        if process.identity != *expected {
            return Err(blocked("recorded process identity changed"));
        }
        Ok(process)
    }
    pub(crate) fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }
    pub(super) fn is_in_job(&self, job: Option<HANDLE>) -> io::Result<bool> {
        let mut assigned = BOOL(0);
        unsafe { IsProcessInJob(handle(&self.process), job, &mut assigned) }.map_err(win_error)?;
        Ok(assigned.as_bool())
    }
    pub(crate) fn pid(&self) -> u32 {
        self.identity.pid
    }
    /// Consume the live-process observation only after an exact terminal wait.
    /// Release the file guard that would conflict with share-zero image fencing,
    /// while preserving the original process handle and its terminal receipt.
    pub(crate) fn release_terminated_image(self) -> io::Result<TerminatedProcess> {
        let terminal = self
            .terminal(0)?
            .ok_or_else(|| blocked("source process has not exited"))?;
        let Self {
            process,
            identity: _,
            _image: image,
        } = self;
        let image_parent = image.parent.clone();
        drop(image);
        Ok(TerminatedProcess {
            process,
            terminal,
            _image_parent: image_parent,
        })
    }
    /// Retain terminal evidence from this same kernel object. Reopening a PID or
    /// querying its image after exit is unnecessary and may no longer succeed.
    pub(super) fn retain_terminal(&self) -> io::Result<TerminatedProcess> {
        let terminal = self
            .terminal(0)?
            .ok_or_else(|| blocked("process has not exited"))?;
        Ok(TerminatedProcess {
            process: self.process.try_clone()?,
            terminal,
            _image_parent: self._image.parent.clone(),
        })
    }
    pub(crate) fn terminal(&self, timeout_ms: u32) -> io::Result<Option<TerminalReceipt>> {
        if timeout_ms == u32::MAX {
            return Err(blocked("unbounded recovery wait is unsupported"));
        }
        match unsafe { WaitForSingleObject(handle(&self.process), timeout_ms) } {
            WAIT_OBJECT_0 => {
                let mut exit_code = 0;
                unsafe {
                    GetExitCodeProcess(handle(&self.process), &mut exit_code).map_err(win_error)?;
                }
                Ok(Some(TerminalReceipt {
                    identity: self.identity.clone(),
                    exit_code,
                }))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }
}
pub(crate) struct TerminatedProcess {
    process: OwnedHandle,
    terminal: TerminalReceipt,
    _image_parent: Arc<Directory>,
}
impl TerminatedProcess {
    pub(crate) fn identity(&self) -> &ProcessIdentity {
        &self.terminal.identity
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        if unsafe { WaitForSingleObject(handle(&self.process), 0) } != WAIT_OBJECT_0 {
            return Err(blocked("retained terminal process evidence changed"));
        }
        let mut code = 0;
        unsafe {
            GetExitCodeProcess(handle(&self.process), &mut code).map_err(win_error)?;
        }
        if code != self.terminal.exit_code {
            return Err(blocked("terminal process exit code differs"));
        }
        Ok(())
    }
}
pub(crate) struct TerminalReceipt {
    identity: ProcessIdentity,
    exit_code: u32,
}
impl TerminalReceipt {
    pub(crate) fn exit_code(&self) -> u32 {
        self.exit_code
    }
}
pub(super) fn session_id(pid: u32) -> io::Result<u32> {
    let mut session = 0;
    unsafe {
        ProcessIdToSessionId(pid, &mut session).map_err(win_error)?;
    }
    Ok(session)
}
pub(super) fn creation_time(process: HANDLE) -> io::Result<u64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)
            .map_err(win_error)?;
    }
    Ok(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
}
fn process_image(process: HANDLE) -> io::Result<PinnedFile> {
    let mut text = vec![0u16; 32768];
    let mut length = text.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(text.as_mut_ptr()),
            &mut length,
        )
        .map_err(win_error)?;
    }
    text.truncate(length as usize);
    let path = OsString::from_wide(&text);
    let path = Path::new(&path);
    let parent = Directory::open_absolute(
        path.parent()
            .ok_or_else(|| blocked("process has no image parent"))?,
    )?;
    parent.open_file(
        ComponentName::new(
            path.file_name()
                .ok_or_else(|| blocked("process has no image filename"))?,
        )?,
        FileAccess::Read,
    )
}
fn identity(process: HANDLE, image: &PinnedFile) -> io::Result<ProcessIdentity> {
    let created = creation_time(process)?;
    let actual = process_image(process)?;
    if actual.identity() != image.identity() {
        return Err(blocked("created process image differs"));
    }
    let pid = unsafe { GetProcessId(process) };
    if pid == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ProcessIdentity {
        pid,
        created,
        session: session_id(pid)?,
        image: image.identity().clone(),
        image_digest: image.digest()?,
    })
}

pub(crate) struct CommandLine {
    application: OsString,
    text: String,
    #[cfg(test)]
    environment: Option<Vec<u16>>,
}
impl CommandLine {
    pub(crate) fn nsis(application: &OsStr, installation: &OsStr) -> io::Result<Self> {
        let app = exact_drive_path(application)?;
        let target = exact_drive_path(installation)?;
        // NSIS parses /D as the final unquoted remainder, including spaces.
        Ok(Self {
            application: application.to_owned(),
            text: format!("\"{app}\" /S /UPDATE /NS /D={target}"),
            #[cfg(test)]
            environment: None,
        })
    }
    pub(crate) fn historical(application: &OsStr) -> io::Result<Self> {
        let app = exact_drive_path(application)?;
        Ok(Self {
            application: application.to_owned(),
            text: format!("\"{app}\""),
            #[cfg(test)]
            environment: None,
        })
    }
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
    fn verify_image(&self, image: &PinnedFile) -> io::Result<()> {
        let path = Path::new(&self.application);
        let parent = Directory::open_absolute(
            path.parent()
                .ok_or_else(|| blocked("missing executable parent"))?,
        )?;
        let other = parent.open_file(
            ComponentName::new(
                path.file_name()
                    .ok_or_else(|| blocked("missing executable name"))?,
            )?,
            FileAccess::Read,
        )?;
        if other.identity() != image.identity() {
            return Err(blocked("command is bound to a different image"));
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn probe_controlled(application: &Path, marker: &Path) -> io::Result<Self> {
        let mut command = Self::probe(application, marker)?;
        command.probe_variable(
            "CC_DESK_HISTORY_PROBE_RELEASE",
            marker.with_extension("release").as_os_str(),
        );
        Ok(command)
    }
    #[cfg(test)]
    pub(crate) fn probe_wait(application: &Path, marker: &Path) -> io::Result<Self> {
        let mut command = Self::probe(application, marker)?;
        command.probe_variable("CC_DESK_HISTORY_PROBE_WAIT", OsStr::new("1"));
        Ok(command)
    }
    #[cfg(test)]
    fn probe_variable(&mut self, name: &str, value: &OsStr) {
        let old = self.environment.take().expect("probe environment");
        let mut entries: Vec<Vec<u16>> = old
            .split(|unit| *unit == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.to_vec())
            .collect();
        let mut entry: Vec<u16> = name.encode_utf16().collect();
        entry.push(b'=' as u16);
        entry.extend(value.encode_wide());
        entries.push(entry);
        entries.sort_by_key(|entry| String::from_utf16_lossy(entry).to_uppercase());
        let mut environment = vec![];
        for entry in entries {
            environment.extend(entry);
            environment.push(0);
        }
        environment.push(0);
        self.environment = Some(environment);
    }
    #[cfg(test)]
    pub(crate) fn probe(application: &Path, marker: &Path) -> io::Result<Self> {
        let app = exact_drive_path(application.as_os_str())?;
        let mut variables: Vec<_> = std::env::vars_os()
            .filter(|(key, _)| {
                !key.to_string_lossy()
                    .to_uppercase()
                    .starts_with("CC_DESK_HISTORY_PROBE_")
            })
            .collect();
        variables.push((
            OsString::from("CC_DESK_HISTORY_PROBE_MARKER"),
            marker.as_os_str().to_owned(),
        ));
        variables.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
        let mut environment = vec![];
        for (key, value) in variables {
            environment.extend(key.encode_wide());
            environment.push(b'=' as u16);
            environment.extend(value.encode_wide());
            environment.push(0);
        }
        environment.push(0);
        Ok(Self { application: application.as_os_str().to_owned(), text: format!("\"{app}\" --exact tests::version_history_windows::HistoryWindows_ProcessWorker_013 --ignored --nocapture"), environment: Some(environment) })
    }
}
fn exact_drive_path(value: &OsStr) -> io::Result<String> {
    let units: Vec<_> = value.encode_wide().collect();
    validate_absolute(&units)?;
    let text = String::from_utf16(&units).map_err(|_| blocked("unrepresentable process path"))?;
    if text.chars().any(char::is_control) || text.contains('"') || text.ends_with([' ', '.']) {
        return Err(blocked("unsafe process path"));
    }
    Ok(text)
}

// 启动工作目录保留同一对象的盘符路径；卷 GUID 路径只用于身份核对。
fn launch_directory(parent: &Arc<Directory>) -> io::Result<(Arc<Directory>, OsString)> {
    parent.recheck()?;
    let path = super::manager_process::launch_path(parent.raw())?;
    let retained = Directory::open_absolute(Path::new(&path))?;
    if retained.identity() != parent.identity() {
        return Err(blocked("process current directory changed identity"));
    }
    parent.recheck()?;
    Ok((retained, path))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobIdentity {
    name: String,
    owner: String,
    session: u32,
    kind: JobKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum JobPhase {
    ArmedPreparation,
    HistoricalLifetime,
}
pub(crate) struct PrivateJob {
    handle: OwnedHandle,
    identity: JobIdentity,
    phase: Option<JobPhase>,
}
impl PrivateJob {
    fn new(identity: JobIdentity, user: &CurrentUser) -> io::Result<Self> {
        let descriptor = user.job_descriptor()?;
        let attributes = descriptor.attributes();
        let name: Vec<_> = identity.name.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe {
            SetLastError(ERROR_SUCCESS);
            let raw =
                CreateJobObjectW(Some(&attributes), PCWSTR(name.as_ptr())).map_err(win_error)?;
            let last_error = GetLastError(); // Capture before any other Windows call.
            let owned = own(raw);
            if last_error == ERROR_ALREADY_EXISTS {
                return Err(blocked("private job name already exists"));
            }
            owned
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // No user code can run during preparation. Creation-time containment
        // must clean up even if the owner crashes before it records the PID.
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                super::handle(&handle),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(win_error)?;
        }
        let result = Self {
            handle,
            identity,
            phase: Some(JobPhase::ArmedPreparation),
        };
        user.verify_private_job(super::handle(&result.handle))?;
        result.verify_limits()?;
        Ok(result)
    }
    fn verify_limits(&self) -> io::Result<()> {
        let phase = self
            .phase
            .ok_or_else(|| blocked("job disarm outcome is unknown"))?;
        self.verify_phase(phase)
    }
    fn verify_phase(&self, phase: JobPhase) -> io::Result<()> {
        if session_id(unsafe { GetCurrentProcessId() })? != self.identity.session {
            return Err(blocked("named job belongs to another Windows session"));
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(handle(&self.handle)),
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                None,
            )
            .map_err(win_error)?;
        }
        let expected = match (self.identity.kind, phase) {
            (_, JobPhase::ArmedPreparation) => JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0,
            (JobKind::HistoricalApplication, JobPhase::HistoricalLifetime) => 0,
            _ => return Err(blocked("installer job cannot disarm")),
        };
        if limits.BasicLimitInformation.LimitFlags.0 != expected {
            return Err(blocked("job containment or lifetime differs"));
        }
        Ok(())
    }
    fn disarm_historical(&mut self) -> io::Result<()> {
        self.begin_historical_disarm()?;
        self.verify_phase(JobPhase::HistoricalLifetime)?;
        self.phase = Some(JobPhase::HistoricalLifetime);
        Ok(())
    }
    fn begin_historical_disarm(&mut self) -> io::Result<()> {
        if self.identity.kind != JobKind::HistoricalApplication
            || self.phase != Some(JobPhase::ArmedPreparation)
        {
            return Err(blocked("historical job is not armed preparation"));
        }
        self.verify_limits()?;
        // Mark unknown before the mutating call. No error path may re-arm a
        // possibly disarmed job; that could later kill user work on owner exit.
        self.phase = None;
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        unsafe {
            SetInformationJobObject(
                handle(&self.handle),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(win_error)?;
        }
        Ok(())
    }
    pub(crate) fn active_processes(&self) -> io::Result<u32> {
        self.verify_limits()?;
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(handle(&self.handle)),
                JobObjectBasicAccountingInformation,
                (&mut information as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
            .map_err(win_error)?;
        }
        Ok(information.ActiveProcesses)
    }
    fn contains(&self, process: &ExactProcess) -> io::Result<()> {
        self.contains_handle(handle(&process.process))
    }
    fn contains_handle(&self, process: HANDLE) -> io::Result<()> {
        self.verify_limits()?;
        let mut member = BOOL::default();
        unsafe {
            IsProcessInJob(process, Some(handle(&self.handle)), &mut member).map_err(win_error)?;
        }
        if !member.as_bool() {
            return Err(blocked("process is not in the recorded job"));
        }
        Ok(())
    }
    /// Absence or access denial stays unknown. Empty job does not prove an
    /// installer exit code, all UDF holders, or permission to replay a launch.
    pub(crate) fn reopen(receipt: &DurableProcessIdentity, user: &CurrentUser) -> io::Result<Self> {
        receipt.record.verify()?;
        Self::open_recorded(&receipt.binding.job, receipt.binding.job_phase, user)
    }
    fn open_recorded(
        identity: &JobIdentity,
        phase: JobPhase,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        if identity.owner != user.sid_text()
            || identity.session != session_id(unsafe { GetCurrentProcessId() })?
        {
            return Err(blocked("job owner or session differs"));
        }
        let name: Vec<_> = identity.name.encode_utf16().chain(Some(0)).collect();
        let raw = unsafe {
            OpenJobObjectW(0x0004 | 0x0002_0000, false, PCWSTR(name.as_ptr())).map_err(win_error)?
        };
        let result = Self {
            handle: unsafe { own(raw) },
            identity: identity.clone(),
            phase: Some(phase),
        };
        user.verify_private_job(handle(&result.handle))?;
        result.verify_limits()?;
        Ok(result)
    }
    fn only_member(&self) -> io::Result<u32> {
        self.verify_limits()?;
        let mut members = JOBOBJECT_BASIC_PROCESS_ID_LIST::default();
        unsafe {
            QueryInformationJobObject(
                Some(handle(&self.handle)),
                JobObjectBasicProcessIdList,
                (&mut members as *mut JOBOBJECT_BASIC_PROCESS_ID_LIST).cast(),
                size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() as u32,
                None,
            )
            .map_err(win_error)?;
        }
        if members.NumberOfAssignedProcesses != 1 || members.NumberOfProcessIdsInList != 1 {
            return Err(blocked(
                "never-resumed job does not have exactly one owned child",
            ));
        }
        u32::try_from(members.ProcessIdList[0])
            .map_err(|_| blocked("invalid recorded job process identity"))
    }
}

struct AttributeList {
    _storage: Vec<usize>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}
impl AttributeList {
    fn new(jobs: &[HANDLE]) -> io::Result<Self> {
        let mut size = 0;
        let sizing = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
        if sizing.is_ok()
            || sizing.err().map(|e| e.code()) != Some(ERROR_INSUFFICIENT_BUFFER.to_hresult())
            || size == 0
            || size > 65536
        {
            return Err(blocked("unsupported process attribute list"));
        }
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        unsafe {
            InitializeProcThreadAttributeList(Some(list), 1, None, &mut size).map_err(win_error)?;
        }
        let result = Self {
            _storage: storage,
            list,
        };
        unsafe {
            UpdateProcThreadAttribute(
                result.list,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                Some(jobs.as_ptr().cast()),
                std::mem::size_of_val(jobs),
                None,
                None,
            )
            .map_err(win_error)?;
        }
        Ok(result)
    }
}
impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.list);
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchIntent {
    schema: u32,
    launch: String,
    image: FileIdentity,
    image_digest: String,
    command_digest: String,
    job: JobIdentity,
    job_phase: JobPhase,
    lease: FileIdentity,
}
pub(crate) struct DurableLaunchIntent {
    record: DurableRecord,
    binding: LaunchIntent,
}
impl DurableLaunchIntent {
    pub(crate) fn open(record: DurableRecord, user: &CurrentUser) -> io::Result<Self> {
        record.verify()?;
        let binding: LaunchIntent =
            serde_json::from_slice(record.bytes()).map_err(io::Error::other)?;
        if binding.schema != 3
            || binding.job_phase != JobPhase::ArmedPreparation
            || binding.launch.len() != 32
            || !binding.launch.bytes().all(|c| c.is_ascii_hexdigit())
            || binding.job.owner != user.sid_text()
            || binding.job.name
                != format!(
                    "Local\\CCDeskRecovery-{}-{}",
                    user.sid_text(),
                    binding.launch
                )
            || binding.image_digest.len() != 64
            || binding.command_digest.len() != 64
        {
            return Err(blocked("invalid durable launch intent"));
        }
        Ok(Self { record, binding })
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityBinding {
    schema: u32,
    launch: String,
    intent: String,
    process: ProcessIdentity,
    command_digest: String,
    job: JobIdentity,
    job_phase: JobPhase,
    lease: FileIdentity,
}
pub(crate) struct DurableProcessIdentity {
    record: DurableRecord,
    binding: IdentityBinding,
}
impl DurableProcessIdentity {
    pub(crate) fn open(record: DurableRecord, user: &CurrentUser) -> io::Result<Self> {
        record.verify()?;
        let binding: IdentityBinding =
            serde_json::from_slice(record.bytes()).map_err(io::Error::other)?;
        if binding.schema != 3
            || binding.job_phase != JobPhase::ArmedPreparation
            || binding.launch.len() != 32
            || !binding.launch.bytes().all(|c| c.is_ascii_hexdigit())
            || binding.job.owner != user.sid_text()
            || binding.job.name
                != format!(
                    "Local\\CCDeskRecovery-{}-{}",
                    user.sid_text(),
                    binding.launch
                )
            || binding.intent.len() != 64
            || binding.command_digest.len() != 64
        {
            return Err(blocked("invalid persisted process binding"));
        }
        binding.process.validate()?;
        Ok(Self { record, binding })
    }
    pub(crate) fn reopen_process(&self) -> io::Result<ExactProcess> {
        self.record.verify()?;
        ExactProcess::reopen(&self.binding.process)
    }
    /// Actual held receipt bytes for protected journal observation, never a
    /// filename reopen or a replacement process authority.
    pub(crate) fn record_bytes(&self) -> io::Result<&[u8]> {
        self.record.verify()?;
        Ok(self.record.bytes())
    }
    pub(crate) fn record_reference(&self) -> io::Result<(ComponentName, FileIdentity, String)> {
        self.record.verify()?;
        Ok((
            self.record.name().clone(),
            self.record.file_identity().clone(),
            self.record.digest().into(),
        ))
    }
}
pub(crate) struct PreparedProcess<'lease> {
    process: ExactProcess,
    thread: OwnedHandle,
    job: PrivateJob,
    root: Arc<PrivateDirectory>,
    launch: String,
    command_digest: String,
    intent: DurableRecord,
    identity_persisted: bool,
    resume_attempted: bool,
    cancel_attempted: bool,
    cancel_identity: Option<Arc<DurableProcessIdentity>>,
    cancel_intent: Option<Arc<DurableRecord>>,
    cancel_custody_attempted: AtomicBool,
    resume_intent: Option<DurableRecord>,
    historical_lifetime: Option<DurableRecord>,
    lease: &'lease mut ExclusiveLease,
    manager_job: Option<&'lease super::manager_process::AdmittedManagerJob>,
}
impl<'lease> PreparedProcess<'lease> {
    /// Low-level creation primitive. The later coordinator must supply validated
    /// package/scope and commit its installer effect intent before reaching here.
    /// This primitive additionally persists its own exact launch intent first.
    pub(crate) fn create_suspended(
        image: PinnedFile,
        command: CommandLine,
        kind: JobKind,
        root: Arc<PrivateDirectory>,
        user: &CurrentUser,
        lease: &'lease mut ExclusiveLease,
    ) -> io::Result<Self> {
        Self::create_suspended_inner(image, command, kind, root, user, lease, None)
    }
    /// Only an admitted copied manager can request explicit breakaway into its
    /// installer/historical dedicated job. The general primitive never escapes.
    pub(crate) fn create_suspended_from_manager(
        image: PinnedFile,
        command: CommandLine,
        kind: JobKind,
        root: Arc<PrivateDirectory>,
        user: &CurrentUser,
        lease: &'lease mut ExclusiveLease,
        manager_job: &'lease super::manager_process::AdmittedManagerJob,
    ) -> io::Result<Self> {
        manager_job.verify_current()?;
        Self::create_suspended_inner(image, command, kind, root, user, lease, Some(manager_job))
    }
    fn create_suspended_inner(
        image: PinnedFile,
        command: CommandLine,
        kind: JobKind,
        root: Arc<PrivateDirectory>,
        user: &CurrentUser,
        lease: &'lease mut ExclusiveLease,
        manager_job: Option<&'lease super::manager_process::AdmittedManagerJob>,
    ) -> io::Result<Self> {
        lease.verify()?;
        root.verify(user)?;
        command.verify_image(&image)?;
        let launch = uuid::Uuid::new_v4().simple().to_string();
        let job_identity = JobIdentity {
            name: format!("Local\\CCDeskRecovery-{}-{launch}", user.sid_text()),
            owner: user.sid_text().into(),
            session: session_id(unsafe { GetCurrentProcessId() })?,
            kind,
        };
        let command_digest =
            crate::version_history::verified_package::sha256(command.text.as_bytes());
        let intent_bytes = serde_json::to_vec(&LaunchIntent {
            schema: 3,
            launch: launch.clone(),
            image: image.identity().clone(),
            image_digest: image.digest()?,
            command_digest: command_digest.clone(),
            job: job_identity.clone(),
            job_phase: JobPhase::ArmedPreparation,
            lease: lease.identity().clone(),
        })
        .map_err(io::Error::other)?;
        let intent = DurableRecord::create(
            root.clone(),
            ComponentName::new(OsStr::new(&format!("launch-{launch}.json")))?,
            &intent_bytes,
            user,
        )?;
        let job = PrivateJob::new(job_identity, user)?;
        let jobs = [handle(&job.handle)];
        let attributes = AttributeList::new(&jobs)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attributes.list;
        let application: Vec<_> = command.application.encode_wide().chain(Some(0)).collect();
        let mut line: Vec<_> = command.text.encode_utf16().chain(Some(0)).collect();
        if line.len() > 32767 {
            return Err(blocked("command line exceeds Windows limit"));
        }
        let (_current_directory_guard, current_directory) = launch_directory(&image.parent)?;
        let current_directory: Vec<_> = current_directory.encode_wide().chain(Some(0)).collect();
        let mut information = PROCESS_INFORMATION::default();
        #[cfg(test)]
        let environment = command.environment.as_ref().map(|v| v.as_ptr().cast());
        #[cfg(not(test))]
        let environment = None;
        let mut flags =
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;
        if let Some(manager) = manager_job {
            manager.verify_current()?;
            flags |= CREATE_BREAKAWAY_FROM_JOB;
        }
        unsafe {
            CreateProcessW(
                PCWSTR(application.as_ptr()),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                false,
                flags,
                environment,
                PCWSTR(current_directory.as_ptr()),
                &startup.StartupInfo,
                &mut information,
            )
            .map_err(win_error)?;
        }
        // This armed owner exists before any fallible post-create inspection.
        // Only a definitely never-resumed child can be terminated by its Drop.
        let mut pending = NeverResumedChild {
            process: Some(unsafe { own(information.hProcess) }),
        };
        let thread = unsafe { own(information.hThread) };
        let observed = identity(
            handle(pending.process.as_ref().expect("owned child")),
            &image,
        )?;
        job.contains_handle(handle(pending.process.as_ref().expect("owned child")))?;
        // Keep the explicit cleanup guard armed through every membership check.
        let process = ExactProcess {
            process: pending.process.as_ref().expect("owned child").try_clone()?,
            identity: observed,
            _image: image,
        };
        if let Some(manager) = manager_job {
            manager.verify_child_outside(&process)?;
        }
        drop(pending.process.take());
        Ok(Self {
            process,
            thread,
            job,
            root,
            launch,
            command_digest,
            intent,
            identity_persisted: false,
            resume_attempted: false,
            cancel_attempted: false,
            cancel_identity: None,
            cancel_intent: None,
            cancel_custody_attempted: AtomicBool::new(false),
            resume_intent: None,
            historical_lifetime: None,
            lease,
            manager_job,
        })
    }
    pub(crate) fn persist_identity(
        &mut self,
        user: &CurrentUser,
    ) -> io::Result<DurableProcessIdentity> {
        if self.identity_persisted || self.resume_attempted || self.cancel_attempted {
            return Err(blocked("process identity was already persisted"));
        }
        self.intent.verify()?;
        self.job.contains(&self.process)?;
        let binding = IdentityBinding {
            schema: 3,
            launch: self.launch.clone(),
            intent: self.intent.digest().into(),
            process: self.process.identity.clone(),
            command_digest: self.command_digest.clone(),
            job: self.job.identity.clone(),
            job_phase: JobPhase::ArmedPreparation,
            lease: self.lease.identity().clone(),
        };
        let bytes = serde_json::to_vec(&binding).map_err(io::Error::other)?;
        let record = DurableRecord::create(
            self.root.clone(),
            ComponentName::new(OsStr::new(&format!("process-{}.json", self.launch)))?,
            &bytes,
            user,
        )?;
        self.identity_persisted = true;
        Ok(DurableProcessIdentity { record, binding })
    }
    fn commit_resume_intent(&mut self, receipt: &DurableProcessIdentity) -> io::Result<()> {
        if self.resume_attempted || self.cancel_attempted {
            return Err(blocked(
                "process resume or cancellation was already attempted",
            ));
        }
        // A failed call is still an attempt. Set this BEFORE validation or a
        // durable record write can fail; no such failure authorizes cleanup.
        self.resume_attempted = true;
        self.lease.verify()?;
        receipt.record.verify()?;
        self.intent.verify()?;
        if !self.identity_persisted
            || receipt.binding.launch != self.launch
            || receipt.binding.process != self.process.identity
            || receipt.binding.intent != self.intent.digest()
            || receipt.binding.command_digest != self.command_digest
            || receipt.binding.job != self.job.identity
            || receipt.binding.job_phase != JobPhase::ArmedPreparation
            || self.job.phase != Some(JobPhase::ArmedPreparation)
            || &receipt.binding.lease != self.lease.identity()
        {
            return Err(blocked("resume lacks its exact durable process receipt"));
        }
        self.job.contains(&self.process)?;
        if let Some(manager) = self.manager_job {
            manager.verify_child_outside(&self.process)?;
        }
        if self.process.terminal(0)?.is_some() {
            return Err(blocked("suspended process already terminated"));
        }
        let user = CurrentUser::capture()?;
        let bytes = serde_json::to_vec(&serde_json::json!({ "schema": 2, "launch": self.launch, "processReceipt": receipt.record.digest(), "operation": "resume", "fromJobPhase": JobPhase::ArmedPreparation })).map_err(io::Error::other)?;
        self.resume_intent = Some(DurableRecord::create(
            self.root.clone(),
            ComponentName::new(OsStr::new(&format!("resume-{}.json", self.launch)))?,
            &bytes,
            &user,
        )?);
        // Once the durable resume intent exists, a restart cannot distinguish
        // before-disarm/call from after-call. Never explicitly terminate or retry
        // from this state. The still-armed job remains safe until disarm starts:
        // ResumeThread is unreachable before disarm and its successful readback.
        Ok(())
    }
    fn prepare_historical_lifetime(&mut self, receipt: &DurableProcessIdentity) -> io::Result<()> {
        if !self.resume_attempted || self.resume_intent.is_none() {
            return Err(blocked("job disarm lacks a durable resume intent"));
        }
        if self.job.identity.kind == JobKind::HistoricalApplication {
            self.job.disarm_historical()?;
            let user = CurrentUser::capture()?;
            let bytes = serde_json::to_vec(&serde_json::json!({
                "schema": 1, "launch": self.launch, "processReceipt": receipt.record.digest(),
                "job": self.job.identity, "jobPhase": JobPhase::HistoricalLifetime,
                "operation": "historical-job-disarmed"
            }))
            .map_err(io::Error::other)?;
            self.historical_lifetime = Some(DurableRecord::create(
                self.root.clone(),
                ComponentName::new(OsStr::new(&format!("lifetime-{}.json", self.launch)))?,
                &bytes,
                &user,
            )?);
        }
        self.job.verify_limits()
    }
    pub(crate) fn resume(&mut self, receipt: &DurableProcessIdentity) -> io::Result<()> {
        self.commit_resume_intent(receipt)?;
        self.prepare_historical_lifetime(receipt)?;
        self.job.contains(&self.process)?;
        if let Some(manager) = self.manager_job {
            manager.verify_child_outside(&self.process)?;
        }
        let previous = unsafe { ResumeThread(handle(&self.thread)) };
        if previous != 1 {
            return Err(blocked("process resume outcome is unknown"));
        }
        Ok(())
    }
    pub(crate) fn cancel_before_resume(&mut self) -> io::Result<TerminalReceipt> {
        if self.resume_attempted || self.cancel_attempted {
            return Err(blocked("historical process may have begun user work"));
        }
        // Spend before ALL native/record work. A failed cleanup write or call
        // cannot be retried, resumed, or silently repeated by Drop.
        self.cancel_attempted = true;
        let user = CurrentUser::capture()?;
        self.verify_never_resumed_owner(&user)?;
        let binding = self.created_identity_binding();
        let record = DurableRecord::create(
            self.root.clone(),
            ComponentName::new(OsStr::new(&format!("cancel-identity-{}.json", self.launch)))?,
            &serde_json::to_vec(&binding).map_err(io::Error::other)?,
            &user,
        )?;
        self.cancel_identity = Some(Arc::new(DurableProcessIdentity { record, binding }));
        let identity = self
            .cancel_identity
            .as_ref()
            .expect("held cancellation identity");
        let intent = CancellationIntentBinding {
            schema: 1,
            launch: self.launch.clone(),
            identity_receipt: identity.record.digest().into(),
            operation: "cancel-never-resumed".into(),
        };
        self.cancel_intent = Some(Arc::new(DurableRecord::create(
            self.root.clone(),
            ComponentName::new(OsStr::new(&format!("cancel-intent-{}.json", self.launch)))?,
            &serde_json::to_vec(&intent).map_err(io::Error::other)?,
            &user,
        )?));
        self.verify_never_resumed_owner(&user)?;
        stop_never_resumed(handle(&self.process.process))?;
        self.process
            .terminal(0)?
            .ok_or_else(|| blocked("never-resumed cleanup is unresolved"))
    }
    /// Derived only from this original creation owner, never a caller boolean
    /// or missing PID/record. No attempted resume or cancellation is eligible.
    pub(crate) fn can_cancel_before_resume(&self) -> io::Result<bool> {
        if self.resume_attempted || self.cancel_attempted {
            return Ok(false);
        }
        self.verify_never_resumed_owner(&CurrentUser::capture()?)?;
        Ok(true)
    }
    fn created_identity_binding(&self) -> IdentityBinding {
        IdentityBinding {
            schema: 3,
            launch: self.launch.clone(),
            intent: self.intent.digest().into(),
            process: self.process.identity.clone(),
            command_digest: self.command_digest.clone(),
            job: self.job.identity.clone(),
            job_phase: JobPhase::ArmedPreparation,
            lease: self.lease.identity().clone(),
        }
    }
    fn verify_never_resumed_owner(&self, user: &CurrentUser) -> io::Result<()> {
        self.lease.verify()?;
        self.root.verify(user)?;
        self.intent.verify()?;
        self.process._image.verify()?;
        user.verify_private_job(handle(&self.job.handle))?;
        self.job.verify_limits()?;
        let intent: LaunchIntent =
            serde_json::from_slice(self.intent.bytes()).map_err(io::Error::other)?;
        if self.resume_attempted
            || self.resume_intent.is_some()
            || self.historical_lifetime.is_some()
            || self.job.phase != Some(JobPhase::ArmedPreparation)
            || self.job.identity.owner != user.sid_text()
            || self.intent.root_identity() != self.root.directory().identity()
            || intent.schema != 3
            || intent.launch != self.launch
            || intent.image != self.process.identity.image
            || intent.image_digest != self.process.identity.image_digest
            || intent.command_digest != self.command_digest
            || intent.job != self.job.identity
            || intent.job_phase != JobPhase::ArmedPreparation
            || &intent.lease != self.lease.identity()
        {
            return Err(blocked("never-resumed original creation custody changed"));
        }
        if self.process.terminal(0)?.is_none() {
            self.job.contains(&self.process)?;
            if self.job.only_member()? != self.process.pid() {
                return Err(blocked("never-resumed private job has another child"));
            }
            if let Some(manager) = self.manager_job {
                manager.verify_child_outside(&self.process)?;
            }
        }
        Ok(())
    }
    pub(crate) fn launch_record(&self) -> (String, String) {
        (
            format!("launch-{}.json", self.launch),
            self.intent.digest().into(),
        )
    }
    #[cfg(test)]
    pub(crate) fn probe_commit_resume_intent(
        &mut self,
        receipt: &DurableProcessIdentity,
    ) -> io::Result<()> {
        self.commit_resume_intent(receipt)
    }
    #[cfg(test)]
    pub(crate) fn probe_disarm_before_resume(
        &mut self,
        receipt: &DurableProcessIdentity,
    ) -> io::Result<()> {
        self.commit_resume_intent(receipt)?;
        self.prepare_historical_lifetime(receipt)
    }
    #[cfg(test)]
    pub(crate) fn probe_disarm_before_readback(
        &mut self,
        receipt: &DurableProcessIdentity,
    ) -> io::Result<()> {
        self.commit_resume_intent(receipt)?;
        self.job.begin_historical_disarm()
    }
    #[cfg(test)]
    pub(crate) fn probe_suspend_primary(&self) -> io::Result<()> {
        let previous =
            unsafe { windows::Win32::System::Threading::SuspendThread(handle(&self.thread)) };
        if previous != 1 {
            return Err(blocked("probe expected one initial suspension"));
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn probe_identity_name(&self) -> String {
        format!("process-{}.json", self.launch)
    }
    #[cfg(test)]
    pub(crate) fn probe_resume_name(&self) -> String {
        format!("resume-{}.json", self.launch)
    }
    #[cfg(test)]
    pub(crate) fn probe_lifetime_name(&self) -> String {
        format!("lifetime-{}.json", self.launch)
    }
    #[cfg(test)]
    pub(crate) fn probe_exact(&self) -> io::Result<ExactProcess> {
        ExactProcess::reopen(&self.process.identity)
    }
    /// Retain terminal custody before releasing this owner's image and mutable
    /// lease borrow. The exact process receipt is borrowed directly, avoiding a
    /// sharing-conflicting reopen of its original durable writer.
    pub(crate) fn observe_terminal_guard(
        &self,
        receipt: &DurableProcessIdentity,
        user: &CurrentUser,
    ) -> io::Result<Option<TerminalProcessJob>> {
        if self.cancel_attempted {
            return self.observe_cancelled_terminal(Some(receipt), user);
        }
        self.lease.verify()?;
        self.root.verify(user)?;
        self.intent.verify()?;
        receipt.record.verify()?;
        let phase = self
            .job
            .phase
            .ok_or_else(|| blocked("terminal job phase is unknown"))?;
        if !self.identity_persisted
            || !self.resume_attempted
            || receipt.binding.schema != 3
            || receipt.binding.launch != self.launch
            || receipt.binding.process != self.process.identity
            || receipt.binding.intent != self.intent.digest()
            || receipt.binding.command_digest != self.command_digest
            || receipt.binding.job != self.job.identity
            || receipt.binding.job_phase != JobPhase::ArmedPreparation
            || &receipt.binding.lease != self.lease.identity()
            || receipt.record.root_identity() != self.root.directory().identity()
            || self.intent.root_identity() != self.root.directory().identity()
            || !matches!(
                (self.job.identity.kind, phase),
                (JobKind::Installer, JobPhase::ArmedPreparation)
                    | (JobKind::HistoricalApplication, JobPhase::HistoricalLifetime)
            )
        {
            return Err(blocked("terminal custody belongs to another launch"));
        }
        let intent: LaunchIntent =
            serde_json::from_slice(self.intent.bytes()).map_err(io::Error::other)?;
        if intent.schema != 3
            || intent.launch != self.launch
            || intent.image != self.process.identity.image
            || intent.image_digest != self.process.identity.image_digest
            || intent.command_digest != self.command_digest
            || intent.job != self.job.identity
            || intent.job_phase != JobPhase::ArmedPreparation
            || &intent.lease != self.lease.identity()
        {
            return Err(blocked("terminal launch intent binding differs"));
        }
        let resume = self
            .resume_intent
            .as_ref()
            .ok_or_else(|| blocked("terminal launch lacks resume intent"))?;
        resume.verify()?;
        let expected_resume = serde_json::to_vec(&serde_json::json!({
            "schema": 2, "launch": self.launch, "processReceipt": receipt.record.digest(),
            "operation": "resume", "fromJobPhase": JobPhase::ArmedPreparation
        }))
        .map_err(io::Error::other)?;
        if resume.root_identity() != self.root.directory().identity()
            || resume.bytes() != expected_resume.as_slice()
        {
            return Err(blocked("terminal resume intent differs"));
        }
        let historical_lifetime = if self.job.identity.kind == JobKind::HistoricalApplication {
            let lifetime = self
                .historical_lifetime
                .as_ref()
                .ok_or_else(|| blocked("terminal historical lifetime receipt missing"))?;
            lifetime.verify()?;
            let expected = serde_json::to_vec(&serde_json::json!({
                "schema": 1, "launch": self.launch, "processReceipt": receipt.record.digest(),
                "job": self.job.identity, "jobPhase": JobPhase::HistoricalLifetime,
                "operation": "historical-job-disarmed"
            }))
            .map_err(io::Error::other)?;
            if lifetime.root_identity() != self.root.directory().identity()
                || lifetime.bytes() != expected.as_slice()
            {
                return Err(blocked("terminal historical lifetime receipt differs"));
            }
            Some(lifetime.digest().to_owned())
        } else {
            None
        };
        // Never query IsProcessInJob on a terminated process. Creation/resume
        // already authenticated its membership; this is the same retained job.
        let Some(terminal) = self.process.terminal(0)? else {
            return Ok(None);
        };
        let job = PrivateJob::open_recorded(&self.job.identity, phase, user)?;
        if job.active_processes()? != 0 {
            return Ok(None);
        }
        let process = self.process.retain_terminal()?;
        let binding = TerminalCustodyBinding {
            schema: 1,
            launch: self.launch.clone(),
            root: self.root.directory().identity().clone(),
            launch_intent: self.intent.digest().into(),
            process_receipt: receipt.record.digest().into(),
            process: terminal.identity,
            exit_code: terminal.exit_code,
            job: self.job.identity.clone(),
            job_phase: phase,
            active_processes: 0,
            historical_lifetime,
            cancellation_intent: None,
        };
        let record = DurableRecord::create(
            self.root.clone(),
            ComponentName::new(OsStr::new(&format!(
                "terminal-custody-{}.json",
                self.launch
            )))?,
            &serde_json::to_vec(&binding).map_err(io::Error::other)?,
            user,
        )?;
        let result = TerminalProcessJob {
            process,
            job,
            record,
            binding,
            root: self.root.clone(),
            cancellation: None,
        };
        result.verify()?;
        Ok(Some(result))
    }
    /// A distinct typed result proves cancellation preceded every resume call.
    /// The ordinary terminal guard alone must never justify NotApplied resume.
    pub(crate) fn observe_cancelled_before_resume(
        &self,
        receipt: Option<&DurableProcessIdentity>,
        user: &CurrentUser,
    ) -> io::Result<Option<CancelledBeforeResume>> {
        if !self.cancel_attempted || self.resume_attempted {
            return Err(blocked("owned never-resumed cancellation was not admitted"));
        }
        let terminal = match receipt {
            Some(receipt) => self.observe_terminal_guard(receipt, user)?,
            None => self.observe_cancelled_terminal(None, user)?,
        };
        terminal
            .map(|terminal| {
                let result = CancelledBeforeResume { terminal };
                result.verify()?;
                Ok(result)
            })
            .transpose()
    }
    fn observe_cancelled_terminal(
        &self,
        receipt: Option<&DurableProcessIdentity>,
        user: &CurrentUser,
    ) -> io::Result<Option<TerminalProcessJob>> {
        self.verify_never_resumed_owner(user)?;
        if !self.cancel_attempted {
            return Err(blocked("cancellation has not been attempted"));
        }
        let identity = self
            .cancel_identity
            .as_ref()
            .ok_or_else(|| blocked("cancellation identity is unresolved"))?;
        let intent = self
            .cancel_intent
            .as_ref()
            .ok_or_else(|| blocked("cancellation intent is unresolved"))?;
        identity.record.verify()?;
        intent.verify()?;
        if identity.binding != self.created_identity_binding()
            || identity.record.root_identity() != self.root.directory().identity()
            || intent.root_identity() != self.root.directory().identity()
        {
            return Err(blocked("cancellation belongs to another creation"));
        }
        let expected_intent = CancellationIntentBinding {
            schema: 1,
            launch: self.launch.clone(),
            identity_receipt: identity.record.digest().into(),
            operation: "cancel-never-resumed".into(),
        };
        if serde_json::from_slice::<CancellationIntentBinding>(intent.bytes())
            .map_err(io::Error::other)?
            != expected_intent
        {
            return Err(blocked("cancellation intent binding differs"));
        }
        let original_process_receipt = match receipt {
            Some(receipt) => {
                receipt.record.verify()?;
                if !self.identity_persisted
                    || receipt.binding != identity.binding
                    || receipt.record.root_identity() != self.root.directory().identity()
                {
                    return Err(blocked("cancellation process receipt differs"));
                }
                Some(receipt.record.digest().to_owned())
            }
            None => {
                if self.identity_persisted {
                    return Err(blocked("existing exact process receipt must be retained"));
                }
                None
            }
        };
        let Some(terminal) = self.process.terminal(0)? else {
            return Ok(None);
        };
        if self.job.active_processes()? != 0 {
            return Ok(None);
        }
        // Spend before reopening custody or writing its record. Failure cannot
        // repeat cancellation or overwrite/recreate an uncertain receipt.
        if self.cancel_custody_attempted.swap(true, Ordering::SeqCst) {
            return Err(blocked(
                "cancellation terminal custody was already attempted",
            ));
        }
        let job = PrivateJob::open_recorded(&self.job.identity, JobPhase::ArmedPreparation, user)?;
        if job.active_processes()? != 0 {
            return Err(blocked("cancelled private job changed"));
        }
        let process = self.process.retain_terminal()?;
        let binding = TerminalCustodyBinding {
            schema: 1,
            launch: self.launch.clone(),
            root: self.root.directory().identity().clone(),
            launch_intent: self.intent.digest().into(),
            process_receipt: original_process_receipt
                .clone()
                .unwrap_or_else(|| identity.record.digest().into()),
            process: terminal.identity,
            exit_code: terminal.exit_code,
            job: self.job.identity.clone(),
            job_phase: JobPhase::ArmedPreparation,
            active_processes: 0,
            historical_lifetime: None,
            cancellation_intent: Some(intent.digest().into()),
        };
        let record = DurableRecord::create(
            self.root.clone(),
            ComponentName::new(OsStr::new(&format!(
                "terminal-custody-{}.json",
                self.launch
            )))?,
            &serde_json::to_vec(&binding).map_err(io::Error::other)?,
            user,
        )?;
        let result = TerminalProcessJob {
            process,
            job,
            record,
            binding,
            root: self.root.clone(),
            cancellation: Some(CancellationEvidence {
                identity: identity.clone(),
                intent: intent.clone(),
                original_process_receipt,
            }),
        };
        result.verify()?;
        Ok(Some(result))
    }
    pub(crate) fn persist_terminal(&self, user: &CurrentUser) -> io::Result<DurableRecord> {
        if self.job.phase == Some(JobPhase::HistoricalLifetime) {
            self.historical_lifetime
                .as_ref()
                .ok_or_else(|| blocked("historical lifetime receipt is missing"))?
                .verify()?;
        }
        let terminal = self
            .process
            .terminal(0)?
            .ok_or_else(|| blocked("process is still running"))?;
        if self.job.active_processes()? != 0 {
            return Err(blocked("owned descendants have not exited"));
        }
        let bytes = serde_json::to_vec(&serde_json::json!({ "schema": 2, "launch": self.launch, "process": terminal.identity, "exitCode": terminal.exit_code, "job": self.job.identity, "jobPhase": self.job.phase, "activeProcesses": 0 })).map_err(io::Error::other)?;
        DurableRecord::create(
            self.root.clone(),
            ComponentName::new(OsStr::new(&format!("terminal-{}.json", self.launch)))?,
            &bytes,
            user,
        )
    }
    pub(crate) fn try_terminal(&self) -> io::Result<Option<TerminalReceipt>> {
        self.process.terminal(0)
    }
    pub(crate) fn wait_terminal(&self, timeout: u32) -> io::Result<Option<TerminalReceipt>> {
        self.process.terminal(timeout)
    }
    pub(crate) fn active_processes(&self) -> io::Result<u32> {
        self.job.active_processes()
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TerminalCustodyBinding {
    schema: u32,
    launch: String,
    root: FileIdentity,
    launch_intent: String,
    process_receipt: String,
    process: ProcessIdentity,
    exit_code: u32,
    job: JobIdentity,
    job_phase: JobPhase,
    active_processes: u32,
    historical_lifetime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cancellation_intent: Option<String>,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancellationIntentBinding {
    schema: u32,
    launch: String,
    identity_receipt: String,
    operation: String,
}
struct CancellationEvidence {
    identity: Arc<DurableProcessIdentity>,
    intent: Arc<DurableRecord>,
    original_process_receipt: Option<String>,
}
impl CancellationEvidence {
    fn verify(&self, binding: &TerminalCustodyBinding, root: &PrivateDirectory) -> io::Result<()> {
        self.identity.record.verify()?;
        self.intent.verify()?;
        let identity: IdentityBinding =
            serde_json::from_slice(self.identity.record.bytes()).map_err(io::Error::other)?;
        let intent: CancellationIntentBinding =
            serde_json::from_slice(self.intent.bytes()).map_err(io::Error::other)?;
        if identity != self.identity.binding
            || identity.schema != 3
            || identity.launch != binding.launch
            || identity.intent != binding.launch_intent
            || identity.process != binding.process
            || identity.job != binding.job
            || identity.job_phase != JobPhase::ArmedPreparation
            || binding.job_phase != JobPhase::ArmedPreparation
            || binding.historical_lifetime.is_some()
            || binding.cancellation_intent.as_deref() != Some(self.intent.digest())
            || binding.process_receipt
                != self
                    .original_process_receipt
                    .as_deref()
                    .unwrap_or(self.identity.record.digest())
            || self.identity.record.root_identity() != root.directory().identity()
            || self.intent.root_identity() != root.directory().identity()
            || intent.schema != 1
            || intent.launch != binding.launch
            || intent.identity_receipt != self.identity.record.digest()
            || intent.operation != "cancel-never-resumed"
        {
            return Err(blocked("never-resumed cancellation custody changed"));
        }
        Ok(())
    }
}

/// Nonserializable original-owner proof that an actual created child was
/// cancelled before any resume call and its authenticated private job is empty.
pub(crate) struct CancelledBeforeResume {
    terminal: TerminalProcessJob,
}
impl CancelledBeforeResume {
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.terminal.verify()?;
        if self.terminal.cancellation.is_none() {
            return Err(blocked(
                "terminal custody is not never-resumed cancellation",
            ));
        }
        Ok(())
    }
    pub(crate) fn creation_bytes(&self) -> io::Result<&[u8]> {
        self.verify()?;
        self.terminal
            .cancellation
            .as_ref()
            .expect("verified cancellation")
            .identity
            .record_bytes()
    }
    pub(crate) fn terminal_bytes(&self) -> &[u8] {
        self.terminal.terminal_bytes()
    }
    pub(crate) fn terminal(&self) -> &TerminalProcessJob {
        &self.terminal
    }
    pub(crate) fn into_terminal(self) -> TerminalProcessJob {
        self.terminal
    }
}

/// Exact terminal process, actual empty private job and durable custody record.
/// This owns neither an image file nor an installation lease. It proves terminal
/// custody only; an exit code is not itself installer or Return success.
pub(crate) struct TerminalProcessJob {
    process: TerminatedProcess,
    job: PrivateJob,
    record: DurableRecord,
    binding: TerminalCustodyBinding,
    root: Arc<PrivateDirectory>,
    cancellation: Option<CancellationEvidence>,
}
impl TerminalProcessJob {
    /// 仅区分已有原生终态的来源，不为尚未终止的进程创建权限。
    pub(crate) fn was_cancelled_before_resume(&self) -> bool {
        self.cancellation.is_some()
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        let user = CurrentUser::capture()?;
        self.root.verify(&user)?;
        self.process.verify()?;
        self.record.verify()?;
        user.verify_private_job(handle(&self.job.handle))?;
        self.job.verify_limits()?;
        let persisted: TerminalCustodyBinding =
            serde_json::from_slice(self.record.bytes()).map_err(io::Error::other)?;
        if persisted != self.binding
            || self.binding.schema != 1
            || self.record.root_identity() != self.root.directory().identity()
            || &self.binding.root != self.root.directory().identity()
            || &self.binding.process != self.process.identity()
            || self.binding.exit_code != self.process.terminal.exit_code
            || self.binding.job != self.job.identity
            || self.job.phase != Some(self.binding.job_phase)
            || self.binding.active_processes != 0
            || self.job.active_processes()? != 0
            || self.job.identity.owner != user.sid_text()
        {
            return Err(blocked("terminal process custody changed"));
        }
        match &self.cancellation {
            Some(cancellation) => cancellation.verify(&self.binding, &self.root)?,
            None if self.binding.cancellation_intent.is_none() => {}
            None => return Err(blocked("missing original cancellation custody")),
        }
        Ok(())
    }
    pub(crate) fn job_kind(&self) -> JobKind {
        self.binding.job.kind
    }
    pub(crate) fn root_identity(&self) -> &FileIdentity {
        &self.binding.root
    }
    pub(crate) fn process_identity(&self) -> &ProcessIdentity {
        self.process.identity()
    }
    /// Bind an actual newly acquired image fence to this original terminal
    /// process object; serialized metadata never supplies execution authority.
    pub(crate) fn verify_image(&self, identity: &FileIdentity, digest: &str) -> io::Result<()> {
        self.verify()?;
        if &self.binding.process.image != identity || self.binding.process.image_digest != digest {
            return Err(blocked("image differs from terminal process custody"));
        }
        Ok(())
    }
    pub(crate) fn terminal_bytes(&self) -> &[u8] {
        self.record.bytes()
    }
    pub(crate) fn terminal_reference(&self) -> (String, String) {
        (
            format!("terminal-custody-{}.json", self.binding.launch),
            self.record.digest().into(),
        )
    }
}

impl Drop for PreparedProcess<'_> {
    fn drop(&mut self) {
        if !self.resume_attempted && !self.cancel_attempted {
            // Failure is not success evidence. The armed job also cleans up on
            // final handle closure. Its vanished name never proves success.
            let _ = stop_never_resumed(handle(&self.process.process));
        }
    }
}
struct NeverResumedChild {
    process: Option<OwnedHandle>,
}
impl Drop for NeverResumedChild {
    fn drop(&mut self) {
        if let Some(process) = &self.process {
            let _ = stop_never_resumed(handle(process));
        }
    }
}
fn stop_never_resumed(process: HANDLE) -> io::Result<()> {
    match unsafe { WaitForSingleObject(process, 0) } {
        WAIT_OBJECT_0 => return Ok(()),
        WAIT_TIMEOUT => (),
        _ => return Err(io::Error::last_os_error()),
    }
    if let Err(error) = unsafe { TerminateProcess(process, 0xccde0001) } {
        if unsafe { WaitForSingleObject(process, 0) } != WAIT_OBJECT_0 {
            return Err(win_error(error));
        }
    }
    if unsafe { WaitForSingleObject(process, 5000) } != WAIT_OBJECT_0 {
        return Err(blocked("never-resumed termination is unresolved"));
    }
    Ok(())
}

/// This path is only for a historical child whose exclusive creation owner has
/// gone away before a resume intent existed. The mutable lifetime-lease borrow
/// excludes another live creator/resumer, including use inside the same process.
pub(crate) fn recover_never_resumed(
    intent: DurableLaunchIntent,
    root: Arc<PrivateDirectory>,
    user: &CurrentUser,
    lease: &mut ExclusiveLease,
) -> io::Result<RecoveredNeverResumed> {
    lease.verify()?;
    root.verify(user)?;
    intent.record.verify()?;
    if intent.binding.job.kind != JobKind::HistoricalApplication
        || &intent.binding.lease != lease.identity()
        || intent.record.root_identity() != root.directory().identity()
    {
        return Err(blocked("never-resumed recovery ownership differs"));
    }
    let resume_name = ComponentName::new(OsStr::new(&format!(
        "resume-{}.json",
        intent.binding.launch
    )))?;
    match root.directory().open_file(resume_name, FileAccess::Read) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        // Any existing, corrupt, inaccessible or partial resume record is
        // uncertain. Never infer safe termination from a parse/hash failure.
        _ => return Err(blocked("historical resume may have been attempted")),
    }
    let job = PrivateJob::open_recorded(&intent.binding.job, intent.binding.job_phase, user)?;
    let process = ExactProcess::capture_with_access(job.only_member()?, PROCESS_TERMINATE)?;
    job.contains(&process)?;
    if process.identity.image != intent.binding.image
        || process.identity.image_digest != intent.binding.image_digest
        || process.identity.session != intent.binding.job.session
    {
        return Err(blocked(
            "never-resumed job child differs from launch intent",
        ));
    }
    let bytes = serde_json::to_vec(&serde_json::json!({ "schema": 1, "launchIntent": intent.record.digest(), "process": process.identity, "operation": "cancel-never-resumed" })).map_err(io::Error::other)?;
    let cleanup_intent = DurableRecord::create(
        root.clone(),
        ComponentName::new(OsStr::new(&format!(
            "cancel-unresumed-{}.json",
            intent.binding.launch
        )))?,
        &bytes,
        user,
    )?;
    stop_never_resumed(handle(&process.process))?;
    let terminal = process
        .terminal(0)?
        .ok_or_else(|| blocked("never-resumed child termination is unknown"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while job.active_processes()? != 0 {
        if std::time::Instant::now() >= deadline {
            return Err(blocked("never-resumed job is not empty"));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let bytes = serde_json::to_vec(&serde_json::json!({ "schema": 1, "cleanupIntent": cleanup_intent.digest(), "process": terminal.identity, "exitCode": terminal.exit_code })).map_err(io::Error::other)?;
    let receipt = DurableRecord::create(
        root,
        ComponentName::new(OsStr::new(&format!(
            "cancelled-unresumed-{}.json",
            intent.binding.launch
        )))?,
        &bytes,
        user,
    )?;
    Ok(RecoveredNeverResumed {
        _launch: intent,
        _cleanup_intent: cleanup_intent,
        _receipt: receipt,
        _job: job,
        process: process.release_terminated_image()?,
    })
}
pub(crate) struct RecoveredNeverResumed {
    _launch: DurableLaunchIntent,
    _cleanup_intent: DurableRecord,
    _receipt: DurableRecord,
    _job: PrivateJob,
    process: TerminatedProcess,
}
impl RecoveredNeverResumed {
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.process.verify()?;
        self._receipt.verify()
    }
}

#[cfg(test)]
#[path = "../../tests/version_history_process_terminal_windows.rs"]
#[allow(non_snake_case)]
mod terminal_tests;

#[cfg(test)]
#[path = "../../tests/version_history_process_cancel_windows.rs"]
#[allow(non_snake_case)]
mod cancel_tests;

#[cfg(test)]
#[path = "../../tests/version_history_restart_lifetime_windows.rs"]
#[allow(non_snake_case)]
mod restart_lifetime_tests;
