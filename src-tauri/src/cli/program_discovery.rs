//! Read-only executable candidates. Selection remains an explicit profile mutation.
use super::environment::{build_environment, EnvMap};
use super::profile_service::authorize_profile_window;
use super::profiles::{error, Launcher};
use super::snapshot::{configured_program, discover_candidates};
use super::source_scope::{is_verified_key, resolve_path_key};
use super::storage::WorkspaceRepository;
use super::types::{CliKind, SafeError, WireU64};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiscoveryRequest {
    pub(crate) profile_id: String,
    pub(crate) expected_revision: WireU64,
    pub(crate) project_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProgramCandidate {
    pub(crate) program_path: String,
    pub(crate) launcher: Launcher,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProgramDiscovery {
    pub(crate) profile_id: String,
    pub(crate) profile_revision: WireU64,
    pub(crate) workspace_revision: WireU64,
    pub(crate) project_id: String,
    pub(crate) cli: CliKind,
    pub(crate) candidates: Vec<ProgramCandidate>,
}

pub(crate) fn discover_programs(
    repository: &WorkspaceRepository,
    caller: &str,
    request: &DiscoveryRequest,
    inherited: &EnvMap,
) -> Result<ProgramDiscovery, SafeError> {
    authorize_profile_window(caller)?;
    if [&request.profile_id, &request.project_id].iter().any(|id| {
        id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    }) {
        return Err(SafeError::invalid("request"));
    }
    let document = repository.read()?;
    let profile = document
        .profiles
        .get(&request.profile_id)
        .ok_or_else(|| error("PROFILE_NOT_FOUND"))?;
    if profile.revision != request.expected_revision {
        return Err(error("REVISION_CONFLICT"));
    }
    if profile.cli == CliKind::Shell || !matches!(profile.launcher, Launcher::Native) {
        return Err(error("DISCOVERY_UNAVAILABLE"));
    }
    let legacy = profile.read_legacy(&repository.metadata_directory().join("config.json"))?;
    match configured_program(profile, legacy.as_ref()) {
        Err(issue) if issue.code == "PROGRAM_TRUST_REQUIRED" => {}
        Err(issue) => return Err(issue), // An explicit broken selection never falls back to PATH.
        Ok(_) => return Err(error("PROGRAM_ALREADY_CONFIGURED")),
    }
    if !document
        .registered_projects
        .contains_key(&request.project_id)
    {
        return Err(error("PROJECT_NOT_FOUND"));
    }
    let mut roots = vec![repository.metadata_directory().to_path_buf()];
    for project in document.registered_projects.values() {
        let current = resolve_path_key(&project.selected_path)?;
        if !is_verified_key(&current.key) || current.key != project.source_path_key {
            return Err(error("PROJECT_IDENTITY_CHANGED"));
        }
        roots.push(project.selected_path.clone());
    }
    let environment = build_environment(inherited, &EnvMap::new(), profile, legacy.as_ref(), None)?;
    let mut candidates = vec![];
    for path in discover_candidates(profile.cli.as_str(), &environment, &roots)? {
        let Some(launcher) = candidate_launcher(&path, &environment, &roots)? else {
            continue;
        };
        candidates.push(ProgramCandidate {
            program_path: launch_spelling(&path)?
                .to_str()
                .ok_or_else(|| error("PATH_NOT_REPRESENTABLE"))?
                .into(),
            launcher,
        });
        if candidates.len() == 32 {
            break;
        }
    }
    if repository.read()?.revision != document.revision {
        return Err(error("REVISION_CONFLICT"));
    }
    Ok(ProgramDiscovery {
        profile_id: profile.id.clone(),
        profile_revision: profile.revision,
        workspace_revision: document.revision,
        project_id: request.project_id.clone(),
        cli: profile.cli,
        candidates,
    })
}

fn launch_spelling(path: &std::path::Path) -> Result<std::path::PathBuf, SafeError> {
    #[cfg(windows)]
    {
        // cmd cannot execute a batch file using the verbatim spelling returned by
        // canonicalize. Change spelling only after proving it resolves identically.
        let text = path
            .to_str()
            .ok_or_else(|| error("PATH_NOT_REPRESENTABLE"))?;
        let selected = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            std::path::PathBuf::from(format!(r"\\{unc}"))
        } else if let Some(dos) = text.strip_prefix(r"\\?\") {
            if dos.as_bytes().get(1) != Some(&b':') {
                return Err(error("PATH_NOT_REPRESENTABLE"));
            }
            std::path::PathBuf::from(dos)
        } else {
            path.to_owned()
        };
        if std::fs::canonicalize(&selected).map_err(|_| error("PROGRAM_UNAVAILABLE"))? != path {
            return Err(error("PATH_NOT_REPRESENTABLE"));
        }
        Ok(selected)
    }
    #[cfg(not(windows))]
    Ok(path.to_owned())
}

#[cfg(not(windows))]
fn candidate_launcher(
    _path: &std::path::Path,
    _environment: &EnvMap,
    _roots: &[std::path::PathBuf],
) -> Result<Option<Launcher>, SafeError> {
    Ok(Some(Launcher::Native))
}

#[cfg(windows)]
fn candidate_launcher(
    path: &std::path::Path,
    environment: &EnvMap,
    roots: &[std::path::PathBuf],
) -> Result<Option<Launcher>, SafeError> {
    use super::environment::lookup;
    use super::profiles::Dialect;
    use std::{ffi::OsStr, path::Path};
    match path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("exe" | "com") => Ok(Some(Launcher::Native)),
        Some("cmd" | "bat") => {
            // Npm Windows shims need a selected cmd runner; never execute discovery or change policy.
            let Some(runner) = lookup(environment, OsStr::new("ComSpec")).map(Path::new) else {
                return Ok(None);
            };
            if !runner.is_absolute()
                || !runner
                    .file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
            {
                return Ok(None);
            }
            let Some(parent) = runner.parent() else {
                return Ok(None);
            };
            let mut runner_environment = EnvMap::new();
            runner_environment.insert(
                "PATH".into(),
                std::env::join_paths([parent]).map_err(|_| error("PATH_NOT_REPRESENTABLE"))?,
            );
            let Some(runner) = discover_candidates("cmd.exe", &runner_environment, roots)?
                .into_iter()
                .next()
            else {
                return Ok(None);
            };
            Ok(Some(Launcher::Shim {
                runner: launch_spelling(&runner)?
                    .to_str()
                    .ok_or_else(|| error("PATH_NOT_REPRESENTABLE"))?
                    .into(),
                dialect: Dialect::Cmd,
            }))
        }
        _ => Ok(None),
    }
}
