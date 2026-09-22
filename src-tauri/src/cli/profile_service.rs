//! Profile operations with safe wire errors and no native credential projection.
use super::profiles::{error, Profile};
use super::storage::{Patch, WorkspaceDocument, WorkspaceRepository};
use super::types::{SafeError, WireU64};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileList {
    pub(crate) revision: WireU64,
    pub(crate) profiles: Vec<Profile>,
}

impl From<WorkspaceDocument> for ProfileList {
    fn from(document: WorkspaceDocument) -> Self {
        Self {
            revision: document.revision,
            profiles: document.profiles.into_values().collect(),
        }
    }
}

/// The command adapter supplies the actual invoking WebView label, never a request field.
pub(crate) fn authorize_profile_window(label: &str) -> Result<(), SafeError> {
    if label != "main" {
        return Err(error("FORBIDDEN"));
    }
    Ok(())
}

pub(crate) fn list_profiles(
    repository: &WorkspaceRepository,
    caller_label: &str,
) -> Result<ProfileList, SafeError> {
    authorize_profile_window(caller_label)?;
    Ok(repository.read()?.into())
}

pub(crate) fn patch_profile(
    repository: &WorkspaceRepository,
    caller_label: &str,
    expected_revision: WireU64,
    patch: Patch,
) -> Result<ProfileList, SafeError> {
    authorize_profile_window(caller_label)?;
    Ok(repository.apply(expected_revision, patch)?.into())
}

/// Decode inside the safe boundary: serde error text can contain submitted values.
pub(crate) fn parse_patch(
    expected_revision: &Value,
    patch: Value,
) -> Result<(WireU64, Patch), SafeError> {
    let value = expected_revision
        .as_str()
        .ok_or_else(|| SafeError::invalid("expectedRevision"))?;
    let revision = WireU64::parse(value).map_err(|_| SafeError::invalid("expectedRevision"))?;
    let patch = serde_json::from_value(patch).map_err(|_| SafeError::invalid("patch"))?;
    Ok((revision, patch))
}
