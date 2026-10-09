#![allow(dead_code)]

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

const INVALID_REQUEST: &str = "INVALID_REQUEST";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SafeError {
    pub(crate) code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) index: Option<usize>,
    #[serde(default)]
    pub(crate) retryable: bool,
}

impl SafeError {
    pub(crate) fn invalid(field: impl Into<String>) -> Self {
        Self {
            code: INVALID_REQUEST.to_string(),
            field: Some(field.into()),
            index: None,
            retryable: false,
        }
    }
}

impl fmt::Display for SafeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.code)?;
        if let Some(field) = &self.field {
            write!(formatter, ":{field}")?;
        }
        if let Some(index) = self.index {
            write!(formatter, ":index={index}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SafeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct WireU64(u64);

impl WireU64 {
    pub(crate) fn parse(value: &str) -> Result<Self, SafeError> {
        let canonical = !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && (value == "0" || !value.starts_with('0'));
        if !canonical {
            return Err(SafeError::invalid("u64"));
        }

        value
            .parse::<u64>()
            .map(Self)
            .map_err(|_| SafeError::invalid("u64"))
    }

    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WireU64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl Serialize for WireU64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for WireU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct WireBytes(Vec<u8>);

impl WireBytes {
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CliKind {
    Claude,
    Codex,
    Shell,
}

impl CliKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Shell => "shell",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NativeCliKind {
    Claude,
    Codex,
}

impl NativeCliKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ResumeScope {
    CurrentProject,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum LaunchAction {
    New,
    ResumePicker {
        scope: ResumeScope,
    },
    ResumeId {
        #[serde(rename = "nativeSessionId")]
        native_session_id: String,
    },
    Raw {
        argv: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LaunchRequest {
    pub(crate) request_id: String,
    pub(crate) tab_id: String,
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) profile_id: String,
    pub(crate) expected_profile_revision: WireU64,
    pub(crate) cli: CliKind,
    pub(crate) launch_cwd: String,
    pub(crate) action: LaunchAction,
    pub(crate) extra_args: Vec<String>,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

impl LaunchRequest {
    pub(crate) fn validate(&self) -> Result<(), SafeError> {
        validate_required("requestId", &self.request_id)?;
        validate_required("tabId", &self.tab_id)?;
        validate_required("runId", &self.run_id)?;
        validate_required("profileId", &self.profile_id)?;
        validate_required("launchCwd", &self.launch_cwd)?;
        validate_arguments("extraArgs", &self.extra_args)?;

        if self.cols == 0 {
            return Err(SafeError::invalid("cols"));
        }
        if self.rows == 0 {
            return Err(SafeError::invalid("rows"));
        }

        match &self.action {
            LaunchAction::New | LaunchAction::ResumePicker { .. } => {}
            LaunchAction::ResumeId { native_session_id } => {
                validate_required("action.nativeSessionId", native_session_id)?;
            }
            LaunchAction::Raw { argv } => {
                validate_arguments("action.argv", argv)?;
                if !self.extra_args.is_empty() {
                    return Err(SafeError::invalid("extraArgs"));
                }
            }
        }

        Ok(())
    }
}

fn validate_required(field: &str, value: &str) -> Result<(), SafeError> {
    if value.is_empty() || value.contains('\0') {
        return Err(SafeError::invalid(field));
    }
    Ok(())
}

fn validate_arguments(field: &str, values: &[String]) -> Result<(), SafeError> {
    for (index, value) in values.iter().enumerate() {
        if value.contains('\0') {
            return Err(SafeError::invalid(format!("{field}[{index}]")));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ResolutionSource {
    Launch,
    OfficialEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub(crate) enum Resolution<T> {
    Known { value: T, source: ResolutionSource },
    Unknown { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunPublicIdentity {
    run_id: String,
    generation: u32,
    cli: CliKind,
    launch_cwd: String,
    effective_cwd: Resolution<String>,
    effective_config_root: Resolution<String>,
    native_session_id: Resolution<String>,
}

impl RunPublicIdentity {
    /// Launch intent cannot verify the CLI's eventual directory, root or session.
    pub(crate) fn unverified_launch(request: &LaunchRequest) -> Self {
        let unknown = || Resolution::Unknown {
            reason: "NATIVE_RUNTIME_NOT_OBSERVED".into(),
        };
        Self {
            run_id: request.run_id.clone(),
            generation: request.generation,
            cli: request.cli,
            launch_cwd: request.launch_cwd.clone(),
            effective_cwd: unknown(),
            effective_config_root: unknown(),
            native_session_id: unknown(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeSessionRef {
    pub(crate) host_id: String,
    pub(crate) cli: NativeCliKind,
    pub(crate) source_root_key: String,
    pub(crate) native_session_id: String,
}

impl NativeSessionRef {
    pub(crate) fn stable_key(&self) -> String {
        serde_json::to_string(&[
            self.host_id.as_str(),
            self.cli.as_str(),
            self.source_root_key.as_str(),
            self.native_session_id.as_str(),
        ])
        .expect("string arrays are always serializable")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProcessState {
    Starting,
    Running,
    Exited,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutputState {
    Open,
    Draining,
    Drained,
    Incomplete,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ActivityState {
    Unknown,
    Working,
    Waiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ObservationState {
    Off,
    Connecting,
    Active,
    Unavailable,
}
