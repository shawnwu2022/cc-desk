//! Public projection DTOs. No path, owner, environment or native write request is admitted.
use crate::cli::types::{CliKind, SafeError, WireU64};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub(crate) enum ScopeTarget {
    Profile { profile_id: String, expected_profile_revision: WireU64, project_id: Option<String> },
    Run { run_id: String, generation: u32 },
}
impl ScopeTarget {
    pub(crate) fn validate(&self) -> Result<(), SafeError> { Ok(()) }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SourceBasis { ConfiguredProfile, LaunchEnvironment }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceRef {
    pub(crate) scope_id: String,
    pub(crate) instance_id: String,
    pub(crate) cli: CliKind,
    pub(crate) source_root_key: String,
    pub(crate) identity_epoch: WireU64,
    pub(crate) profile_id: String,
    pub(crate) profile_revision: WireU64,
    pub(crate) target: ScopeTarget,
    pub(crate) basis: SourceBasis,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ResourceKind { History, Messages, Search, Config, Mcp, Skills, Agents, Plugins, Instructions }
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadRequest {
    pub(crate) source: SourceRef,
    pub(crate) resource_kind: ResourceKind,
    pub(crate) request_epoch: WireU64,
    pub(crate) query: Option<String>,
    pub(crate) session_id: Option<String>,
    #[serde(default = "default_limit")]
    pub(crate) limit: u16,
    #[serde(default)]
    pub(crate) offset: u32,
}
fn default_limit() -> u16 { 100 }
impl ReadRequest {
    pub(crate) fn validate(&self) -> Result<(), SafeError> { Ok(()) }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProjectionState { Ready, Unavailable }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectionResult {
    pub(crate) source: SourceRef,
    pub(crate) resource_kind: ResourceKind,
    pub(crate) request_epoch: WireU64,
    pub(crate) observed_at: WireU64,
    pub(crate) state: ProjectionState,
    pub(crate) reason: Option<String>,
    pub(crate) items: Vec<ResourceItem>,
    pub(crate) has_more: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub(crate) enum ResourceItem {
    Session { session_key: String, native_session_id: String, title: String, truncated: bool, cwd: Option<String>, updated_at: Option<String> },
    Message { session_key: String, native_session_id: String, role: String, text: String, truncated: bool },
    Setting { name: String, value: String, origin: String },
    Mcp { name: String, transport: String, origin: String },
    Skill { name: String, description: String, origin: String },
    Agent { name: String, description: String, model: Option<String>, origin: String },
    Plugin { id: String, name: String, version: Option<String>, enabled: Option<bool>, installed: Option<bool>, origin: String },
    Document { name: String, text: String, truncated: bool, origin: String },
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_target_cannot_become_a_root_reference() {
        for value in [
            serde_json::json!({"kind":"profile","profileId":"","expectedProfileRevision":"0","projectId":null}),
            serde_json::json!({"kind":"run","runId":"r","generation":0}),
        ] {
            let target: ScopeTarget = serde_json::from_value(value).unwrap();
            assert!(target.validate().is_err());
        }
    }
    #[test]
    fn frontend_cannot_inject_paths_owners_or_numbers_for_u64() {
        for value in [
            serde_json::json!({"kind":"profile","profileId":"p","expectedProfileRevision":"0","projectId":null,"root":"/tmp"}),
            serde_json::json!({"kind":"profile","profileId":"p","expectedProfileRevision":0,"projectId":null}),
            serde_json::json!({"kind":"run","runId":"r","generation":1,"owner":"main"}),
        ] { assert!(serde_json::from_value::<ScopeTarget>(value).is_err()); }
    }
}
