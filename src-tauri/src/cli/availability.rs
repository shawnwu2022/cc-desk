//! Read-only per-profile preflight. Never executes a candidate or returns resolved secrets.

use super::environment::{build_environment, EnvMap};
use super::profile_service::authorize_profile_window;
use super::profiles::error;
use super::snapshot::{configured_program, configured_runner, Availability, HostStatus};
use super::storage::WorkspaceRepository;
use super::types::{SafeError, WireU64};
use portable_pty::{native_pty_system, PtySize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AvailabilityRequest {
    pub(crate) profile_id: String,
    pub(crate) expected_revision: WireU64,
}

impl AvailabilityRequest {
    fn validate(&self) -> Result<(), SafeError> {
        if self.profile_id.is_empty()
            || self.profile_id.len() > 128
            || !self
                .profile_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(SafeError::invalid("profileId"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileAvailability {
    pub(crate) profile_id: String,
    #[serde(flatten)]
    pub(crate) availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) issue: Option<SafeError>,
}

/// Serde's original error may contain caller values; keep it inside this boundary.
pub(crate) fn parse_request(value: Value) -> Result<AvailabilityRequest, SafeError> {
    let request: AvailabilityRequest =
        serde_json::from_value(value).map_err(|_| SafeError::invalid("request"))?;
    request.validate()?;
    Ok(request)
}

pub(crate) fn get_availability(
    repository: &WorkspaceRepository,
    caller: &str,
    request: &AvailabilityRequest,
    inherited: &EnvMap,
    check_host: impl FnOnce() -> HostStatus,
) -> Result<ProfileAvailability, SafeError> {
    authorize_profile_window(caller)?;
    request.validate()?;
    let profile = repository.get_profile(&request.profile_id)?;
    // Unrelated workspace changes must not invalidate this profile's revision.
    if profile.revision != request.expected_revision {
        return Err(error("REVISION_CONFLICT"));
    }
    let host_status = check_host();
    let profile_check = || -> Result<(), SafeError> {
        // Independent profiles return before even opening the legacy file.
        let legacy = profile.read_legacy(&repository.metadata_directory().join("config.json"))?;
        // Share the exact selected-path resolver used by freeze_launch. No PATH fallback.
        configured_program(&profile, legacy.as_ref())?;
        configured_runner(&profile)?;
        // Validate explicit environment references but never return or log their values.
        // Runtime observer fields and terminal declarations belong to actual launch setup.
        build_environment(inherited, &EnvMap::new(), &profile, legacy.as_ref(), None)?;
        Ok(())
    };
    let issue = profile_check().err();
    let state = match issue.as_ref().map(|issue| issue.code.as_str()) {
        None => "available-unverified",
        Some("PROGRAM_TRUST_REQUIRED") => "configuration-required",
        Some(_) => "unavailable",
    };
    Ok(ProfileAvailability {
        profile_id: profile.id,
        availability: Availability {
            profile_revision: profile.revision,
            cli: profile.cli,
            state: state.into(),
            host_status,
            certified: false,
        },
        issue,
    })
}

/// Allocate and immediately release an empty PTY; no CLI or shell is launched.
/// Windows main.rs must initialize the verified bundled runtime before Tauri.
/// This proves allocation only, not WebView, keyboard, or CLI compatibility.
pub(crate) fn probe_host() -> HostStatus {
    match native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => {
            drop(pair);
            HostStatus::Available
        }
        Err(_) => HostStatus::Unavailable,
    }
}
