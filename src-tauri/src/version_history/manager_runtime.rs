//! Independent initial-manager host. It never initializes ordinary repositories,
//! NativeRuntime, ConPTY, logger, plugins, or the ordinary command table.
use super::{
    journal::{JournalStore, ManifestRole},
    manager_document::{
        self, ManagerDocumentBinding, ManagerDocumentRegistry, MANAGER_DOCUMENT_HEADER,
    },
    manager_entry::ManagerRequest,
    manager_types::{InspectManagerRequest, ManagerAction, ManagerBlockReason, ManagerStatus},
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
    State, Url, Webview, WebviewWindow,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Readiness {
    Unstarted,
    Pending,
    Ready,
    Failed,
}
struct InitialManagerRuntime {
    owner: Mutex<InitialManager>,
    ui: ManagerUiEnvironment,
    document: OnceLock<Arc<ManagerDocumentBinding>>,
    window: OnceLock<WebviewWindow>,
    readiness: Mutex<Readiness>,
}
impl InitialManagerRuntime {
    fn new(owner: InitialManager) -> Result<Arc<Self>, SafeError> {
        let ui = ManagerUiEnvironment::prepare(&owner.data)
            .map_err(|_| error("HISTORY_MANAGER_UI_UNAVAILABLE"))?;
        Ok(Arc::new(Self {
            owner: Mutex::new(owner),
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
        result
    }
    async fn begin_ready(self: &Arc<Self>) -> Result<(), SafeError> {
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
                    .and_then(|ready| owner.owner.lock().child.publish_ready(ready).map(|_| ()));
                let _ = send.send(result.map_err(|_| error("HISTORY_HANDOFF_CHANGED")));
            })
            .map_err(|_| error("HISTORY_MANAGER_UI_UNAVAILABLE"))?;
        receive
            .await
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?
    }
    fn status(&self) -> Result<ManagerStatus, SafeError> {
        let owner = self.owner.lock();
        owner
            .child
            .verify_material()
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?;
        let control = owner.installation.acquire_control()?;
        let store = JournalStore::open_windows_transaction(
            owner.installation.root().clone(),
            &owner.binding.transaction_id,
        )?;
        let inspection = store.inspect(&owner.binding)?;
        if inspection.blocked {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let journal = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        if journal.manifest(ManifestRole::ManagerHandoff).is_none() {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let source_exited = owner
            .child
            .source_terminal()
            .map_err(|_| error("HISTORY_SOURCE_EXIT_UNCONFIRMED"))?;
        if source_exited {
            owner
                .child
                .release_ui_after_source_exit()
                .map_err(|_| error("HISTORY_SOURCE_EXIT_UNCONFIRMED"))?;
        }
        control
            .verify_root(owner.installation.root())
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        ManagerStatus::project(
            journal,
            owner.child.selection(),
            Some(if source_exited {
                ManagerBlockReason::SourceExitUnconfirmed
            } else {
                ManagerBlockReason::SourceStillRunning
            }),
            &[ManagerAction::Refresh],
        )
    }
    fn blocks_exit(&self) -> bool {
        let Some(document) = self.document.get() else {
            return false;
        };
        // Do not wait for a material-observation lock on the UI thread: its
        // owner may itself be awaiting this thread's native WebView query.
        // Exact source-terminal observation releases the pin during inspect.
        document.blocks_native_close()
    }
}
struct AdmittedManagerDocument {
    binding: Arc<ManagerDocumentBinding>,
    webview: Webview,
    headers: tauri::http::HeaderMap,
    transaction: String,
}
impl AdmittedManagerDocument {
    fn check(&self) -> Result<(), SafeError> {
        if self.binding.admit_native(&self.webview, &self.headers)? != self.transaction {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }
}
fn admit_inspect(
    runtime: &InitialManagerRuntime,
    webview: Webview,
    request: &Request<'_>,
) -> Result<AdmittedManagerDocument, SafeError> {
    let binding = runtime
        .document
        .get()
        .ok_or_else(|| error("FORBIDDEN"))?
        .clone();
    let transaction = binding.admit_native(&webview, request.headers())?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(error("RAW_BODY_REQUIRED"));
    };
    if bytes.len() > 1024 {
        return Err(error("REQUEST_TOO_LARGE"));
    }
    let _: InspectManagerRequest =
        serde_json::from_slice(bytes).map_err(|_| error("INVALID_REQUEST"))?;
    if transaction != runtime.owner.lock().binding.transaction_id {
        return Err(error("FORBIDDEN"));
    }
    let mut headers = tauri::http::HeaderMap::new();
    headers.insert(
        MANAGER_DOCUMENT_HEADER,
        request.headers()[MANAGER_DOCUMENT_HEADER].clone(),
    );
    Ok(AdmittedManagerDocument {
        binding,
        webview,
        headers,
        transaction,
    })
}
#[tauri::command]
async fn inspect_version_switch(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<InitialManagerRuntime>>,
) -> Result<ManagerStatus, SafeError> {
    let document = admit_inspect(&runtime, webview, &request)?;
    let runtime = runtime.inner().clone();
    runtime.ensure_ready().await?;
    document.check()?;
    let result = tauri::async_runtime::spawn_blocking(move || runtime.status())
        .await
        .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
    document.check()?;
    result
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
/// This entry currently admits the protected initial child only. Recovery entry
/// requires separate retained-bundle/terminal reconciliation, never child replay.
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
        .invoke_handler(tauri::generate_handler![inspect_version_switch])
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
