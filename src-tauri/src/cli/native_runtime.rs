//! Application-owned document/launch composition. Never deserialized authority.
use super::document::{native::build_main, DocumentBinding, DOCUMENT_HEADER};
use super::launch_service::{LaunchService, NativeRun, RunAccess};
use super::output_route::{parse_channel, CHANNEL_HEADER};
use super::profiles::error;
use super::run_registry::{LaunchStatus, RunKey};
use super::storage::WorkspaceRepository;
use super::types::SafeError;
use parking_lot::Mutex;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};
use tauri::utils::config::{Config, FrontendDist, WindowConfig};
use tauri::{Manager, Runtime, Url, Webview, WebviewUrl, WebviewWindow};

pub(crate) struct NativeRuntime {
    service: Arc<LaunchService>,
    projections: Arc<super::native_projection::service::ProjectionService>,
    binding: Mutex<Option<Arc<DocumentBinding<NativeRun>>>>,
    initialized: AtomicBool,
}
impl NativeRuntime {
    pub(crate) fn new(service: Arc<LaunchService>) -> Self {
        Self {
            projections: Arc::new(super::native_projection::service::ProjectionService::new(
                service.clone(),
            )),
            service,
            binding: Mutex::new(None),
            initialized: AtomicBool::new(false),
        }
    }
    pub(crate) fn production() -> Result<Self, SafeError> {
        // D14/D15 install the backend supervisor with bounded output/reaping.
        // Until then the service rejects before I/O/spawn, not a discard pump.
        let mut service = LaunchService::new(WorkspaceRepository::production()?, None, None);
        if let Some(observer) = crate::hook_server::observer_host() {
            service = service.with_observer(observer);
        }
        Ok(Self::new(Arc::new(service)))
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
        let descriptor = parse_channel(request.headers());
        let proof = request.headers()[DOCUMENT_HEADER].clone();
        let service = self.service.clone();
        // Keep only validated routing metadata, not the raw body or arbitrary
        // request headers. A replay never evaluates the connect closure.
        tauri::async_runtime::spawn_blocking(move || {
            service.start(&caller, &launch, |_| {
                let id = descriptor?;
                let mut headers = HeaderMap::new();
                headers.insert(DOCUMENT_HEADER, proof);
                headers.insert(
                    CHANNEL_HEADER,
                    format!("__CHANNEL__:{id}")
                        .parse()
                        .map_err(|_| SafeError::invalid("outputChannel"))?,
                );
                binding.channel_native::<_, serde_json::Value>(&webview, &headers)
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
    pub(crate) async fn projection_scope<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<super::native_projection::wire::SourceRef, SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let target: super::native_projection::wire::ScopeTarget =
            decode_projection(request.body(), 4096)?;
        target.validate()?;
        let service = self.projections.clone();
        let admitted = caller.clone();
        let value = tauri::async_runtime::spawn_blocking(move || service.scope(&admitted, &target))
            .await
            .map_err(|_| error("SOURCE_TASK_FAILED"))??;
        self.projections.check_caller(&caller)?;
        Ok(value)
    }
    pub(crate) async fn projection_read<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<super::native_projection::wire::ProjectionResult, SafeError> {
        let caller = self.binding()?.admit_native(webview, request.headers())?;
        let query: super::native_projection::wire::ReadRequest =
            decode_projection(request.body(), 16384)?;
        query.validate()?;
        let service = self.projections.clone();
        let admitted = caller.clone();
        let value = tauri::async_runtime::spawn_blocking(move || service.read(&admitted, &query))
            .await
            .map_err(|_| error("SOURCE_TASK_FAILED"))??;
        self.projections.check_caller(&caller)?;
        Ok(value)
    }
    /// Shared native admission for later input/resize/stop/snapshot adapters.
    /// An acquired access rechecks caller/run ownership again at each operation.
    #[allow(dead_code)] // D15/D17 operation adapters use this authenticated port.
    pub(crate) fn access<T: Runtime>(
        &self,
        webview: &Webview<T>,
        headers: &HeaderMap,
        body: &InvokeBody,
    ) -> Result<RunAccess, SafeError> {
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
        self.service.access(
            &caller,
            &RunKey {
                run_id: query.run_id,
                generation: query.generation,
            },
        )
    }
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

/// Mirrors pinned Tauri 2.10.3 desktop App URL resolution, including its special
/// index.html base-URL rule. Nonlocal external windows cannot become main here.
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
