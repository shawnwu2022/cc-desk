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
    sync::Arc,
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
            WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
            EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_ACCESS_RIGHTS,
            PROCESS_INFORMATION, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTUPINFOEXW,
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
    fn from_created(process: OwnedHandle, image: PinnedFile) -> io::Result<Self> {
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
fn session_id(pid: u32) -> io::Result<u32> {
    let mut session = 0;
    unsafe {
        ProcessIdToSessionId(pid, &mut session).map_err(win_error)?;
    }
    Ok(session)
}
fn creation_time(process: HANDLE) -> io::Result<u64> {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobIdentity {
    name: String,
    owner: String,
    session: u32,
    kind: JobKind,
}
pub(crate) struct PrivateJob {
    handle: OwnedHandle,
    identity: JobIdentity,
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
        if identity.kind == JobKind::Installer {
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        unsafe {
            SetInformationJobObject(
                super::handle(&handle),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(win_error)?;
        }
        let result = Self { handle, identity };
        user.verify_private_job(super::handle(&result.handle))?;
        result.verify_limits()?;
        Ok(result)
    }
    fn verify_limits(&self) -> io::Result<()> {
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
        let expected = if self.identity.kind == JobKind::Installer {
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0
        } else {
            0
        };
        if limits.BasicLimitInformation.LimitFlags.0 != expected {
            return Err(blocked("job containment or lifetime differs"));
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
        Self::open_recorded(&receipt.binding.job, user)
    }
    fn open_recorded(identity: &JobIdentity, user: &CurrentUser) -> io::Result<Self> {
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
        if binding.schema != 2
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
        if binding.schema != 2
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
    resume_intent: Option<DurableRecord>,
    lease: &'lease mut ExclusiveLease,
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
            schema: 2,
            launch: launch.clone(),
            image: image.identity().clone(),
            image_digest: image.digest()?,
            command_digest: command_digest.clone(),
            job: job_identity.clone(),
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
        let current_directory: Vec<_> = image.parent.path()?.encode_wide().chain(Some(0)).collect();
        let mut information = PROCESS_INFORMATION::default();
        #[cfg(test)]
        let environment = command.environment.as_ref().map(|v| v.as_ptr().cast());
        #[cfg(not(test))]
        let environment = None;
        unsafe {
            CreateProcessW(
                PCWSTR(application.as_ptr()),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
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
        let process = ExactProcess {
            process: pending.process.take().expect("owned child"),
            identity: observed,
            _image: image,
        };
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
            resume_intent: None,
            lease,
        })
    }
    pub(crate) fn persist_identity(
        &mut self,
        user: &CurrentUser,
    ) -> io::Result<DurableProcessIdentity> {
        if self.identity_persisted || self.resume_attempted {
            return Err(blocked("process identity was already persisted"));
        }
        self.intent.verify()?;
        self.job.contains(&self.process)?;
        let binding = IdentityBinding {
            schema: 2,
            launch: self.launch.clone(),
            intent: self.intent.digest().into(),
            process: self.process.identity.clone(),
            command_digest: self.command_digest.clone(),
            job: self.job.identity.clone(),
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
        self.lease.verify()?;
        receipt.record.verify()?;
        self.intent.verify()?;
        if self.resume_attempted
            || !self.identity_persisted
            || receipt.binding.launch != self.launch
            || receipt.binding.process != self.process.identity
            || receipt.binding.intent != self.intent.digest()
            || receipt.binding.command_digest != self.command_digest
            || receipt.binding.job != self.job.identity
            || &receipt.binding.lease != self.lease.identity()
        {
            return Err(blocked("resume lacks its exact durable process receipt"));
        }
        self.job.contains(&self.process)?;
        if self.process.terminal(0)?.is_some() {
            return Err(blocked("suspended process already terminated"));
        }
        let user = CurrentUser::capture()?;
        let bytes = serde_json::to_vec(&serde_json::json!({ "schema": 1, "launch": self.launch, "processReceipt": receipt.record.digest(), "operation": "resume" })).map_err(io::Error::other)?;
        self.resume_intent = Some(DurableRecord::create(
            self.root.clone(),
            ComponentName::new(OsStr::new(&format!("resume-{}.json", self.launch)))?,
            &bytes,
            &user,
        )?);
        // Once the durable resume intent exists, a restart cannot distinguish
        // before-call from after-call. Never terminate this historical process
        // automatically, and never retry resume from this state.
        self.resume_attempted = true;
        Ok(())
    }
    pub(crate) fn resume(&mut self, receipt: &DurableProcessIdentity) -> io::Result<()> {
        self.commit_resume_intent(receipt)?;
        let previous = unsafe { ResumeThread(handle(&self.thread)) };
        if previous != 1 {
            return Err(blocked("process resume outcome is unknown"));
        }
        Ok(())
    }
    pub(crate) fn cancel_before_resume(&mut self) -> io::Result<TerminalReceipt> {
        if self.resume_attempted {
            return Err(blocked("historical process may have begun user work"));
        }
        stop_never_resumed(handle(&self.process.process))?;
        self.process
            .terminal(0)?
            .ok_or_else(|| blocked("never-resumed cleanup is unresolved"))
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
    pub(crate) fn probe_exact(&self) -> io::Result<ExactProcess> {
        ExactProcess::reopen(&self.process.identity)
    }
    pub(crate) fn persist_terminal(&self, user: &CurrentUser) -> io::Result<DurableRecord> {
        let terminal = self
            .process
            .terminal(0)?
            .ok_or_else(|| blocked("process is still running"))?;
        if self.job.active_processes()? != 0 {
            return Err(blocked("owned descendants have not exited"));
        }
        let bytes = serde_json::to_vec(&serde_json::json!({ "schema": 1, "launch": self.launch, "process": terminal.identity, "exitCode": terminal.exit_code, "job": self.job.identity, "activeProcesses": 0 })).map_err(io::Error::other)?;
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

impl Drop for PreparedProcess<'_> {
    fn drop(&mut self) {
        if !self.resume_attempted {
            // Failure is not success evidence. The durable launch intent and
            // named job remain available to exclusive-lease recovery.
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
    let job = PrivateJob::open_recorded(&intent.binding.job, user)?;
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
