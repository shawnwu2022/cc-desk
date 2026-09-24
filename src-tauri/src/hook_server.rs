//! Application wiring for the authenticated loopback observer. No legacy bypass.
use crate::observer_host::{ObserverHost, PreparedObservation};
use crate::observer_registry::{ObserverDelivery, ObserverRegistry, ObserverRun};
use std::sync::{Arc, LazyLock, OnceLock};
use tauri::{AppHandle, Emitter, EventTarget, Manager};

static HOOK_PORT: OnceLock<u16> = OnceLock::new();
static OBSERVERS: LazyLock<Arc<ObserverRegistry>> =
    LazyLock::new(|| Arc::new(ObserverRegistry::new()));
pub fn get_port() -> Option<u16> {
    HOOK_PORT.get().copied()
}

pub(crate) fn observer_host() -> Option<Arc<ObserverHost>> {
    let plugin = dirs::home_dir()?.join(".cc-box/claude-plugin");
    Some(Arc::new(ObserverHost::new(
        OBSERVERS.clone(),
        plugin,
        Arc::new(get_port),
    )))
}

/// Legacy Claude has a fresh UUID for each PTY. Its observer additionally retains
/// this exact main-document lifetime; reloading a window cannot inherit authority.
pub(crate) fn prepare_legacy(app: &AppHandle, pty_id: &str) -> Option<PreparedObservation> {
    let runtime = app.try_state::<Arc<crate::cli::native_runtime::NativeRuntime>>()?;
    let binding = runtime.binding().ok()?;
    let weak = Arc::downgrade(&binding);
    let authorize = Arc::new(move || {
        weak.upgrade()
            .is_some_and(|binding| binding.observation_alive())
    });
    observer_host()?
        .prepare(
            ObserverRun {
                run_id: pty_id.into(),
                generation: 1,
            },
            ObserverDelivery::legacy(pty_id),
            authorize,
        )
        .ok()
}

pub async fn init(app: AppHandle) {
    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(_) => {
            log::warn!("Observer unavailable: bind failed");
            return;
        }
    };
    let Ok(address) = listener.local_addr() else {
        return;
    };
    if HOOK_PORT.set(address.port()).is_err() {
        return;
    }
    let publish = Arc::new(
        move |delivery: &ObserverDelivery, payload: crate::hook_events::HookPayload| {
            if delivery.legacy_pty.is_some() && payload.event_name == "SessionStart" {
                // Cache invalidation only; event cwd/transcript never grants a read path.
                crate::store::invalidate_project_path_mapping();
            }
            let topic = if delivery.legacy_pty.is_some() {
                "hook-event"
            } else {
                "native-observation"
            };
            // Only the owner WebView, never an application-wide broadcast. No prompt,
            // assistant text, raw errors or environment values reach this payload.
            app.emit_to(
                EventTarget::webview_window(delivery.window_label.clone()),
                topic,
                payload,
            )
            .map_err(|_| ())
        },
    );
    let router = crate::observer_http::router(OBSERVERS.clone(), publish);
    tokio::spawn(async move {
        if axum::serve(listener, router).await.is_err() {
            log::warn!("Observer unavailable: server ended");
        }
    });
}
