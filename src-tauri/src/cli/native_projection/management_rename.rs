//! Desktop-only dispatch for the separately admitted management path.
#![allow(dead_code)] // Unreachable externally until the independent management transport is wired.
use super::{admit_project, owner, ProjectionService};
use crate::cli::{
    native_projection::{catalogue_history_source::RetainedHistory, wire::SourceRef},
    native_rename::{RenameFence, RenameRequest},
    profiles::error,
    run_registry::{CatalogueNativeId, LaunchStatus},
    snapshot::CallerIdentity,
    types::{CliKind, SafeError, WireU64},
};
use cc_desk_remote_security::{
    session_catalogue::{ArchiveEvidence, OpaqueKey},
    session_management::{CurrentManagementScope, ManagementAction, ManagementGrant},
    session_management_channel::ManagementRow,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

type PairFact = (OpaqueKey, OpaqueKey);
/// Only the independent management owner may supply this verifier. It returns
/// the pinned endpoint identity and digest of the currently admitted receipt.
pub(crate) type CurrentManagementPeer = Arc<dyn Fn() -> Result<PairFact, SafeError> + Send + Sync>;

struct RenameTarget {
    source: SourceRef,
    session_key: String,
    operation_id: String,
    project: String,
    native: CatalogueNativeId,
    project_root: String,
    run: LaunchStatus,
}

pub(crate) struct RemoteRenameGrant {
    grant: ManagementGrant<RenameTarget>,
    selector: OpaqueKey,
}

impl RemoteRenameGrant {
    pub(crate) fn token(&self) -> OpaqueKey {
        self.grant.token()
    }
}

fn denied() -> SafeError {
    error("REMOTE_MANAGEMENT_TARGET_UNAVAILABLE")
}

fn binding(domain: &[u8], fields: &[&[u8]]) -> OpaqueKey {
    let mut hash = Sha256::new();
    hash.update(domain);
    for bytes in fields {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    hash.finalize().into()
}

impl ProjectionService {
    pub(crate) fn management_inventory(
        &self,
        caller: &CallerIdentity,
        retained: &RetainedHistory,
        epoch: WireU64,
        check_peer: &CurrentManagementPeer,
    ) -> Result<Vec<(ManagementRow, OpaqueKey)>, SafeError> {
        use crate::cli::run_registry::LaunchPhase;
        use cc_desk_remote_security::session_catalogue::DisplayLifecycle;
        let peer = check_peer()?;
        let check = || {
            if check_peer()? == peer {
                Ok(())
            } else {
                Err(denied())
            }
        };
        let mut unambiguous = std::collections::BTreeMap::new();
        for row in self.read_remote_scoped_history(caller, retained, epoch, check)? {
            unambiguous
                .entry(row.display.physical)
                .and_modify(|slot| *slot = None)
                .or_insert(Some(row));
        }
        let source = retained.source();
        let document = self.launch.repository().read()?;
        let mut projects = std::collections::BTreeMap::new();
        let mut result = Vec::new();
        for (physical, entry) in unambiguous
            .into_iter()
            .filter_map(|(key, row)| row.map(|v| (key, v)))
        {
            check()?;
            let title = entry.display.title;
            if entry.display.archive == ArchiveEvidence::Unknown
                || title.len() > 256
                || title.trim().is_empty()
                || title.chars().any(char::is_control)
            {
                continue;
            }
            if !projects.contains_key(&entry.project) {
                let project = document
                    .registered_projects
                    .get(&entry.project)
                    .ok_or_else(denied)?;
                if project.host_id != "local" {
                    return Err(denied());
                }
                projects.insert(entry.project.clone(), admit_project(project)?.0);
            }
            let root = &projects[&entry.project];
            if entry
                .observed_project_root
                .as_ref()
                .is_some_and(|key| key != root.key())
            {
                continue;
            }
            let native = native_identity(source, &entry.native_id)?;
            let run = match self.launch.registry().resolve_original_management_run(
                caller,
                &source.profile_id,
                source.profile_revision,
                &source.source_root_key,
                root.key(),
                &native,
            ) {
                Ok(run) => run,
                Err(failure) if failure.code == "REMOTE_MANAGEMENT_TARGET_UNAVAILABLE" => continue,
                Err(failure) => return Err(failure),
            };
            let lifecycle = match run.phase {
                LaunchPhase::Running if entry.display.archive == ArchiveEvidence::NotArchived => {
                    DisplayLifecycle::Open
                }
                LaunchPhase::Exited => DisplayLifecycle::Closed,
                _ => continue,
            };
            let mut reference = [0; 32];
            reference[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
            reference[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
            result.push((
                ManagementRow {
                    reference,
                    title,
                    lifecycle,
                    archived: entry.display.archive == ArchiveEvidence::Archived,
                },
                physical,
            ));
            if result.len() > 512 {
                return Err(denied());
            }
        }
        check()?;
        Ok(result)
    }

    /// The selector is resolved by the desktop's management-only reference
    /// table, not parsed from a phone-provided provider ID.
    pub(crate) fn prepare_remote_rename(
        &self,
        caller: &CallerIdentity,
        retained: &RetainedHistory,
        epoch: WireU64,
        selector: OpaqueKey,
        action: ManagementAction,
        check_peer: &CurrentManagementPeer,
    ) -> Result<RemoteRenameGrant, SafeError> {
        let (scope, target) =
            self.resolve_remote_rename(caller, retained, epoch, selector, &action, check_peer)?;
        let grant = ManagementGrant::issue(scope, action, target).map_err(|_| denied())?;
        Ok(RemoteRenameGrant { grant, selector })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_remote_rename(
        self: &Arc<Self>,
        caller: &CallerIdentity,
        retained: &RetainedHistory,
        epoch: WireU64,
        mut prepared: RemoteRenameGrant,
        token: OpaqueKey,
        action: &ManagementAction,
        check_peer: CurrentManagementPeer,
        claude_archive_changed: impl FnOnce(),
        close: impl FnOnce(&LaunchStatus, RenameFence) -> Result<(), SafeError>,
        reconnect: impl FnOnce(&LaunchStatus, RenameFence) -> Result<(), SafeError>,
    ) -> Result<(), SafeError> {
        // Re-observe the same admitted source before consuming the capability.
        let (scope, fresh) = self.resolve_remote_rename(
            caller,
            retained,
            epoch,
            prepared.selector,
            action,
            &check_peer,
        )?;
        let target = prepared
            .grant
            .redeem(Some(&scope), &token, action)
            .map_err(|_| denied())?;
        if target.run != fresh.run
            || target.project_root != fresh.project_root
            || target.native != fresh.native
            || target.source != fresh.source
            || target.session_key != fresh.session_key
        {
            return Err(denied());
        }

        let service = self.clone();
        let authority = caller.clone();
        let exact_source = target.source.clone();
        let exact_peer = (scope.peer, scope.membership);
        let fence: RenameFence = Arc::new(move || {
            if check_peer()? != exact_peer {
                return Err(denied());
            }
            service
                .registry
                .check_source(&owner(&authority), &exact_source)?;
            let current = service.launch.registry().resolve_original_management_run(
                &authority,
                &exact_source.profile_id,
                exact_source.profile_revision,
                &exact_source.source_root_key,
                &target.project_root,
                &target.native,
            )?;
            if current != target.run {
                return Err(denied());
            }
            Ok(())
        });
        match action {
            ManagementAction::Close => close(&fresh.run, fence)?,
            ManagementAction::Reconnect => reconnect(&fresh.run, fence)?,
            ManagementAction::Rename { title } => {
                self.rename_observed_guarded(
                    caller,
                    RenameRequest {
                        operation_id: target.operation_id,
                        source: target.source,
                        session_key: target.session_key,
                        title: title.clone(),
                    },
                    None,
                    Some(fence),
                )?;
            }
            ManagementAction::Archive | ManagementAction::Restore
                if target.source.cli == CliKind::Opencode =>
            {
                self.set_opencode_archive_guarded(
                    caller,
                    crate::cli::opencode_archive::Request {
                        operation_id: target.operation_id,
                        source: target.source,
                        session_key: target.session_key,
                        archived: matches!(action, ManagementAction::Archive),
                        acknowledge_recoverable: true,
                    },
                    Some(fence),
                )?;
            }
            ManagementAction::Archive | ManagementAction::Restore
                if target.source.cli == CliKind::Claude =>
            {
                self.change_claude_archive(
                    caller,
                    &target.source,
                    &target.session_key,
                    &target.project,
                    &fresh.project_root,
                    matches!(action, ManagementAction::Archive),
                    fence,
                    claude_archive_changed,
                )?;
            }
            ManagementAction::Restore if target.source.cli == CliKind::Codex => {
                self.restore_remote_codex(
                    caller,
                    &target.source,
                    &target.session_key,
                    target.operation_id,
                    fence,
                )?;
            }
            ManagementAction::Archive if target.source.cli == CliKind::Codex => {
                self.archive_observed_guarded(
                    caller,
                    crate::cli::native_archive::ArchiveRequest {
                        operation_id: target.operation_id,
                        source: target.source,
                        session_key: target.session_key,
                    },
                    None,
                    Some(fence),
                )?;
            }
            _ => return Err(denied()),
        }
        Ok(())
    }

    fn restore_remote_codex(
        &self,
        caller: &CallerIdentity,
        source: &SourceRef,
        session: &str,
        operation_id: String,
        fence: RenameFence,
    ) -> Result<(), SafeError> {
        fence()?;
        let authority = self
            .registry
            .authorize_rename(&owner(caller), source, session)?;
        if !authority.archived {
            return Err(denied());
        }
        let request = crate::cli::types::UnarchiveRequest {
            request_id: operation_id,
            cli: CliKind::Codex,
            profile_id: source.profile_id.clone(),
            expected_profile_revision: source.profile_revision,
            project_path: authority.cwd.clone(),
            native_session_id: authority.native_id.clone(),
        };
        request.validate()?;
        self.reconcile_before_unarchive(caller, &request)?;
        // Keep the same canonical provider root and current authorization up to
        // the existing owned command's creation point. No path came from the phone.
        self.launch.run_command_guarded(
            caller,
            &request.to_launch_request(),
            None,
            Some((source.source_root_key.clone(), fence.clone())),
        )?;
        fence().map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?;
        let after = self
            .registry
            .authorize_rename(&owner(caller), source, session)
            .map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?;
        if after.archived || after.native_id != request.native_session_id {
            return Err(error("ARCHIVE_OUTCOME_UNKNOWN"));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn change_claude_archive(
        &self,
        caller: &CallerIdentity,
        source: &SourceRef,
        session: &str,
        project_id: &str,
        expected_root: &str,
        archived: bool,
        fence: RenameFence,
        changed: impl FnOnce(),
    ) -> Result<(), SafeError> {
        let authority = self
            .registry
            .authorize_rename(&owner(caller), source, session)?;
        authority.verify_claude(None)?;
        let id = uuid::Uuid::parse_str(&authority.native_id).map_err(|_| denied())?;
        let document = self.launch.repository().read()?;
        let project = document
            .registered_projects
            .get(project_id)
            .ok_or_else(denied)?;
        let (root, paths) = admit_project(project)?;
        if project.host_id != "local" || root.key() != expected_root {
            return Err(denied());
        }
        let aliases = paths
            .iter()
            .map(|path| {
                path.to_str()
                    .map(crate::store::normalize_path_str)
                    .ok_or_else(denied)
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        let (data, lock) = crate::store::data_and_lock_paths().map_err(|_| denied())?;
        fence()?;
        crate::store::with_projects_state_locked(&data, &lock, |state| {
            fence().map_err(|_| anyhow::anyhow!("Management revoked"))?;
            authority
                .verify_claude(None)
                .map_err(|_| anyhow::anyhow!("Session changed"))?;
            modify_claude_archive(
                state, source, session, project_id, &root, &aliases, id, archived,
            )?;
            fence().map_err(|_| anyhow::anyhow!("Management revoked"))
        })
        .map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?;
        // 磁盘已提交，即使远程回执随后失效也要同步本机视图。
        changed();
        fence().map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))
    }

    fn resolve_remote_rename(
        &self,
        caller: &CallerIdentity,
        retained: &RetainedHistory,
        epoch: WireU64,
        physical: OpaqueKey,
        action: &ManagementAction,
        check_peer: &CurrentManagementPeer,
    ) -> Result<(CurrentManagementScope, RenameTarget), SafeError> {
        if !matches!(
            action,
            ManagementAction::Rename { .. }
                | ManagementAction::Archive
                | ManagementAction::Restore
                | ManagementAction::Close
                | ManagementAction::Reconnect
        ) {
            return Err(denied());
        }
        // Use the same wire validation even for host calls from typed APIs.
        let wire = serde_json::to_vec(action).map_err(|_| denied())?;
        ManagementAction::decode(&wire).map_err(|_| denied())?;
        let pair = check_peer()?;
        let check = || {
            if check_peer()? == pair {
                Ok(())
            } else {
                Err(denied())
            }
        };
        let mut matches = self
            .read_remote_scoped_history(caller, retained, epoch, check)?
            .into_iter()
            .filter(|entry| entry.display.physical == physical);
        let entry = matches.next().ok_or_else(denied)?;
        if matches.next().is_some()
            || !supports_metadata_action(retained.source().cli, entry.display.archive, action)
        {
            return Err(denied());
        }
        let source = retained.source();
        let repository = self.launch.repository().read()?;
        let project = repository
            .registered_projects
            .get(&entry.project)
            .ok_or_else(denied)?;
        if project.host_id != "local" {
            return Err(denied());
        }
        let (root, _) = admit_project(project)?;
        if entry
            .observed_project_root
            .as_ref()
            .is_some_and(|key| key != root.key())
        {
            return Err(denied());
        }
        let native = native_identity(source, &entry.native_id)?;
        let run = self.launch.registry().resolve_original_management_run(
            caller,
            &source.profile_id,
            source.profile_revision,
            &source.source_root_key,
            root.key(),
            &native,
        )?;
        root.current().map_err(error)?;
        check()?;
        use crate::cli::run_registry::LaunchPhase;
        match action {
            ManagementAction::Close | ManagementAction::Reconnect
                if run.phase != LaunchPhase::Running =>
            {
                return Err(denied())
            }
            ManagementAction::Archive | ManagementAction::Restore
                if run.phase != LaunchPhase::Exited =>
            {
                return Err(denied())
            }
            _ => {}
        }
        let operation_id = uuid::Uuid::new_v4().to_string();
        let run_fact = serde_json::to_vec(&run).map_err(|_| denied())?;
        let document_epoch = caller.webview_epoch.get().to_be_bytes();
        let scope = CurrentManagementScope {
            peer: pair.0,
            membership: pair.1,
            document: binding(
                b"cc-desk.management.document.v1",
                &[
                    caller.instance_id.as_bytes(),
                    caller.window_label.as_bytes(),
                    &document_epoch,
                ],
            ),
            source: retained.key(),
            project: binding(
                b"cc-desk.management.project.v1",
                &[entry.project.as_bytes(), root.key().as_bytes()],
            ),
            session: binding(
                b"cc-desk.management.session.v1",
                &[&physical, entry.session_key.as_bytes()],
            ),
            profile_revision: source.profile_revision.get(),
            run: Some(binding(b"cc-desk.management.run.v1", &[&run_fact])),
        };
        Ok((
            scope,
            RenameTarget {
                source: source.clone(),
                session_key: entry.session_key,
                operation_id,
                project: entry.project,
                native,
                project_root: root.key().to_owned(),
                run,
            },
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn modify_claude_archive(
    state: &mut crate::store::ProjectsState,
    source: &SourceRef,
    session: &str,
    project: &str,
    root: &super::super::scoped_fs::Root,
    aliases: &std::collections::BTreeSet<String>,
    id: uuid::Uuid,
    archived: bool,
) -> anyhow::Result<()> {
    use super::super::catalogue_history_source::{ClaudeArchiveProject, ClaudeArchiveSnapshot};
    let projects = std::collections::BTreeMap::from([(
        project.to_owned(),
        ClaudeArchiveProject {
            root: root.key().to_owned(),
            aliases: aliases.clone(),
        },
    )]);
    let before = ClaudeArchiveSnapshot::new(projects, state.archived_sessions.clone())
        .map_err(|_| anyhow::anyhow!("Ambiguous archive state"))?;
    let observed = before.classify(source, project, session, id).map(|v| v.1);
    if archived {
        anyhow::ensure!(observed == Some(ArchiveEvidence::NotArchived));
        let path = aliases
            .first()
            .ok_or_else(|| anyhow::anyhow!("Missing verified alias"))?;
        let revision = source.profile_revision.to_string();
        let context = serde_json::to_string(&[
            "native-history-v1",
            "claude",
            &source.profile_id,
            &revision,
            project,
            path,
        ])?;
        let id = id.to_string();
        let claim = format!(
            "native-history:{}",
            serde_json::to_string(&["native-history-v2", &context, session, &id,])?
        );
        crate::store::apply_archive_session(state, path, &claim)
    } else {
        anyhow::ensure!(observed == Some(ArchiveEvidence::Archived));
        for (path, claim) in before.exact_claims(id) {
            crate::store::apply_restore_session(state, path, claim)?;
        }
        Ok(())
    }
}

fn supports_metadata_action(
    cli: CliKind,
    state: ArchiveEvidence,
    action: &ManagementAction,
) -> bool {
    match action {
        ManagementAction::Rename { .. } | ManagementAction::Close | ManagementAction::Reconnect => {
            state == ArchiveEvidence::NotArchived
        }
        ManagementAction::Archive => {
            matches!(cli, CliKind::Opencode | CliKind::Codex | CliKind::Claude)
                && state == ArchiveEvidence::NotArchived
        }
        ManagementAction::Restore => {
            matches!(cli, CliKind::Opencode | CliKind::Codex | CliKind::Claude)
                && state == ArchiveEvidence::Archived
        }
    }
}

fn native_identity(source: &SourceRef, id: &str) -> Result<CatalogueNativeId, SafeError> {
    match source.cli {
        CliKind::Codex => uuid::Uuid::parse_str(id)
            .map(CatalogueNativeId::Codex)
            .map_err(|_| denied()),
        CliKind::Claude => uuid::Uuid::parse_str(id)
            .map(CatalogueNativeId::Claude)
            .map_err(|_| denied()),
        CliKind::Opencode => Ok(CatalogueNativeId::Opencode(id.to_owned())),
        _ => Err(denied()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        environment::EnvMap,
        launch_service::LaunchService,
        native_projection::wire::ScopeTarget,
        profiles::{EnvValue, Override, Profile},
        storage::{Patch, WorkspaceRepository},
    };

    #[test]
    fn remote_archive_claude_exact_marker_roundtrip_preserves_other_entries() {
        let temp = tempfile::tempdir().unwrap();
        let root = super::super::super::scoped_fs::Root::open(temp.path()).unwrap();
        let alias = crate::store::normalize_path_str(temp.path().to_str().unwrap());
        let aliases = std::collections::BTreeSet::from([alias.clone()]);
        let data = temp.path().join("projects.json");
        let lock = temp.path().join("projects.lock");
        let revision = WireU64::parse("7").unwrap();
        let source = SourceRef {
            scope_id: "scope".into(),
            instance_id: "fixture".into(),
            cli: CliKind::Claude,
            source_root_key: root.key().to_owned(),
            identity_epoch: revision,
            profile_id: "default".into(),
            profile_revision: revision,
            target: ScopeTarget::Profile {
                profile_id: "default".into(),
                expected_profile_revision: revision,
                project_ids: vec!["project".into()],
            },
            basis: crate::cli::native_projection::wire::SourceBasis::ConfiguredProfile,
        };
        let id = uuid::Uuid::from_u128(42);
        let other = uuid::Uuid::from_u128(43).to_string();
        let session =
            serde_json::to_string(&["local", "claude", root.key(), &id.to_string()]).unwrap();
        let exact = |state: &mut crate::store::ProjectsState, archived| {
            modify_claude_archive(
                state, &source, &session, "project", &root, &aliases, id, archived,
            )
        };
        let written = crate::store::with_projects_state_locked(&data, &lock, |state| {
            crate::store::apply_archive_session(state, &alias, &other)?;
            exact(state, true)
        })
        .unwrap();
        assert_eq!(written.archived_sessions[&alias].len(), 2);
        let restored =
            crate::store::with_projects_state_locked(&data, &lock, |state| exact(state, false))
                .unwrap();
        assert_eq!(restored.archived_sessions[&alias], vec![other.clone()]);
        let bare = id.to_string();
        crate::store::with_projects_state_locked(&data, &lock, |state| {
            crate::store::apply_archive_session(state, &alias, &bare)
        })
        .unwrap();
        assert!(
            crate::store::with_projects_state_locked(&data, &lock, |state| exact(state, false))
                .is_err()
        );
        let kept = crate::store::read_projects_state_locked(&data, &lock).unwrap();
        assert_eq!(kept.archived_sessions[&alias], vec![other, bare]);
    }

    #[test]
    fn remote_archive_state_requires_positive_evidence_and_matching_operation() {
        for cli in [CliKind::Codex, CliKind::Claude, CliKind::Opencode] {
            assert!(supports_metadata_action(
                cli,
                ArchiveEvidence::NotArchived,
                &ManagementAction::Archive
            ));
            assert!(supports_metadata_action(
                cli,
                ArchiveEvidence::Archived,
                &ManagementAction::Restore
            ));
            assert!(!supports_metadata_action(
                cli,
                ArchiveEvidence::Unknown,
                &ManagementAction::Restore
            ));
            assert!(!supports_metadata_action(
                cli,
                ArchiveEvidence::Unknown,
                &ManagementAction::Archive
            ));
            assert!(!supports_metadata_action(
                cli,
                ArchiveEvidence::NotArchived,
                &ManagementAction::Restore
            ));
        }
    }

    #[test]
    fn remote_rename_refuses_visible_history_without_original_owned_run() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("isolated-codex");
        let project = temp.path().join("isolated-project");
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::create_dir(&project).unwrap();
        let id = "00000000-0000-4000-8000-000000004276";
        let path = root.join("sessions").join(format!("{id}.jsonl"));
        let original = format!(
            "{}\n{}\n",
            serde_json::json!({"type":"session_meta","payload":{"id":id,"cwd":project,"source":"cli"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"synthetic fixture"}}),
        );
        std::fs::write(&path, &original).unwrap();

        let repo = WorkspaceRepository::open(temp.path().join("desk/workspace.json")).unwrap();
        let registration = crate::cli::workspace::register_project(&repo, &project).unwrap();
        let mut profile = Profile::new("fixture", CliKind::Codex);
        profile.env.insert(
            "CODEX_HOME".into(),
            Override::Set(EnvValue::Literal {
                value: root.to_str().unwrap().into(),
                non_secret: true,
            }),
        );
        let doc = repo
            .apply(repo.read().unwrap().revision, Patch::Create { profile })
            .unwrap();
        let launch = Arc::new(LaunchService::new(repo, Some(EnvMap::new()), None));
        let caller = launch.registry().activate_window("main").unwrap();
        let service = ProjectionService::new(launch);
        let source = service
            .scope(
                &caller,
                &ScopeTarget::Profile {
                    profile_id: "fixture".into(),
                    expected_profile_revision: doc.profiles["fixture"].revision,
                    project_ids: vec![registration.project_id],
                },
            )
            .unwrap();
        let retained = service
            .retain_remote_catalogue_source(&caller, source)
            .unwrap();
        let one = WireU64::parse("1").unwrap();
        let rows = service
            .read_remote_scoped_history(&caller, &retained, one, || Ok(()))
            .unwrap();
        assert_eq!(
            rows.len(),
            1,
            "fixture must be visible to the scoped reader"
        );
        assert_eq!(rows[0].native_id, id);
        let selector = rows[0].display.physical;
        let peer: CurrentManagementPeer = Arc::new(|| Ok(([1; 32], [2; 32])));
        let result = service.prepare_remote_rename(
            &caller,
            &retained,
            WireU64::parse("2").unwrap(),
            selector,
            ManagementAction::Rename {
                title: "Renamed".into(),
            },
            &peer,
        );
        assert_eq!(
            result.err().unwrap().code,
            "REMOTE_MANAGEMENT_TARGET_UNAVAILABLE"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}
