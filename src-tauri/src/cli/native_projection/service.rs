//! Binds directory capabilities to D11's live document/run registry and Desk revisions.
use super::diagnostics::{ProjectionFailure, ProjectionStage};
use super::registry::{AdmittedProject, Grant, Owner, ScopeRegistry};
use super::scoped_fs::{ReadResult, Root};
use super::selection::{locations, read_scope_known, Locations};
use super::wire::{ProjectionResult, ReadRequest, ScopeTarget, SourceBasis, SourceRef};
use crate::cli::environment::{build_environment, EnvMap};
use crate::cli::launch_service::LaunchService;
use crate::cli::profiles::{error, Override, Profile};
use crate::cli::run_registry::RunKey;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{LaunchAction, SafeError};
use crate::cli::workspace::RegisteredProject;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) struct ProjectionService {
    launch: Arc<LaunchService>,
    registry: ScopeRegistry,
    rename_operations: crate::cli::native_rename::Operations,
}
impl ProjectionService {
    pub(crate) fn rename(
        &self,
        caller: &CallerIdentity,
        request: crate::cli::native_rename::RenameRequest,
    ) -> Result<crate::cli::native_rename::RenameReceipt, SafeError> {
        self.launch.registry().check_caller(caller)?;
        request.validate()?;
        if let Some(result) = self.rename_operations.begin(caller, &request)? {
            result?;
            return Ok(request.receipt());
        }
        let result = (|| {
            let authority = self.registry.authorize_rename(
                &owner(caller),
                &request.source,
                &request.session_key,
            )?;
            let launch = crate::cli::types::LaunchRequest {
                request_id: format!("rename-{}", request.operation_id),
                tab_id: "metadata".into(),
                run_id: "metadata".into(),
                generation: 1,
                profile_id: request.source.profile_id.clone(),
                expected_profile_revision: request.source.profile_revision,
                cli: request.source.cli,
                launch_cwd: authority.cwd.clone(),
                action: LaunchAction::Raw {
                    argv: vec!["app-server".into()],
                },
                extra_args: vec![],
                cols: 80,
                rows: 24,
            };
            self.launch
                .rename_metadata(caller, &launch, authority, request.title.clone())
        })();
        self.rename_operations
            .finish(caller, &request, result.clone());
        result?;
        self.launch.registry().check_caller(caller)?;
        Ok(request.receipt())
    }
    pub(crate) fn new(launch: Arc<LaunchService>) -> Self {
        Self {
            launch,
            registry: ScopeRegistry::new(64),
            rename_operations: Default::default(),
        }
    }
    #[cfg(test)]
    pub(crate) fn scope(
        &self,
        caller: &CallerIdentity,
        target: &ScopeTarget,
    ) -> Result<SourceRef, SafeError> {
        self.scope_diagnosed(caller, target)
            .map_err(|failure| failure.cause)
    }
    pub(crate) fn scope_diagnosed(
        &self,
        caller: &CallerIdentity,
        target: &ScopeTarget,
    ) -> Result<SourceRef, ProjectionFailure> {
        let mut stage = ProjectionStage::ScopeCapability;
        self.scope_inner(caller, target, &mut stage)
            .map_err(|error| stage.failure(error))
    }
    fn scope_inner(
        &self,
        caller: &CallerIdentity,
        target: &ScopeTarget,
        stage: &mut ProjectionStage,
    ) -> Result<SourceRef, SafeError> {
        self.launch.registry().check_caller(caller)?;
        *stage = ProjectionStage::ScopeRequestValidation;
        target.validate()?;
        let grant = match target {
            ScopeTarget::Profile {
                profile_id,
                expected_profile_revision,
                project_ids,
            } => {
                *stage = ProjectionStage::ScopeProfileValidation;
                let document = self.launch.repository().read()?;
                let profile = document
                    .profiles
                    .get(profile_id)
                    .cloned()
                    .ok_or_else(|| error("PROFILE_NOT_FOUND"))?;
                if profile.revision != *expected_profile_revision {
                    return Err(error("REVISION_CONFLICT"));
                }
                let (env, selected) = profile_locations_diagnosed(&self.launch, &profile, stage)?;
                drop(env); // Never retain an ambient environment snapshot in an IPC DTO.
                *stage = ProjectionStage::ScopeProjectRegistration;
                // 一次观察可覆盖多个已注册项目；每个项目仍逐个 admission，集合不是授权捷径。
                let mut projects = Vec::with_capacity(project_ids.len());
                let mut expected_projects = Vec::with_capacity(project_ids.len());
                for project_id in project_ids {
                    let registered = document
                        .registered_projects
                        .get(project_id)
                        .cloned()
                        .ok_or_else(|| error("PROJECT_NOT_FOUND"))?;
                    let (root, paths) = admit_project(&registered)?;
                    projects.push(AdmittedProject {
                        id: project_id.clone(),
                        root,
                        paths,
                    });
                    expected_projects.push(registered);
                }
                let service = Arc::downgrade(&self.launch);
                let expected_profile = profile.clone();
                let expected_locations = selected.clone();
                let authority = caller.clone();
                let check = Arc::new(move || -> ReadResult<()> {
                    let service = service.upgrade().ok_or("SCOPE_REVOKED")?;
                    service
                        .registry()
                        .check_caller(&authority)
                        .map_err(|_| "FORBIDDEN")?;
                    // 同次检查只读一份新快照，避免项目数乘以文件操作数的重复解析。
                    let document = service.repository().read().map_err(|_| "SCOPE_REVOKED")?;
                    let current = document
                        .profiles
                        .get(&expected_profile.id)
                        .ok_or("SCOPE_REVOKED")?;
                    if current != &expected_profile {
                        return Err("SCOPE_REVOKED");
                    }
                    let (_, now) =
                        profile_locations(&service, current).map_err(|_| "SCOPE_REVOKED")?;
                    if now != expected_locations {
                        return Err("SCOPE_REVOKED");
                    }
                    for expected in &expected_projects {
                        let current = document
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
                *stage = ProjectionStage::ScopeSourceRoot;
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
                    projects,
                    check,
                }
            }
            ScopeTarget::Run { run_id, generation } => {
                *stage = ProjectionStage::ScopeCapability;
                let run = RunKey {
                    run_id: run_id.clone(),
                    generation: *generation,
                };
                let snapshot = self.launch.registry().source_provenance(caller, &run)?;
                *stage = ProjectionStage::ScopeSourceSelection;
                let selected = snapshot.selection()?;
                *stage = ProjectionStage::ScopeSourceRoot;
                // Only a new launch with proven root-neutral args authorizes its frozen requested cwd.
                // Resume/picker can choose another cwd; they expose root-wide observations only.
                let (project, project_paths) = if let Some(capability) = selected.project.as_ref() {
                    let path = capability.selected_path();
                    let root = capability.clone_source().map_err(error)?;
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
                        .registry()
                        .source_provenance(&authority, &run)
                        .map_err(|_| "SCOPE_REVOKED")?;
                    if !Arc::ptr_eq(&expected, &current) {
                        return Err("SCOPE_REVOKED");
                    }
                    Ok(())
                });
                Grant {
                    owner: owner(caller),
                    cli: snapshot.cli,
                    profile_id: snapshot.profile_id.clone(),
                    profile_revision: snapshot.profile_revision,
                    target: target.clone(),
                    basis: SourceBasis::LaunchEnvironment,
                    root: selected.root.clone_source().map_err(error)?,
                    user_config: selected
                        .user_config
                        .as_ref()
                        .map(Root::clone_source)
                        .transpose()
                        .map_err(error)?,
                    projects: project
                        .map(|root| {
                            // Run 观察的项目不是注册项目；冻结的 launch cwd 就是它的身份，
                            // 只用于前端行归属，不引入新的授权面。
                            vec![AdmittedProject {
                                id: selected
                                    .project
                                    .as_ref()
                                    .and_then(|root| root.selected_path().to_str())
                                    .expect("frozen UTF-8 launch cwd")
                                    .to_owned(),
                                root,
                                paths: project_paths,
                            }]
                        })
                        .unwrap_or_default(),
                    check,
                }
            }
        };
        *stage = ProjectionStage::ScopeCapability;
        self.launch.registry().check_caller(caller)?;
        self.registry.register(grant)
    }
    pub(crate) fn read_diagnosed(
        &self,
        caller: &CallerIdentity,
        request: &ReadRequest,
    ) -> Result<ProjectionResult, ProjectionFailure> {
        request
            .validate()
            .map_err(|e| ProjectionStage::ReadRequestValidation.failure(e))?;
        self.read(caller, request).map_err(|e| {
            let stage = if e.code.starts_with("SCOPE_") || e.code == "FORBIDDEN" {
                ProjectionStage::ReadCapability
            } else {
                ProjectionStage::ReadSourceEnumeration
            };
            stage.failure(e)
        })
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
    profile_locations_diagnosed(
        service,
        profile,
        &mut ProjectionStage::ScopeProfileValidation,
    )
}
fn profile_locations_diagnosed(
    service: &LaunchService,
    profile: &Profile,
    stage: &mut ProjectionStage,
) -> Result<(EnvMap, Locations), SafeError> {
    *stage = ProjectionStage::ScopeProfileValidation;
    let args = match &profile.default_args {
        Override::Set(args) => args.as_slice(),
        Override::Inherit | Override::Unset => &[],
    };
    if !read_scope_known(profile.cli, &profile.launcher, args) {
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
    *stage = ProjectionStage::ScopeEnvironment;
    let env = build_environment(
        &service.inherited_environment()?,
        &EnvMap::new(),
        profile,
        legacy.as_ref(),
        None,
    )?;
    *stage = ProjectionStage::ScopeSourceSelection;
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
