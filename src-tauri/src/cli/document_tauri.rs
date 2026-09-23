//! Compiled native adapter. Not installed into the application startup or IPC.

use super::{decode_start, DocumentAuthority, DocumentBinding, NativeContext, DOCUMENT_HEADER};
use crate::cli::output_route::{parse_channel, OutputRoute};
use crate::cli::profiles::error;
use crate::cli::run_registry::{LaunchStatus, RunRegistry};
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{LaunchRequest, SafeError};
use std::sync::{Arc, Once};
use tauri::http::HeaderMap;
use tauri::ipc::{JavaScriptChannelId, Request};
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
        .initialization_script(authority.bootstrap())
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
        // The real injected Webview's table supplies the witness. The temporary
        // guard ends at this statement, before registry access or typed decode.
        let caller = self.admit_witness(&webview.resources_table(), &context, headers)?;
        self.authority.registry.check_caller(&caller)?;
        Ok(caller)
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

    /// Call only from the reservation winner's connect callback. A replay must
    /// return its retained status without constructing or dropping any Channel.
    pub(crate) fn channel_native<T: Runtime, E>(
        self: &Arc<Self>,
        webview: &Webview<T>,
        headers: &HeaderMap,
    ) -> Result<OutputRoute<E>, SafeError>
    where
        R: Send + Sync + 'static,
    {
        self.admit_native(webview, headers)?;
        let id = parse_channel(headers)?;
        let binding = Arc::downgrade(self);
        let target = webview.clone();
        // Retain only the validated proof, not arbitrary caller-supplied headers.
        let mut proof_headers = HeaderMap::new();
        proof_headers.insert(DOCUMENT_HEADER, headers[DOCUMENT_HEADER].clone());
        self.output_routes.bind(
            id,
            Box::new(move || {
                // A route must not keep its document/registry alive. Reuse the
                // same native admission predicate for every subsequent send.
                let binding = binding.upgrade().ok_or_else(|| error("FORBIDDEN"))?;
                binding.admit_native(&target, &proof_headers).map(|_| ())
            }),
            || {
                let descriptor: JavaScriptChannelId = format!("__CHANNEL__:{id}")
                    .parse()
                    .map_err(|_| SafeError::invalid("outputChannel"))?;
                Ok(descriptor.channel_on(webview.clone()))
            },
        )
    }
}
