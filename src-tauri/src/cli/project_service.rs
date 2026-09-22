//! Project IPC operations. Public responses never expose unknown persisted fields.
use super::profile_service::authorize_profile_window;
use super::profiles::error;
use super::storage::WorkspaceRepository;
use super::types::{SafeError, WireU64};
use super::workspace::{self, LegacyMetadata, RegisteredProject};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectList {
    pub(crate) revision: WireU64,
    pub(crate) projects: Vec<RegisteredProject>,
    pub(crate) metadata: BTreeMap<String, LegacyMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project_id: Option<String>,
}

pub(crate) fn list_projects(
    repository: &WorkspaceRepository,
    caller: &str,
) -> Result<ProjectList, SafeError> {
    authorize_profile_window(caller)?;
    let document = repository.read()?;
    let mut metadata = BTreeMap::new();
    let projects = document
        .registered_projects
        .into_values()
        .map(|mut project| {
            metadata.insert(
                project.project_id.clone(),
                project.resolve_metadata(&LegacyMetadata::default()),
            );
            project.extra.clear();
            project
        })
        .collect();
    Ok(ProjectList {
        revision: document.revision,
        projects,
        metadata,
        project_id: None,
    })
}

pub(crate) fn register(
    repository: &WorkspaceRepository,
    caller: &str,
    selected_path: &Path,
) -> Result<ProjectList, SafeError> {
    authorize_profile_window(caller)?;
    let project = workspace::register_project(repository, selected_path)?;
    let mut result = list_projects(repository, caller).map_err(|_| error("COMMIT_STATE_UNKNOWN"))?;
    result.project_id = Some(project.project_id);
    Ok(result)
}

pub(crate) fn patch(
    repository: &WorkspaceRepository,
    caller: &str,
    revision: WireU64,
    project_id: &str,
    changes: Value,
) -> Result<ProjectList, SafeError> {
    authorize_profile_window(caller)?;
    workspace::patch_project(repository, revision, project_id, changes)?;
    list_projects(repository, caller).map_err(|_| error("COMMIT_STATE_UNKNOWN"))
}

pub(crate) fn remove(
    repository: &WorkspaceRepository,
    caller: &str,
    revision: WireU64,
    project_id: &str,
) -> Result<ProjectList, SafeError> {
    authorize_profile_window(caller)?;
    workspace::remove_project(repository, revision, project_id)?;
    list_projects(repository, caller).map_err(|_| error("COMMIT_STATE_UNKNOWN"))
}

pub(crate) fn parse_text(value: &Value, field: &str) -> Result<String, SafeError> {
    value
        .as_str()
        .filter(|text| !text.is_empty() && !text.contains('\0') && text.len() <= 32768)
        .map(str::to_owned)
        .ok_or_else(|| SafeError::invalid(field))
}

pub(crate) fn parse_revision(value: &Value) -> Result<WireU64, SafeError> {
    let text = parse_text(value, "expectedRevision")?;
    WireU64::parse(&text).map_err(|_| SafeError::invalid("expectedRevision"))
}
