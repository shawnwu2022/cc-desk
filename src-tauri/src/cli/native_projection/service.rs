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
use crate::cli::types::{CliKind, LaunchAction, SafeError};
use crate::cli::workspace::RegisteredProject;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) struct ProjectionService {
    launch: Arc<LaunchService>,
    registry: ScopeRegistry,
    archive_operations: crate::cli::native_archive::Operations,
    rename_operations: crate::cli::native_rename::Operations,
    delete_operations: crate::cli::native_delete::Operations,
}
impl ProjectionService {
    pub(crate) fn preview_delete(
        &self,
        caller: &CallerIdentity,
        request: crate::cli::native_delete::PreviewRequest,
    ) -> Result<crate::cli::native_delete::Preview, SafeError> {
        self.launch.registry().check_caller(caller)?;
        request.validate()?;
        self.registry
            .check_source(&owner(caller), &request.source)?;
        if request.source.cli == CliKind::Shell
            || (request.source.cli == CliKind::Codex && !cfg!(windows))
        {
            return Ok(request.preview("unsupported", Some("NATIVE_DELETE_UNSUPPORTED")));
        }
        if self
            .launch
            .repository()
            .admission()
            .check_metadata_idle()
            .is_err()
        {
            return Ok(request.preview("blocked", Some("SOURCE_BUSY")));
        }
        let authority = match self.registry.authorize_session(
            &owner(caller),
            &request.source,
            &request.session_key,
        ) {
            Ok(authority) => authority,
            Err(_) => return Ok(request.preview("blocked", Some("DELETE_SOURCE_UNAVAILABLE"))),
        };
        if matches!(request.source.cli, CliKind::Claude | CliKind::Opencode) {
            authority.current()?;
            // 身份观察不等于维护准入；不签发票据，也不把 Desk 归档当原生归档证明。
            let reason = if request.source.cli == CliKind::Claude {
                "DELETE_MAINTENANCE_UNAVAILABLE"
            } else {
                "DELETE_SERVICE_UNVERIFIED"
            };
            let mut preview = request.preview("unsupported", Some(reason));
            preview.native_session_id = Some(authority.native_id);
            return Ok(preview);
        }
        if !authority.archived {
            return Ok(request.preview("blocked", Some("DELETE_ROOT_NOT_ARCHIVED")));
        }
        let authority = match authority.pin_delete_containers() {
            Ok(authority) => authority,
            Err(_) => return Ok(request.preview("blocked", Some("DELETE_CONTAINER_UNSAFE"))),
        };
        self.delete_operations.register(
            caller,
            &request,
            &authority.native_id,
            authority.observation.clone(),
        )
    }
    fn delete_root_present(
        &self,
        caller: &CallerIdentity,
        request: &crate::cli::native_delete::PreviewRequest,
    ) -> Option<bool> {
        use super::wire::{ProjectionState, ResourceItem, ResourceKind};
        static NEXT_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let epoch = crate::cli::types::WireU64::parse(
            &NEXT_EPOCH
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .to_string(),
        )
        .ok()?;
        for offset in (0..16400).step_by(200) {
            let result = self
                .registry
                .read(
                    &owner(caller),
                    &ReadRequest {
                        source: request.source.clone(),
                        resource_kind: ResourceKind::History,
                        request_epoch: epoch,
                        query: None,
                        session_id: None,
                        offset,
                        limit: 200,
                    },
                )
                .ok()?;
            if result.state != ProjectionState::Ready
                || result.history_metadata_incomplete == Some(true)
                || !result.history_read_failures.is_empty()
            {
                return None;
            }
            if result.items.iter().any(|item| matches!(item,ResourceItem::Session {session_key,..} if session_key==&request.session_key)) {return Some(true);}
            if !result.has_more {
                return Some(false);
            }
        }
        None
    }
    pub(crate) fn confirm_delete(
        &self,
        caller: &CallerIdentity,
        request: crate::cli::native_delete::ConfirmRequest,
    ) -> Result<crate::cli::native_delete::Receipt, SafeError> {
        self.launch.registry().check_caller(caller)?;
        request.validate()?;
        let entry = self.delete_operations.get(caller, &request.operation_id)?;
        self.registry
            .check_source(&owner(caller), &entry.request.source)?;
        if entry.confirmed.is_some() {
            return self
                .delete_operations
                .begin(caller, &request, &entry.observation)?
                .ok_or_else(|| error("DELETE_OUTCOME_UNKNOWN"));
        }
        let authority = match self.registry.authorize_session(
            &owner(caller),
            &entry.request.source,
            &entry.request.session_key,
        ) {
            Ok(authority) if authority.archived => authority,
            _ => {
                return self
                    .delete_operations
                    .invalidate(caller, &request.operation_id);
            }
        };
        let authority = match authority.pin_delete_containers() {
            Ok(authority) => authority,
            Err(_) => {
                return self
                    .delete_operations
                    .invalidate(caller, &request.operation_id)
            }
        };
        if let Some(receipt) =
            self.delete_operations
                .begin(caller, &request, &authority.observation)?
        {
            return Ok(receipt);
        }
        let launch = crate::cli::types::LaunchRequest {
            request_id: format!("delete-{}", request.operation_id),
            tab_id: "metadata".into(),
            run_id: "metadata".into(),
            generation: 1,
            profile_id: entry.request.source.profile_id.clone(),
            expected_profile_revision: entry.request.source.profile_revision,
            cli: CliKind::Codex,
            launch_cwd: authority.cwd.clone(),
            action: LaunchAction::Raw {
                argv: vec!["app-server".into()],
            },
            extra_args: vec![],
            cols: 80,
            rows: 24,
        };
        let result = self.launch.delete_metadata(
            caller,
            &launch,
            authority,
            entry.active.clone(),
            entry.outcome.clone(),
        );
        let root_present = self.delete_root_present(caller, &entry.request);
        let receipt =
            match result {
                Ok(()) if root_present == Some(true) => {
                    entry
                        .request
                        .receipt("partial", root_present, Some("DELETE_ROOT_PRESENT"))
                }
                Ok(()) => entry.request.receipt("acknowledged", root_present, None),
                Err(failure)
                    if crate::cli::native_delete::pre_dispatch_reason(&failure.code).is_some() =>
                {
                    entry.request.receipt(
                        "blocked",
                        root_present,
                        crate::cli::native_delete::pre_dispatch_reason(&failure.code),
                    )
                }
                Err(failure) if failure.code == "SOURCE_BUSY" => {
                    entry
                        .request
                        .receipt("blocked", root_present, Some("SOURCE_BUSY"))
                }
                Err(failure) if failure.code == "DELETE_ARGUMENTS_UNSUPPORTED" => {
                    entry.request.receipt(
                        "blocked",
                        root_present,
                        Some("DELETE_ARGUMENTS_UNSUPPORTED"),
                    )
                }
                Err(failure) if failure.code == "DELETE_ROOT_NOT_ARCHIVED" => entry
                    .request
                    .receipt("blocked", root_present, Some("DELETE_ROOT_NOT_ARCHIVED")),
                Err(failure) if failure.code == "DELETE_SOURCE_UNAVAILABLE" => entry
                    .request
                    .receipt("blocked", root_present, Some("DELETE_SOURCE_UNAVAILABLE")),
                Err(failure) if failure.code == "DELETE_OWNER_PENDING" => {
                    entry
                        .request
                        .receipt("unknown", None, Some("DELETE_OWNER_PENDING"))
                }
                Err(_) => {
                    entry
                        .request
                        .receipt("unknown", root_present, Some("DELETE_OUTCOME_UNKNOWN"))
                }
            };
        self.delete_operations
            .finish(caller, &request.operation_id, receipt.clone());
        self.launch.registry().check_caller(caller)?;
        Ok(receipt)
    }
    pub(crate) fn inspect_delete(
        &self,
        caller: &CallerIdentity,
        request: crate::cli::native_delete::InspectRequest,
    ) -> Result<crate::cli::native_delete::Receipt, SafeError> {
        self.launch.registry().check_caller(caller)?;
        uuid::Uuid::parse_str(&request.operation_id)
            .map_err(|_| SafeError::invalid("operationId"))?;
        let entry = self.delete_operations.get(caller, &request.operation_id)?;
        self.registry
            .check_source(&owner(caller), &entry.request.source)?;
        if entry.confirmed.is_none() {
            if let Some(receipt) = entry.receipt.filter(|receipt| receipt.state == "blocked") {
                return Ok(receipt);
            }
            return Err(error("DELETE_CONFIRMATION_REQUIRED"));
        }
        if entry.active.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(entry
                .request
                .receipt("unknown", None, Some("DELETE_OWNER_PENDING")));
        }
        let root = self.delete_root_present(caller, &entry.request);
        let receipt = crate::cli::native_delete::reconcile(&entry, root);
        self.delete_operations
            .finish(caller, &request.operation_id, receipt.clone());
        self.launch.registry().check_caller(caller)?;
        Ok(receipt)
    }
    pub(crate) fn reconcile_before_unarchive(
        &self,
        caller: &CallerIdentity,
        request: &crate::cli::types::UnarchiveRequest,
    ) -> Result<(), SafeError> {
        self.launch.registry().check_caller(caller)?;
        request.validate()?;
        if !self.archive_operations.has_unknown(None)? {
            return Ok(());
        }
        let document = self.launch.repository().read()?;
        let profile = document
            .profiles
            .get(&request.profile_id)
            .ok_or_else(|| error("PROFILE_NOT_FOUND"))?;
        if profile.cli != CliKind::Codex || profile.revision != request.expected_profile_revision {
            return Err(error("REVISION_CONFLICT"));
        }
        let (_, selected) = profile_locations(&self.launch, profile)?;
        let root = selected.open_root(CliKind::Codex).map_err(error)?;
        let physical = serde_json::to_string(&(root.key(), &request.native_session_id))
            .map_err(|_| error("SOURCE_INVALID"))?;
        if !self.archive_operations.has_unknown(Some(&physical))? {
            return Ok(());
        }
        let path = Path::new(&request.project_path);
        let project = document
            .registered_projects
            .values()
            .find(|project| {
                project.selected_path == path || project.canonical_path.as_deref() == Some(path)
            })
            .ok_or_else(|| error("PROJECT_NOT_FOUND"))?;
        let target = ScopeTarget::Profile {
            profile_id: request.profile_id.clone(),
            expected_profile_revision: request.expected_profile_revision,
            project_ids: vec![project.project_id.clone()],
        };
        let (frozen, admission) = self.launch.repository().admission().begin_codex_archive()?;
        let result = (|| {
            let source =
                self.scope_inner(caller, &target, &mut ProjectionStage::ScopeCapability)?;
            let key = serde_json::json!([
                "local",
                "codex",
                source.source_root_key,
                request.native_session_id
            ])
            .to_string();
            let authority = self
                .registry
                .authorize_rename(&owner(caller), &source, &key)?;
            if authority.root_key() != root.key() || !authority.archived {
                return Err(error("ARCHIVE_OUTCOME_UNKNOWN"));
            }
            self.archive_operations.reconcile_archived(&physical)
        })();
        drop(admission);
        let released = frozen.release_review();
        result?;
        released
    }
    pub(crate) fn archive(
        &self,
        caller: &CallerIdentity,
        request: crate::cli::native_archive::ArchiveRequest,
    ) -> Result<crate::cli::native_archive::ArchiveReceipt, SafeError> {
        self.launch.registry().check_caller(caller)?;
        request.validate()?;
        if !cfg!(windows) {
            return Err(error("NATIVE_ARCHIVE_UNSUPPORTED"));
        }
        self.launch
            .repository()
            .admission()
            .check_codex_archive_idle()
            .map_err(|_| error("ARCHIVE_BUSY"))?;
        let authority = self.registry.authorize_rename(
            &owner(caller),
            &request.source,
            &request.session_key,
        )?;
        let physical = serde_json::to_string(&(authority.root_key(), &authority.native_id))
            .map_err(|_| error("SOURCE_INVALID"))?;
        self.archive_operations
            .release_revoked(|owner| self.launch.registry().check_caller(owner).is_ok())?;
        self.archive_operations
            .validate_request(caller, &request, &physical)?;
        if authority.archived {
            // 空闲与新来源共同证明根已归档，不能把尚在处理中的移动当完成。
            self.launch
                .repository()
                .admission()
                .check_codex_archive_idle()
                .map_err(|_| error("ARCHIVE_BUSY"))?;
            self.archive_operations.reconcile_archived(&physical)?;
            if let Some(result) = self.archive_operations.begin(caller, &request, &physical)? {
                result?;
            } else {
                self.archive_operations.finish(caller, &request, Ok(()));
            }
            return Ok(request.receipt());
        }
        if let Some(result) = self.archive_operations.begin(caller, &request, &physical)? {
            result?;
            return Ok(request.receipt());
        }
        let result = (|| {
            let authority = authority.pin_archive_containers()?;
            let launch = crate::cli::types::LaunchRequest {
                request_id: format!("archive-{}", request.operation_id),
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
            self.launch.archive_metadata(caller, &launch, authority)
        })();
        self.archive_operations
            .finish(caller, &request, result.clone());
        result?;
        self.launch.registry().check_caller(caller)?;
        Ok(request.receipt())
    }
    pub(crate) fn rename(
        &self,
        caller: &CallerIdentity,
        request: crate::cli::native_rename::RenameRequest,
    ) -> Result<crate::cli::native_rename::RenameReceipt, SafeError> {
        self.launch.registry().check_caller(caller)?;
        request.validate()?;
        self.rename_operations
            .release_revoked(|owner| self.launch.registry().check_caller(owner).is_ok())?;
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
            archive_operations: Default::default(),
            rename_operations: Default::default(),
            delete_operations: Default::default(),
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
                let (env, configured) = profile_locations_diagnosed(&self.launch, &profile, stage)?;
                let selected = if profile.cli == CliKind::Opencode
                    && configured.sqlite_file.is_none()
                {
                    let project_id = project_ids.first().ok_or_else(|| error("SCOPE_UNKNOWN"))?;
                    let project = document
                        .registered_projects
                        .get(project_id)
                        .ok_or_else(|| error("PROJECT_NOT_FOUND"))?;
                    let _ = admit_project(project)?;
                    // v2 重命名了 db 路径查询命令；探测失败（或 CLI 再度改名）
                    // 时**显式失败**而非回退默认库：channel 用户（真实库为
                    // opencode-<channel>.db）的默认 opencode.db 若存在必是旧
                    // 镜像，静默展示陈旧投影比可见的「来源不可用」更糟——
                    // 前端将其呈现为 opencode-history 警告，保留归因日志。
                    let output = self
                        .launch
                        .probe_opencode_database(caller, &profile, &project.selected_path, &env)
                        .inspect_err(|failure| {
                            log::warn!("opencode database probe failed: {}", failure.code);
                        })?;
                    configured.with_probed_database(&output).map_err(error)?
                } else {
                    configured.clone()
                };
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
                let expected_locations = configured;
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
                    root: selected.open_root(profile.cli).map_err(error)?,
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
        // 只读历史刷新也能收敛丢 ACK 的归档；恢复前不需要重放任何原生写命令。
        if request.source.cli == CliKind::Codex
            && request.source.basis == SourceBasis::ConfiguredProfile
            && request.resource_kind == super::wire::ResourceKind::History
        {
            self.registry
                .check_source(&owner(caller), &request.source)?;
            let keys = self
                .archive_operations
                .unknown_session_keys(&request.source.source_root_key)?;
            if !keys.is_empty() {
                // 普通历史读取不能因某次未知写阻塞；忙时保留屏障，恢复由独立后端门强制核对。
                let Ok((frozen, admission)) =
                    self.launch.repository().admission().begin_codex_archive()
                else {
                    return self.registry.read(&owner(caller), request);
                };
                let result = (|| {
                    for key in keys {
                        if let Ok(authority) =
                            self.registry
                                .authorize_rename(&owner(caller), &request.source, &key)
                        {
                            if authority.archived {
                                let physical = serde_json::to_string(&(
                                    authority.root_key(),
                                    &authority.native_id,
                                ))
                                .map_err(|_| error("SOURCE_INVALID"))?;
                                self.archive_operations.reconcile_archived(&physical)?;
                            }
                        }
                    }
                    self.launch.registry().check_caller(caller)
                })();
                drop(admission);
                let released = frozen.release_review();
                let _ = (result, released);
            }
        }
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
