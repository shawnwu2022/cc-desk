//! Compiled native adapter. Not installed into the application startup or IPC.

use super::{decode_start, DocumentAuthority, DocumentBinding, NativeContext};
use crate::cli::profiles::error;
use crate::cli::run_registry::{LaunchStatus, RunRegistry};
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{LaunchRequest, SafeError};
use std::sync::{Arc, Once};
use tauri::http::HeaderMap;
use tauri::ipc::Request;
use tauri::utils::config::WindowConfig;
use tauri::webview::PageLoadEvent;
use tauri::{Manager, Runtime, Url, Webview, WebviewWindow, WebviewWindowBuilder, WindowEvent};

pub(crate) struct BoundWindow<T: Runtime, R> {
    pub(crate) window: WebviewWindow<T>,
    pub(crate) binding: DocumentBinding<R>,
}

/// The application must serialize main-window creation and own the returned
/// binding. Use setup or an async command, never a synchronous Windows command.
/// This helper is deliberately not called until live lifecycle acceptance.
pub(crate) fn build_main<T, R, M>(
    manager: &M,
    config: &WindowConfig,
    expected_url: Url,
    registry: Arc<RunRegistry<R>>,
) -> Result<BoundWindow<T, R>, SafeError>
where
    T: Runtime,
    R: Send + Sync + 'static,
    M: Manager<T>,
{
    if config.label != "main" || manager.get_webview_window("main").is_some() {
        return Err(error("DOCUMENT_WINDOW_UNAVAILABLE"));
    }
    let builder = WebviewWindowBuilder::from_config(manager, config)
        .map_err(|_| error("DOCUMENT_WINDOW_UNAVAILABLE"))?;
    let authority = DocumentAuthority::new(registry, expected_url)?;
    let navigation = authority.clone();
    let loading = authority.clone();
    let destroy_listener = Once::new();
    let built = builder
        .initialization_script(&authority.bootstrap())
        .on_navigation(move |url| navigation.navigation(url))
        .on_page_load(move |window, payload| {
            // Install before a page can become Ready, including page events
            // delivered while build() is still in flight. Do not look up main
            // by label inside a delayed destruction callback.
            destroy_listener.call_once(|| {
                let destroyed = loading.clone();
                window.on_window_event(move |event| {
                    if matches!(event, WindowEvent::Destroyed) {
                        destroyed.revoke();
                    }
                });
            });
            match payload.event() {
                PageLoadEvent::Started => loading.started(payload.url()),
                PageLoadEvent::Finished => loading.finished(payload.url()),
            }
        })
        .build();
    let window = match built {
        Ok(window) => window,
        Err(_) => {
            authority.revoke();
            return Err(error("DOCUMENT_WINDOW_UNAVAILABLE"));
        }
    };
    let binding = authority.attach(&mut window.resources_table());
    match binding {
        Ok(binding) => Ok(BoundWindow { window, binding }),
        Err(failure) => {
            authority.revoke();
            let _ = window.destroy();
            Err(failure)
        }
    }
}

impl<R> DocumentBinding<R> {
    fn admit_native<T: Runtime>(
        &self,
        webview: &Webview<T>,
        headers: &HeaderMap,
    ) -> Result<CallerIdentity, SafeError> {
        let window = webview.window();
        let url = webview.url().map_err(|_| error("FORBIDDEN"))?;
        let context = NativeContext {
            window_label: window.label(),
            webview_label: webview.label(),
            url: &url,
        };
        // The real injected Webview's table supplies the witness. Its lock is
        // released on return, before any typed JSON decode or registry work.
        self.admit(&webview.resources_table(), &context, headers)
    }

    pub(crate) fn start_native<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<(CallerIdentity, LaunchRequest), SafeError> {
        let caller = self.admit_native(webview, request.headers())?;
        Ok((caller, decode_start(request.body())?))
    }

    pub(crate) fn query_native<T: Runtime>(
        &self,
        webview: &Webview<T>,
        request: &Request<'_>,
    ) -> Result<LaunchStatus, SafeError> {
        let caller = self.admit_native(webview, request.headers())?;
        self.query_after_admission(&caller, request.body())
    }
}
