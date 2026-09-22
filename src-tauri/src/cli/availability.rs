//! Per-profile preflight contract. Behavioral implementation follows observed RED.
#![allow(dead_code)]

use super::environment::EnvMap;
use super::profiles::error;
use super::snapshot::{Availability, HostStatus};
use super::storage::WorkspaceRepository;
use super::types::{SafeError, WireU64};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AvailabilityRequest {
    pub(crate) profile_id: String,
    pub(crate) expected_revision: WireU64,
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

pub(crate) fn parse_request(_value: Value) -> Result<AvailabilityRequest, SafeError> {
    Err(error("AVAILABILITY_NOT_IMPLEMENTED"))
}

pub(crate) fn get_availability(
    _repository: &WorkspaceRepository,
    _caller: &str,
    _request: &AvailabilityRequest,
    _inherited: &EnvMap,
    _check_host: impl FnOnce() -> HostStatus,
) -> Result<ProfileAvailability, SafeError> {
    Err(error("AVAILABILITY_NOT_IMPLEMENTED"))
}

pub(crate) fn probe_host() -> HostStatus {
    HostStatus::NotChecked
}
