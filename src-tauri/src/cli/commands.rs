//! Tauri boundary for Desk profiles and preflight. No caller-supplied filesystem paths.
use super::availability::{get_availability, parse_request, probe_host, ProfileAvailability};
use super::native_runtime::NativeRuntime;
use super::profile_service::{
    authorize_profile_window, list_profiles, parse_patch, patch_profile, ProfileList,
};
use super::profiles::error;
use super::run_registry::LaunchStatus;
use super::storage::WorkspaceRepository;
use super::types::SafeError;
use serde_json::Value;
use std::sync::Arc;
use tauri::ipc::Request;
use tauri::{State, Webview, WebviewWindow};

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

#[tauri::command]
pub(crate) async fn cli_get_availability(
    window: WebviewWindow,
    request: Value,
) -> Result<ProfileAvailability, SafeError> {
    let caller = window.label().to_string();
    authorize_profile_window(&caller)?;
    let request = parse_request(request)?;
    tauri::async_runtime::spawn_blocking(move || {
        let repository = WorkspaceRepository::production()?;
        let inherited = std::env::vars_os().collect();
        get_availability(&repository, &caller, &request, &inherited, probe_host)
    })
    .await
    .map_err(|_| error("AVAILABILITY_TASK_FAILED"))?
}

// Raw document-authenticated launch boundary. No wire owner fields.
#[tauri::command]
pub(crate) async fn cli_start(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<LaunchStatus, SafeError> {
    runtime.start(webview, request).await
}

#[tauri::command]
pub(crate) async fn cli_get_launch_status(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<LaunchStatus, SafeError> {
    runtime.status(&webview, &request)
}

#[tauri::command]
pub(crate) async fn cli_ack_output(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<(), SafeError> {
    runtime.ack_output(&webview, &request)
}

#[tauri::command]
pub(crate) async fn cli_input_begin(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<(), SafeError> {
    runtime.input_begin(&webview, &request)
}

#[tauri::command]
pub(crate) async fn cli_input_chunk(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<(), SafeError> {
    runtime.input_chunk(&webview, &request)
}

#[tauri::command]
pub(crate) async fn cli_input_commit(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<crate::terminal_input::InputWriteReceipt, SafeError> {
    runtime.input_commit(&webview, &request).await
}

#[tauri::command]
pub(crate) async fn cli_input_abort(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<(), SafeError> {
    runtime.input_abort(&webview, &request)
}

#[tauri::command]
pub(crate) async fn cli_input_protocol(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<crate::terminal_input::ProtocolWriteReceipt, SafeError> {
    runtime.input_protocol(&webview, &request).await
}

#[tauri::command]
pub(crate) async fn cli_stop(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<(), SafeError> {
    runtime.stop(&webview, &request)
}
