//! Independent manager hosts. Neither initializes ordinary repositories,
//! NativeRuntime, ConPTY, logger, plugins, or the ordinary command table.
//! Recovery has its own authenticated host and never recreates initial handoff.
use super::{
    manager_document::{
        self, ManagerDocumentBinding, ManagerDocumentRegistry, MANAGER_DOCUMENT_HEADER,
    },
    manager_entry::ManagerRequest,
    manager_types::{InspectManagerRequest, ManagerAction, ManagerActionRequest, ManagerStatus},
    manager_worker::{ManagerDocumentProof, ManagerWorker},
    windows::{
        manager_handoff::InitialManager, manager_ui::ManagerUiEnvironment, process::ExactProcess,
    },
};
use crate::cli::{profiles::error, types::SafeError};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};
use tauri::{
    ipc::{InvokeBody, Request},
    utils::config::{Config, FrontendDist},
    Manager, State, Url, Webview, WebviewWindow,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Readiness {
    Unstarted,
    Pending,
    Ready,
    Failed,
}
struct InitialManagerRuntime {
    owner: Arc<Mutex<InitialManager>>,
    transaction: String,
    worker: ManagerWorker,
    ui: ManagerUiEnvironment,
    document: OnceLock<Arc<ManagerDocumentBinding>>,
    window: OnceLock<WebviewWindow>,
    readiness: Mutex<Readiness>,
}
impl InitialManagerRuntime {
    fn new(owner: InitialManager) -> Result<Arc<Self>, SafeError> {
        let ui = ManagerUiEnvironment::prepare(&owner.data)
            .map_err(|_| error("HISTORY_MANAGER_UI_UNAVAILABLE"))?;
        let transaction = owner.binding.transaction_id.clone();
        let owner = Arc::new(Mutex::new(owner));
        let worker = ManagerWorker::start(&owner)?;
        Ok(Arc::new(Self {
            owner,
            transaction,
            worker,
            ui,
            document: OnceLock::new(),
            window: OnceLock::new(),
            readiness: Mutex::new(Readiness::Unstarted),
        }))
    }
    async fn ensure_ready(self: &Arc<Self>) -> Result<(), SafeError> {
        {
            let mut readiness = self.readiness.lock();
            match *readiness {
                Readiness::Ready => return Ok(()),
                Readiness::Pending => return Err(error("HISTORY_MANAGER_NOT_READY")),
                Readiness::Failed => return Err(error("HISTORY_RECOVERY_REQUIRED")),
                Readiness::Unstarted => *readiness = Readiness::Pending,
            }
        }
        let result = self.begin_ready().await;
        *self.readiness.lock() = if result.is_ok() {
            Readiness::Ready
        } else {
            Readiness::Failed
        };
        if result.is_err() {
            self.fail_initial_readiness();
        }
        result
    }
    fn fail_initial_readiness(&self) {
        *self.readiness.lock() = Readiness::Failed;
        // Only the manager's initial close pin is released. Original source
        // admission, marker and retained recovery data remain authoritative.
        if let Some(document) = self.document.get() {
            document.release_initial_handoff();
        }
        self.worker.shutdown();
        if let Some(window) = self.window.get() {
            window.app_handle().exit(2);
        }
    }
    async fn begin_ready(self: &Arc<Self>) -> Result<(), SafeError> {
        // The dedicated thread must finish capture and release writer/control
        // before the native window can publish its Ready receipt.
        self.worker.await_prepared().await?;
        let window = self
            .window
            .get()
            .ok_or_else(|| error("DOCUMENT_WINDOW_UNAVAILABLE"))?
            .clone();
        let document = self
            .document
            .get()
            .ok_or_else(|| error("FORBIDDEN"))?
            .clone();
        let source = {
            let owner = self.owner.lock();
            ExactProcess::reopen(
                owner
                    .child
                    .source()
                    .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?
                    .identity(),
            )
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?
        };
        let (send, receive) = tokio::sync::oneshot::channel();
        let owner = self.clone();
        self.ui
            .capture_ready(window, document, &source, move |ready| {
                let result = ready
                    .and_then(|ready| owner.owner.lock().child.publish_ready(ready).map(|_| ()))
                    .map_err(|_| error("HISTORY_HANDOFF_CHANGED"));
                let _ = send.send(result);
            })
            .map_err(|_| error("HISTORY_MANAGER_UI_UNAVAILABLE"))?;
        // capture_ready copied the actual identity for its native callback.
        // Release this duplicate reader before the worker's later image fence.
        drop(source);
        receive
            .await
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))??;
        // Only after the actual native publication AND duplicate-reader release
        // can the worker start its source wait and one-use source transfer.
        self.worker.ready_published()
    }
    fn status(&self) -> Result<ManagerStatus, SafeError> {
        // Diagnostic only; inspect never contends for the long-lived writer.
        self.worker.status()
    }
    fn blocks_exit(&self) -> bool {
        if *self.readiness.lock() == Readiness::Failed {
            return false;
        }
        let Some(document) = self.document.get() else {
            return false;
        };
        // Do not wait for a material-observation lock on the UI thread: its
        // owner may itself be awaiting this thread's native WebView query.
        // Exact source-terminal transfer releases the pin on the worker.
        document.blocks_native_close()
    }
}
fn admit_request<T: serde::de::DeserializeOwned>(
    document: &OnceLock<Arc<ManagerDocumentBinding>>,
    expected_transaction: &str,
    webview: Webview,
    request: &Request<'_>,
) -> Result<(Arc<ManagerDocumentProof>, T), SafeError> {
    let binding = document.get().ok_or_else(|| error("FORBIDDEN"))?.clone();
    let transaction = binding.admit_native(&webview, request.headers())?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(error("RAW_BODY_REQUIRED"));
    };
    if bytes.len() > 1024 {
        return Err(error("REQUEST_TOO_LARGE"));
    }
    let body: T = serde_json::from_slice(bytes).map_err(|_| error("INVALID_REQUEST"))?;
    if transaction != expected_transaction {
        return Err(error("FORBIDDEN"));
    }
    let mut headers = tauri::http::HeaderMap::new();
    headers.insert(
        MANAGER_DOCUMENT_HEADER,
        request.headers()[MANAGER_DOCUMENT_HEADER].clone(),
    );
    Ok((
        ManagerDocumentProof::admit(binding, webview, headers, &transaction)?,
        body,
    ))
}
#[tauri::command]
async fn inspect_version_switch(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<InitialManagerRuntime>>,
) -> Result<ManagerStatus, SafeError> {
    let (document, _): (_, InspectManagerRequest) =
        admit_request(&runtime.document, &runtime.transaction, webview, &request)?;
    let runtime = runtime.inner().clone();
    runtime.ensure_ready().await?;
    document.check()?;
    let result = runtime.status();
    document.check()?;
    result
}

async fn dispatch_action(
    action: ManagerAction,
    webview: Webview,
    request: Request<'_>,
    runtime: Arc<InitialManagerRuntime>,
) -> Result<ManagerStatus, SafeError> {
    let (document, body): (_, ManagerActionRequest) =
        admit_request(&runtime.document, &runtime.transaction, webview, &request)?;
    runtime.ensure_ready().await?;
    document.check()?;
    let result = runtime
        .worker
        .dispatch(action, body.expected_generation.get(), document.clone())
        .await;
    document.check()?;
    result
}

#[tauri::command]
async fn confirm_historical_version(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<InitialManagerRuntime>>,
) -> Result<ManagerStatus, SafeError> {
    dispatch_action(
        ManagerAction::ConfirmHistoricalVersion,
        webview,
        request,
        runtime.inner().clone(),
    )
    .await
}

#[tauri::command]
async fn restore_previous_version(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<InitialManagerRuntime>>,
) -> Result<ManagerStatus, SafeError> {
    dispatch_action(
        ManagerAction::ReturnToPrevious,
        webview,
        request,
        runtime.inner().clone(),
    )
    .await
}
fn manager_url(config: &Config) -> Result<Url, SafeError> {
    let base = if tauri::is_dev() {
        config.build.dev_url.clone()
    } else {
        None
    }
    .or_else(|| match &config.build.frontend_dist {
        Some(FrontendDist::Url(url)) => Some(url.clone()),
        _ => None,
    })
    .unwrap_or_else(|| {
        "http://tauri.localhost"
            .parse()
            .expect("fixed manager origin")
    });
    base.join("version-manager.html")
        .map_err(|_| error("DOCUMENT_WINDOW_UNAVAILABLE"))
}
/// Only the exact protected initial child can enter this host. Failure does not
/// fall through into another startup or replay the initial handoff.
pub(crate) fn run(request: ManagerRequest) -> Result<(), SafeError> {
    let runtime = InitialManagerRuntime::new(InitialManager::open(&request)?)?;
    let mut context = tauri::generate_context!();
    for window in &mut context.config_mut().app.windows {
        window.create = false;
    }
    let expected_url = manager_url(context.config())?;
    let setup = runtime.clone();
    let events = runtime.clone();
    let app = tauri::Builder::default()
        .manage(runtime)
        .invoke_handler(tauri::generate_handler![
            inspect_version_switch,
            confirm_historical_version,
            restore_previous_version
        ])
        .setup(move |app| {
            let data_directory = setup.ui.data_directory()?;
            let window_config = setup.ui.window_config();
            let bound = manager_document::build_manager(
                app,
                &window_config,
                expected_url.clone(),
                request.transaction_id(),
                &ManagerDocumentRegistry::default(),
                &data_directory,
            )?;
            setup
                .document
                .set(Arc::new(bound.binding))
                .map_err(|_| error("DOCUMENT_WINDOW_UNAVAILABLE"))?;
            setup
                .window
                .set(bound.window)
                .map_err(|_| error("DOCUMENT_WINDOW_UNAVAILABLE"))?;
            // Preparation failure must close an unready manager even if its
            // frontend never issues inspect. The receiver carries SafeError
            // only; partial registry-bearing evidence stays on its OS thread.
            let preparation = setup.worker.take_preparation_observer()?;
            let weak = Arc::downgrade(&setup);
            tauri::async_runtime::spawn(async move {
                if !matches!(preparation.await, Ok(Ok(()))) {
                    if let Some(runtime) = weak.upgrade() {
                        runtime.fail_initial_readiness();
                    }
                }
            });
            Ok(())
        })
        .build(context)
        .map_err(|_| error("HISTORY_MANAGER_UI_UNAVAILABLE"))?;
    app.run(move |_, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if events.blocks_exit() {
                api.prevent_exit();
            }
        }
    });
    Ok(())
}

/// 重开使用独立原生恢复入口。选择器只能与受保护标记匹配，不能充当子进程权限。
pub(crate) fn run_reentry(request: Option<&ManagerRequest>) -> Result<(), SafeError> {
    reentry_runtime::run(request)
}

mod reentry_runtime {
    use super::{
        admit_request, error, manager_document, manager_url, Arc, InspectManagerRequest,
        ManagerActionRequest, ManagerDocumentBinding, ManagerDocumentRegistry, ManagerRequest,
        ManagerStatus, ManagerUiEnvironment, Mutex, OnceLock, Request, SafeError, State, Webview,
    };
    use crate::version_history::manager_worker::{ReentryCommandState, ReentryReturnOperation};
    use crate::version_history::windows::reentry::ReenteredManager;

    struct ReenteredManagerRuntime {
        owner: Arc<Mutex<ReenteredManager>>,
        transaction: String,
        ui: ManagerUiEnvironment,
        document: OnceLock<Arc<ManagerDocumentBinding>>,
        inspection: Arc<tokio::sync::Semaphore>,
        progress: Arc<Mutex<ReentryCommandState>>,
    }

    #[tauri::command]
    async fn inspect_version_switch(
        webview: Webview,
        request: Request<'_>,
        runtime: State<'_, Arc<ReenteredManagerRuntime>>,
    ) -> Result<ManagerStatus, SafeError> {
        let (document, _): (_, InspectManagerRequest) =
            admit_request(&runtime.document, &runtime.transaction, webview, &request)?;
        document.check()?;
        // 运行中的恢复持有实际对象锁，UI只读取该工作刚发布的诊断缓存。
        // 缓存从不授予写权限，也不阻塞正在等待主线程验证文档的原生工作。
        {
            let progress = runtime.progress.lock();
            if progress.returning() {
                let result = progress.status();
                drop(progress);
                document.check()?;
                return result;
            }
        }
        // At most one read may be pending or running. Keep filesystem/hash work
        // off the UI thread and retain the permit if the requesting future dies.
        let permit = runtime
            .inspection
            .clone()
            .try_acquire_owned()
            .map_err(|_| error("HISTORY_OPERATION_PENDING"))?;
        let owner = runtime.owner.clone();
        let progress = runtime.progress.clone();
        let observed_document = document.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _permit = permit;
            let document = observed_document;
            document.check()?;
            let status = owner.lock().inspect()?;
            document.check()?;
            let mut progress = progress.lock();
            progress.publish(status)?;
            progress.status()
        })
        .await
        .map_err(|_| error("HISTORY_RECOVERY_REQUIRED"))?;
        document.check()?;
        result
    }

    #[tauri::command]
    async fn restore_previous_version(
        webview: Webview,
        request: Request<'_>,
        runtime: State<'_, Arc<ReenteredManagerRuntime>>,
    ) -> Result<ManagerStatus, SafeError> {
        let (document, body): (_, ManagerActionRequest) =
            admit_request(&runtime.document, &runtime.transaction, webview, &request)?;
        document.check()?;
        // 检查和恢复共用单次许可；真正的后台工作结束前不会因为IPC取消而释放。
        let permit = runtime
            .inspection
            .clone()
            .try_acquire_owned()
            .map_err(|_| error("HISTORY_OPERATION_PENDING"))?;
        let operation =
            ReentryReturnOperation::begin(&runtime.progress, body.expected_generation.get())?;
        let owner = runtime.owner.clone();
        let progress = runtime.progress.clone();
        let observed_document = document.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _permit = permit;
            let document = observed_document;
            let result = (|| {
                document.check()?;
                let result = owner.lock().return_previous(
                    document.clone(),
                    body.expected_generation.get(),
                    |status| {
                        let mut progress = progress.lock();
                        if let Err(failure) = progress.publish(status) {
                            // 原生发布失败不能被当作可用的新恢复按钮。
                            progress.fail(failure);
                        }
                    },
                );
                document.check()?;
                result
            })();
            operation.finish(result)
        })
        .await
        .map_err(|_| error("HISTORY_RECOVERY_REQUIRED"))?;
        document.check()?;
        result
    }

    pub(super) fn run(request: Option<&ManagerRequest>) -> Result<(), SafeError> {
        let owner = ReenteredManager::open(request)?;
        let transaction = owner.transaction_id().to_owned();
        let ui = ManagerUiEnvironment::prepare(owner.data())
            .map_err(|_| error("HISTORY_MANAGER_UI_UNAVAILABLE"))?;
        let progress = Arc::new(Mutex::new(ReentryCommandState::new(&transaction)));
        let runtime = Arc::new(ReenteredManagerRuntime {
            owner: Arc::new(Mutex::new(owner)),
            transaction,
            ui,
            document: OnceLock::new(),
            inspection: Arc::new(tokio::sync::Semaphore::new(1)),
            progress,
        });
        let mut context = tauri::generate_context!();
        for window in &mut context.config_mut().app.windows {
            window.create = false;
        }
        let expected_url = manager_url(context.config())?;
        let setup = runtime.clone();
        let events = runtime.clone();
        let app = tauri::Builder::default()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![
                self::inspect_version_switch,
                self::restore_previous_version
            ])
            .setup(move |app| {
                let data_directory = setup.ui.data_directory()?;
                let bound = manager_document::build_manager(
                    app,
                    &setup.ui.window_config(),
                    expected_url.clone(),
                    &setup.transaction,
                    &ManagerDocumentRegistry::default(),
                    &data_directory,
                )?;
                setup
                    .document
                    .set(Arc::new(bound.binding))
                    .map_err(|_| error("DOCUMENT_WINDOW_UNAVAILABLE"))?;
                let progress = setup.progress.clone();
                bound.window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if progress.lock().returning() {
                            api.prevent_close();
                        }
                    }
                });
                Ok(())
            })
            .build(context)
            .map_err(|_| error("HISTORY_MANAGER_UI_UNAVAILABLE"))?;
        // 只读检查不阻止关闭；已接收的恢复继续持有原文档与后台工作寿命。
        app.run(move |_, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if events.progress.lock().returning() {
                    api.prevent_exit();
                }
            }
        });
        Ok(())
    }
}
