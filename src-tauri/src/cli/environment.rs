//! Backend-only child environment construction; implementation follows behavioral RED.
#![allow(dead_code)]
use super::profiles::Profile;
use super::types::SafeError;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;

pub(crate) type EnvMap = BTreeMap<OsString, OsString>;

pub(crate) struct ObserverEnv {
    pub(crate) values: EnvMap,
}

pub(crate) fn build_environment(
    inherited: &EnvMap,
    _terminal: &EnvMap,
    _profile: &Profile,
    _legacy: Option<&Value>,
    _observer: Option<&ObserverEnv>,
) -> Result<EnvMap, SafeError> {
    Ok(inherited.clone())
}
