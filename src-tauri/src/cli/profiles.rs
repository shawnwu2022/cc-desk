//! Desk-owned launch preferences. Native credentials are never imported.
#![allow(dead_code)]

use super::types::{CliKind, SafeError, WireU64};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "value", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Override<T> {
    #[default]
    Inherit,
    Set(T),
    Unset,
}

pub(crate) fn resolve_override<T>(_value: Override<T>, legacy: Option<T>) -> Option<T> {
    // Initial inheritance-only implementation: explicit set/unset remain RED.
    legacy
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum EnvValue {
    Literal {
        value: String,
        #[serde(rename = "nonSecret")]
        non_secret: bool,
    },
    HostRef { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Launcher {
    #[default]
    Native,
    Shell { program: String, dialect: Dialect },
    Shim { runner: String, dialect: Dialect },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Dialect {
    Bash,
    PowerShell,
    Cmd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Profile {
    pub(crate) id: String,
    pub(crate) revision: WireU64,
    pub(crate) cli: CliKind,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) launcher: Launcher,
    #[serde(default)]
    pub(crate) program_path: Override<String>,
    #[serde(default)]
    pub(crate) default_args: Override<Vec<String>>,
    #[serde(default)]
    pub(crate) skip_permissions: Override<bool>,
    #[serde(default)]
    pub(crate) observer: Override<bool>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, Override<EnvValue>>,
}

impl Profile {
    pub(crate) fn new(id: &str, cli: CliKind) -> Self {
        Self {
            id: id.into(),
            revision: WireU64::parse("0").expect("canonical zero"),
            cli,
            name: id.into(),
            launcher: Launcher::Native,
            program_path: Override::Inherit,
            default_args: Override::Inherit,
            skip_permissions: Override::Inherit,
            observer: Override::Inherit,
            env: BTreeMap::new(),
        }
    }

    pub(crate) fn is_legacy_claude(&self) -> bool {
        self.id == "legacyClaude" && self.cli == CliKind::Claude
    }

    pub(crate) fn validate(&self) -> Result<(), SafeError> {
        if self.id.is_empty()
            || self.id.len() > 128
            || !self.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            || (self.id == "legacyClaude" && self.cli != CliKind::Claude)
        {
            return Err(SafeError::invalid("profileId"));
        }
        validate_text(&self.name, "name", false)?;
        match &self.launcher {
            Launcher::Native => {}
            Launcher::Shell { program, .. } => validate_text(program, "launcher", false)?,
            Launcher::Shim { runner, .. } => validate_text(runner, "launcher", false)?,
        }
        if let Override::Set(path) = &self.program_path {
            validate_text(path, "programPath", false)?;
        }
        if let Override::Set(args) = &self.default_args {
            for arg in args {
                validate_text(arg, "defaultArgs", true)?;
            }
        }
        if self.cli != CliKind::Claude && matches!(self.skip_permissions, Override::Set(_)) {
            return Err(SafeError::invalid("skipPermissions"));
        }
        for (key, value) in &self.env {
            validate_env_name(key)?;
            match value {
                Override::Set(EnvValue::Literal { value, non_secret }) => {
                    if !non_secret {
                        return Err(SafeError::invalid("env.nonSecret"));
                    }
                    validate_text(value, "env.value", true)?;
                }
                Override::Set(EnvValue::HostRef { name }) => validate_env_name(name)?,
                Override::Inherit | Override::Unset => {}
            }
        }
        Ok(())
    }

    pub(crate) fn resolve_skip_permissions(&self, legacy: Option<&Value>) -> Option<bool> {
        if self.cli != CliKind::Claude {
            return None;
        }
        let inherited = if self.is_legacy_claude() {
            legacy.and_then(|v| v.get("defaultSkipPermissions")).and_then(Value::as_bool)
        } else {
            None
        };
        resolve_override(self.skip_permissions.clone(), inherited)
    }

    /// Backend-only read. A Codex/new Claude profile does not even open this file.
    pub(crate) fn read_legacy(&self, path: &Path) -> Result<Option<Value>, SafeError> {
        if !self.is_legacy_claude() {
            return Ok(None);
        }
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(error("LEGACY_READ_FAILED")),
        };
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error("LEGACY_READ_FAILED"))?;
        if bytes.len() > 1024 * 1024 {
            return Err(error("LEGACY_TOO_LARGE"));
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| error("LEGACY_INVALID"))?;
        if !value.is_object() {
            return Err(error("LEGACY_INVALID"));
        }
        Ok(Some(value))
    }

    /// Values stay backend-only. None is an explicit deletion, not inheritance.
    pub(crate) fn resolve_env(
        &self,
        legacy: Option<&Value>,
        host: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, Option<String>>, SafeError> {
        self.validate()?;
        let mut result = BTreeMap::new();
        if self.is_legacy_claude() {
            if let Some(values) = legacy.and_then(|v| v.get("claudeEnvVars")) {
                if !values.is_null() {
                    let values = values.as_object().ok_or_else(|| error("LEGACY_INVALID"))?;
                    for (key, value) in values {
                        validate_env_name(key)?;
                        let value = value.as_str().ok_or_else(|| error("LEGACY_INVALID"))?;
                        validate_text(value, "env.value", true)?;
                        result.insert(key.clone(), Some(value.to_string()));
                    }
                }
            }
        }
        for (key, value) in &self.env {
            match value {
                Override::Inherit => {}
                Override::Unset => {
                    result.insert(key.clone(), None);
                }
                Override::Set(EnvValue::Literal { value, .. }) => {
                    result.insert(key.clone(), Some(value.clone()));
                }
                Override::Set(EnvValue::HostRef { name }) => {
                    let value = host.get(name).ok_or_else(|| error("ENV_SOURCE_MISSING"))?;
                    result.insert(key.clone(), Some(value.clone()));
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn patched(&self, changes: &Map<String, Value>) -> Result<Self, SafeError> {
        let mut value = serde_json::to_value(self).map_err(|_| error("SERIALIZE_FAILED"))?;
        let object = value.as_object_mut().ok_or_else(|| error("SERIALIZE_FAILED"))?;
        for (key, next) in changes {
            if !matches!(key.as_str(), "name" | "launcher" | "programPath" | "defaultArgs" | "skipPermissions" | "observer" | "env") || next.is_null() {
                return Err(SafeError::invalid("changes"));
            }
            if key == "env" {
                let patch = next.as_object().ok_or_else(|| SafeError::invalid("env"))?;
                let target = object.get_mut("env").and_then(Value::as_object_mut).ok_or_else(|| SafeError::invalid("env"))?;
                for (name, entry) in patch {
                    target.insert(name.clone(), entry.clone());
                }
            } else {
                object.insert(key.clone(), next.clone());
            }
        }
        let profile: Self = serde_json::from_value(value).map_err(|_| SafeError::invalid("changes"))?;
        profile.validate()?;
        Ok(profile)
    }
}

fn validate_text(value: &str, field: &str, allow_empty: bool) -> Result<(), SafeError> {
    if value.contains('\0') || (!allow_empty && value.is_empty()) {
        return Err(SafeError::invalid(field));
    }
    Ok(())
}

fn validate_env_name(value: &str) -> Result<(), SafeError> {
    if value.is_empty() || value.contains(['=', '\0']) {
        return Err(SafeError::invalid("env.name"));
    }
    Ok(())
}

pub(crate) fn error(code: &str) -> SafeError {
    SafeError {
        code: code.to_string(),
        field: None,
        index: None,
        retryable: matches!(code, "REVISION_CONFLICT" | "STORAGE_BUSY"),
    }
}
