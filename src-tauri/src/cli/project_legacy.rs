//! Read-only, bounded inheritance of Desk project metadata. No native session/auth projection.
use super::profiles::Override;
use super::source_scope::{is_verified_key, resolve_path_key};
use super::workspace::{LegacyMetadata, RegisteredProject};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

const MAX_LEGACY_BYTES: u64 = 1024 * 1024;

fn read_optional(path: &Path, warnings: &mut Vec<String>) -> Option<Value> {
    let read = || -> Result<Option<Value>, ()> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Ok(metadata) if metadata.is_file() => {}
            _ => return Err(()),
        }
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|_| ())?
            .take(MAX_LEGACY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes.len() as u64 > MAX_LEGACY_BYTES {
            return Err(());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
        if !value.is_object() {
            return Err(());
        }
        Ok(Some(value))
    };
    match read() {
        Ok(value) => value,
        Err(()) => {
            warnings.push("LEGACY_METADATA_UNAVAILABLE".into());
            None
        }
    }
}

fn matches(project: &RegisteredProject, candidate: &str) -> bool {
    if project.selected_path.to_str() == Some(candidate) {
        return true;
    }
    is_verified_key(&project.source_path_key)
        && resolve_path_key(&project.selected_path)
            .is_ok_and(|current| current.key == project.source_path_key)
        && resolve_path_key(Path::new(candidate))
            .is_ok_and(|current| current.key == project.source_path_key)
}

fn flag(
    project: &RegisteredProject,
    value: Option<&Value>,
    warnings: &mut Vec<String>,
) -> Option<bool> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => Some(
            values
                .iter()
                .any(|value| matches(project, value.as_str().expect("validated string"))),
        ),
        _ => {
            warnings.push("LEGACY_METADATA_INVALID_FIELD".into());
            None
        }
    }
}

pub(crate) fn resolve_metadata(
    directory: &Path,
    projects: &[RegisteredProject],
) -> (BTreeMap<String, LegacyMetadata>, Vec<String>) {
    let mut warnings = Vec::new();
    let needs_projects = projects.iter().any(|project| {
        matches!(project.alias, Override::Inherit) || matches!(project.pinned, Override::Inherit)
    });
    let needs_config = projects
        .iter()
        .any(|project| matches!(project.hidden, Override::Inherit));
    let old_projects = needs_projects
        .then(|| read_optional(&directory.join("projects.json"), &mut warnings))
        .flatten();
    let old_config = needs_config
        .then(|| read_optional(&directory.join("config.json"), &mut warnings))
        .flatten();
    let mut result = BTreeMap::new();
    for project in projects {
        let mut legacy = LegacyMetadata::default();
        if matches!(project.alias, Override::Inherit) {
            let value = old_projects
                .as_ref()
                .and_then(|value| value.get("displayNames"));
            match value {
                None | Some(Value::Null) => {}
                Some(Value::Object(aliases)) => {
                    let mut candidates = Vec::new();
                    for (path, alias) in aliases {
                        if !matches(project, path) {
                            continue;
                        }
                        match alias.as_str() {
                            Some(alias)
                                if alias.encode_utf16().count() <= 200
                                    && !alias.chars().any(char::is_control) =>
                            {
                                candidates.push(alias.to_string());
                            }
                            _ => warnings.push("LEGACY_METADATA_INVALID_FIELD".into()),
                        }
                    }
                    candidates.sort();
                    candidates.dedup();
                    if candidates.len() == 1 {
                        legacy.alias = candidates.pop();
                    } else if !candidates.is_empty() {
                        warnings.push("LEGACY_ALIAS_CONFLICT".into());
                    }
                }
                _ => warnings.push("LEGACY_METADATA_INVALID_FIELD".into()),
            }
        }
        if matches!(project.pinned, Override::Inherit) {
            legacy.pinned = flag(
                project,
                old_projects
                    .as_ref()
                    .and_then(|value| value.get("pinnedProjects")),
                &mut warnings,
            );
        }
        if matches!(project.hidden, Override::Inherit) {
            legacy.hidden = flag(
                project,
                old_config
                    .as_ref()
                    .and_then(|value| value.get("hiddenProjects")),
                &mut warnings,
            );
        }
        result.insert(
            project.project_id.clone(),
            project.resolve_metadata(&legacy),
        );
    }
    warnings.sort();
    warnings.dedup();
    (result, warnings)
}
