//! Backend-only immutable launch inputs. Behavioral implementation follows RED.
#![allow(dead_code)]
use super::environment::{EnvMap, ObserverEnv};
use super::profiles::{error, Profile};
use super::types::{LaunchRequest, SafeError, WireU64};
use serde::Serialize;
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CallerIdentity {
    pub(crate) instance_id: String,
    pub(crate) window_label: String,
    pub(crate) webview_epoch: WireU64,
}

pub(crate) struct FreezeContext<'a> {
    pub(crate) inherited: &'a EnvMap,
    pub(crate) terminal: &'a EnvMap,
    pub(crate) legacy: Option<&'a Value>,
    pub(crate) observer: Option<&'a ObserverEnv>,
}

pub(crate) struct LaunchSnapshot {
    profile: Profile,
    owner: CallerIdentity,
    environment: EnvMap,
    program: PathBuf,
    runner: Option<PathBuf>,
    raw_args: Option<Vec<OsString>>,
    default_args: Vec<OsString>,
    extra_args: Vec<OsString>,
}

impl std::fmt::Debug for LaunchSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LaunchSnapshot(<redacted>)")
    }
}

impl LaunchSnapshot {
    pub(crate) fn environment(&self) -> &EnvMap { &self.environment }
    pub(crate) fn program(&self) -> &Path { &self.program }
    pub(crate) fn runner(&self) -> Option<&Path> { self.runner.as_deref() }
    pub(crate) fn profile_revision(&self) -> WireU64 { self.profile.revision }
    pub(crate) fn owner(&self) -> &CallerIdentity { &self.owner }
    pub(crate) fn raw_args(&self) -> Option<&[OsString]> { self.raw_args.as_deref() }
    pub(crate) fn default_args(&self) -> &[OsString] { &self.default_args }
    pub(crate) fn extra_args(&self) -> &[OsString] { &self.extra_args }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HostStatus { NotChecked, Available, Unavailable }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Availability {
    pub(crate) profile_revision: WireU64,
    pub(crate) state: String,
    pub(crate) host_status: HostStatus,
    pub(crate) certified: bool,
}

pub(crate) fn freeze_launch(
    _request: &LaunchRequest,
    _profile: &Profile,
    _caller: &CallerIdentity,
    _context: &FreezeContext<'_>,
) -> Result<LaunchSnapshot, SafeError> {
    Err(error("SNAPSHOT_NOT_IMPLEMENTED"))
}

pub(crate) fn availability(snapshot: &LaunchSnapshot, host_status: HostStatus) -> Availability {
    Availability { profile_revision: snapshot.profile_revision(), state: "unavailable".into(), host_status, certified: false }
}

pub(crate) fn discover_candidates(
    _name: &str,
    _environment: &EnvMap,
    _excluded_roots: &[PathBuf],
) -> Result<Vec<PathBuf>, SafeError> {
    Ok(Vec::new())
}
