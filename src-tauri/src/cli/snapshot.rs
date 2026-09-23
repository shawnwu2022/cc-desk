//! Backend-only immutable launch inputs. No shell execution or global environment mutation.
#![allow(dead_code)]

use super::environment::{build_environment, lookup, same_name, EnvMap, ObserverEnv};
use super::profile_service::authorize_profile_window;
use super::profiles::{error, Launcher, Override, Profile};
use super::types::{CliKind, LaunchAction, LaunchRequest, SafeError, WireU64};
use serde::Serialize;
use serde_json::Value;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

/// Constructed by the backend, never deserialized from a WebView request.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CallerIdentity {
    pub(crate) instance_id: String,
    pub(crate) window_label: String,
    pub(crate) webview_epoch: WireU64,
}

pub(crate) struct FreezeContext<'a> {
    pub(crate) inherited: &'a EnvMap,
    pub(crate) terminal: &'a EnvMap,
    pub(crate) legacy: Option<&'a Value>,
    pub(crate) observer: Option<&'a ObserverEnv>,
}

/// Values and launch intent are frozen, not the executable's on-disk bytes.
/// D10 must use these selected paths; D11 supplies validated caller lifetime.
pub(crate) struct LaunchSnapshot {
    request: LaunchRequest,
    profile: Profile,
    owner: CallerIdentity,
    environment: EnvMap,
    program: PathBuf,
    runner: Option<PathBuf>,
    raw_args: Option<Vec<OsString>>,
    default_args: Vec<OsString>,
    legacy_default_args: Option<String>,
    extra_args: Vec<OsString>,
    skip_permissions: Option<bool>,
}

impl std::fmt::Debug for LaunchSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LaunchSnapshot(<redacted>)")
    }
}

impl LaunchSnapshot {
    pub(crate) fn environment(&self) -> &EnvMap {
        &self.environment
    }

    pub(crate) fn program(&self) -> &Path {
        &self.program
    }

    pub(crate) fn runner(&self) -> Option<&Path> {
        self.runner.as_deref()
    }

    pub(crate) fn profile_revision(&self) -> WireU64 {
        self.profile.revision
    }

    pub(crate) fn owner(&self) -> &CallerIdentity {
        &self.owner
    }

    pub(crate) fn raw_args(&self) -> Option<&[OsString]> {
        self.raw_args.as_deref()
    }

    pub(crate) fn default_args(&self) -> &[OsString] {
        &self.default_args
    }

    /// Legacy shell text is not silently split on spaces. D09/D10 must migrate it explicitly.
    pub(crate) fn legacy_default_args(&self) -> Option<&str> {
        self.legacy_default_args.as_deref()
    }

    pub(crate) fn extra_args(&self) -> &[OsString] {
        &self.extra_args
    }

    pub(crate) fn request(&self) -> &LaunchRequest {
        &self.request
    }

    pub(crate) fn launcher(&self) -> &Launcher {
        &self.profile.launcher
    }

    pub(crate) fn skip_permissions(&self) -> Option<bool> {
        self.skip_permissions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HostStatus {
    NotChecked,
    Available,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Availability {
    pub(crate) profile_revision: WireU64,
    pub(crate) cli: CliKind,
    pub(crate) state: String,
    pub(crate) host_status: HostStatus,
    pub(crate) certified: bool,
}

pub(crate) fn configured_program(
    profile: &Profile,
    legacy: Option<&Value>,
) -> Result<PathBuf, SafeError> {
    let selected = match &profile.program_path {
        Override::Set(path) => Some(path.as_str()),
        Override::Inherit if profile.is_legacy_claude() => legacy
            .and_then(|value| value.get("claudePath"))
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty()),
        Override::Inherit | Override::Unset => None,
    }
    .ok_or_else(|| error("PROGRAM_TRUST_REQUIRED"))?;
    let path = Path::new(selected);
    if selected.contains('\0') || !path.is_absolute() {
        return Err(SafeError::invalid("programPath"));
    }
    if !usable_file(path, matches!(profile.launcher, Launcher::Native)) {
        return Err(error("PROGRAM_UNAVAILABLE"));
    }
    // Preserve the selected absolute spelling: canonicalizing an executable can
    // change virtual-environment or wrapper behavior. Never silently PATH-search.
    Ok(path.to_path_buf())
}

pub(crate) fn configured_runner(profile: &Profile) -> Result<Option<PathBuf>, SafeError> {
    let selected = match &profile.launcher {
        Launcher::Native => return Ok(None),
        Launcher::Shell { program, .. } => program,
        Launcher::Shim { runner, .. } => runner,
    };
    let path = Path::new(selected);
    if !path.is_absolute() {
        return Err(SafeError::invalid("launcher"));
    }
    if !usable_file(path, true) {
        return Err(error("RUNNER_UNAVAILABLE"));
    }
    Ok(Some(path.to_path_buf()))
}

fn usable_file(path: &Path, executable: bool) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if executable && metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    #[cfg(not(unix))]
    let _ = executable;
    true
}

pub(crate) fn freeze_launch(
    request: &LaunchRequest,
    profile: &Profile,
    caller: &CallerIdentity,
    context: &FreezeContext<'_>,
) -> Result<LaunchSnapshot, SafeError> {
    authorize_profile_window(&caller.window_label)?;
    if caller.instance_id.is_empty() || caller.instance_id.contains('\0') {
        return Err(error("FORBIDDEN"));
    }
    request.validate()?;
    profile.validate()?;
    if request.profile_id != profile.id {
        return Err(error("PROFILE_MISMATCH"));
    }
    if request.cli != profile.cli {
        return Err(error("PROFILE_CLI_MISMATCH"));
    }
    if request.expected_profile_revision != profile.revision {
        return Err(error("REVISION_CONFLICT"));
    }
    let cwd = Path::new(&request.launch_cwd);
    if !cwd.is_absolute() {
        return Err(SafeError::invalid("launchCwd"));
    }
    if !cwd.is_dir() {
        return Err(error("WORKING_DIRECTORY_UNAVAILABLE"));
    }

    let raw_args = match &request.action {
        LaunchAction::Raw { argv } => Some(argv.iter().map(OsString::from).collect()),
        _ => None,
    };
    let raw = raw_args.is_some();
    let program = configured_program(profile, context.legacy)?;
    let runner = configured_runner(profile)?;
    let environment = build_environment(
        context.inherited,
        context.terminal,
        profile,
        context.legacy,
        if raw { None } else { context.observer },
    )?;
    let default_args = match (&profile.default_args, raw) {
        (Override::Set(args), false) => args.iter().map(OsString::from).collect(),
        _ => Vec::new(),
    };
    let legacy_default_args = if !raw
        && profile.is_legacy_claude()
        && matches!(profile.default_args, Override::Inherit)
    {
        match context
            .legacy
            .and_then(|value| value.get("defaultCustomArgs"))
        {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if !value.contains('\0') => Some(value.clone()),
            _ => return Err(error("LEGACY_INVALID")),
        }
    } else {
        None
    };
    Ok(LaunchSnapshot {
        request: request.clone(),
        profile: profile.clone(),
        owner: caller.clone(),
        environment,
        program,
        runner,
        raw_args,
        default_args,
        legacy_default_args,
        extra_args: request.extra_args.iter().map(OsString::from).collect(),
        skip_permissions: if raw {
            None
        } else {
            profile.resolve_skip_permissions(context.legacy)
        },
    })
}

pub(crate) fn availability(snapshot: &LaunchSnapshot, host_status: HostStatus) -> Availability {
    let available = usable_file(
        snapshot.program(),
        matches!(snapshot.launcher(), Launcher::Native),
    ) && snapshot.runner().is_none_or(|path| usable_file(path, true));
    Availability {
        profile_revision: snapshot.profile_revision(),
        cli: snapshot.request.cli,
        state: if available {
            "available-unverified"
        } else {
            "unavailable"
        }
        .into(),
        host_status,
        certified: false,
    }
}

fn within(path: &Path, root: &Path) -> bool {
    let mut parts = path.components();
    root.components().all(|part| {
        parts
            .next()
            .is_some_and(|candidate| same_name(candidate.as_os_str(), part.as_os_str()))
    })
}

/// Filesystem-only discovery. A result is a candidate, never automatic approval.
pub(crate) fn discover_candidates(
    name: &str,
    environment: &EnvMap,
    excluded_roots: &[PathBuf],
) -> Result<Vec<PathBuf>, SafeError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err(SafeError::invalid("programName"));
    }
    let roots = excluded_roots
        .iter()
        .map(|root| fs::canonicalize(root).map_err(|_| error("PROJECT_SCOPE_UNAVAILABLE")))
        .collect::<Result<Vec<_>, _>>()?;
    let Some(path) = lookup(environment, OsStr::new("PATH")) else {
        return Ok(Vec::new());
    };
    let names: Vec<OsString> = if cfg!(windows) && Path::new(name).extension().is_none() {
        [".exe", ".com", ".cmd", ".bat", ".ps1"]
            .iter()
            .map(|extension| format!("{name}{extension}").into())
            .collect()
    } else {
        vec![name.into()]
    };
    let mut result = Vec::new();
    for directory in std::env::split_paths(path) {
        if !directory.is_absolute() {
            continue;
        }
        let Ok(directory) = fs::canonicalize(directory) else {
            continue;
        };
        if roots.iter().any(|root| within(&directory, root)) {
            continue;
        }
        for name in &names {
            let candidate = directory.join(name);
            if !usable_file(&candidate, cfg!(unix)) {
                continue;
            }
            let Ok(candidate) = fs::canonicalize(candidate) else {
                continue;
            };
            if candidate.to_str().is_none() {
                return Err(error("PATH_NOT_REPRESENTABLE"));
            }
            if !roots.iter().any(|root| within(&candidate, root)) && !result.contains(&candidate) {
                result.push(candidate);
            }
        }
    }
    Ok(result)
}
