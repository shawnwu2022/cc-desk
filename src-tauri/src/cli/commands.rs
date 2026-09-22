//! Tauri boundary for Desk profile storage. No caller-supplied filesystem paths.
use super::profile_service::{
    authorize_profile_window, list_profiles, parse_patch, patch_profile, ProfileList,
};
use super::profiles::error;
use super::storage::WorkspaceRepository;
use super::types::SafeError;
use serde_json::Value;
use tauri::WebviewWindow;

#[tauri::command]
pub(crate) async fn cli_list_profiles(window: WebviewWindow) -> Result<ProfileList, SafeError> {
    let caller = window.label().to_string();
    authorize_profile_window(&caller)?;
    tauri::async_runtime::spawn_blocking(move || {
        let repository = WorkspaceRepository::production()?;
        list_profiles(&repository, &caller)
    })
    .await
    .map_err(|_| error("WORKSPACE_TASK_FAILED"))?
}

#[tauri::command]
pub(crate) async fn cli_patch_profile(
    window: WebviewWindow,
    expected_revision: Value,
    patch: Value,
) -> Result<ProfileList, SafeError> {
    let caller = window.label().to_string();
    authorize_profile_window(&caller)?;
    let (revision, patch) = parse_patch(&expected_revision, patch)?;
    tauri::async_runtime::spawn_blocking(move || {
        let repository = WorkspaceRepository::production()?;
        patch_profile(&repository, &caller, revision, patch)
    })
    .await
    .map_err(|_| error("WORKSPACE_TASK_FAILED"))?
}
