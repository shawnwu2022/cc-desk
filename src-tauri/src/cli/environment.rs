//! Per-launch OS environment. Never mutates the process or executes a shell.
#![allow(dead_code)]

use super::profiles::{error, EnvValue, Override, Profile};
use super::types::{CliKind, SafeError};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

pub(crate) type EnvMap = BTreeMap<OsString, OsString>;

/// Internal observer data, not an IPC type and deliberately not Debug/Serialize.
pub(crate) struct ObserverEnv {
    pub(crate) values: EnvMap,
}

/// Delegate Windows key equivalence to the standard library's OS environment
/// table. Constructing a Command does not execute it; no process is ever started.
pub(crate) fn same_name(left: &OsStr, right: &OsStr) -> bool {
    if left == right {
        return true;
    }
    #[cfg(windows)]
    {
        let mut table = std::process::Command::new("environment-key-table-only");
        table.env_clear().env(left, "").env(right, "");
        table.get_envs().count() == 1
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(crate) fn lookup<'a>(values: &'a EnvMap, name: &OsStr) -> Option<&'a OsString> {
    values
        .iter()
        .find(|(key, _)| same_name(key, name))
        .map(|(_, value)| value)
}

fn contains_nul(value: &OsStr) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().any(|unit| unit == 0)
    }
    #[cfg(not(windows))]
    {
        value.as_encoded_bytes().contains(&0)
    }
}

fn validate_name(name: &OsStr, inherited: bool) -> Result<(), SafeError> {
    // Windows may inherit its native per-drive current-directory entries.
    let drive_entry = cfg!(windows)
        && inherited
        && name.to_str().is_some_and(|value| {
            let bytes = value.as_bytes();
            bytes.len() == 3
                && bytes[0] == b'='
                && bytes[1].is_ascii_alphabetic()
                && bytes[2] == b':'
        });
    if name.is_empty()
        || contains_nul(name)
        || (name.as_encoded_bytes().contains(&b'=') && !drive_entry)
    {
        return Err(SafeError::invalid("environment.name"));
    }
    Ok(())
}

fn validated_layer(values: &EnvMap, inherited: bool) -> Result<EnvMap, SafeError> {
    let mut result = EnvMap::new();
    for (name, value) in values {
        validate_name(name, inherited)?;
        if contains_nul(value) {
            return Err(SafeError::invalid("environment.value"));
        }
        if let Some(previous) = lookup(&result, name) {
            if previous != value {
                return Err(SafeError::invalid("environment.aliasConflict"));
            }
        } else {
            result.insert(name.clone(), value.clone());
        }
    }
    Ok(result)
}

fn replace(values: &mut EnvMap, name: &OsStr, value: Option<&OsStr>) {
    values.retain(|key, _| !same_name(key, name));
    if let Some(value) = value {
        values.insert(name.to_owned(), value.to_owned());
    }
}

fn merge(values: &mut EnvMap, layer: &EnvMap) {
    for (name, value) in layer {
        replace(values, name, Some(value));
    }
}

fn permitted(name: &OsStr, names: &[&str]) -> bool {
    names
        .iter()
        .any(|allowed| same_name(name, OsStr::new(allowed)))
}

pub(crate) fn observer_enabled(profile: &Profile) -> bool {
    profile.cli == CliKind::Claude && matches!(profile.observer, Override::Set(true))
}

pub(crate) fn build_environment(
    inherited: &EnvMap,
    terminal: &EnvMap,
    profile: &Profile,
    legacy: Option<&Value>,
    observer: Option<&ObserverEnv>,
) -> Result<EnvMap, SafeError> {
    profile.validate()?;
    let original = validated_layer(inherited, true)?;
    let mut result = original.clone();
    let terminal = validated_layer(terminal, false)?;
    for name in terminal.keys() {
        if !permitted(
            name,
            &["TERM", "COLORTERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION"],
        ) {
            return Err(SafeError::invalid("terminal.environment"));
        }
    }
    merge(&mut result, &terminal);

    if profile.is_legacy_claude() {
        if let Some(values) = legacy.and_then(|value| value.get("claudeEnvVars")) {
            if !values.is_null() {
                let values = values.as_object().ok_or_else(|| error("LEGACY_INVALID"))?;
                let mut layer = EnvMap::new();
                for (name, value) in values {
                    let value = value.as_str().ok_or_else(|| error("LEGACY_INVALID"))?;
                    layer.insert(name.into(), value.into());
                }
                merge(&mut result, &validated_layer(&layer, false)?);
            }
        }
    }

    let mut seen: Vec<(&str, &Override<EnvValue>)> = Vec::new();
    for (name, change) in &profile.env {
        for (previous, previous_change) in &seen {
            if same_name(OsStr::new(name), OsStr::new(previous)) && previous_change != &change {
                return Err(SafeError::invalid("environment.aliasConflict"));
            }
        }
        seen.push((name, change));
        match change {
            Override::Inherit => {}
            Override::Unset => replace(&mut result, OsStr::new(name), None),
            Override::Set(EnvValue::Literal { value, .. }) => {
                replace(&mut result, OsStr::new(name), Some(OsStr::new(value)));
            }
            Override::Set(EnvValue::HostRef { name: source }) => {
                let value = lookup(&original, OsStr::new(source))
                    .ok_or_else(|| error("ENV_SOURCE_MISSING"))?;
                replace(&mut result, OsStr::new(name), Some(value));
            }
        }
    }

    if observer_enabled(profile) {
        if let Some(observer) = observer {
            let layer = validated_layer(&observer.values, false)?;
            for name in layer.keys() {
                if !permitted(name, &["CC_BOX_HOOK_PORT", "CC_BOX_SESSION_ID"]) {
                    return Err(SafeError::invalid("observer.environment"));
                }
            }
            merge(&mut result, &layer);
        }
    }
    Ok(result)
}
