use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionUiRecord {
    pub runtime: String,
    pub cli: String,
    pub project_path: String,
    pub adapter_session_id: String,
    pub native_session_id: Option<String>,
    pub title: String,
    pub last_activity_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened_at: Option<u64>,
}

/// Called inside the existing locked, incremental projects.json transaction.
pub(crate) fn merge_record(
    existing: Option<&SessionUiRecord>,
    mut incoming: SessionUiRecord,
    open_only: bool,
) -> SessionUiRecord {
    if let Some(existing) = existing {
        if existing.runtime == incoming.runtime
            && existing.cli == incoming.cli
            && existing.project_path == incoming.project_path
            && existing.adapter_session_id == incoming.adapter_session_id
            && existing.native_session_id == incoming.native_session_id
        {
            incoming.last_opened_at = existing.last_opened_at.max(incoming.last_opened_at);
            if open_only {
                incoming.title.clone_from(&existing.title);
                incoming.last_activity_at = existing.last_activity_at;
            }
        }
    }
    incoming
}
