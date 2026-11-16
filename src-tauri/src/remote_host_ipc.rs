//! 所有命令复用原 document/run 原始字节鉴权，不接收批准或配置字段。
use crate::{
    cli::{native_runtime::NativeRuntime, types::SafeError},
    remote_host_controller::{HostControlReply, RemoteHostController},
};
use std::sync::Arc;
use tauri::{ipc::Request, State, Webview};

#[cfg(windows)]
pub(crate) async fn remote_registration_request(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    controller: State<'_, Arc<RemoteHostController>>,
    service: State<'_, Arc<crate::remote_host::RemoteHostService>>,
) -> Result<crate::remote_host_controller::startup::RegistrationReply, SafeError> {
    use tauri::Manager;
    let (binding, caller, admission, text) =
        runtime.admit_host_configuration(&webview, request.headers(), request.body())?;
    let frozen =
        crate::remote_host_controller::startup::configuration::FrozenRegistrationStart::parse(
            &text,
        )?;
    service
        .registration
        .request(
            binding,
            caller,
            admission,
            frozen,
            controller.inner().clone(),
            webview.app_handle().clone(),
            webview.window(),
        )
        .await
}
#[cfg(windows)]
pub(crate) async fn remote_registration_current(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    service: State<'_, Arc<crate::remote_host::RemoteHostService>>,
    cancel: bool,
) -> Result<crate::remote_host_controller::startup::RegistrationReply, SafeError> {
    let (binding, caller, admission) =
        runtime.admit_host_control(&webview, request.headers(), request.body())?;
    service
        .registration
        .current(&binding, &caller, &admission, cancel)
        .await
}

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

#[cfg(windows)]
pub(crate) fn remote_pairing_list(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<crate::remote_pairing_product::PairingList, SafeError> {
    let (binding, caller, admission) =
        runtime.admit_host_control(&webview, request.headers(), request.body())?;
    binding.check_admitted(&caller)?;
    admission.scope()?;
    let result = crate::remote_pairing_product::list()?;
    binding.check_admitted(&caller)?;
    admission.scope()?;
    Ok(result)
}
#[cfg(windows)]
pub(crate) fn remote_pairing_select(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    controller: State<'_, Arc<RemoteHostController>>,
) -> Result<crate::remote_host_configuration::ConfigurationReply, SafeError> {
    use crate::remote_pairing_product as product;
    let (binding, caller, admission, text) =
        runtime.admit_host_configuration(&webview, request.headers(), request.body())?;
    let store = product::open().map_err(product::store_error)?;
    store
        .with_locked_snapshot(|snapshot| {
            use crate::remote_native_pairing_store::StoreError;
            let record = product::selected(&snapshot, &text).map_err(|_| StoreError::Conflict)?;
            let frozen =
                product::freeze(&record, &admission).map_err(|_| StoreError::Unauthorized)?;
            let receipt = controller
                .install_candidate(&binding, caller, &admission, frozen)
                .map_err(|_| StoreError::Unauthorized)?;
            Ok(
                crate::remote_host_configuration::ConfigurationReply::Configured {
                    receipt: receipt.to_string(),
                },
            )
        })
        .map_err(product::store_error)
}
#[cfg(windows)]
pub(crate) async fn remote_pairing_forget(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<crate::remote_pairing_product::ForgetReply, SafeError> {
    use crate::{remote_native_pairing_store::Mutation, remote_pairing_product as product};
    use tauri::Manager;
    let (binding, caller, admission, text) =
        runtime.admit_host_configuration(&webview, request.headers(), request.body())?;
    let (request_id, sequence, selection) = product::forget_request(&text)?;
    let operation = product::forget_operation(request_id, sequence, &binding, &caller, &admission)?;
    let store = product::open().map_err(product::store_error)?;
    let snapshot = store.load().map_err(product::store_error)?;
    let record = product::selected(&snapshot, &selection)?;
    let mutation = Mutation::Forget {
        expected: snapshot.expected,
        device_id: record.device_id.clone(),
    };
    operation.show(|| {
        crate::remote_host_approval::NativeForgetApproval::show(
            webview.app_handle(),
            &webview.window(),
            &binding,
            &caller,
            &admission,
            store.target(),
            mutation,
            &record,
        )
    })?;
    loop {
        if binding.check_admitted(&caller).is_err() || admission.scope().is_err() {
            operation.cancel();
            return Err(crate::cli::profiles::error("FORBIDDEN"));
        }
        if let Some(reply) = operation.commit(&admission, &store)? {
            return Ok(reply);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
#[cfg(windows)]
pub(crate) fn remote_pairing_forget_cancel(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<(), SafeError> {
    use crate::remote_pairing_product as product;
    let (binding, caller, admission, text) =
        runtime.admit_host_configuration(&webview, request.headers(), request.body())?;
    let (id, sequence) = product::cancel_request(&text)?;
    match product::forget_operation(id, sequence, &binding, &caller, &admission) {
        Ok(operation) => operation.cancel(),
        Err(e) if e.code == "REMOTE_NATIVE_STORE_CANCELLED" => (),
        Err(e) => return Err(e),
    }
    Ok(())
}
