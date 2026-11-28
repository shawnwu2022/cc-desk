//! Host-only view of a previously admitted native history source.
//! No request from the phone is deserialized into this module's inputs.
use super::wire::{
    ProjectionResult, ProjectionState, ReadRequest, ResourceItem, ResourceKind, ScopeTarget,
    SourceBasis, SourceRef, TitleSource,
};
use crate::cli::{
    profiles::error,
    types::{CliKind, SafeError, WireU64},
};
use cc_desk_remote_security::session_catalogue::{
    ArchiveEvidence, DisplayLifecycle, OpaqueKey, SourceRow, MAX_CATALOGUE_CANDIDATES,
};
use sha2::{Digest, Sha256};

const PAGE: u16 = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeArchiveProject {
    pub(super) root: String,
    pub(super) aliases: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeArchiveSnapshot {
    projects: std::collections::BTreeMap<String, ClaudeArchiveProject>,
    markers: std::collections::BTreeMap<uuid::Uuid, Vec<(String, String)>>,
}

impl ClaudeArchiveSnapshot {
    pub(super) fn new(
        projects: std::collections::BTreeMap<String, ClaudeArchiveProject>,
        stored: std::collections::HashMap<String, Vec<String>>,
    ) -> Result<Self, SafeError> {
        let mut markers: std::collections::BTreeMap<_, Vec<_>> = Default::default();
        let mut total = 0usize;
        let mut bytes = 0usize;
        for (path, values) in stored {
            for value in values {
                total += 1;
                bytes = bytes.saturating_add(value.len()).saturating_add(path.len());
                if total > 65_536 || bytes > 8 * 1024 * 1024 {
                    return Err(error("REMOTE_CATALOGUE_SOURCE_UNAVAILABLE"));
                }
                let payload = value.strip_prefix("native-history:");
                let id = uuid::Uuid::parse_str(&value).ok().or_else(|| {
                    let data: Vec<String> = serde_json::from_str(payload?).ok()?;
                    uuid::Uuid::parse_str(data.last()?).ok()
                });
                if let Some(id) = id {
                    markers
                        .entry(id)
                        .or_default()
                        .push((crate::store::normalize_path_str(&path), value));
                } else if payload.is_some() {
                    return Err(error("REMOTE_CATALOGUE_SOURCE_UNAVAILABLE"));
                }
            }
        }
        for claims in markers.values_mut() {
            claims.sort();
            claims.dedup();
        }
        Ok(Self { projects, markers })
    }

    pub(super) fn exact_claims(&self, id: uuid::Uuid) -> &[(String, String)] {
        self.markers.get(&id).map(Vec::as_slice).unwrap_or_default()
    }

    pub(super) fn classify(
        &self,
        source: &SourceRef,
        project: &str,
        session: &str,
        id: uuid::Uuid,
    ) -> Option<(&str, ArchiveEvidence)> {
        let project_scope = self.projects.get(project)?;
        let Some(markers) = self.markers.get(&id) else {
            return Some((project_scope.root.as_str(), ArchiveEvidence::NotArchived));
        };
        let id = id.to_string();
        let rev = source.profile_revision.to_string();
        let exact = markers.iter().all(|(storage, value)| {
            let parse = || -> Option<bool> {
                let marker: [String; 4] =
                    serde_json::from_str(value.strip_prefix("native-history:")?).ok()?;
                let context: [String; 6] = serde_json::from_str(&marker[1]).ok()?;
                Some(
                    marker[0] == "native-history-v2"
                        && marker[2] == session
                        && marker[3] == id
                        && context[0] == "native-history-v1"
                        && context[1] == "claude"
                        && context[2] == source.profile_id
                        && context[3] == rev
                        && context[4] == project
                        && project_scope.aliases.contains(storage)
                        && project_scope
                            .aliases
                            .contains(&crate::store::normalize_path_str(&context[5])),
                )
            };
            parse().unwrap_or(false)
        });
        Some((
            project_scope.root.as_str(),
            if exact {
                ArchiveEvidence::Archived
            } else {
                ArchiveEvidence::Unknown
            },
        ))
    }
}

fn archive_flag(value: bool) -> ArchiveEvidence {
    if value {
        ArchiveEvidence::Archived
    } else {
        ArchiveEvidence::NotArchived
    }
}

#[allow(dead_code)] // Used by the feature-gated native host pairing consumer.
pub(crate) struct RetainedHistory(SourceRef);

// Backend-only. The display projection never serializes provider locators.
#[cfg_attr(not(all(windows, feature = "remote-observer")), allow(dead_code))]
pub(super) struct ScopedHistoryRow {
    pub(super) display: SourceRow,
    pub(super) native_id: String,
    pub(super) session_key: String,
    pub(super) project: String,
    pub(super) observed_project_root: Option<String>,
}

#[allow(dead_code)]
impl RetainedHistory {
    pub(super) fn from_owner_scope(source: SourceRef) -> Result<Self, SafeError> {
        // Codex history has a native archive discriminator. The other
        // providers need their own positive archive evidence before exposure.
        if !matches!(
            source.cli,
            CliKind::Codex | CliKind::Opencode | CliKind::Claude
        ) || source.basis != SourceBasis::ConfiguredProfile
            || !crate::cli::source_scope::is_verified_key(&source.source_root_key)
            || !matches!(&source.target, ScopeTarget::Profile { project_ids, .. } if !project_ids.is_empty())
        {
            return Err(error("REMOTE_CATALOGUE_SOURCE_UNAVAILABLE"));
        }
        Ok(Self(source))
    }

    pub(crate) fn source(&self) -> &SourceRef {
        &self.0
    }

    pub(crate) fn key(&self) -> OpaqueKey {
        digest(b"cc-desk.catalogue.admitted-source.v1", &[&self.0.scope_id])
    }

    pub(crate) fn apply_owned_lifecycle(
        &self,
        rows: &mut [SourceRow],
        lifecycle: impl IntoIterator<Item = (uuid::Uuid, crate::cli::run_registry::LaunchPhase)>,
    ) {
        use crate::cli::run_registry::LaunchPhase;
        let mut states = std::collections::BTreeMap::<OpaqueKey, Option<DisplayLifecycle>>::new();
        for (id, phase) in lifecycle {
            let display = match phase {
                LaunchPhase::Running => DisplayLifecycle::Open,
                LaunchPhase::Exited => DisplayLifecycle::Closed,
                _ => continue,
            };
            states
                .entry(self.physical(id))
                .and_modify(|slot| *slot = None)
                .or_insert(Some(display));
        }
        let source_key = self.key();
        for row in rows.iter_mut().filter(|row| row.source == source_key) {
            if let Some(Some(phase)) = states.get(&row.physical) {
                row.lifecycle = *phase;
            }
        }
    }

    pub(crate) fn apply_claude_owned_lifecycle(
        &self,
        rows: &mut [SourceRow],
        lifecycle: impl IntoIterator<Item = (uuid::Uuid, String, crate::cli::run_registry::LaunchPhase)>,
    ) {
        use crate::cli::run_registry::LaunchPhase;
        let mut states = std::collections::BTreeMap::<OpaqueKey, Option<DisplayLifecycle>>::new();
        for (id, project, phase) in lifecycle {
            let display = match phase {
                LaunchPhase::Running => DisplayLifecycle::Open,
                LaunchPhase::Exited => DisplayLifecycle::Closed,
                _ => continue,
            };
            states
                .entry(self.claude_physical(id, &project))
                .and_modify(|v| *v = None)
                .or_insert(Some(display));
        }
        let key = self.key();
        for row in rows.iter_mut().filter(|row| row.source == key) {
            if let Some(Some(phase)) = states.get(&row.physical) {
                row.lifecycle = *phase;
            }
        }
    }

    fn claude_physical(&self, id: uuid::Uuid, project: &str) -> OpaqueKey {
        digest(
            b"cc-desk.catalogue.claude-physical.v1",
            &[&self.0.source_root_key, project, &id.to_string()],
        )
    }

    fn opencode_physical(&self, id: &str, project: &str) -> OpaqueKey {
        digest(
            b"cc-desk.catalogue.opencode-physical.v1",
            &[&self.0.source_root_key, project, id],
        )
    }

    pub(crate) fn apply_opencode_owned_lifecycle(
        &self,
        rows: &mut [SourceRow],
        lifecycle: impl IntoIterator<Item = (String, String, crate::cli::run_registry::LaunchPhase)>,
    ) {
        use crate::cli::run_registry::LaunchPhase;
        let mut states = std::collections::BTreeMap::<OpaqueKey, Option<DisplayLifecycle>>::new();
        for (id, project, phase) in lifecycle {
            let display = match phase {
                LaunchPhase::Running => DisplayLifecycle::Open,
                LaunchPhase::Exited => DisplayLifecycle::Closed,
                _ => continue,
            };
            states
                .entry(self.opencode_physical(&id, &project))
                .and_modify(|v| *v = None)
                .or_insert(Some(display));
        }
        let key = self.key();
        for row in rows.iter_mut().filter(|row| row.source == key) {
            if let Some(Some(phase)) = states.get(&row.physical) {
                row.lifecycle = *phase;
            }
        }
    }

    fn physical(&self, id: uuid::Uuid) -> OpaqueKey {
        digest(
            b"cc-desk.catalogue.codex-physical.v1",
            &[&self.0.source_root_key, &id.to_string()],
        )
    }

    #[cfg(test)]
    pub(super) fn read(
        &self,
        epoch: WireU64,
        current: impl Fn() -> Result<(), SafeError>,
        read_page: impl Fn(&ReadRequest) -> Result<ProjectionResult, SafeError>,
    ) -> Result<Vec<SourceRow>, SafeError> {
        self.read_with_archive(epoch, current, read_page, None)
    }

    #[cfg(test)]
    pub(super) fn read_with_archive(
        &self,
        epoch: WireU64,
        current: impl Fn() -> Result<(), SafeError>,
        read_page: impl Fn(&ReadRequest) -> Result<ProjectionResult, SafeError>,
        claude: Option<&ClaudeArchiveSnapshot>,
    ) -> Result<Vec<SourceRow>, SafeError> {
        Ok(self
            .read_scoped_history(epoch, current, read_page, claude)?
            .into_iter()
            .map(|entry| entry.display)
            .collect())
    }

    pub(super) fn read_scoped_history(
        &self,
        epoch: WireU64,
        current: impl Fn() -> Result<(), SafeError>,
        read_page: impl Fn(&ReadRequest) -> Result<ProjectionResult, SafeError>,
        claude: Option<&ClaudeArchiveSnapshot>,
    ) -> Result<Vec<ScopedHistoryRow>, SafeError> {
        let mut rows = Vec::new();
        let mut offset = 0u32;
        let mut observed = None;
        let admitted_source = self.key();
        let ScopeTarget::Profile { project_ids, .. } = &self.0.target else {
            return Err(error("REMOTE_CATALOGUE_SOURCE_UNAVAILABLE"));
        };
        loop {
            current()?;
            let page = read_page(&ReadRequest {
                source: self.0.clone(),
                resource_kind: ResourceKind::History,
                request_epoch: epoch,
                query: None,
                session_id: None,
                limit: PAGE,
                offset,
            })?;
            current()?;
            if page.source != self.0
                || page.resource_kind != ResourceKind::History
                || page.request_epoch != epoch
                || page.state != ProjectionState::Ready
                || page.history_metadata_incomplete == Some(true)
                || !page.history_read_failures.is_empty()
                || page.items.len() > usize::from(PAGE)
                || observed.is_some_and(|stamp| stamp != page.observed_at)
            {
                return Err(error("REMOTE_CATALOGUE_SOURCE_CHANGED"));
            }
            observed = Some(page.observed_at);
            let page_len = page.items.len();
            if (offset as usize).saturating_add(page_len) > MAX_CATALOGUE_CANDIDATES
                || (page.has_more && page_len == 0)
            {
                return Err(error("REMOTE_CATALOGUE_SOURCE_CHANGED"));
            }
            for item in page.items {
                let ResourceItem::Session {
                    native_session_id,
                    session_key,
                    project_id: Some(project),
                    title,
                    title_source,
                    title_unknown,
                    metadata_incomplete,
                    read_failure,
                    archived,
                    opencode_archive,
                    ..
                } = item
                else {
                    continue;
                };
                if !project_ids.contains(&project)
                    || metadata_incomplete == Some(true)
                    || read_failure.is_some()
                {
                    continue;
                }
                let (physical, archive, observed_project_root) = match self.0.cli {
                    CliKind::Codex => {
                        let Ok(id) = uuid::Uuid::parse_str(&native_session_id) else {
                            continue;
                        };
                        if id.is_nil() {
                            continue;
                        }
                        (self.physical(id), archive_flag(archived), None)
                    }
                    CliKind::Opencode => {
                        let Some((state, project_root)) = opencode_archive else {
                            continue;
                        };
                        if !crate::cli::source_scope::is_verified_key(&project_root)
                            || !native_session_id.starts_with("ses_")
                            || native_session_id.len() <= 4
                            || native_session_id.len() > 256
                            || !native_session_id
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        {
                            continue;
                        }
                        (
                            self.opencode_physical(&native_session_id, &project_root),
                            archive_flag(state),
                            Some(project_root),
                        )
                    }
                    CliKind::Claude => {
                        let Ok(id) = uuid::Uuid::parse_str(&native_session_id) else {
                            continue;
                        };
                        if id.is_nil() || id.to_string() != native_session_id {
                            continue;
                        }
                        let exact = serde_json::to_string(&[
                            "local",
                            "claude",
                            &self.0.source_root_key,
                            &native_session_id,
                        ])
                        .map_err(|_| error("REMOTE_CATALOGUE_SOURCE_CHANGED"))?;
                        if exact != session_key {
                            continue;
                        }
                        let Some((root, archive)) =
                            claude.and_then(|e| e.classify(&self.0, &project, &session_key, id))
                        else {
                            continue;
                        };
                        (
                            self.claude_physical(id, root),
                            archive,
                            Some(root.to_owned()),
                        )
                    }
                    _ => continue,
                };
                // History preview titles can be derived from the first user
                // message. Until owner-approved labels are integrated, keep
                // those message-derived fields out of the paired catalogue.
                let title = if matches!(title_source, Some(TitleSource::Custom))
                    && title_unknown != Some(true)
                {
                    title
                } else {
                    match self.0.cli {
                        CliKind::Codex => "Codex session",
                        CliKind::Opencode => "OpenCode session",
                        _ => "Claude session",
                    }
                    .into()
                };
                rows.push(ScopedHistoryRow {
                    display: SourceRow {
                        source: admitted_source,
                        physical,
                        archive,
                        // A history row alone never proves the original run closed.
                        lifecycle: DisplayLifecycle::History,
                        title,
                    },
                    native_id: native_session_id,
                    session_key,
                    project,
                    observed_project_root,
                });
            }
            if !page.has_more {
                current()?;
                return Ok(rows);
            }
            offset += page_len as u32;
        }
    }
}

fn digest(domain: &[u8], fields: &[&str]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    for field in fields {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field.as_bytes());
    }
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_catalogue_only_projects_unambiguous_owned_lifecycle() {
        use crate::cli::run_registry::LaunchPhase;
        let retained = RetainedHistory::from_owner_scope(expected_source()).unwrap();
        let ids = [1u128, 2, 3, 4, 5].map(uuid::Uuid::from_u128);
        let mut rows = ids.map(|id| SourceRow {
            source: retained.key(),
            physical: retained.physical(id),
            archive: ArchiveEvidence::NotArchived,
            lifecycle: DisplayLifecycle::History,
            title: "Allowed title".into(),
        });
        rows[4].source = [9; 32];
        retained.apply_owned_lifecycle(
            &mut rows,
            [
                (ids[0], LaunchPhase::Running),
                (ids[1], LaunchPhase::Exited),
                (ids[2], LaunchPhase::Running),
                (ids[2], LaunchPhase::Exited),
                (ids[4], LaunchPhase::Exited),
            ],
        );
        assert_eq!(
            rows.map(|row| row.lifecycle),
            [
                DisplayLifecycle::Open,
                DisplayLifecycle::Closed,
                DisplayLifecycle::History,
                DisplayLifecycle::History,
                DisplayLifecycle::History,
            ]
        );
    }

    #[test]
    fn opencode_catalogue_requires_native_archive_evidence_and_exact_project() {
        use crate::cli::run_registry::LaunchPhase;
        let mut source = expected_source();
        source.cli = CliKind::Opencode;
        let retained = RetainedHistory::from_owner_scope(source).unwrap();
        let project = "local:windows:current-project";
        let fixture = |id: &str, evidence| {
            let mut record = row("a", TitleSource::Prompt, false);
            if let ResourceItem::Session {
                native_session_id,
                opencode_archive,
                ..
            } = &mut record
            {
                *native_session_id = id.into();
                *opencode_archive = evidence;
            }
            record
        };
        let mut rows = retained
            .read(
                number("3"),
                || Ok(()),
                |r| {
                    Ok(page(
                        r,
                        vec![
                            fixture("ses_unknown", None),
                            fixture("ses_open", Some((false, project.into()))),
                            fixture("ses_closed", Some((true, project.into()))),
                            fixture("ses_other", Some((false, project.into()))),
                        ],
                        "4",
                        false,
                    ))
                },
            )
            .unwrap();
        retained.apply_opencode_owned_lifecycle(
            &mut rows,
            [
                ("ses_open".into(), project.into(), LaunchPhase::Running),
                ("ses_closed".into(), project.into(), LaunchPhase::Exited),
                (
                    "ses_other".into(),
                    "local:windows:wrong-project".into(),
                    LaunchPhase::Running,
                ),
            ],
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].lifecycle, DisplayLifecycle::Open);
        assert_eq!(rows[1].lifecycle, DisplayLifecycle::Closed);
        assert_eq!(rows[1].archive, ArchiveEvidence::Archived);
        assert_eq!(rows[2].lifecycle, DisplayLifecycle::History);
        assert!(rows.iter().all(|row| row.title == "OpenCode session"));
    }

    #[test]
    fn claude_catalogue_archive_requires_exact_profile_project_root_and_marker() {
        use crate::cli::run_registry::LaunchPhase;
        use std::collections::{BTreeMap, BTreeSet, HashMap};
        let mut source = expected_source();
        source.cli = CliKind::Claude;
        let catalogue = RetainedHistory::from_owner_scope(source.clone()).unwrap();
        let project_root = "local:windows:project-root";
        let path = crate::store::normalize_path_str("C:/fixture/project");
        let projects = BTreeMap::from([(
            "a".into(),
            ClaudeArchiveProject {
                root: project_root.into(),
                aliases: BTreeSet::from([path.clone()]),
            },
        )]);
        let id = uuid::Uuid::from_u128(42);
        let native = id.to_string();
        let session =
            serde_json::to_string(&["local", "claude", &source.source_root_key, &native]).unwrap();
        let marker = |profile: &str| {
            let context =
                serde_json::to_string(&["native-history-v1", "claude", profile, "2", "a", &path])
                    .unwrap();
            format!(
                "native-history:{}",
                serde_json::to_string(&["native-history-v2", &context, &session, &native]).unwrap()
            )
        };
        let observe = |claims: Vec<String>| {
            let stored = ClaudeArchiveSnapshot::new(
                projects.clone(),
                HashMap::from([(path.clone(), claims)]),
            )
            .unwrap();
            let mut original = row("a", TitleSource::Prompt, false);
            if let ResourceItem::Session {
                native_session_id,
                session_key,
                ..
            } = &mut original
            {
                *native_session_id = native.clone();
                *session_key = session.clone();
            }
            catalogue
                .read_with_archive(
                    number("4"),
                    || Ok(()),
                    |q| Ok(page(q, vec![original.clone()], "5", false)),
                    Some(&stored),
                )
                .unwrap()
        };
        let mut rows = observe(vec![]);
        assert_eq!(rows[0].archive, ArchiveEvidence::NotArchived);
        catalogue.apply_claude_owned_lifecycle(
            &mut rows,
            [(id, "wrong-root".into(), LaunchPhase::Running)],
        );
        assert_eq!(rows[0].lifecycle, DisplayLifecycle::History);
        catalogue.apply_claude_owned_lifecycle(
            &mut rows,
            [(id, project_root.into(), LaunchPhase::Running)],
        );
        assert_eq!(rows[0].lifecycle, DisplayLifecycle::Open);
        assert_eq!(rows[0].title, "Claude session");
        assert_eq!(
            observe(vec![marker("profile")])[0].archive,
            ArchiveEvidence::Archived
        );
        assert_eq!(
            observe(vec![marker("other-profile")])[0].archive,
            ArchiveEvidence::Unknown
        );
        assert_eq!(
            observe(vec![native.clone()])[0].archive,
            ArchiveEvidence::Unknown
        );
    }

    fn number(value: &str) -> WireU64 {
        WireU64::parse(value).unwrap()
    }

    fn expected_source() -> SourceRef {
        SourceRef {
            scope_id: "scope".into(),
            instance_id: "instance".into(),
            cli: CliKind::Codex,
            source_root_key: "local:windows:synthetic".into(),
            identity_epoch: number("1"),
            profile_id: "profile".into(),
            profile_revision: number("2"),
            target: ScopeTarget::Profile {
                profile_id: "profile".into(),
                expected_profile_revision: number("2"),
                project_ids: vec!["a".into()],
            },
            basis: SourceBasis::ConfiguredProfile,
        }
    }

    fn page(
        request: &ReadRequest,
        items: Vec<ResourceItem>,
        stamp: &str,
        more: bool,
    ) -> ProjectionResult {
        ProjectionResult {
            source: request.source.clone(),
            resource_kind: request.resource_kind,
            request_epoch: request.request_epoch,
            observed_at: number(stamp),
            state: ProjectionState::Ready,
            reason: None,
            items,
            has_more: more,
            history_metadata_incomplete: None,
            history_read_failures: Vec::new(),
        }
    }

    fn row(project_id: &str, title: TitleSource, archived: bool) -> ResourceItem {
        ResourceItem::Session {
            session_key: "native-key".into(),
            native_session_id: "30ee8ec2-91bd-4b64-ab41-d121aca5bb76".into(),
            project_id: Some(project_id.into()),
            title: "private first prompt".into(),
            title_unknown: None,
            title_source: Some(title),
            metadata_incomplete: None,
            read_failure: None,
            truncated: false,
            archived,
            opencode_archive: None,
            cwd: Some("C:/private".into()),
            updated_at: None,
            created_at: None,
        }
    }

    #[test]
    fn remote_catalogue_only_projects_explicit_project_and_does_not_export_prompts() {
        let retained = RetainedHistory::from_owner_scope(expected_source()).unwrap();
        let rows = retained
            .read(
                number("3"),
                || Ok(()),
                |r| {
                    Ok(page(
                        r,
                        vec![
                            row("other", TitleSource::Custom, false),
                            row("a", TitleSource::Prompt, true),
                        ],
                        "4",
                        false,
                    ))
                },
            )
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Codex session");
        assert_eq!(rows[0].lifecycle, DisplayLifecycle::History);
        assert_eq!(rows[0].archive, ArchiveEvidence::Archived);
    }

    #[test]
    fn remote_catalogue_rejects_changed_snapshot_or_pair() {
        let retained = RetainedHistory::from_owner_scope(expected_source()).unwrap();
        let rows = retained.read(
            number("3"),
            || Ok(()),
            |r| {
                let stamp = if r.offset == 0 { "4" } else { "5" };
                Ok(page(
                    r,
                    vec![row("a", TitleSource::Prompt, false)],
                    stamp,
                    r.offset == 0,
                ))
            },
        );
        assert_eq!(rows.unwrap_err().code, "REMOTE_CATALOGUE_SOURCE_CHANGED");

        let permitted = std::cell::Cell::new(true);
        let rows = retained.read(
            number("3"),
            || {
                if permitted.get() {
                    Ok(())
                } else {
                    Err(error("FORBIDDEN"))
                }
            },
            |r| {
                permitted.set(false);
                Ok(page(
                    r,
                    vec![row("a", TitleSource::Prompt, false)],
                    "4",
                    false,
                ))
            },
        );
        assert_eq!(rows.unwrap_err().code, "FORBIDDEN");
    }
}
