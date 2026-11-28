//! All eligible desktop scopes; the phone never supplies profile or project selectors.
use super::{CallerIdentity, NativeRuntime};
use crate::cli::{
    native_projection::{
        catalogue_history_source::RetainedHistory, service::admit_project, wire::ScopeTarget,
    },
    profiles::{error, Override},
    storage::WorkspaceDocument,
    types::{CliKind, SafeError, WireU64},
    workspace::RegisteredProject,
};
use std::sync::Arc;

const PROJECT_BATCH: usize = 64;
const MAX_SCOPES: usize = 64;

pub(crate) struct ManagementPortfolio {
    workspace_revision: WireU64,
    pub(super) sources: Vec<Arc<RetainedHistory>>,
}

fn management_targets(
    document: &WorkspaceDocument,
    available: impl Fn(&RegisteredProject) -> bool,
) -> Result<Vec<ScopeTarget>, SafeError> {
    let projects = document
        .registered_projects
        .iter()
        .filter_map(|(id, project)| {
            (id == &project.project_id
                && project.host_id == "local"
                && !matches!(project.hidden, Override::Set(true))
                && available(project))
            .then(|| id.clone())
        })
        .collect::<Vec<_>>();
    let mut targets = Vec::new();
    for (id, profile) in &document.profiles {
        if id != &profile.id
            || !matches!(
                profile.cli,
                CliKind::Claude | CliKind::Codex | CliKind::Opencode
            )
        {
            continue;
        }
        for group in projects.chunks(PROJECT_BATCH) {
            if targets.len() == MAX_SCOPES {
                return Err(error("REMOTE_MANAGEMENT_PORTFOLIO_LIMIT"));
            }
            targets.push(ScopeTarget::Profile {
                profile_id: id.clone(),
                expected_profile_revision: profile.revision,
                project_ids: group.to_vec(),
            });
        }
    }
    Ok(targets)
}

impl NativeRuntime {
    pub(crate) fn retain_owner_management_portfolio(
        &self,
        caller: &CallerIdentity,
        check_pair: impl Fn() -> Result<(), SafeError>,
    ) -> Result<ManagementPortfolio, SafeError> {
        self.binding()?.check_admitted(caller)?;
        let document = self.service.repository().read()?;
        let mut portfolio = ManagementPortfolio {
            workspace_revision: document.revision,
            sources: Vec::new(),
        };
        // 初筛不扩大授权；实际 scope 仍逐个项目重新验证注册目录和身份。
        for target in management_targets(&document, |project| admit_project(project).is_ok())? {
            check_pair()?;
            // Uninstalled or ambiguous history sources cannot become targets.
            // Do not broaden them to ambient/default history.
            if let Ok(source) = self.retain_owner_catalogue(caller, &target) {
                portfolio.sources.push(Arc::new(source));
            }
        }
        check_pair()?;
        self.check_owner_management_portfolio(caller, &portfolio)?;
        Ok(portfolio)
    }

    pub(crate) fn check_owner_management_portfolio(
        &self,
        caller: &CallerIdentity,
        portfolio: &ManagementPortfolio,
    ) -> Result<(), SafeError> {
        self.binding()?.check_admitted(caller)?;
        if self.service.repository().read()?.revision != portfolio.workspace_revision {
            return Err(error("REMOTE_MANAGEMENT_SCOPE_CHANGED"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        profiles::{Override, Profile},
        workspace::RegisteredProject,
    };

    #[test]
    fn management_portfolio_authoritative_scope_covers_providers_and_excludes_hidden_or_remote() {
        let mut doc = WorkspaceDocument::default();
        for (id, cli) in [
            ("claude", CliKind::Claude),
            ("codex", CliKind::Codex),
            ("codex-alt", CliKind::Codex),
            ("opencode", CliKind::Opencode),
            ("agy", CliKind::Antigravity),
            ("shell", CliKind::Shell),
        ] {
            doc.profiles.insert(id.into(), Profile::new(id, cli));
        }
        for (id, host, hidden) in [
            ("visible", "local", Override::Inherit),
            ("unavailable", "local", Override::Inherit),
            ("hidden", "local", Override::Set(true)),
            ("remote", "other", Override::Inherit),
        ] {
            doc.registered_projects.insert(
                id.into(),
                RegisteredProject {
                    project_id: id.into(),
                    host_id: host.into(),
                    source_path_key: "fixture".into(),
                    selected_path: "unused".into(),
                    canonical_path: None,
                    alias: Override::Inherit,
                    pinned: Override::Inherit,
                    hidden,
                    extra: Default::default(),
                },
            );
        }
        let targets = management_targets(&doc, |p| p.project_id != "unavailable").unwrap();
        assert_eq!(targets.len(), 4);
        let mut actual = Vec::new();
        for target in targets {
            let ScopeTarget::Profile {
                profile_id,
                project_ids,
                ..
            } = target
            else {
                panic!()
            };
            assert_eq!(project_ids, ["visible"]);
            actual.push(profile_id);
        }
        assert_eq!(actual, ["claude", "codex", "codex-alt", "opencode"]);
    }

    #[test]
    fn management_portfolio_keeps_healthy_project_when_other_root_is_unavailable() {
        use crate::cli::native_projection::scoped_fs::Root;

        let parent = tempfile::tempdir().unwrap();
        let healthy = parent.path().join("healthy");
        std::fs::create_dir(&healthy).unwrap();
        let root = Root::open(&healthy).unwrap();
        let mut doc = WorkspaceDocument::default();
        doc.profiles
            .insert("claude".into(), Profile::new("claude", CliKind::Claude));
        let healthy_id = uuid::Uuid::new_v4().to_string();
        for (id, selected_path, source_path_key) in [
            (healthy_id.clone(), healthy.clone(), root.key().to_owned()),
            (
                uuid::Uuid::new_v4().to_string(),
                parent.path().join("missing"),
                root.key().to_owned(),
            ),
            (
                uuid::Uuid::new_v4().to_string(),
                healthy,
                "replaced-root".into(),
            ),
        ] {
            doc.registered_projects.insert(
                id.clone(),
                RegisteredProject {
                    project_id: id,
                    host_id: "local".into(),
                    source_path_key,
                    selected_path,
                    canonical_path: None,
                    alias: Override::Inherit,
                    pinned: Override::Inherit,
                    hidden: Override::Inherit,
                    extra: Default::default(),
                },
            );
        }

        let targets = management_targets(&doc, |project| admit_project(project).is_ok()).unwrap();
        let [ScopeTarget::Profile { project_ids, .. }] = targets.as_slice() else {
            panic!("Expected one healthy management scope");
        };
        assert_eq!(project_ids, &[healthy_id]);
    }
}
