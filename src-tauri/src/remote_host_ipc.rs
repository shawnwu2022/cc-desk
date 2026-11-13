//! 所有命令复用原 document/run 原始字节鉴权，不接收批准或配置字段。
use crate::{
    cli::{native_runtime::NativeRuntime, types::SafeError},
    remote_host_controller::{HostControlReply, RemoteHostController},
};
use std::sync::Arc;
use tauri::{ipc::Request, State, Webview};
pub(crate) fn remote_host_current(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    controller: State<'_, Arc<RemoteHostController>>,
) -> Result<HostControlReply, SafeError> {
    let (binding, caller, admission) =
        runtime.admit_host_control(&webview, request.headers(), request.body())?;
    controller.current(&binding, &caller, &admission)
}
pub(crate) fn remote_host_request(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    controller: State<'_, Arc<RemoteHostController>>,
) -> Result<HostControlReply, SafeError> {
    use tauri::Manager;
    let (binding, caller, admission) =
        runtime.admit_host_control(&webview, request.headers(), request.body())?;
    controller.request(
        &binding,
        &caller,
        &admission,
        webview.app_handle(),
        &webview.window(),
    )
}
pub(crate) fn remote_host_cancel(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    controller: State<'_, Arc<RemoteHostController>>,
) -> Result<HostControlReply, SafeError> {
    let (binding, caller, admission) =
        runtime.admit_host_control(&webview, request.headers(), request.body())?;
    controller.cancel(&binding, &caller, &admission)
}

#[cfg(windows)]
pub(crate) fn remote_host_start(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    controller: State<'_, Arc<RemoteHostController>>,
    service: State<'_, Arc<crate::remote_host::RemoteHostService>>,
) -> Result<HostControlReply, SafeError> {
    let (binding, caller, admission) =
        runtime.admit_host_control(&webview, request.headers(), request.body())?;
    controller.start_owned(
        &binding,
        &caller,
        admission,
        runtime.inner().clone(),
        service.inner().clone(),
    )
}
