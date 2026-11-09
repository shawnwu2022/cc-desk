//! Authenticated, bounded diagnostics for exactly one selected profile revision.
use super::{
    build_invocation, freeze_launch, resolve_process, EnvMap, FreezeContext, LaunchService,
};
use crate::cli::profiles::{error, Dialect, Launcher};
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, SafeError, WireU64};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProbeRequest {
    pub(crate) request_id: String,
    pub(crate) profile_id: String,
    pub(crate) expected_profile_revision: WireU64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CancelRequest {
    pub(crate) request_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProbeResult {
    request_id: String,
    profile_id: String,
    profile_revision: WireU64,
    cli: CliKind,
    state: &'static str,
    version: Option<String>,
    reason: Option<&'static str>,
}
#[derive(Serialize)]
pub(crate) struct CancelResult {
    pub(crate) status: &'static str,
}
struct Entry {
    caller: CallerIdentity,
    id: String,
    state: Mutex<EntryState>,
    ready: Condvar,
}
#[derive(Default)]
struct EntryState {
    request: Option<ProbeRequest>,
    cancelled: bool,
    started: bool,
    settled: bool,
    result: Option<Result<ProbeResult, SafeError>>,
    #[cfg(all(test, windows))]
    retain_job: bool,
    #[cfg(all(test, windows))]
    retained_job: Option<std::os::windows::io::OwnedHandle>,
}
#[derive(Default)]
pub(crate) struct Probes {
    entries: Mutex<Vec<Arc<Entry>>>,
    profiles: Arc<Mutex<std::collections::HashSet<String>>>,
}
struct ProfileOwner {
    profiles: Arc<Mutex<std::collections::HashSet<String>>>,
    profile_id: String,
}
impl Drop for ProfileOwner {
    fn drop(&mut self) {
        self.profiles.lock().remove(&self.profile_id);
    }
}
fn validate_id(id: &str) -> Result<(), SafeError> {
    if uuid::Uuid::parse_str(id).is_err() || id.len() != 36 {
        return Err(SafeError::invalid("requestId"));
    }
    Ok(())
}
impl Probes {
    fn claim_profile(&self, profile_id: &str) -> Option<ProfileOwner> {
        if !self.profiles.lock().insert(profile_id.to_owned()) {
            return None;
        }
        Some(ProfileOwner {
            profiles: self.profiles.clone(),
            profile_id: profile_id.into(),
        })
    }
    fn entry(
        &self,
        service: &LaunchService,
        caller: &CallerIdentity,
        id: &str,
    ) -> Result<Arc<Entry>, SafeError> {
        validate_id(id)?;
        service.registry().check_caller(caller)?;
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.iter().find(|e| e.caller == *caller && e.id == id) {
            return Ok(entry.clone());
        }
        // Never forget an active request or a current-document cancellation fence.
        entries.retain(|e| {
            let state = e.state.lock();
            service.registry().check_caller(&e.caller).is_ok() || (state.started && !state.settled)
        });
        if entries.len() >= 512 {
            return Err(error("REGISTRY_CAPACITY"));
        }
        let entry = Arc::new(Entry {
            caller: caller.clone(),
            id: id.into(),
            state: Mutex::new(EntryState::default()),
            ready: Condvar::new(),
        });
        entries.push(entry.clone());
        Ok(entry)
    }
}
impl LaunchService {
    pub(crate) fn cancel_cli_version(
        &self,
        caller: &CallerIdentity,
        request: CancelRequest,
    ) -> Result<CancelResult, SafeError> {
        let entry = self
            .version_probes
            .entry(self, caller, &request.request_id)?;
        let mut state = entry.state.lock();
        state.cancelled = true;
        Ok(CancelResult {
            status: if state.settled {
                "settled"
            } else {
                "requested"
            },
        })
    }
    pub(crate) fn probe_cli_version(
        self: &Arc<Self>,
        caller: &CallerIdentity,
        query: ProbeRequest,
    ) -> Result<ProbeResult, SafeError> {
        if query.profile_id.is_empty()
            || query.profile_id.len() > 128
            || !query
                .profile_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(SafeError::invalid("profileId"));
        }
        let entry = self.version_probes.entry(self, caller, &query.request_id)?;
        let mut state = entry.state.lock();
        if state.request.as_ref().is_some_and(|r| r != &query) {
            return Err(error("REQUEST_CONFLICT"));
        }
        state.request = Some(query.clone());
        if !state.started {
            state.started = true;
            let service = self.clone();
            let work = entry.clone();
            let caller = caller.clone();
            std::thread::Builder::new()
                .name("cli-version-owner".into())
                .spawn(move || {
                    let mut result = service.collect_cli_version(&caller, &query, &work);
                    let mut state = work.state.lock();
                    if state.cancelled {
                        if let Ok(value) = &mut result {
                            value.state = "cancelled";
                            value.version = None;
                            value.reason = None;
                        }
                    }
                    state.settled = true;
                    if state.result.is_none() {
                        state.result = Some(result);
                    }
                    drop(state);
                    work.ready.notify_all();
                })
                .map_err(|_| {
                    state.settled = true;
                    state.result = Some(Err(error("PROCESS_START_FAILED")));
                    entry.ready.notify_all();
                    error("PROCESS_START_FAILED")
                })?;
        }
        while state.result.is_none() {
            entry.ready.wait(&mut state);
        }
        self.registry().check_caller(caller)?;
        let mut result = state.result.clone().unwrap()?;
        if self
            .repository
            .get_profile(&result.profile_id)
            .map_or(true, |p| p.revision != result.profile_revision)
        {
            result.state = "unknown";
            result.version = None;
            result.reason = Some("profile-changed");
        }
        Ok(result)
    }
    fn collect_cli_version(
        &self,
        caller: &CallerIdentity,
        query: &ProbeRequest,
        entry: &Entry,
    ) -> Result<ProbeResult, SafeError> {
        self.registry().check_caller(caller)?;
        let profile = self.repository.get_profile(&query.profile_id)?;
        let mut result = ProbeResult {
            request_id: query.request_id.clone(),
            profile_id: query.profile_id.clone(),
            profile_revision: query.expected_profile_revision,
            cli: profile.cli,
            state: "unknown",
            version: None,
            reason: None,
        };
        let current = || -> Option<&'static str> {
            if self.registry().check_caller(caller).is_err() {
                Some("document-revoked")
            } else if *self.shutting_down.read() {
                Some("unavailable")
            } else if self
                .repository
                .get_profile(&query.profile_id)
                .map_or(true, |p| p != profile)
            {
                Some("profile-changed")
            } else {
                None
            }
        };
        if let Some(reason) = current().or_else(|| {
            (profile.revision != query.expected_profile_revision).then_some("profile-changed")
        }) {
            result.reason = Some(reason);
            return Ok(result);
        }
        if entry.state.lock().cancelled {
            result.state = "cancelled";
            return Ok(result);
        }
        if profile.cli == CliKind::Shell
            || !matches!(
                profile.launcher,
                Launcher::Native
                    | Launcher::Shim {
                        dialect: Dialect::Cmd,
                        ..
                    }
            )
        {
            result.reason = Some("unsupported-launcher");
            return Ok(result);
        }
        let Some(_profile_owner) = self.version_probes.claim_profile(&query.profile_id) else {
            result.reason = Some("probe-in-progress");
            return Ok(result);
        };
        let prepare = || {
            // A private neutral cwd, never a project or user script input.
            let cwd = tempfile::tempdir().map_err(|_| error("UNAVAILABLE"))?;
            let request = LaunchRequest {
                request_id: query.request_id.clone(),
                tab_id: "version-probe".into(),
                run_id: "version-probe".into(),
                generation: 1,
                profile_id: profile.id.clone(),
                expected_profile_revision: profile.revision,
                cli: profile.cli,
                launch_cwd: cwd
                    .path()
                    .to_str()
                    .ok_or_else(|| error("UNAVAILABLE"))?
                    .into(),
                action: LaunchAction::Raw {
                    argv: vec!["--version".into()],
                },
                extra_args: vec![],
                cols: 80,
                rows: 24,
            };
            let legacy =
                profile.read_legacy(&self.repository.metadata_directory().join("config.json"))?;
            let inherited = self.inherited_environment()?;
            let snapshot = freeze_launch(
                &request,
                &profile,
                caller,
                &FreezeContext {
                    inherited: &inherited,
                    terminal: &EnvMap::new(),
                    legacy: legacy.as_ref(),
                    observer: None,
                },
            )?;
            let spec = resolve_process(&build_invocation(&request, &snapshot)?)?;
            Ok::<_, SafeError>((cwd, spec))
        };
        let (_cwd, spec) = match prepare() {
            Ok(value) => value,
            Err(e) => {
                result.reason = Some(match e.code.as_str() {
                    "PROGRAM_UNAVAILABLE" => "program-unavailable",
                    "RUNNER_UNAVAILABLE" => "runner-unavailable",
                    "UNSUPPORTED_LAUNCHER" | "ARG_NOT_REPRESENTABLE" => "unsupported-launcher",
                    _ => "unavailable",
                });
                return Ok(result);
            }
        };
        let (mut admission, mut owner) = match self.admit_owned_command() {
            Ok(value) => value,
            Err(_) => {
                result.reason = Some("unavailable");
                return Ok(result);
            }
        };
        // Windows 的直接进程与整个 Job 都已收口，才可释放准入。
        let collected = (|| {
            let mut command = spec.metadata_command().map_err(|_| "program-unavailable")?;
            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            #[cfg(windows)]
            let job =
                crate::cli::opencode_prepare::server_job::Job::new().map_err(|_| "unavailable")?;
            #[cfg(all(test, windows))]
            {
                let mut state = entry.state.lock();
                if state.retain_job {
                    state.retained_job = Some(job.duplicate_handle().map_err(|_| "unavailable")?);
                }
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000 | 0x4);
            }
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            if let Some(reason) = current() {
                return Err(reason);
            }
            let shutdown = self.shutting_down.read();
            if *shutdown {
                return Err("unavailable");
            }
            let cancel = entry.state.lock();
            if cancel.cancelled {
                return Err("cancelled");
            }
            let ticket = admission.begin_creation();
            let mut child = match self
                .registry()
                .with_current_caller(caller, || command.spawn())
            {
                Ok(Ok(child)) => child,
                Err(_) => {
                    ticket.no_child_created();
                    return Err("document-revoked");
                }
                Ok(Err(_)) => {
                    ticket.no_child_created();
                    return Err("command-failed");
                }
            };
            let ticket = ticket.child_created();
            drop(cancel);
            drop(shutdown);
            let output = Arc::new(Mutex::new(Output::default()));
            let mut failure = None;
            #[cfg(windows)]
            if job.assign_and_resume(&child).is_err() {
                failure = Some("command-failed");
            }
            for (reader, stderr) in [
                (
                    child
                        .stdout
                        .take()
                        .map(|v| Box::new(v) as Box<dyn Read + Send>),
                    false,
                ),
                (
                    child
                        .stderr
                        .take()
                        .map(|v| Box::new(v) as Box<dyn Read + Send>),
                    true,
                ),
            ] {
                if let Some(reader) = reader {
                    if drain(reader, output.clone(), stderr).is_err() {
                        failure = Some("command-failed");
                    }
                } else {
                    failure = Some("command-failed");
                }
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stopping_since = None;
            loop {
                if failure.is_none() {
                    failure = current()
                        .or_else(|| entry.state.lock().cancelled.then_some("cancelled"))
                        .or_else(|| output.lock().limit.then_some("output-limit"))
                        .or_else(|| (Instant::now() >= deadline).then_some("timeout"));
                }
                if failure.is_some() {
                    stopping_since.get_or_insert_with(Instant::now);
                    #[cfg(windows)]
                    let _ = job.terminate();
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.kill();
                }
                if stopping_since.is_some_and(|since| since.elapsed() >= Duration::from_secs(2)) {
                    let mut state = entry.state.lock();
                    if state.result.is_none() {
                        let mut unknown = result.clone();
                        unknown.reason = Some("termination-unknown");
                        state.result = Some(Ok(unknown));
                        entry.ready.notify_all();
                    }
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        #[cfg(windows)]
                        {
                            // 终止只是请求；查询失败或后代未退出时继续保留 owner 与票据。
                            let _ = job.terminate();
                            if !job.is_empty().unwrap_or(false) {
                                stopping_since.get_or_insert_with(Instant::now);
                                std::thread::sleep(Duration::from_millis(10));
                                continue;
                            }
                        }
                        #[cfg(unix)]
                        unsafe {
                            libc::kill(-(child.id() as i32), libc::SIGKILL);
                        }
                        ticket.reaped();
                        if let Some(reason) = failure {
                            return Err(reason);
                        }
                        if !status.success() {
                            return Err("command-failed");
                        }
                        let drain_deadline = Instant::now() + Duration::from_secs(1);
                        loop {
                            let state = output.lock();
                            if state.limit {
                                return Err("output-limit");
                            }
                            if state.eof == 2 {
                                if state.failed || state.stderr {
                                    return Err("invalid-output");
                                }
                                return crate::cli::diagnostic_version_parser::parse_version(
                                    profile.cli.as_str(),
                                    &state.bytes,
                                )
                                .ok_or("invalid-output");
                            }
                            drop(state);
                            if Instant::now() >= drain_deadline {
                                return Err("invalid-output");
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                    Ok(None) => {}
                    Err(_) => failure = Some("termination-unknown"),
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        owner.settled = true;
        let collected = match current() {
            Some(reason) => Err(reason),
            None => collected,
        };
        if entry.state.lock().cancelled {
            result.state = "cancelled";
            return Ok(result);
        }
        match collected {
            Ok(version) => {
                result.state = "known";
                result.version = Some(version);
            }
            Err("cancelled") => result.state = "cancelled",
            Err(reason) => result.reason = Some(reason),
        }
        Ok(result)
    }
}
#[derive(Default)]
struct Output {
    bytes: Vec<u8>,
    stderr: bool,
    limit: bool,
    eof: usize,
    failed: bool,
}
fn drain(
    mut reader: Box<dyn Read + Send>,
    output: Arc<Mutex<Output>>,
    stderr: bool,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("cli-version-drain".into())
        .spawn(move || {
            let mut buffer = [0; 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut state = output.lock();
                        state.stderr |= stderr;
                        let remaining = 8192usize.saturating_sub(state.bytes.len());
                        if n > remaining {
                            state.limit = true;
                            break;
                        }
                        state.bytes.extend_from_slice(&buffer[..n]);
                    }
                    Err(_) => {
                        output.lock().failed = true;
                        break;
                    }
                }
            }
            output.lock().eof += 1;
        })
        .map(|_| ())
}

#[cfg(test)]
#[path = "../tests/native_cli_versions.rs"]
mod tests;
