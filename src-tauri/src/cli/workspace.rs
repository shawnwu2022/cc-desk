//! CLI-independent project registration. Native history is never the source of project existence.
use super::profiles::{error, resolve_override, Override};
use super::source_scope::{is_verified_key, resolve_path_key, validate_selected_path, ResolvedPathKey};
use super::storage::WorkspaceRepository;
use super::types::{SafeError, WireU64};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegisteredProject {
    pub(crate) project_id: String,
    pub(crate) host_id: String,
    pub(crate) source_path_key: String,
    pub(crate) selected_path: PathBuf,
    pub(crate) canonical_path: Option<PathBuf>,
    #[serde(default)]
    pub(crate) alias: Override<String>,
    #[serde(default)]
    pub(crate) pinned: Override<bool>,
    #[serde(default)]
    pub(crate) hidden: Override<bool>,
    #[serde(flatten)]
    pub(crate) extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyMetadata {
    pub(crate) alias: Option<String>,
    pub(crate) pinned: Option<bool>,
    pub(crate) hidden: Option<bool>,
}

impl RegisteredProject {
    fn from_path(path: ResolvedPathKey) -> Self {
        Self {
            project_id: Uuid::new_v4().to_string(),
            host_id: "local".into(),
            source_path_key: path.key,
            selected_path: path.selected_path,
            canonical_path: path.canonical_path,
            alias: Override::Inherit,
            pinned: Override::Inherit,
            hidden: Override::Inherit,
            extra: Map::new(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), SafeError> {
        if Uuid::parse_str(&self.project_id).is_err() || self.host_id != "local" {
            return Err(error("PROJECT_INVALID"));
        }
        validate_selected_path(&self.selected_path)?;
        if let Some(path) = &self.canonical_path {
            validate_selected_path(path)?;
        }
        if self.source_path_key.is_empty()
            || self.source_path_key.len() > 262144
            || self.source_path_key.contains('\0')
        {
            return Err(error("PROJECT_INVALID"));
        }
        if let Override::Set(alias) = &self.alias {
            if alias.encode_utf16().count() > 200 || alias.chars().any(char::is_control) {
                return Err(SafeError::invalid("alias"));
            }
        }
        Ok(())
    }

    pub(crate) fn resolve_metadata(&self, legacy: &LegacyMetadata) -> LegacyMetadata {
        LegacyMetadata {
            alias: resolve_override(self.alias.clone(), legacy.alias.clone()),
            pinned: resolve_override(self.pinned.clone(), legacy.pinned),
            hidden: resolve_override(self.hidden.clone(), legacy.hidden),
        }
    }
}

pub(crate) fn register_project(
    repo: &WorkspaceRepository,
    selected_path: &Path,
) -> Result<RegisteredProject, SafeError> {
    // Resolve outside the file lock. Recheck existing aliases before using a persisted file ID.
    let selected = resolve_path_key(selected_path)?;
    let candidates = list_registered_projects(repo)?;
    let verified_aliases: Vec<String> = candidates
        .iter()
        .filter(|project| {
            project.source_path_key == selected.key
                && is_verified_key(&selected.key)
                && resolve_path_key(&project.selected_path)
                    .is_ok_and(|current| current.key == selected.key)
        })
        .map(|project| project.project_id.clone())
        .collect();
    repo.transact_projects(None, move |projects| {
        if let Some(existing) = projects.values().find(|project| {
            (project.source_path_key == selected.key
                && (!is_verified_key(&selected.key)
                    || verified_aliases.contains(&project.project_id)
                    || project.selected_path == selected.selected_path))
                || (project.selected_path == selected.selected_path
                    && !is_verified_key(&project.source_path_key))
        }) {
            // Unknown -> verified identity is refreshed only on the same explicitly selected path.
            if existing.source_path_key != selected.key {
                let mut promoted = existing.clone();
                promoted.source_path_key = selected.key;
                promoted.canonical_path = selected.canonical_path;
                projects.insert(promoted.project_id.clone(), promoted.clone());
                return Ok((promoted, true));
            }
            return Ok((existing.clone(), false));
        }
        let project = RegisteredProject::from_path(selected);
        project.validate()?;
        projects.insert(project.project_id.clone(), project.clone());
        Ok((project, true))
    })
}

pub(crate) fn list_registered_projects(
    repo: &WorkspaceRepository,
) -> Result<Vec<RegisteredProject>, SafeError> {
    Ok(repo.read()?.registered_projects.into_values().collect())
}

pub(crate) fn patch_project(
    repo: &WorkspaceRepository,
    expected_revision: WireU64,
    project_id: &str,
    changes: Value,
) -> Result<RegisteredProject, SafeError> {
    let fields = changes
        .as_object()
        .ok_or_else(|| SafeError::invalid("changes"))?;
    for (key, value) in fields {
        if !matches!(key.as_str(), "alias" | "pinned" | "hidden") || value.is_null() {
            return Err(SafeError::invalid("changes"));
        }
    }
    repo.transact_projects(Some(expected_revision), |projects| {
        let original = projects
            .get(project_id)
            .ok_or_else(|| error("PROJECT_NOT_FOUND"))?;
        let mut updated = original.clone();
        for (key, value) in fields {
            match key.as_str() {
                "alias" => {
                    updated.alias = serde_json::from_value(value.clone())
                        .map_err(|_| SafeError::invalid("alias"))?;
                }
                "pinned" => {
                    updated.pinned = serde_json::from_value(value.clone())
                        .map_err(|_| SafeError::invalid("pinned"))?;
                }
                "hidden" => {
                    updated.hidden = serde_json::from_value(value.clone())
                        .map_err(|_| SafeError::invalid("hidden"))?;
                }
                _ => return Err(SafeError::invalid("changes")),
            }
        }
        updated.validate()?;
        let changed = &updated != original;
        projects.insert(project_id.to_string(), updated.clone());
        Ok((updated, changed))
    })
}

pub(crate) fn remove_project(
    repo: &WorkspaceRepository,
    expected_revision: WireU64,
    project_id: &str,
) -> Result<(), SafeError> {
    repo.transact_projects(Some(expected_revision), |projects| {
        if projects.remove(project_id).is_none() {
            return Err(error("PROJECT_NOT_FOUND"));
        }
        Ok(((), true))
    })
}
