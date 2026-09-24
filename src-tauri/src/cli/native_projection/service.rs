//! Binds directory capabilities to D11's live document/run registry and Desk revisions.
use super::registry::{Grant, Owner, ScopeRegistry};
use super::scoped_fs::{ReadResult, Root};
use super::selection::{locations, Locations};
use super::wire::{ProjectionResult, ReadRequest, ScopeTarget, SourceBasis, SourceRef};
use crate::cli::environment::{build_environment, EnvMap};
use crate::cli::launch_service::LaunchService;
use crate::cli::profiles::{error, Launcher, Override, Profile};
use crate::cli::run_registry::RunKey;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{CliKind, LaunchAction, SafeError};
use crate::cli::workspace::RegisteredProject;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) struct ProjectionService {
    launch: Arc<LaunchService>,
    registry: ScopeRegistry,
}
impl ProjectionService {
    pub(crate) fn new(launch: Arc<LaunchService>) -> Self {
        Self {
            launch,
            registry: ScopeRegistry::new(64),
        }
    }
    pub(crate) fn scope(
        &self,
        caller: &CallerIdentity,
        target: &ScopeTarget,
    ) -> Result<SourceRef, SafeError> {
        self.launch.registry().check_caller(caller)?;
        target.validate()?;
        let grant = match target {
            ScopeTarget::Profile {
                profile_id,
                expected_profile_revision,
                project_id,
            } => {
                let profile = self.launch.repository().get_profile(profile_id)?;
                if profile.revision != *expected_profile_revision {
                    return Err(error("REVISION_CONFLICT"));
                }
                let (env, selected) = profile_locations(&self.launch, &profile)?;
                drop(env); // Never retain an ambient environment snapshot in an IPC DTO.
                let project = project_id
                    .as_ref()
                    .map(|id| {
                        self.launch
                            .repository()
                            .read()?
                            .registered_projects
                            .get(id)
                            .cloned()
                            .ok_or_else(|| error("PROJECT_NOT_FOUND"))
                    })
                    .transpose()?;
                let (project_root, project_paths) =
                    project.as_ref().map(admit_project).transpose()?.unzip();
                let service = Arc::downgrade(&self.launch);
                let expected_profile = profile.clone();
                let expected_locations = selected.clone();
                let expected_project = project;
                let authority = caller.clone();
                let check = Arc::new(move || -> ReadResult<()> {
                    let service = service.upgrade().ok_or("SCOPE_REVOKED")?;
                    service
                        .registry()
                        .check_caller(&authority)
                        .map_err(|_| "FORBIDDEN")?;
                    let current = service
                        .repository()
                        .get_profile(&expected_profile.id)
                        .map_err(|_| "SCOPE_REVOKED")?;
                    if current != expected_profile {
                        return Err("SCOPE_REVOKED");
                    }
                    let (_, now) =
                        profile_locations(&service, &current).map_err(|_| "SCOPE_REVOKED")?;
                    if now != expected_locations {
                        return Err("SCOPE_REVOKED");
                    }
                    if let Some(expected) = &expected_project {
                        let current = service.repository().read().map_err(|_| "SCOPE_REVOKED")?;
                        let current = current
                            .registered_projects
                            .get(&expected.project_id)
                            .ok_or("SCOPE_REVOKED")?;
                        if current.source_path_key != expected.source_path_key
                            || current.selected_path != expected.selected_path
                            || current.canonical_path != expected.canonical_path
                        {
                            return Err("SCOPE_REVOKED");
                        }
                    }
                    Ok(())
                });
                Grant {
                    owner: owner(caller),
                    cli: profile.cli,
                    profile_id: profile.id,
                    profile_revision: profile.revision,
                    target: target.clone(),
                    basis: SourceBasis::ConfiguredProfile,
                    root: Root::open(&selected.root).map_err(error)?,
                    user_config: selected
                        .user_config
                        .as_ref()
                        .map(|p| Root::open(p))
                        .transpose()
                        .map_err(error)?,
                    project: project_root,
                    project_paths: project_paths.unwrap_or_default(),
                    check,
                }
            }
            ScopeTarget::Run { run_id, generation } => {
                let run = RunKey {
                    run_id: run_id.clone(),
                    generation: *generation,
                };
                let snapshot = self.launch.access(caller, &run)?.snapshot()?;
                // Unknown/native-picker changes are not inferred by inspecting terminal text.
                if snapshot.raw_args().is_some()
                    || !matches!(snapshot.launcher(), Launcher::Native)
                    || !snapshot.default_args().is_empty()
                    || !snapshot.extra_args().is_empty()
                    || snapshot
                        .legacy_default_args()
                        .is_some_and(|s| !s.is_empty())
                {
                    return Err(error("SCOPE_UNKNOWN"));
                }
                let selected =
                    locations(snapshot.request().cli, snapshot.environment()).map_err(error)?;
                // Only a new no-extra-args launch authorizes its frozen requested cwd for project resources.
                // Resume/picker can choose another cwd; they expose root-wide observations only.
                let (project, project_paths) =
                    if matches!(snapshot.request().action, LaunchAction::New) {
                        let path = Path::new(&snapshot.request().launch_cwd);
                        let root = Root::open(path).map_err(error)?;
                        let paths = verified_spellings(path, &root)?;
                        (Some(root), paths)
                    } else {
                        (None, vec![])
                    };
                let service = Arc::downgrade(&self.launch);
                let expected = snapshot.clone();
                let authority = caller.clone();
                let check = Arc::new(move || -> ReadResult<()> {
                    let service = service.upgrade().ok_or("SCOPE_REVOKED")?;
                    let current = service
                        .access(&authority, &run)
                        .and_then(|a| a.snapshot())
                        .map_err(|_| "SCOPE_REVOKED")?;
                    if !Arc::ptr_eq(&expected, &current) {
                        return Err("SCOPE_REVOKED");
                    }
                    Ok(())
                });
                Grant {
                    owner: owner(caller),
                    cli: snapshot.request().cli,
                    profile_id: snapshot.request().profile_id.clone(),
                    profile_revision: snapshot.profile_revision(),
                    target: target.clone(),
                    basis: SourceBasis::LaunchEnvironment,
                    root: Root::open(&selected.root).map_err(error)?,
                    user_config: selected
                        .user_config
                        .as_ref()
                        .map(|p| Root::open(p))
                        .transpose()
                        .map_err(error)?,
                    project,
                    project_paths,
                    check,
                }
            }
        };
        self.launch.registry().check_caller(caller)?;
        self.registry.register(grant)
    }
    pub(crate) fn read(
        &self,
        caller: &CallerIdentity,
        request: &ReadRequest,
    ) -> Result<ProjectionResult, SafeError> {
        self.launch.registry().check_caller(caller)?;
        self.registry.read(&owner(caller), request)
    }
    pub(crate) fn check_caller(&self, caller: &CallerIdentity) -> Result<(), SafeError> {
        self.launch.registry().check_caller(caller)
    }
}
fn owner(caller: &CallerIdentity) -> Owner {
    Owner {
        instance: caller.instance_id.clone(),
        window: caller.window_label.clone(),
        epoch: caller.webview_epoch,
    }
}
fn profile_locations(
    service: &LaunchService,
    profile: &Profile,
) -> Result<(EnvMap, Locations), SafeError> {
    if profile.cli == CliKind::Shell
        || !matches!(profile.launcher, Launcher::Native)
        || matches!(&profile.default_args,Override::Set(args) if !args.is_empty())
    {
        return Err(error("SCOPE_UNKNOWN"));
    }
    let legacy = profile.read_legacy(
        &service
            .repository()
            .metadata_directory()
            .join("config.json"),
    )?;
    if profile.is_legacy_claude() && matches!(profile.default_args, Override::Inherit) {
        if let Some(args) = legacy.as_ref().and_then(|v| v.get("defaultCustomArgs")) {
            if !args.is_null() && args.as_str() != Some("") {
                return Err(error("SCOPE_UNKNOWN"));
            }
        }
    }
    let env = build_environment(
        &service.inherited_environment(),
        &EnvMap::new(),
        profile,
        legacy.as_ref(),
        None,
    )?;
    let selected = locations(profile.cli, &env).map_err(error)?;
    Ok((env, selected))
}
fn admit_project(project: &RegisteredProject) -> Result<(Root, Vec<PathBuf>), SafeError> {
    project.validate()?;
    let root = Root::open(&project.selected_path).map_err(error)?;
    if root.key() != project.source_path_key {
        return Err(error("PROJECT_IDENTITY_CHANGED"));
    }
    let paths = verified_spellings(&project.selected_path, &root)?;
    Ok((root, paths))
}
fn verified_spellings(selected: &Path, root: &Root) -> Result<Vec<PathBuf>, SafeError> {
    let mut paths = vec![selected.to_owned()];
    // Alias proof only: no transcript-provided cwd is ever opened or canonicalized.
    let canonical = std::fs::canonicalize(selected).map_err(|_| error("SOURCE_CHANGED"))?;
    let alias = Root::open(&canonical).map_err(error)?;
    if alias.key() != root.key() {
        return Err(error("SOURCE_CHANGED"));
    }
    root.current().map_err(error)?;
    if canonical != selected {
        paths.push(canonical);
    }
    Ok(paths)
}
