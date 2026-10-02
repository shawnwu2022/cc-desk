//! Ordinary-document catalogue, preparation and authenticated source handoff.
//! Retain the exact admitted binding, WebView and proof across blocking IO.
use super::catalog::CatalogService;
use super::download::{
    BeginPrepareRequest, CancelPrepareSummary, PreparationTicket, PrepareService,
    PrepareTransactionRequest, PreparedPackageSummary,
};
use super::types::{
    HistoryCatalogPage, HistorySelection, ListHistoryRequest, SelectHistoryRequest,
};
use crate::cli::native_runtime::NativeRuntime;
use crate::cli::profiles::error;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::SafeError;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use tauri::ipc::{InvokeBody, Request};
use tauri::{State, Webview};

type OwnerCheck = dyn Fn(&CallerIdentity) -> Result<(), SafeError> + Send + Sync;
pub(crate) struct HistoryDocument {
    caller: CallerIdentity,
    check: Arc<OwnerCheck>,
}
impl HistoryDocument {
    #[cfg(test)]
    pub(crate) fn test_bound<R: Send + Sync + 'static>(
        binding: Arc<crate::cli::document::DocumentBinding<R>>,
        table: Arc<Mutex<tauri::ResourceTable>>,
        url: tauri::Url,
        headers: tauri::http::HeaderMap,
    ) -> Result<Self, SafeError> {
        let context = crate::cli::document::NativeContext {
            window_label: "main",
            webview_label: "main",
            url: &url,
        };
        let caller = binding.admit(&table.lock(), &context, &headers)?;
        let check = Arc::new(move |expected: &CallerIdentity| {
            let context = crate::cli::document::NativeContext {
                window_label: "main",
                webview_label: "main",
                url: &url,
            };
            let actual = binding.admit(&table.lock(), &context, &headers)?;
            if &actual != expected {
                return Err(error("FORBIDDEN"));
            }
            Ok(())
        });
        Ok(Self { caller, check })
    }
    fn recheck(&self) -> Result<(), SafeError> {
        (self.check)(&self.caller)
    }
}

/// Evaluate original native admission before examining even the wire format.
/// The Result is created by DocumentBinding, never decoded from the wire.
pub(crate) fn decode_history_request<T: DeserializeOwned>(
    admission: Result<CallerIdentity, SafeError>,
    body: &InvokeBody,
) -> Result<(CallerIdentity, T), SafeError> {
    let caller = admission?;
    let InvokeBody::Raw(bytes) = body else {
        return Err(error("RAW_BODY_REQUIRED"));
    };
    if bytes.len() > 1024 {
        return Err(error("REQUEST_TOO_LARGE"));
    }
    let payload = serde_json::from_slice(bytes).map_err(|_| error("INVALID_REQUEST"))?;
    Ok((caller, payload))
}
fn admit<T: DeserializeOwned>(
    runtime: &NativeRuntime,
    webview: Webview,
    request: &Request<'_>,
) -> Result<(HistoryDocument, T), SafeError> {
    let binding = runtime.binding()?;
    let (caller, payload) = decode_history_request(
        binding.admit_native(&webview, request.headers()),
        request.body(),
    )?;
    // No unrelated headers/body are retained. Never look up a replacement binding.
    let mut headers = tauri::http::HeaderMap::new();
    headers.insert(
        crate::cli::document::DOCUMENT_HEADER,
        request.headers()[crate::cli::document::DOCUMENT_HEADER].clone(),
    );
    let check = Arc::new(move |expected: &CallerIdentity| {
        let current = binding.admit_native(&webview, &headers)?;
        if &current != expected {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    });
    Ok((HistoryDocument { caller, check }, payload))
}

struct OwnedPreparation {
    caller: CallerIdentity,
    // Drops package service before its original pinned parent/ancestor chain.
    service: Arc<PrepareService>,
    #[cfg(windows)]
    _private_parent: Arc<super::windows::files::PrivateDirectory>,
}
#[derive(Default)]
pub(crate) struct HistoryService {
    catalog: Mutex<Option<Arc<CatalogService>>>,
    preparation: Mutex<Option<Arc<OwnedPreparation>>>,
}
impl HistoryService {
    #[cfg(test)]
    pub(crate) fn with_catalog(catalog: Arc<CatalogService>) -> Self {
        Self {
            catalog: Mutex::new(Some(catalog)),
            preparation: Mutex::new(None),
        }
    }
    pub(crate) fn list(
        &self,
        document: &HistoryDocument,
        query: &ListHistoryRequest,
    ) -> Result<HistoryCatalogPage, SafeError> {
        document.recheck()?;
        let result = self
            .catalog()?
            .list(&document.caller, query.cursor.as_deref());
        document.recheck()?;
        result
    }
    fn catalog(&self) -> Result<Arc<CatalogService>, SafeError> {
        let mut held = self.catalog.lock();
        if let Some(catalog) = held.as_ref() {
            return Ok(catalog.clone());
        }
        let catalog = Arc::new(CatalogService::production()?);
        *held = Some(catalog.clone());
        Ok(catalog)
    }
    fn preparation(&self, document: &HistoryDocument) -> Result<Arc<OwnedPreparation>, SafeError> {
        document.recheck()?;
        self.preparation
            .lock()
            .as_ref()
            .filter(|held| held.caller == document.caller)
            .cloned()
            .ok_or_else(|| error("HISTORY_PREPARE_UNKNOWN"))
    }
    fn begin(
        &self,
        document: &HistoryDocument,
        selection: &str,
    ) -> Result<PreparationTicket, SafeError> {
        document.recheck()?;
        let catalog = self.catalog()?;
        // Validate the document-owned selection before any filesystem work.
        catalog.resolve_selection(&document.caller, selection)?;
        let mut held = self.preparation.lock();
        if held.is_none() {
            *held = Some(Arc::new(create_preparation(document, catalog)?));
        }
        let preparation = held
            .as_ref()
            .filter(|held| held.caller == document.caller)
            .ok_or_else(|| error("FORBIDDEN"))?;
        document.recheck()?;
        preparation
            .service
            .begin_prepare(&document.caller, selection)
    }
}
#[cfg(not(windows))]
fn create_preparation(
    _document: &HistoryDocument,
    _catalog: Arc<CatalogService>,
) -> Result<OwnedPreparation, SafeError> {
    Err(error("HISTORY_PREPARATION_UNAVAILABLE"))
}
#[cfg(windows)]
fn create_preparation(
    document: &HistoryDocument,
    catalog: Arc<CatalogService>,
) -> Result<OwnedPreparation, SafeError> {
    use super::windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        security::CurrentUser,
    };
    use std::ffi::OsStr;
    let storage_error = |_| error("HISTORY_STORAGE_UNAVAILABLE");
    let user = CurrentUser::capture().map_err(storage_error)?;
    user.require_unelevated().map_err(storage_error)?;
    let local = dirs::data_local_dir().ok_or_else(|| error("HISTORY_STORAGE_UNAVAILABLE"))?;
    let parent = Directory::open_absolute(&local).map_err(storage_error)?;
    let name = format!("CCDesk-Preparation-{}", uuid::Uuid::new_v4().simple());
    let private = Arc::new(
        PrivateDirectory::create_new(
            parent,
            ComponentName::new(OsStr::new(&name)).map_err(storage_error)?,
            &user,
        )
        .map_err(storage_error)?,
    );
    private.verify(&user).map_err(storage_error)?;
    let capability = private.directory().capability().map_err(storage_error)?;
    // Service owner checks also retain the private parent during detached IO.
    // This is preparation storage only, not a recovery-root or switch proof.
    let retained = private.clone();
    let original = document.check.clone();
    let check: Arc<OwnerCheck> = Arc::new(move |caller| {
        original(caller)?;
        retained
            .verify(&user)
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))
    });
    let service = Arc::new(PrepareService::production(catalog, capability, check)?);
    document.recheck()?;
    Ok(OwnedPreparation {
        caller: document.caller.clone(),
        service,
        _private_parent: private,
    })
}

#[tauri::command]
pub(crate) async fn list_history(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    history: State<'_, Arc<HistoryService>>,
) -> Result<HistoryCatalogPage, SafeError> {
    let (document, query): (_, ListHistoryRequest) = admit(&runtime, webview, &request)?;
    let service = history.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.list(&document, &query))
        .await
        .map_err(|_| error("HISTORY_TASK_FAILED"))?
}
#[tauri::command]
pub(crate) async fn select_history(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    history: State<'_, Arc<HistoryService>>,
) -> Result<HistorySelection, SafeError> {
    let (document, query): (_, SelectHistoryRequest) = admit(&runtime, webview, &request)?;
    let service = history.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        document.recheck()?;
        let result =
            service
                .catalog()?
                .select(&document.caller, &query.release_id, &query.asset_id);
        document.recheck()?;
        result
    })
    .await
    .map_err(|_| error("HISTORY_TASK_FAILED"))?
}
#[tauri::command]
pub(crate) async fn begin_prepare_history(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    history: State<'_, Arc<HistoryService>>,
) -> Result<PreparationTicket, SafeError> {
    let (document, query): (_, BeginPrepareRequest) = admit(&runtime, webview, &request)?;
    let service = history.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = service.begin(&document, &query.selection_token);
        document.recheck()?;
        result
    })
    .await
    .map_err(|_| error("HISTORY_TASK_FAILED"))?
}
#[tauri::command]
pub(crate) async fn prepare_history(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    history: State<'_, Arc<HistoryService>>,
) -> Result<PreparedPackageSummary, SafeError> {
    let (document, query): (_, PrepareTransactionRequest) = admit(&runtime, webview, &request)?;
    let service = history.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let preparation = service.preparation(&document)?;
        let result = preparation
            .service
            .prepare_history(&document.caller, &query.transaction_id);
        document.recheck()?;
        result
    })
    .await
    .map_err(|_| error("HISTORY_TASK_FAILED"))?
}
#[tauri::command]
pub(crate) async fn cancel_prepare_history(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    history: State<'_, Arc<HistoryService>>,
) -> Result<CancelPrepareSummary, SafeError> {
    let (document, query): (_, PrepareTransactionRequest) = admit(&runtime, webview, &request)?;
    let service = history.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = service
            .preparation(&document)?
            .service
            .cancel_prepare(&document.caller, &query.transaction_id);
        document.recheck()?;
        result
    })
    .await
    .map_err(|_| error("HISTORY_TASK_FAILED"))?
}

/// The review command itself selects fresh settings; there is no caller-supplied
/// compatibility proof, installed path, PID, target digest or executable argument.
#[cfg(windows)]
#[tauri::command]
pub(crate) async fn begin_switch(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    history: State<'_, Arc<HistoryService>>,
    startup: State<'_, Arc<super::windows::startup::OrdinaryStartup>>,
) -> Result<super::manager::SwitchTicket, SafeError> {
    use tauri::Manager;
    let (document, query): (_, super::manager::BeginSwitchRequest) =
        admit(&runtime, webview.clone(), &request)?;
    let super::manager::SwitchDataMode::FreshSettings = query.data_mode;
    super::compatibility::admit_data_mode(super::compatibility::ReviewedDataMode::FreshSettings)?;
    let binding = runtime.binding()?;
    let window = webview
        .app_handle()
        .get_webview_window("main")
        .ok_or_else(|| error("FORBIDDEN"))?;
    let mut headers = tauri::http::HeaderMap::new();
    headers.insert(
        crate::cli::document::DOCUMENT_HEADER,
        request.headers()[crate::cli::document::DOCUMENT_HEADER].clone(),
    );
    if binding.admit_window(&window, &headers)? != document.caller {
        return Err(error("FORBIDDEN"));
    }
    let service = history.inner().clone();
    let (reservation, payload) = tauri::async_runtime::spawn_blocking(move || {
        let preparation = service.preparation(&document)?;
        let reserved = preparation.service.reserve_handoff_with_source(
            &document.caller,
            &query.preparation_id,
            super::windows::source_begin::SourcePreflight::admit,
        )?;
        document.recheck()?;
        Ok::<_, SafeError>(reserved)
    })
    .await
    .map_err(|_| error("HISTORY_TASK_FAILED"))??;
    let transaction_id = reservation.transaction_id;
    if let Some(transfer) = reservation.transfer {
        let payload = payload.ok_or_else(|| error("HISTORY_PAYLOAD_UNVERIFIED"))?;
        let startup = startup.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            super::windows::source_begin::begin(
                transfer, payload, startup, window, binding, headers,
            )
        })
        .await
        .map_err(|_| error("HISTORY_TASK_FAILED"))??;
    }
    Ok(super::manager::SwitchTicket { transaction_id })
}

#[tauri::command]
pub(crate) async fn inspect_switch(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    history: State<'_, Arc<HistoryService>>,
) -> Result<super::manager::SwitchReview, SafeError> {
    let (document, query): (_, super::manager::InspectSwitchRequest) =
        admit(&runtime, webview, &request)?;
    let service = history.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = service
            .preparation(&document)?
            .service
            .inspect_switch(&document.caller, &query.preparation_id);
        document.recheck()?;
        result
    })
    .await
    .map_err(|_| error("HISTORY_TASK_FAILED"))?
}

#[cfg(not(windows))]
#[tauri::command]
pub(crate) async fn begin_switch(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
) -> Result<super::manager::SwitchTicket, SafeError> {
    let (_document, _query): (_, super::manager::BeginSwitchRequest) =
        admit(&runtime, webview, &request)?;
    Err(error("HISTORY_PLATFORM_UNSUPPORTED"))
}
