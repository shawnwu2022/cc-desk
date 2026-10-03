//! Test-only capability probe. This never changes an account, credential, UAC,
//! policy, session, or production token. Unsupported restricted tokens block.
use super::{blocked, bounded_read, report};
use crate::version_history::{
    verified_package::sha256,
    windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        process::ExactProcess,
        security::CurrentUser,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows::Win32::{
    Foundation::{LocalFree, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL, WAIT_OBJECT_0},
    Security::{
        Authorization::ConvertSidToStringSidW, CreateRestrictedToken, CreateWellKnownSid,
        GetTokenInformation, IsValidSid, SetTokenInformation, TokenElevation, TokenGroups,
        TokenIntegrityLevel, TokenPrivileges, TokenSessionId, TokenUser, WinMediumLabelSid,
        DISABLE_MAX_PRIVILEGE, LUA_TOKEN, PSID, SID_AND_ATTRIBUTES, TOKEN_ADJUST_DEFAULT,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_GROUPS,
        TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
    },
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        SystemServices::SE_GROUP_INTEGRITY,
        Threading::{
            CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess, OpenProcess,
            OpenProcessToken, ResumeThread, TerminateProcess, WaitForSingleObject,
            CREATE_SUSPENDED, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
        },
    },
};
use windows_core::{PCWSTR, PWSTR};
fn win(e: windows_core::Error) -> io::Error {
    io::Error::other(e)
}
fn raw(h: &OwnedHandle) -> HANDLE {
    HANDLE(h.as_raw_handle())
}
unsafe fn owned(h: HANDLE) -> OwnedHandle {
    unsafe { OwnedHandle::from_raw_handle(h.0) }
}
const MAX_TOKEN_INFORMATION_BYTES: u32 = 65_536;
fn token_class_name(class: TOKEN_INFORMATION_CLASS) -> &'static str {
    match class {
        value if value == TokenUser => "TokenUser",
        value if value == TokenGroups => "TokenGroups",
        value if value == TokenPrivileges => "TokenPrivileges",
        value if value == TokenSessionId => "TokenSessionId",
        value if value == TokenElevation => "TokenElevation",
        value if value == TokenIntegrityLevel => "TokenIntegrityLevel",
        _ => "UnsupportedClass",
    }
}
fn token_read_error(
    class: TOKEN_INFORMATION_CLASS,
    stage: &'static str,
    returned: u32,
    capacity: u32,
    hresult: Option<i32>,
) -> io::Error {
    // Numeric API diagnostics only: never a token handle, address or token data.
    io::Error::other(format!(
        "token observation class={}({}) stage={stage} returnedBytes={returned} capacityBytes={capacity} hresult={}",
        token_class_name(class), class.0,
        hresult.map(|value|format!("0x{:08x}",value as u32)).unwrap_or_else(||"none".into())
    ))
}
struct TokenBuffer {
    words: Vec<usize>,
    length: usize,
}
impl TokenBuffer {
    fn as_ptr(&self) -> *const usize {
        self.words.as_ptr()
    }
    fn sid_text(&self, sid: PSID) -> io::Result<String> {
        let start = self.words.as_ptr() as usize;
        let offset = (sid.0 as usize)
            .checked_sub(start)
            .filter(|offset| offset.checked_add(8).is_some_and(|end| end <= self.length))
            .ok_or_else(|| blocked("token SID header is outside returned bytes"))?;
        // SID: revision, subauthority count, six identifier-authority bytes,
        // then at most 15 DWORD subauthorities. Validate range before OS reads.
        let count = unsafe { *sid.0.cast::<u8>().add(1) } as usize;
        if count > 15 || offset + 8 + count * 4 > self.length {
            return Err(blocked("token SID extends outside returned bytes"));
        }
        sid_text(sid)
    }
}
fn info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<TokenBuffer> {
    let minimum = match class {
        value if value == TokenUser => size_of::<TOKEN_USER>(),
        value if value == TokenGroups => std::mem::offset_of!(TOKEN_GROUPS, Groups),
        value if value == TokenIntegrityLevel => size_of::<TOKEN_MANDATORY_LABEL>(),
        value if value == TokenPrivileges => std::mem::offset_of!(TOKEN_PRIVILEGES, Privileges),
        _ => return Err(token_read_error(class, "variable-class", 0, 0, None)),
    } as u32;
    let mut required = 0;
    let sizing = unsafe { GetTokenInformation(token, class, None, 0, &mut required) };
    let error = match sizing {
        Err(error) => error,
        Ok(()) => {
            return Err(token_read_error(
                class,
                "unexpected-sizing-success",
                required,
                0,
                None,
            ))
        }
    };
    if error.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult()
        || !(minimum..=MAX_TOKEN_INFORMATION_BYTES).contains(&required)
    {
        return Err(token_read_error(
            class,
            "size",
            required,
            0,
            Some(error.code().0),
        ));
    }
    let mut words = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
    let mut returned = 0;
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(words.as_mut_ptr().cast()),
            required,
            &mut returned,
        )
        .map_err(|error| {
            token_read_error(
                class,
                "variable-read",
                returned,
                required,
                Some(error.code().0),
            )
        })?;
    }
    if !(minimum..=required).contains(&returned) {
        return Err(token_read_error(
            class,
            "variable-return-length",
            returned,
            required,
            None,
        ));
    }
    Ok(TokenBuffer {
        words,
        length: returned as usize,
    })
}
fn fixed_info<T: Default>(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<T> {
    // Only the documented fixed structures below use this path. A typed buffer
    // does not assume NULL/0 probing behaves identically for every token class.
    if !(class == TokenElevation || class == TokenSessionId) || size_of::<T>() != 4 {
        return Err(token_read_error(
            class,
            "fixed-class",
            0,
            size_of::<T>() as u32,
            None,
        ));
    }
    let mut value = T::default();
    let size = size_of::<T>() as u32;
    let mut returned = 0;
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some((&mut value as *mut T).cast()),
            size,
            &mut returned,
        )
        .map_err(|error| {
            token_read_error(class, "fixed-read", returned, size, Some(error.code().0))
        })?;
    }
    if returned != size {
        return Err(token_read_error(
            class,
            "fixed-return-length",
            returned,
            size,
            None,
        ));
    }
    Ok(value)
}
fn sid_text(sid: PSID) -> io::Result<String> {
    if !unsafe { IsValidSid(sid) }.as_bool() {
        return Err(blocked("invalid token SID"));
    }
    let mut text = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut text).map_err(win)?;
    }
    let output = unsafe { text.to_string() }.map_err(io::Error::other);
    unsafe {
        LocalFree(Some(HLOCAL(text.0.cast())));
    }
    output
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    sid: String,
    session: u32,
    elevated: bool,
    integrity_sid: String,
    administrator_attributes: Option<u32>,
    privileges: Vec<(u32, i32, u32)>,
}
impl Observation {
    fn require(&self, expected: &Self) -> io::Result<()> {
        // S-1-16-8192 is medium, excluding high, system, low and medium-plus.
        // SE_GROUP_ENABLED=4, SE_GROUP_USE_FOR_DENY_ONLY=16.
        if self.sid != expected.sid
            || self.session != expected.session
            || self.elevated
            || self.integrity_sid != "S-1-16-8192"
            || self
                .administrator_attributes
                .is_some_and(|v| v & 4 != 0 || v & 16 == 0)
            || self
                .privileges
                .iter()
                .any(|(low, high, _)| *low != 23 || *high != 0)
        {
            return Err(blocked("restricted token does not model same-user medium-integrity unelevated scope; no fallback"));
        }
        Ok(())
    }
}
fn observe(token: HANDLE) -> io::Result<Observation> {
    let user = info(token, TokenUser)?;
    let groups = info(token, TokenGroups)?;
    let integrity = info(token, TokenIntegrityLevel)?;
    let elevation: TOKEN_ELEVATION = fixed_info(token, TokenElevation)?;
    let session: u32 = fixed_info(token, TokenSessionId)?;
    let privileges = info(token, TokenPrivileges)?;
    unsafe {
        let count = *groups.as_ptr().cast::<u32>() as usize;
        let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
        if count
            .checked_mul(size_of::<windows::Win32::Security::SID_AND_ATTRIBUTES>())
            .and_then(|bytes| offset.checked_add(bytes))
            .is_none_or(|end| end > groups.length)
        {
            return Err(blocked("invalid token group size"));
        }
        let entries = std::slice::from_raw_parts(
            groups
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<windows::Win32::Security::SID_AND_ATTRIBUTES>(),
            count,
        );
        let mut admin = None;
        for group in entries {
            if groups.sid_text(group.Sid)? == "S-1-5-32-544" {
                admin = Some(group.Attributes);
            }
        }
        let count = *privileges.as_ptr().cast::<u32>() as usize;
        let offset = std::mem::offset_of!(TOKEN_PRIVILEGES, Privileges);
        if count
            .checked_mul(size_of::<windows::Win32::Security::LUID_AND_ATTRIBUTES>())
            .and_then(|bytes| offset.checked_add(bytes))
            .is_none_or(|end| end > privileges.length)
        {
            return Err(blocked("invalid token privilege size"));
        }
        let privileges = std::slice::from_raw_parts(
            privileges
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<windows::Win32::Security::LUID_AND_ATTRIBUTES>(),
            count,
        )
        .iter()
        .map(|p| (p.Luid.LowPart, p.Luid.HighPart, p.Attributes.0))
        .collect();
        Ok(Observation {
            sid: user.sid_text((*user.as_ptr().cast::<TOKEN_USER>()).User.Sid)?,
            session,
            elevated: elevation.TokenIsElevated != 0,
            integrity_sid: integrity.sid_text(
                (*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())
                    .Label
                    .Sid,
            )?,
            administrator_attributes: admin,
            privileges,
        })
    }
}
fn process_token(process: HANDLE) -> io::Result<OwnedHandle> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(win)?;
        Ok(owned(token))
    }
}
pub(super) fn current() -> io::Result<Observation> {
    observe(raw(&process_token(unsafe { GetCurrentProcess() })?))
}
pub(super) fn require_process(pid: u32, expected: &Observation) -> io::Result<Observation> {
    let process =
        unsafe { owned(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(win)?) };
    let observed = observe(raw(&process_token(raw(&process))?))?;
    observed.require(expected)?;
    Ok(observed)
}
// A projected label only validates the requested mutation. It never admits a
// token: only the observed native readback and actual child may pass require().
fn medium_request(before: &Observation, parent: &Observation) -> io::Result<Observation> {
    if before.integrity_sid != "S-1-16-12288" && before.integrity_sid != "S-1-16-8192" {
        return Err(blocked(
            "only a fresh High or already-Medium restricted fixture token is supported",
        ));
    }
    let mut requested = before.clone();
    requested.integrity_sid = "S-1-16-8192".into();
    requested.require(parent)?;
    Ok(requested)
}

// The payload gate remains separate from the two fixed lifetime diagnostics.
// Only run_restart_lifetime can supply the borrowed freshly owned directory.
enum WorkerScope<'a> {
    Payload,
    RestartLifetime {
        owned: &'a PrivateDirectory,
        live: bool,
    },
}
impl WorkerScope<'_> {
    fn require_creation(&self) -> io::Result<()> {
        match self {
            Self::Payload => {
                super::fixture_root()?; // Preserve the dedicated payload workflow gate.
                Ok(())
            }
            Self::RestartLifetime { owned, .. } => {
                if !cfg!(target_arch = "x86_64") {
                    return Err(blocked("restart lifetime fixture requires Windows x64"));
                }
                owned.verify(&CurrentUser::capture()?)
            }
        }
    }
    fn require_root(&self, root: &Path) -> io::Result<()> {
        match self {
            Self::Payload => {
                if super::fixture_root()? != root {
                    return Err(blocked("fixture label evidence root differs"));
                }
            }
            Self::RestartLifetime { owned, .. } => {
                self.require_creation()?;
                if Directory::open_absolute(root)?.identity() != owned.directory().identity() {
                    return Err(blocked(
                        "restart fixture root differs from its owned directory",
                    ));
                }
            }
        }
        Ok(())
    }
    fn worker_test(&self) -> &'static str {
        match self {
            Self::Payload => "tests::version_history_payload::HistoryPayload_Worker_002",
            Self::RestartLifetime { live, .. } => restart_worker_test(*live),
        }
    }
}
fn restart_worker_test(live: bool) -> &'static str {
    if live {
        "version_history::windows::process::restart_lifetime_tests::RestartWorker_Live_003"
    } else {
        "version_history::windows::process::restart_lifetime_tests::RestartWorker_Terminal_004"
    }
}

/// No from-handle constructor exists. The sole constructor creates a new token
/// for this test worker and drops the parent handle before any label operation.
/// This type and its sole mutating API are compiled only within the test module.
struct RestrictedWorkerToken {
    token: OwnedHandle,
    lowering_attempted: bool,
    admitted: Option<Observation>,
}
impl RestrictedWorkerToken {
    fn create(parent: &Observation, scope: &WorkerScope<'_>) -> io::Result<Self> {
        scope.require_creation()?;
        if current()? != *parent {
            return Err(blocked("parent token changed before restricted creation"));
        }
        let mut original = HANDLE::default();
        unsafe {
            // CreateRestrictedToken gives the new handle these access rights.
            // The original handle is used solely as the creation source, never
            // by SetTokenInformation, and closes before the method returns.
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
                &mut original,
            )
            .map_err(win)?;
        }
        let original = unsafe { owned(original) };
        let mut created = HANDLE::default();
        unsafe {
            CreateRestrictedToken(
                raw(&original),
                LUA_TOKEN | DISABLE_MAX_PRIVILEGE,
                None,
                None,
                None,
                &mut created,
            )
            .map_err(win)?;
        }
        let token = unsafe { owned(created) };
        drop(original);
        if current()? != *parent {
            return Err(blocked("parent token changed during restricted creation"));
        }
        Ok(Self {
            token,
            lowering_attempted: false,
            admitted: None,
        })
    }
    fn observe(&self) -> io::Result<Observation> {
        observe(raw(&self.token))
    }
    fn lower_to_medium(
        &mut self,
        root: &Path,
        parent: &Observation,
        scope: &WorkerScope<'_>,
    ) -> io::Result<()> {
        scope.require_root(root)?;
        if self.lowering_attempted || self.admitted.is_some() {
            return Err(blocked("fixture label operation cannot be replayed"));
        }
        let before = self.observe()?;
        let requested = medium_request(&before, parent)?;
        if current()? != *parent {
            return Err(blocked(
                "parent token changed before fixture label operation",
            ));
        }
        report(
            root,
            "restricted-medium-intent.json",
            &json!({"operation":"lower-only-new-restricted-worker-token-to-medium","before":before,"requestedIntegritySid":"S-1-16-8192","parentTokenMutation":false}),
        )?;
        // Commit before the native call: an error or unknown result is terminal.
        self.lowering_attempted = true;
        let result = if before.integrity_sid == "S-1-16-8192" {
            Ok(())
        } else {
            let label_bytes = size_of::<TOKEN_MANDATORY_LABEL>();
            let mut sid_bytes = 12u32; // One-subauthority mandatory integrity SID.
            let bytes = label_bytes + sid_bytes as usize;
            // Stable, aligned, contiguous label + SID buffer, bounded to 32 bytes
            // on x64. The SID pointer stays inside this allocation for the call.
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let sid = PSID(unsafe { storage.as_mut_ptr().cast::<u8>().add(label_bytes).cast() });
            unsafe {
                CreateWellKnownSid(WinMediumLabelSid, None, Some(sid), &mut sid_bytes)
                    .map_err(win)?;
            }
            if sid_bytes != 12 || sid_text(sid)? != "S-1-16-8192" {
                return Err(blocked("medium integrity SID construction differed"));
            }
            let label = TOKEN_MANDATORY_LABEL {
                Label: SID_AND_ATTRIBUTES {
                    Sid: sid,
                    Attributes: SE_GROUP_INTEGRITY as u32,
                },
            };
            unsafe {
                storage
                    .as_mut_ptr()
                    .cast::<TOKEN_MANDATORY_LABEL>()
                    .write(label);
                // The only SetTokenInformation target in this module is the
                // fresh handle produced by this type's constructor above.
                SetTokenInformation(
                    raw(&self.token),
                    TokenIntegrityLevel,
                    storage.as_ptr().cast(),
                    bytes as u32,
                )
                .map_err(|error| {
                    token_read_error(
                        TokenIntegrityLevel,
                        "set-new-fixture-token-medium",
                        0,
                        bytes as u32,
                        Some(error.code().0),
                    )
                })
            }
        };
        // Preserve actual child-token and parent observations even if the setter
        // reports an error. Never substitute the requested/projected observation.
        let after = self.observe();
        let parent_after = current();
        report(
            root,
            "restricted-medium-outcome.json",
            &json!({"setError":result.as_ref().err().map(ToString::to_string),"observed":after.as_ref().ok(),"observationError":after.as_ref().err().map(ToString::to_string),"parentUnchanged":parent_after.as_ref().is_ok_and(|actual|actual==parent),"parentObservationError":parent_after.as_ref().err().map(ToString::to_string)}),
        )?;
        result?;
        let after = after?;
        if parent_after? != *parent {
            return Err(blocked(
                "parent token changed during fixture label operation",
            ));
        }
        if after != requested {
            return Err(blocked(
                "actual restricted token differs from requested label-only change",
            ));
        }
        after.require(parent)?;
        report(root, "restricted-medium-token.json", &after)?;
        self.admitted = Some(after);
        Ok(())
    }
    fn for_launch(&self, parent: &Observation) -> io::Result<HANDLE> {
        let admitted = self
            .admitted
            .as_ref()
            .ok_or_else(|| blocked("fixture worker token has no observed Medium admission"))?;
        let actual = self.observe()?;
        if actual != *admitted || current()? != *parent {
            return Err(blocked(
                "admitted fixture or parent token changed before launch",
            ));
        }
        actual.require(parent)?;
        Ok(raw(&self.token))
    }
}

struct Child {
    process: OwnedHandle,
    thread: OwnedHandle,
    resumed: bool,
}
impl Drop for Child {
    fn drop(&mut self) {
        if !self.resumed {
            unsafe {
                let _ = TerminateProcess(raw(&self.process), 0xccde0001);
                WaitForSingleObject(raw(&self.process), 5000);
            }
        }
    }
}
pub(super) fn run_worker(root: &Path) -> io::Result<()> {
    run_scoped_worker(root, WorkerScope::Payload)
}

/// Exactly two diagnostic entrypoints, always in a new fixture-owned directory.
/// No caller-supplied executable, command, root, token or job is accepted.
pub(crate) fn run_restart_lifetime(live: bool) -> io::Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("restart-worker");
    let user = CurrentUser::capture()?;
    let owned = PrivateDirectory::create_new(
        Directory::open_absolute(temporary.path())?,
        ComponentName::new(std::ffi::OsStr::new("restart-worker"))?,
        &user,
    )?;
    let result = run_scoped_worker(
        &root,
        WorkerScope::RestartLifetime {
            owned: &owned,
            live,
        },
    );
    owned.verify(&user)?;
    result
}

/// Called before either child probe. Its current directory is assigned at
/// creation, never by changing the parent process or shared environment.
pub(crate) fn verify_restart_worker(live: bool) -> io::Result<()> {
    let root = std::env::current_dir()?;
    let user = CurrentUser::capture()?;
    user.require_unelevated()?;
    let owned = PrivateDirectory::open_existing(
        Directory::open_absolute(
            root.parent()
                .ok_or_else(|| blocked("missing fixture parent"))?,
        )?,
        ComponentName::new(
            root.file_name()
                .ok_or_else(|| blocked("missing fixture name"))?,
        )?,
        &user,
    )?;
    let admission: serde_json::Value =
        serde_json::from_slice(&bounded_read(&root.join("worker-admission.json"), 65536)?)?;
    let exact = ExactProcess::capture_observed(std::process::id())?;
    exact.verify_current_user(&user)?;
    if admission["workerTest"] != restart_worker_test(live)
        || admission["process"] != serde_json::to_value(exact.identity())?
    {
        return Err(blocked(
            "restart worker differs from its exact fixture admission",
        ));
    }
    verify_worker(&root)?;
    owned.verify(&user)
}

fn run_scoped_worker(root: &Path, scope: WorkerScope<'_>) -> io::Result<()> {
    let parent = current()?;
    report(root, "parent-token.json", &parent)?;
    let mut restricted = RestrictedWorkerToken::create(&parent, &scope)?;
    let candidate = restricted.observe()?;
    report(root, "restricted-candidate-token.json", &candidate)?;
    restricted.lower_to_medium(root, &parent, &scope)?;
    let executable = std::env::current_exe()?;
    let image = executable
        .to_str()
        .ok_or_else(|| blocked("test image is not UTF-16 representable"))?;
    if image.contains(['"', '\r', '\n']) {
        return Err(blocked("unsupported test image path"));
    }
    let application: Vec<_> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let worker_test = scope.worker_test();
    let command =
        format!("\"{image}\" --exact {worker_test} --ignored --nocapture --test-threads=1");
    let working_directory: Vec<_> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    let current_directory = match &scope {
        WorkerScope::Payload => PCWSTR::null(),
        WorkerScope::RestartLifetime { .. } => PCWSTR(working_directory.as_ptr()),
    };
    let mut line: Vec<_> = command.encode_utf16().chain(Some(0)).collect();
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    // No shell, supplied executable, inherited handle, credential or alternate profile.
    let mut output = PROCESS_INFORMATION::default();
    let job = unsafe { owned(CreateJobObjectW(None, PCWSTR::null()).map_err(win)?) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(win)?;
    }
    report(
        root,
        "worker-launch-intent.json",
        &json!({"application":executable,"commandLine":command,"commandSha256":sha256(command.as_bytes()),"flags":["CREATE_SUSPENDED"],"tokenFlags":["LUA_TOKEN","DISABLE_MAX_PRIVILEGE"],"observedWorkerIntegrity":"S-1-16-8192"}),
    )?;
    unsafe {
        CreateProcessAsUserW(
            Some(restricted.for_launch(&parent)?),
            PCWSTR(application.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED,
            None,
            current_directory,
            &startup,
            &mut output,
        )
        .map_err(win)?;
    }
    let mut child = Child {
        process: unsafe { owned(output.hProcess) },
        thread: unsafe { owned(output.hThread) },
        resumed: false,
    };
    // This child remains definitely suspended until exact token and ownership receipts.
    unsafe {
        AssignProcessToJobObject(raw(&job), raw(&child.process)).map_err(win)?;
    }
    let actual = observe(raw(&process_token(raw(&child.process))?))?;
    report(root, "worker-token.json", &actual)?;
    actual.require(&parent)?;
    let exact = ExactProcess::capture_observed(output.dwProcessId)?;
    report(
        root,
        "worker-admission.json",
        &json!({"pid":output.dwProcessId,"token":actual,"process":exact.identity(),"fixtureRoot":root,"sameUserRestrictedProbe":true,"workerTest":worker_test}),
    )?;
    report(
        root,
        "worker-resume-intent.json",
        &json!({"pid":output.dwProcessId,"process":exact.identity()}),
    )?;
    child.resumed = true; // Unknown resume outcome must not be retried.
    if unsafe { ResumeThread(raw(&child.thread)) } != 1 {
        return Err(blocked("worker resume outcome unknown"));
    }
    if unsafe { WaitForSingleObject(raw(&child.process), 300_000) } != WAIT_OBJECT_0 {
        return Err(blocked(
            "worker did not terminate within bounded fixture window",
        ));
    }
    let mut code = 0;
    unsafe {
        GetExitCodeProcess(raw(&child.process), &mut code).map_err(win)?;
    }
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(raw(&job)),
            JobObjectBasicAccountingInformation,
            (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            None,
        )
        .map_err(win)?;
    }
    report(
        root,
        "worker-terminal.json",
        &json!({"process":exact.identity(),"exitCode":code,"activeProcesses":accounting.ActiveProcesses}),
    )?;
    if code != 0 || accounting.ActiveProcesses != 0 {
        return Err(blocked("worker failed or owned job not empty"));
    }
    Ok(())
}
pub(super) fn verify_worker(root: &Path) -> io::Result<()> {
    let admission: serde_json::Value =
        serde_json::from_slice(&bounded_read(&root.join("worker-admission.json"), 65536)?)?;
    let parent: Observation =
        serde_json::from_slice(&bounded_read(&root.join("parent-token.json"), 65536)?)?;
    let observed = current()?;
    observed.require(&parent)?;
    if admission["pid"].as_u64() != Some(std::process::id() as u64)
        || admission["token"] != serde_json::to_value(&observed)?
        || admission["fixtureRoot"] != serde_json::to_value(root)?
    {
        return Err(blocked(
            "worker does not match the exact controller admission",
        ));
    }
    report(root, "worker-self-token.json", &observed)
}

// 检查 elevated、高完整性、启用管理员 SID、不同用户与额外权限全部阻止受限令牌验收。
#[test]
fn HistoryPayload_TokenGate_004() {
    let good = Observation {
        sid: "S-1-5-21-1".into(),
        session: 1,
        elevated: false,
        integrity_sid: "S-1-16-8192".into(),
        administrator_attributes: Some(16),
        privileges: vec![(23, 0, 3)],
    };
    good.require(&good).unwrap();
    for mutation in 0..6 {
        let mut bad = good.clone();
        match mutation {
            0 => bad.elevated = true,
            1 => bad.integrity_sid = "S-1-16-12288".into(),
            2 => bad.administrator_attributes = Some(4),
            3 => bad.sid = "S-1-5-21-2".into(),
            4 => bad.privileges.push((20, 0, 0)),
            _ => bad.session = 2,
        }
        assert!(
            bad.require(&good).is_err(),
            "unsafe token mutation {mutation} admitted"
        );
    }
}

// 检查真实当前进程令牌可完整读取，与生产 SID/提升状态及独立会话 API 一致；不要求安装资格。
#[test]
fn HistoryPayload_ObserveToken_005() {
    let observed =
        current().expect("current-token observation must succeed before eligibility is considered");
    let production = crate::version_history::windows::security::CurrentUser::capture().unwrap();
    assert_eq!(
        observed.sid,
        production.sid_text(),
        "fixture and production must observe the same token user"
    );
    assert_eq!(
        !observed.elevated && cfg!(target_arch = "x86_64"),
        production.require_unelevated().is_ok(),
        "typed elevation must match the independent production read"
    );
    let mut session = 0;
    unsafe {
        windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(
            std::process::id(),
            &mut session,
        )
        .unwrap();
    }
    assert_eq!(
        observed.session, session,
        "typed TokenSessionId must match the actual process session"
    );
    assert!(
        observed.integrity_sid.starts_with("S-1-16-"),
        "integrity label must be a mandatory integrity SID"
    );
    // This test reads the ordinary runner token, which may be elevated. It does
    // not call require(), create a restricted token/process, or install anything.
}

// 检查实际无效令牌调用保留安全的 class/stage/长度/错误码，不输出令牌数据。
#[test]
fn HistoryPayload_TokenReadError_006() {
    let error = match info(HANDLE::default(), TokenUser) {
        Ok(_) => panic!("a null token handle must not produce token information"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("class=TokenUser(1)"),
        "missing token class: {error}"
    );
    assert!(
        error.contains("stage=size"),
        "missing reader stage: {error}"
    );
    assert!(
        error.contains("returnedBytes="),
        "missing returned length: {error}"
    );
    assert!(
        error.contains("capacityBytes=0"),
        "missing supplied length: {error}"
    );
    assert!(
        error.contains("hresult=0x"),
        "missing API error code: {error}"
    );
}

// 检查降级请求仅允许同用户受限 High/Medium 令牌，不能用标签变更掩盖其它资格失败。
#[test]
fn HistoryPayload_MediumRequest_007() {
    let parent = Observation {
        sid: "S-1-5-21-1".into(),
        session: 2,
        elevated: true,
        integrity_sid: "S-1-16-12288".into(),
        administrator_attributes: Some(15),
        privileges: vec![(20, 0, 2)],
    };
    let high = Observation {
        sid: parent.sid.clone(),
        session: parent.session,
        elevated: false,
        integrity_sid: "S-1-16-12288".into(),
        administrator_attributes: Some(16),
        privileges: vec![(23, 0, 3)],
    };
    assert!(
        high.require(&parent).is_err(),
        "High remains ineligible before actual lowering"
    );
    let expected = medium_request(&high, &parent).unwrap();
    assert_eq!(expected.integrity_sid, "S-1-16-8192");
    assert_eq!(medium_request(&expected, &parent).unwrap(), expected);
    for mutation in 0..8 {
        let mut bad = high.clone();
        match mutation {
            0 => bad.sid = "S-1-5-21-2".into(),
            1 => bad.session = 3,
            2 => bad.elevated = true,
            3 => bad.administrator_attributes = Some(4),
            4 => bad.privileges.push((20, 0, 0)),
            5 => bad.integrity_sid = "S-1-16-4096".into(),
            6 => bad.integrity_sid = "S-1-16-16384".into(),
            _ => bad.integrity_sid = "unreadable".into(),
        }
        assert!(
            medium_request(&bad, &parent).is_err(),
            "unsafe medium-label request {mutation} admitted"
        );
    }
}
