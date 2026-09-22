//! Project paths are data, not workspace file locations. Only the trusted main WebView may call.
use super::profile_service::authorize_profile_window;
use super::profiles::error;
use super::project_service::{self, parse_revision, parse_text, ProjectList};
use super::storage::WorkspaceRepository;
use super::types::SafeError;
use serde_json::Value;
use std::path::Path;
use tauri::WebviewWindow;

#[tauri::command]
pub(crate) async fn cli_list_projects(window: WebviewWindow) -> Result<ProjectList, SafeError> {
    let caller = window.label().to_string();
    authorize_profile_window(&caller)?;
    tauri::async_runtime::spawn_blocking(move || {
        project_service::list_projects(&WorkspaceRepository::production()?, &caller)
    })
    .await
    .map_err(|_| error("WORKSPACE_TASK_FAILED"))?
}

#[tauri::command]
pub(crate) async fn cli_register_project(
    window: WebviewWindow,
    selected_path: Value,
) -> Result<ProjectList, SafeError> {
    let caller = window.label().to_string();
    authorize_profile_window(&caller)?;
    let selected_path = parse_text(&selected_path, "selectedPath")?;
    tauri::async_runtime::spawn_blocking(move || {
        project_service::register(
            &WorkspaceRepository::production()?,
            &caller,
            Path::new(&selected_path),
        )
    })
    .await
    .map_err(|_| error("WORKSPACE_TASK_FAILED"))?
}

#[tauri::command]
pub(crate) async fn cli_patch_project(
    window: WebviewWindow,
    expected_revision: Value,
    project_id: Value,
    changes: Value,
) -> Result<ProjectList, SafeError> {
    let caller = window.label().to_string();
    authorize_profile_window(&caller)?;
    let revision = parse_revision(&expected_revision)?;
    let project_id = parse_text(&project_id, "projectId")?;
    tauri::async_runtime::spawn_blocking(move || {
        project_service::patch(
            &WorkspaceRepository::production()?,
            &caller,
            revision,
            &project_id,
            changes,
        )
    })
    .await
    .map_err(|_| error("WORKSPACE_TASK_FAILED"))?
}

#[tauri::command]
pub(crate) async fn cli_remove_project(
    window: WebviewWindow,
    expected_revision: Value,
    project_id: Value,
) -> Result<ProjectList, SafeError> {
    let caller = window.label().to_string();
    authorize_profile_window(&caller)?;
    let revision = parse_revision(&expected_revision)?;
    let project_id = parse_text(&project_id, "projectId")?;
    tauri::async_runtime::spawn_blocking(move || {
        project_service::remove(
            &WorkspaceRepository::production()?,
            &caller,
            revision,
            &project_id,
        )
    })
    .await
    .map_err(|_| error("WORKSPACE_TASK_FAILED"))?
}
