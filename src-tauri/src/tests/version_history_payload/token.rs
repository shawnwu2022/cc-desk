//! Test-only capability probe. This never changes an account, credential, UAC,
//! policy, session, or production token. Unsupported restricted tokens block.
use super::{blocked, bounded_read, report};
use crate::version_history::{verified_package::sha256, windows::process::ExactProcess};
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
        Authorization::ConvertSidToStringSidW, CreateRestrictedToken, GetTokenInformation,
        IsValidSid, TokenElevation, TokenGroups, TokenIntegrityLevel, TokenPrivileges,
        TokenSessionId, TokenUser, DISABLE_MAX_PRIVILEGE, LUA_TOKEN, PSID, TOKEN_ASSIGN_PRIMARY,
        TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS,
        TOKEN_MANDATORY_LABEL, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
    },
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
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
fn info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
    let mut size = 0;
    let error = unsafe { GetTokenInformation(token, class, None, 0, &mut size) }
        .expect_err("token sizing must fail");
    if error.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult() || size == 0 || size > 65536 {
        return Err(blocked("unsupported token observation length"));
    }
    let mut bytes = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(bytes.as_mut_ptr().cast()),
            size,
            &mut size,
        )
        .map_err(win)?;
    }
    Ok(bytes)
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
    let elevation = info(token, TokenElevation)?;
    let session = info(token, TokenSessionId)?;
    let privileges = info(token, TokenPrivileges)?;
    unsafe {
        let groups_ptr = &*groups.as_ptr().cast::<TOKEN_GROUPS>();
        let count = groups_ptr.GroupCount as usize;
        let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
        if offset + count * size_of::<windows::Win32::Security::SID_AND_ATTRIBUTES>()
            > groups.len() * size_of::<usize>()
        {
            return Err(blocked("invalid token group size"));
        }
        let entries = std::slice::from_raw_parts(groups_ptr.Groups.as_ptr(), count);
        let mut admin = None;
        for group in entries {
            if sid_text(group.Sid)? == "S-1-5-32-544" {
                admin = Some(group.Attributes);
            }
        }
        let privileges_ptr = &*privileges.as_ptr().cast::<TOKEN_PRIVILEGES>();
        let count = privileges_ptr.PrivilegeCount as usize;
        if std::mem::offset_of!(TOKEN_PRIVILEGES, Privileges)
            + count * size_of::<windows::Win32::Security::LUID_AND_ATTRIBUTES>()
            > privileges.len() * size_of::<usize>()
        {
            return Err(blocked("invalid token privilege size"));
        }
        let privileges = std::slice::from_raw_parts(privileges_ptr.Privileges.as_ptr(), count)
            .iter()
            .map(|p| (p.Luid.LowPart, p.Luid.HighPart, p.Attributes.0))
            .collect();
        Ok(Observation {
            sid: sid_text((*user.as_ptr().cast::<TOKEN_USER>()).User.Sid)?,
            session: *session.as_ptr().cast::<u32>(),
            elevated: (*elevation.as_ptr().cast::<TOKEN_ELEVATION>()).TokenIsElevated != 0,
            integrity_sid: sid_text(
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
    let parent = current()?;
    report(root, "parent-token.json", &parent)?;
    let mut original = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            &mut original,
        )
        .map_err(win)?;
    }
    let original = unsafe { owned(original) };
    let mut restricted = HANDLE::default();
    unsafe {
        CreateRestrictedToken(
            raw(&original),
            LUA_TOKEN | DISABLE_MAX_PRIVILEGE,
            None,
            None,
            None,
            &mut restricted,
        )
        .map_err(win)?;
    }
    let restricted = unsafe { owned(restricted) };
    let candidate = observe(raw(&restricted))?;
    report(root, "restricted-candidate-token.json", &candidate)?;
    candidate.require(&parent)?;
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
    let command = format!("\"{image}\" --exact tests::version_history_payload::HistoryPayload_Worker_002 --ignored --nocapture --test-threads=1");
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
        &json!({"application":executable,"commandLine":command,"commandSha256":sha256(command.as_bytes()),"flags":["CREATE_SUSPENDED"],"tokenFlags":["LUA_TOKEN","DISABLE_MAX_PRIVILEGE"]}),
    )?;
    unsafe {
        CreateProcessAsUserW(
            Some(raw(&restricted)),
            PCWSTR(application.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED,
            None,
            PCWSTR::null(),
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
        &json!({"pid":output.dwProcessId,"token":actual,"process":exact.identity(),"fixtureRoot":root,"sameUserRestrictedProbe":true}),
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
