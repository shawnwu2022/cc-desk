//! 所有命令复用原 document/run 原始字节鉴权，不接收批准或配置字段。
use crate::{
    cli::{native_runtime::NativeRuntime, types::SafeError},
    remote_host_controller::{HostControlReply, RemoteHostController},
};
use std::sync::Arc;
use tauri::{ipc::Request, State, Webview};

#[cfg(windows)]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostPairingScope {
    session: String,
    run: String,
    generation: String,
}

#[cfg(windows)]
pub(crate) fn read_pairing_scope(
    binding: &crate::cli::document::DocumentBinding<crate::cli::launch_service::NativeRun>,
    caller: &crate::cli::snapshot::CallerIdentity,
    admission: &crate::run_supervisor::NativeRunAdmission,
) -> Result<HostPairingScope, SafeError> {
    binding.check_admitted(caller)?;
    // 只读取原生 producer 的原 scope；公开字段不能重新构造准入或批准。
    let scope = admission.scope()?;
    binding.check_admitted(caller)?;
    Ok(HostPairingScope {
        session: scope.session,
        run: scope.run,
        generation: scope.generation,
    })
}

#[cfg(windows)]
pub(crate) fn remote_host_pairing_scope(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<HostPairingScope, SafeError> {
    let (binding, caller, admission) =
        runtime.admit_host_control(&webview, request.headers(), request.body())?;
    read_pairing_scope(&binding, &caller, &admission)
}
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

#[cfg(windows)]
pub(crate) fn remote_host_configure(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    controller: State<'_, Arc<RemoteHostController>>,
) -> Result<crate::remote_host_configuration::ConfigurationReply, SafeError> {
    let (binding, caller, admission, text) =
        runtime.admit_host_configuration(&webview, request.headers(), request.body())?;
    let frozen = crate::remote_host_configuration::freeze(&text, &admission)?;
    let receipt = controller.install_candidate(&binding, caller, &admission, frozen)?;
    Ok(
        crate::remote_host_configuration::ConfigurationReply::Configured {
            receipt: receipt.to_string(),
        },
    )
}
#[cfg(windows)]
pub(crate) fn remote_host_discard_candidate(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    controller: State<'_, Arc<RemoteHostController>>,
) -> Result<(), SafeError> {
    let (binding, caller, admission, receipt) =
        runtime.admit_host_candidate_discard(&webview, request.headers(), request.body())?;
    controller.discard_candidate(&binding, &caller, &admission, receipt)
}
