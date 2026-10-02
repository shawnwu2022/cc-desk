//! Independent manager document authentication. No NativeRuntime, CLI registry,
//! profile repository, shell or ordinary App bootstrap is constructed here.
use crate::cli::{profiles::error, types::SafeError};
use parking_lot::Mutex;
use std::sync::{Arc, Once, Weak};
use tauri::{
    http::HeaderMap, utils::config::WindowConfig, webview::PageLoadEvent, Manager, ResourceId,
    ResourceTable, Runtime, Url, Webview, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

pub(crate) const MANAGER_LABEL: &str = "version-manager";
pub(crate) const MANAGER_DOCUMENT_HEADER: &str = "x-cc-desk-version-manager";
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Created,
    Navigating,
    Loading,
    Ready,
    Revoked,
}
struct DocumentState {
    phase: Phase,
    attached: bool,
    initial_handoff: bool,
}
#[derive(Default)]
pub(crate) struct ManagerDocumentRegistry {
    current: Mutex<Option<Weak<ManagerDocumentAuthority>>>,
}
pub(crate) struct ManagerDocumentAuthority {
    transaction_id: String,
    expected_url: Url,
    proof: String,
    state: Mutex<DocumentState>,
}
struct ManagerWitness;
impl tauri::Resource for ManagerWitness {}
pub(crate) struct ManagerDocumentBinding {
    authority: Arc<ManagerDocumentAuthority>,
    witness: Arc<ManagerWitness>,
    witness_id: ResourceId,
}
pub(crate) struct ManagerNativeContext<'a> {
    pub(crate) window_label: &'a str,
    pub(crate) webview_label: &'a str,
    pub(crate) url: &'a Url,
}
fn local_entry(url: &Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == "/version-manager.html"
        && match (url.scheme(), url.host_str()) {
            ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost")) => {
                url.port().is_none()
            }
            ("http" | "https", Some("localhost" | "127.0.0.1" | "[::1]")) => cfg!(debug_assertions),
            _ => false,
        }
}
impl ManagerDocumentRegistry {
    pub(crate) fn create(
        &self,
        transaction_id: &str,
        expected_url: Url,
    ) -> Result<Arc<ManagerDocumentAuthority>, SafeError> {
        super::journal::validate_id(transaction_id)?;
        if !local_entry(&expected_url) {
            return Err(error("FORBIDDEN"));
        }
        let mut current = self.current.lock();
        if current
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|current| current.state.lock().phase != Phase::Revoked)
        {
            return Err(error("DOCUMENT_WINDOW_UNAVAILABLE"));
        }
        let authority = Arc::new(ManagerDocumentAuthority {
            transaction_id: transaction_id.into(),
            expected_url,
            proof: uuid::Uuid::new_v4().simple().to_string(),
            state: Mutex::new(DocumentState {
                phase: Phase::Created,
                attached: false,
                initial_handoff: false,
            }),
        });
        *current = Some(Arc::downgrade(&authority));
        Ok(authority)
    }
}
impl ManagerDocumentAuthority {
    fn same_url(&self, url: &Url) -> bool {
        let mut actual = url.clone();
        actual.set_fragment(None);
        actual == self.expected_url
    }
    pub(crate) fn navigation(&self, url: &Url) -> bool {
        let mut state = self.state.lock();
        if state.phase == Phase::Ready && state.initial_handoff {
            // A denied navigation does not destroy the already-ready page.
            // Actual replacement/load events still revoke and stop the manager.
            return false;
        }
        if state.phase == Phase::Created && self.same_url(url) {
            state.phase = Phase::Navigating;
            true
        } else {
            state.phase = Phase::Revoked;
            false
        }
    }
    pub(crate) fn started(&self, url: &Url) {
        let mut state = self.state.lock();
        state.phase =
            if matches!(state.phase, Phase::Created | Phase::Navigating) && self.same_url(url) {
                Phase::Loading
            } else {
                Phase::Revoked
            };
    }
    pub(crate) fn finished(&self, url: &Url) {
        let mut state = self.state.lock();
        state.phase = if matches!(state.phase, Phase::Loading | Phase::Ready) && self.same_url(url)
        {
            Phase::Ready
        } else {
            Phase::Revoked
        };
    }
    pub(crate) fn revoke(&self) {
        self.state.lock().phase = Phase::Revoked;
    }
    fn pin_initial_handoff(&self) -> Result<(), SafeError> {
        let mut state = self.state.lock();
        if state.phase != Phase::Ready || state.initial_handoff {
            return Err(error("FORBIDDEN"));
        }
        state.initial_handoff = true;
        Ok(())
    }
    fn handoff_pinned(&self) -> bool {
        self.state.lock().initial_handoff
    }
    fn blocks_native_close(&self) -> bool {
        let state = self.state.lock();
        state.initial_handoff && state.phase == Phase::Ready
    }
    #[cfg(test)]
    pub(crate) fn fixture_pin_handoff(&self) -> Result<(), SafeError> {
        self.pin_initial_handoff()
    }
    pub(crate) fn attach(
        self: &Arc<Self>,
        table: &mut ResourceTable,
    ) -> Result<ManagerDocumentBinding, SafeError> {
        let mut state = self.state.lock();
        if state.attached || state.phase == Phase::Revoked {
            return Err(error("FORBIDDEN"));
        }
        let witness = Arc::new(ManagerWitness);
        let witness_id = table.add_arc(witness.clone());
        state.attached = true;
        Ok(ManagerDocumentBinding {
            authority: self.clone(),
            witness,
            witness_id,
        })
    }
    fn bootstrap(&self) -> String {
        include_str!("manager_document_bootstrap.js")
            .replace(
                "__MANAGER_PROOF__",
                &serde_json::to_string(&self.proof).expect("string serialization"),
            )
            .replace(
                "__MANAGER_URL__",
                &serde_json::to_string(self.expected_url.as_str()).expect("string serialization"),
            )
    }
    #[cfg(test)]
    pub(crate) fn test_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(MANAGER_DOCUMENT_HEADER, self.proof.parse().unwrap());
        headers
    }
}
impl ManagerDocumentBinding {
    pub(crate) fn pin_initial_handoff_window<T: Runtime>(
        &self,
        window: &WebviewWindow<T>,
    ) -> Result<(), SafeError> {
        self.ready_for_window(window)?;
        self.authority.pin_initial_handoff()
    }
    pub(crate) fn ready_for_window<T: Runtime>(
        &self,
        window: &WebviewWindow<T>,
    ) -> Result<String, SafeError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            MANAGER_DOCUMENT_HEADER,
            self.authority
                .proof
                .parse()
                .map_err(|_| error("FORBIDDEN"))?,
        );
        let url = window.url().map_err(|_| error("FORBIDDEN"))?;
        self.admit(
            &window.resources_table(),
            &ManagerNativeContext {
                window_label: window.label(),
                webview_label: window.label(),
                url: &url,
            },
            &headers,
        )
    }
    pub(crate) fn pin_initial_handoff_native<T: Runtime>(
        &self,
        webview: &Webview<T>,
    ) -> Result<(), SafeError> {
        self.ready_for_native(webview)?;
        self.authority.pin_initial_handoff()
    }
    pub(crate) fn blocks_native_close(&self) -> bool {
        self.authority.blocks_native_close()
    }
    /// Called only by the UI proof after waiting the exact bound source handle.
    pub(super) fn release_initial_handoff(&self) {
        self.authority.state.lock().initial_handoff = false;
    }
    /// Backend lifecycle check for the exact already-created manager document.
    /// This does not mint a frontend capability or admit a replacement window.
    pub(crate) fn ready_for_native<T: Runtime>(
        &self,
        webview: &Webview<T>,
    ) -> Result<String, SafeError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            MANAGER_DOCUMENT_HEADER,
            self.authority
                .proof
                .parse()
                .map_err(|_| error("FORBIDDEN"))?,
        );
        self.admit_native(webview, &headers)
    }
    pub(crate) fn admit(
        &self,
        table: &ResourceTable,
        context: &ManagerNativeContext<'_>,
        headers: &HeaderMap,
    ) -> Result<String, SafeError> {
        if context.window_label != MANAGER_LABEL
            || context.webview_label != MANAGER_LABEL
            || !self.authority.same_url(context.url)
        {
            return Err(error("FORBIDDEN"));
        }
        let witness = table
            .get::<ManagerWitness>(self.witness_id)
            .map_err(|_| error("FORBIDDEN"))?;
        if !Arc::ptr_eq(&witness, &self.witness) {
            return Err(error("FORBIDDEN"));
        }
        let mut values = headers.get_all(MANAGER_DOCUMENT_HEADER).iter();
        let supplied = values.next().ok_or_else(|| error("FORBIDDEN"))?;
        if values.next().is_some()
            || supplied.as_bytes().len() != 32
            || supplied.as_bytes() != self.authority.proof.as_bytes()
            || self.authority.state.lock().phase != Phase::Ready
        {
            return Err(error("FORBIDDEN"));
        }
        Ok(self.authority.transaction_id.clone())
    }
    pub(crate) fn admit_native<T: Runtime>(
        &self,
        webview: &Webview<T>,
        headers: &HeaderMap,
    ) -> Result<String, SafeError> {
        let window = webview.window();
        let url = webview.url().map_err(|_| error("FORBIDDEN"))?;
        self.admit(
            &webview.resources_table(),
            &ManagerNativeContext {
                window_label: window.label(),
                webview_label: webview.label(),
                url: &url,
            },
            headers,
        )
    }
    pub(crate) fn revoke(&self) {
        self.authority.revoke();
    }
}
impl Drop for ManagerDocumentBinding {
    fn drop(&mut self) {
        self.authority.revoke();
    }
}
pub(crate) struct BoundManager<T: Runtime> {
    pub(crate) window: WebviewWindow<T>,
    pub(crate) binding: ManagerDocumentBinding,
}
pub(crate) fn build_manager<T: Runtime, M: Manager<T>>(
    manager: &M,
    config: &WindowConfig,
    expected_url: Url,
    transaction_id: &str,
    registry: &ManagerDocumentRegistry,
    data_directory: &std::path::Path,
) -> Result<BoundManager<T>, SafeError> {
    if config.label != MANAGER_LABEL
        || !manager.webviews().is_empty()
        || !matches!(&config.url, tauri::WebviewUrl::App(path) if path == std::path::Path::new("version-manager.html"))
    {
        return Err(error("DOCUMENT_WINDOW_UNAVAILABLE"));
    }
    let authority = registry.create(transaction_id, expected_url)?;
    let navigation = authority.clone();
    let loading = authority.clone();
    let listener = Once::new();
    let builder = WebviewWindowBuilder::from_config(manager, config)
        .map_err(|_| error("DOCUMENT_WINDOW_UNAVAILABLE"))?;
    #[cfg(windows)]
    let builder = builder.data_directory(data_directory.to_path_buf());
    #[cfg(not(windows))]
    let _ = data_directory;
    let result = builder
        .initialization_script(authority.bootstrap())
        .on_navigation(move |url| navigation.navigation(url))
        .on_page_load(move |window, payload| {
            listener.call_once(|| {
                let destroyed = loading.clone();
                let app = window.app_handle().clone();
                window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } if destroyed.blocks_native_close() => {
                        api.prevent_close()
                    }
                    WindowEvent::Destroyed => {
                        let pinned = destroyed.handoff_pinned();
                        destroyed.revoke();
                        if pinned {
                            app.exit(3);
                        }
                    }
                    _ => {}
                });
            });
            match payload.event() {
                PageLoadEvent::Started => {
                    let pinned = loading.handoff_pinned();
                    loading.started(payload.url());
                    if pinned {
                        window.app_handle().exit(3);
                    }
                }
                PageLoadEvent::Finished => loading.finished(payload.url()),
            }
        })
        .build();
    let window = match result {
        Ok(window) => window,
        Err(_) => {
            authority.revoke();
            return Err(error("DOCUMENT_WINDOW_UNAVAILABLE"));
        }
    };
    let result = authority.attach(&mut window.resources_table());
    match result {
        Ok(binding) => Ok(BoundManager { window, binding }),
        Err(failure) => {
            authority.revoke();
            let _ = window.destroy();
            Err(failure)
        }
    }
}
