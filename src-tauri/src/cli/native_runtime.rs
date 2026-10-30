//! Application-owned document/launch composition. Never deserialized authority.
use super::document::{native::build_main, DocumentBinding, DOCUMENT_HEADER};
use super::launch_service::{LaunchService, NativeRun, RunAccess};
use super::output_route::{parse_channel, CHANNEL_HEADER};
use super::profiles::error;
use super::run_registry::{LaunchStatus, RunKey};
use super::snapshot::CallerIdentity;
use super::storage::WorkspaceRepository;
use super::types::SafeError;
use crate::run_supervisor::NativeRunSupervisor;
use crate::terminal_input::{
    write_host_frame, InputAbortRequest, InputBeginRequest, InputChunkRequest, InputCommitRequest,
    InputStager, InputWriteReceipt, ProtocolInputRequest, ProtocolWriteReceipt,
};
use crate::terminal_transport::{OutputAck, OutputFrame, TerminalTransports};
use parking_lot::Mutex;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};
use tauri::utils::config::{Config, FrontendDist, WindowConfig};
use tauri::{Manager, Runtime, Url, Webview, WebviewUrl, WebviewWindow};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopQuery {
    request_id: String,
    run_id: String,
    generation: u32,
}

pub(crate) fn decode_stop_query(body: &InvokeBody) -> Result<(String, RunKey), SafeError> {
    let InvokeBody::Raw(bytes) = body else {
        return Err(error("RAW_BODY_REQUIRED"));
    };
    if bytes.len() > 1024 {
        return Err(error("REQUEST_TOO_LARGE"));
    }
    let query: StopQuery = serde_json::from_slice(bytes).map_err(|_| error("INVALID_REQUEST"))?;
    if query.generation == 0 {
        return Err(SafeError::invalid("generation"));
    }
    Ok((
        query.request_id,
        RunKey {
            run_id: query.run_id,
            generation: query.generation,
        },
    ))
}

pub(crate) struct NativeRuntime {
    service: Arc<LaunchService>,
    projections: Arc<super::native_projection::service::ProjectionService>,
    binding: Mutex<Option<Arc<DocumentBinding<NativeRun>>>>,
    initialized: AtomicBool,
    transports: Arc<TerminalTransports>,
    inputs: Arc<InputStager>,
    supervisor: Option<Arc<NativeRunSupervisor>>,
}

pub(crate) async fn resize_native_run(
    access: RunAccess,
    size: portable_pty::PtySize,
) -> Result<(), SafeError> {
    // The shared mutation owner may be held by a blocking native write. Never
    // consume an async worker while waiting; retain this exact access in the
    // original blocking task, including if its observer is dropped.
    tauri::async_runtime::spawn_blocking(move || access.resize(size))
        .await
        .map_err(|_| error("RESIZE_TASK_FAILED"))?
}

impl NativeRuntime {
    #[allow(dead_code)] // Staged native WebView harnesses construct isolated runtimes.
    pub(crate) fn new(service: Arc<LaunchService>) -> Self {
        Self::with_components(service, Arc::new(TerminalTransports::new()), None)
    }

    fn with_components(
        service: Arc<LaunchService>,
        transports: Arc<TerminalTransports>,
        supervisor: Option<Arc<NativeRunSupervisor>>,
    ) -> Self {
        Self {
            projections: Arc::new(super::native_projection::service::ProjectionService::new(
                service.clone(),
            )),
            service,
            binding: Mutex::new(None),
            initialized: AtomicBool::new(false),
            transports,
            inputs: Arc::new(InputStager::new()),
            supervisor,
        }
    }

    pub(crate) fn production(
        admission: crate::version_history::maintenance::AdmissionGate,
    ) -> Result<Self, SafeError> {
        let transports = Arc::new(TerminalTransports::new());
        let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
        let mut service = LaunchService::new(
            WorkspaceRepository::open_admitted(
                dirs::home_dir()
                    .ok_or_else(|| error("HOME_UNAVAILABLE"))?
                    .join(".cc-box")
                    .join("cli-workspace.v1.json"),
                admission,
            )?,
            None,
            Some(supervisor.clone()),
        );
        if let Some(observer) = crate::hook_server::observer_host() {
            service = service.with_observer(observer);
        }
        Ok(Self::with_components(
            Arc::new(service),
            transports,
            Some(supervisor),
        ))
    }
    pub(crate) fn initialize_main<T: Runtime, M: Manager<T>>(
        &self,
        manager: &M,
        window: &WindowConfig,
    ) -> Result<WebviewWindow<T>, SafeError> {
        if self
            .initialized
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(error("DOCUMENT_WINDOW_UNAVAILABLE"));
        }
        let url = expected_main_url(manager.config(), window, tauri::is_dev())?;
        let bound = build_main(manager, window, url, self.service.registry().clone())?;
        *self.binding.lock() = Some(Arc::new(bound.binding));
        Ok(bound.window)
    }
    pub(crate) fn binding(&self) -> Result<Arc<DocumentBinding<NativeRun>>, SafeError> {
        self.binding
            .lock()
            .clone()
            .ok_or_else(|| error("FORBIDDEN"))
    }
    pub(crate) async fn start<T: Runtime>(
        &self,
        webview: Webview<T>,
        request: Request<'_>,
    ) -> Result<LaunchStatus, SafeError> {
        let binding = self.binding()?;
        let (caller, launch) = binding.start_native(&webview, &request)?;
        if let Some(status) = self.service.registry().existing(&caller, &launch)? {
            return Ok(status);
        }
        let admission = self.service.admit_start()?;
        let descriptor = parse_channel(request.headers());
        let proof = request.headers()[DOCUMENT_HEADER].clone();
        let service = self.service.clone();
        // Keep only validated routing metadata, not the raw body or arbitrary
        // request headers. A replay never evaluates the connect closure.
        tauri::async_runtime::spawn_blocking(move || {
            service.start_admitted(admission, &caller, &launch, |_| {
                let id = descriptor?;
                let mut headers = HeaderMap::new();
                headers.insert(DOCUMENT_HEADER, proof);
                headers.insert(
                    CHANNEL_HEADER,
                    format!("__CHANNEL__:{id}")
                        .parse()
                        .map_err(|_| SafeError::invalid("outputChannel"))?,
                );
                binding.channel_native::<_, OutputFrame>(&webview, &headers)
            })
        })
        .await
        .map_err(|_| error("LAUNCH_STATE_UNKNOWN"))?
    }
    pub(crate) fn status<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<LaunchStatus, SafeError> {
        self.binding()?.query_native(webview, request)
    }
    pub(crate) async fn prepare_opencode_session<T: Runtime>(
        &self,
        webview: Webview<T>,
        request: Request<'_>,
    ) -> Result<String, SafeError> {
        let (caller, launch) = self.binding()?.start_native(&webview, &request)?;
        let service = self.service.clone();
        tauri::async_runtime::spawn_blocking(move || {
            service.prepare_opencode_session(&caller, &launch)
        })
        .await
        .map_err(|_| error("OPENCODE_SESSION_OUTCOME_UNKNOWN"))?
    }
    pub(crate) fn cancel_launch<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<LaunchStatus, SafeError> {
        // Identical document admission and strict full-request decoding as start;
        // cancellation never reads a new profile or rebuilds a launch snapshot.
        let (caller, launch) = self.binding()?.start_native(webview, request)?;
        self.service.registry().cancel(&caller, &launch)
    }
    pub(crate) fn observer_status<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<super::observer_status::ObserverStatus, SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let query = decode_projection(request.body(), 4096)?;
        self.service.observer_status(&caller, &query)
    }
    pub(crate) async fn rename_session<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<super::native_rename::RenameReceipt, SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let query: super::native_rename::RenameRequest = decode_projection(request.body(), 32768)?;
        query.validate()?;
        let service = self.projections.clone();
        let admitted = caller.clone();
        let result = tauri::async_runtime::spawn_blocking(move || service.rename(&admitted, query))
            .await
            .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))??;
        self.service.registry().check_caller(&caller)?;
        Ok(result)
    }
    pub(crate) async fn session_index_maintenance<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<crate::session_index_maintenance::MaintenanceStatus, SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let query: crate::session_index_maintenance::MaintenanceRequest =
            decode_projection(request.body(), 4096)?;
        let registry = self.service.registry().clone();
        tauri::async_runtime::spawn_blocking(move || {
            registry.check_caller(&caller)?;
            if query.action == "begin" {
                crate::session_index_maintenance::production()
                    .release_revoked(|owner| registry.check_caller(owner).is_ok());
            }
            crate::session_index_maintenance::production().apply_checked(
                &caller,
                &query.id,
                &query.action,
                || registry.check_caller(&caller),
            )
        })
        .await
        .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?
    }
    pub(crate) fn ack_output<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<(), SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let ack: OutputAck = decode_projection(request.body(), 1024)?;
        self.transports.ack(&caller, &ack)?;
        Ok(())
    }

    /// 恢复 CLI 自身归档的会话。归档状态属于 CLI，Desk 只把 `codex unarchive`
    /// 这条意图交给同一套 profile/环境授权去执行；不建立 tab、不注册 run。
    pub(crate) async fn unarchive<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<(), SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let query: super::types::UnarchiveRequest = decode_projection(request.body(), 4096)?;
        query.validate()?;
        let service = self.service.clone();
        let admitted = caller.clone();
        tauri::async_runtime::spawn_blocking(move || {
            service.run_command(&admitted, &query.to_launch_request())
        })
        .await
        .map_err(|_| error("LAUNCH_STATE_UNKNOWN"))??;
        self.service.registry().check_caller(&caller)?;
        Ok(())
    }

    pub(crate) async fn projection_scope<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<
        super::native_projection::wire::SourceRef,
        super::native_projection::diagnostics::ProjectionFailure,
    > {
        use super::native_projection::diagnostics::ProjectionStage as Stage;
        let caller = self
            .binding()
            .and_then(|binding| binding.admit_native(webview, request.headers()))
            .map_err(|e| Stage::ScopeDocumentAdmission.failure(e))?;
        let target: super::native_projection::wire::ScopeTarget =
            decode_projection(request.body(), 4096)
                .map_err(|e| Stage::ScopeRequestDecode.failure(e))?;
        target
            .validate()
            .map_err(|e| Stage::ScopeRequestValidation.failure(e))?;
        let service = self.projections.clone();
        let admitted = caller.clone();
        let value = tauri::async_runtime::spawn_blocking(move || {
            service.scope_diagnosed(&admitted, &target)
        })
        .await
        .map_err(|_| Stage::ScopeTask.failure(error("SOURCE_TASK_FAILED")))??;
        self.projections
            .check_caller(&caller)
            .map_err(|e| Stage::ScopeResponseAdmission.failure(e))?;
        Ok(value)
    }
    pub(crate) async fn projection_read<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<
        super::native_projection::wire::ProjectionResult,
        super::native_projection::diagnostics::ProjectionFailure,
    > {
        use super::native_projection::diagnostics::ProjectionStage as Stage;
        let caller = self
            .binding()
            .and_then(|binding| binding.admit_native(webview, request.headers()))
            .map_err(|e| Stage::ReadDocumentAdmission.failure(e))?;
        let query: super::native_projection::wire::ReadRequest =
            decode_projection(request.body(), 16384)
                .map_err(|e| Stage::ReadRequestDecode.failure(e))?;
        query
            .validate()
            .map_err(|e| Stage::ReadRequestValidation.failure(e))?;
        let service = self.projections.clone();
        let admitted = caller.clone();
        let value =
            tauri::async_runtime::spawn_blocking(move || service.read_diagnosed(&admitted, &query))
                .await
                .map_err(|_| Stage::ReadTask.failure(error("SOURCE_TASK_FAILED")))??;
        self.projections
            .check_caller(&caller)
            .map_err(|e| Stage::ReadResponseAdmission.failure(e))?;
        Ok(value)
    }
    fn admit_run_key<T: Runtime>(
        &self,
        webview: &Webview<T>,
        headers: &HeaderMap,
        body: &InvokeBody,
    ) -> Result<(CallerIdentity, RunKey), SafeError> {
        let binding = self.binding()?;
        let caller = binding.admit_native(webview, headers)?;
        let InvokeBody::Raw(bytes) = body else {
            return Err(error("RAW_BODY_REQUIRED"));
        };
        if bytes.len() > 1024 {
            return Err(error("REQUEST_TOO_LARGE"));
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Query {
            run_id: String,
            generation: u32,
        }
        let query: Query = serde_json::from_slice(bytes).map_err(|_| error("INVALID_REQUEST"))?;
        if query.generation == 0 {
            return Err(SafeError::invalid("generation"));
        }
        let run = RunKey {
            run_id: query.run_id,
            generation: query.generation,
        };
        self.service.registry().check_run(&caller, &run)?;
        Ok((caller, run))
    }

    fn admit_stop<T: Runtime>(
        &self,
        webview: &Webview<T>,
        headers: &HeaderMap,
        body: &InvokeBody,
    ) -> Result<(CallerIdentity, LaunchStatus), SafeError> {
        let binding = self.binding()?;
        let caller = binding.admit_native(webview, headers)?;
        let (request_id, run) = decode_stop_query(body)?;
        let receipt = self
            .service
            .registry()
            .admit_run(&caller, &run, &request_id)?;
        Ok((caller, receipt))
    }

    /// Shared native admission for later input/resize/snapshot adapters.
    /// An acquired access rechecks caller/run ownership again at each operation.
    #[allow(dead_code)] // D17 operation adapters use this authenticated port.
    pub(crate) fn access<T: Runtime>(
        &self,
        webview: &Webview<T>,
        headers: &HeaderMap,
        body: &InvokeBody,
    ) -> Result<RunAccess, SafeError> {
        let (caller, run) = self.admit_run_key(webview, headers, body)?;
        self.service.access(&caller, &run)
    }

    pub(crate) fn input_begin<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<(), SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let input: InputBeginRequest = decode_projection(request.body(), 4096)?;
        let run = input_run_key(&input.run_id, input.generation)?;
        let _access = self.service.access(&caller, &run)?;
        self.inputs.begin(&caller, &input)
    }

    pub(crate) fn input_chunk<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<(), SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let input: InputChunkRequest = decode_projection(request.body(), 512 * 1024)?;
        let run = input_run_key(&input.run_id, input.generation)?;
        let _access = self.service.access(&caller, &run)?;
        self.inputs.chunk(&caller, &input)
    }

    pub(crate) fn input_abort<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<(), SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let input: InputAbortRequest = decode_projection(request.body(), 4096)?;
        let run = input_run_key(&input.run_id, input.generation)?;
        self.service.registry().check_run(&caller, &run)?;
        self.inputs.abort(&caller, &input)
    }

    pub(crate) async fn input_commit<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<InputWriteReceipt, SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let input: InputCommitRequest = decode_projection(request.body(), 4096)?;
        let run = input_run_key(&input.run_id, input.generation)?;
        let access = self.service.access(&caller, &run)?;
        let inputs = self.inputs.clone();
        tauri::async_runtime::spawn_blocking(move || {
            inputs.commit(&caller, &input, |bytes| {
                let result = access.with_writer(|writer| Ok(write_host_frame(writer, bytes)))?;
                Ok(result)
            })
        })
        .await
        .map_err(|_| error("INPUT_TASK_FAILED"))?
    }

    pub(crate) async fn input_protocol<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<ProtocolWriteReceipt, SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let input: ProtocolInputRequest = decode_projection(request.body(), 512 * 1024)?;
        self.inputs.validate_protocol(&caller, &input)?;
        let run = input_run_key(&input.run_id, input.generation)?;
        let access = self.service.access(&caller, &run)?;
        tauri::async_runtime::spawn_blocking(move || {
            let result = access.with_writer(|writer| Ok(write_host_frame(writer, &input.bytes)))?;
            Ok(ProtocolWriteReceipt::from_host(result))
        })
        .await
        .map_err(|_| error("INPUT_TASK_FAILED"))?
    }

    pub(crate) async fn resize<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<(), SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let input: ResizeRequest = decode_projection(request.body(), 4096)?;
        if input.generation == 0 || input.cols == 0 || input.rows == 0 {
            return Err(error("INVALID_REQUEST"));
        }
        let run = input_run_key(&input.run_id, input.generation)?;
        let access = self.service.access(&caller, &run)?;
        resize_native_run(
            access,
            portable_pty::PtySize {
                rows: input.rows,
                cols: input.cols,
                pixel_width: 0,
                pixel_height: 0,
            },
        )
        .await
    }

    pub(crate) fn stop<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<(), SafeError> {
        let (caller, receipt) = self.admit_stop(webview, request.headers(), request.body())?;
        self.supervisor
            .as_ref()
            .ok_or_else(|| error("NATIVE_RUNTIME_NOT_READY"))?
            .stop(&caller, &receipt)
    }

    /// A native provider must first obtain the existing exact run capability.
    #[cfg(feature = "remote-host")]
    #[allow(
        dead_code,
        reason = "native Host provider remains Locked; no JS pin factory"
    )]
    pub(crate) fn pin_remote_run(
        &self,
        caller: &CallerIdentity,
        run: &RunKey,
    ) -> Result<crate::run_supervisor::NativeRunPin, SafeError> {
        let _access = self.service.access(caller, run)?;
        Ok(self
            .supervisor
            .as_ref()
            .ok_or_else(|| error("NATIVE_RUNTIME_NOT_READY"))?
            .pin_shutdown(run))
    }

    #[cfg(feature = "remote-host")]
    pub(crate) fn owns_remote_pin(&self, pin: &crate::run_supervisor::NativeRunPin) -> bool {
        self.supervisor
            .as_ref()
            .is_some_and(|s| s.owns_live_pin(pin))
    }

    #[cfg(feature = "remote-host")]
    pub(crate) fn begin_host_shutdown(
        self: &Arc<Self>,
        pins: Vec<crate::run_supervisor::NativeRunPin>,
    ) -> HostNativeShutdown {
        self.service.begin_shutdown();
        if let Some(binding) = self.binding.lock().as_ref().cloned() {
            binding.revoke();
        }
        let (stop, unknown) = if pins.is_empty() {
            (None, false)
        } else {
            match self
                .supervisor
                .as_ref()
                .ok_or_else(|| error("NATIVE_RUNTIME_NOT_READY"))
                .and_then(|s| s.begin_shutdown_owned(pins))
            {
                Ok(stop) => (Some(stop), false),
                Err(_) => (None, true),
            }
        };
        // Retain the original legacy shutdown call; it can block and is never
        // aborted. Its five-second return is NOT run/IO/command retirement proof.
        // Any remote native pin therefore still yields Unknown below.
        let original = self.clone();
        let desktop = std::thread::Builder::new()
            .name("remote-desktop-stop".into())
            .spawn(move || original.shutdown());
        let (desktop, failed) = match desktop {
            Ok(job) => (Some(job), false),
            Err(_) => (None, true),
        };
        HostNativeShutdown {
            stop,
            desktop,
            joined: failed,
            unknown: unknown || failed,
        }
    }

    pub(crate) fn shutdown(&self) {
        self.service.begin_shutdown();
        // Revoke document/output authority before waiting for process reaping.
        // A closing main-thread WebView must not be required to service new
        // url/resource-table getters from an output sender while shutdown waits.
        if let Some(binding) = self.binding.lock().as_ref().cloned() {
            binding.revoke();
        }
        if let Some(supervisor) = &self.supervisor {
            supervisor.shutdown();
        }
        self.service.wait_for_commands();
    }
}

#[cfg(feature = "remote-host")]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HostNativeObservation {
    Pending,
    NoRemoteNativeResources,
    Unknown,
}
#[cfg(feature = "remote-host")]
pub(crate) struct HostNativeShutdown {
    stop: Option<crate::run_supervisor::NativeShutdown>,
    desktop: Option<std::thread::JoinHandle<()>>,
    joined: bool,
    unknown: bool,
}
#[cfg(feature = "remote-host")]
impl HostNativeShutdown {
    #[cfg(test)]
    pub(crate) fn from_owned_worker_for_test(worker: std::thread::JoinHandle<()>) -> Self {
        Self {
            stop: None,
            desktop: Some(worker),
            joined: false,
            unknown: false,
        }
    }
    pub(crate) fn observe(&mut self) -> HostNativeObservation {
        if self.desktop.as_ref().is_some_and(|job| job.is_finished()) {
            self.unknown |= self
                .desktop
                .take()
                .expect("owned finished desktop stop")
                .join()
                .is_err();
            self.joined = true;
        }
        // Poll all owned stop jobs even if another resource is already unknown.
        let native = self.stop.as_mut().and_then(|stop| stop.observe_all());
        if !self.joined || native == Some(crate::run_supervisor::NativeShutdownObservation::Pending)
        {
            return HostNativeObservation::Pending;
        }
        if self.unknown || native.is_some() {
            HostNativeObservation::Unknown
        } else {
            HostNativeObservation::NoRemoteNativeResources
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResizeRequest {
    run_id: String,
    generation: u32,
    cols: u16,
    rows: u16,
}

fn input_run_key(run_id: &str, generation: u32) -> Result<RunKey, SafeError> {
    if run_id.is_empty() || run_id.contains('\0') {
        return Err(SafeError::invalid("runId"));
    }
    if generation == 0 {
        return Err(SafeError::invalid("generation"));
    }
    Ok(RunKey {
        run_id: run_id.to_string(),
        generation,
    })
}

/// Suppress only the automatic main window; preserve every other configuration.
/// The same config is used by initialize_main during serialized backend setup.
pub(crate) fn take_main_config(config: &mut Config) -> Result<WindowConfig, SafeError> {
    if config
        .app
        .windows
        .iter()
        .filter(|window| window.label == "main")
        .count()
        != 1
    {
        return Err(error("DOCUMENT_WINDOW_UNAVAILABLE"));
    }
    let main = config
        .app
        .windows
        .iter_mut()
        .find(|window| window.label == "main")
        .unwrap();
    let original = main.clone();
    main.create = false;
    Ok(original)
}

/// 对照已锁定 Tauri 2.11.1 的桌面 App URL 解析，保留 index.html 的基准 URL 特例。
/// 外部页面不能在这里成为受信任的主窗口。
fn expected_main_url(config: &Config, window: &WindowConfig, dev: bool) -> Result<Url, SafeError> {
    let WebviewUrl::App(path) = &window.url else {
        return Err(error("FORBIDDEN"));
    };
    let custom = if cfg!(any(windows, target_os = "android")) {
        if window.use_https_scheme {
            "https://tauri.localhost"
        } else {
            "http://tauri.localhost"
        }
    } else {
        "tauri://localhost"
    };
    let mut base = if dev {
        config.build.dev_url.clone()
    } else {
        None
    };
    if base.is_none() {
        if let Some(FrontendDist::Url(url)) = &config.build.frontend_dist {
            base = Some(url.clone());
        }
    }
    let base = base.unwrap_or_else(|| custom.parse().expect("fixed local URL"));
    if path.to_str() == Some("index.html") {
        Ok(base)
    } else {
        base.join(&path.to_string_lossy())
            .map_err(|_| error("FORBIDDEN"))
    }
}

/// Called only after trusted native document admission. Parse errors never echo supplied values.
fn decode_projection<T: serde::de::DeserializeOwned>(
    body: &InvokeBody,
    limit: usize,
) -> Result<T, SafeError> {
    let InvokeBody::Raw(bytes) = body else {
        return Err(error("RAW_BODY_REQUIRED"));
    };
    if bytes.len() > limit {
        return Err(error("REQUEST_TOO_LARGE"));
    }
    serde_json::from_slice(bytes).map_err(|_| error("INVALID_REQUEST"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn d11_startup_config_preserved_001() {
        let mut config: Config = serde_json::from_value(serde_json::json!({"identifier":"d11.test","app":{"windows":[{"label":"main","width":987,"title":"same"},{"label":"peer"}]}})).unwrap();
        let original = serde_json::to_value(&config.app.windows[0]).unwrap();
        let main = take_main_config(&mut config).unwrap();
        assert_eq!(serde_json::to_value(main).unwrap(), original);
        assert!(!config.app.windows[0].create);
        assert!(config.app.windows[1].create);
    }
    #[test]
    fn d11_startup_desktop_url_parity_002() {
        let mut config: Config = serde_json::from_value(serde_json::json!({"identifier":"d11.test","build":{"devUrl":"http://localhost:1420/sub/"}})).unwrap();
        let mut window = WindowConfig::default();
        assert_eq!(
            expected_main_url(&config, &window, true).unwrap().as_str(),
            "http://localhost:1420/sub/"
        );
        window.url = WebviewUrl::App("probe.html".into());
        assert_eq!(
            expected_main_url(&config, &window, true).unwrap().as_str(),
            "http://localhost:1420/sub/probe.html"
        );
        config.build.dev_url = None;
        assert!(expected_main_url(&config, &window, false)
            .unwrap()
            .as_str()
            .ends_with("localhost/probe.html"));
        window.url = WebviewUrl::External("https://example.com/".parse().unwrap());
        assert_eq!(
            expected_main_url(&config, &window, false).unwrap_err().code,
            "FORBIDDEN"
        );
    }
}
