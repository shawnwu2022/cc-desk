//! Backend-owned document admission. No endpoint is registered by this module.
#![allow(dead_code)]

#[path = "document_tauri.rs"]
pub(crate) mod native;

use super::output_route::OutputRoutes;
use super::profiles::error;
use super::run_registry::{LaunchStatus, RunRegistry};
use super::snapshot::CallerIdentity;
use super::types::{LaunchRequest, SafeError};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::sync::Arc;
use tauri::http::HeaderMap;
use tauri::ipc::InvokeBody;
use tauri::{ResourceId, ResourceTable, Url};

pub(crate) const DOCUMENT_HEADER: &str = "x-cc-desk-document";
pub(crate) const MAX_LAUNCH_WIRE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_QUERY_WIRE_BYTES: usize = 1024;

/// The native adapter supplies these values, never deserialized caller fields.
pub(crate) struct NativeContext<'a> {
    pub(crate) window_label: &'a str,
    pub(crate) webview_label: &'a str,
    pub(crate) url: &'a Url,
}

pub(crate) struct DocumentWitness;
impl tauri::Resource for DocumentWitness {}

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
}

pub(crate) struct DocumentAuthority<R> {
    registry: Arc<RunRegistry<R>>,
    caller: CallerIdentity,
    expected_url: Url,
    proof: String,
    state: Mutex<DocumentState>,
}

/// Non-cloneable owner; dropping it revokes authority but not owned processes.
pub(crate) struct DocumentBinding<R> {
    authority: Arc<DocumentAuthority<R>>,
    witness: Arc<DocumentWitness>,
    witness_id: ResourceId,
    output_routes: OutputRoutes,
}

impl<R> std::fmt::Debug for DocumentAuthority<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DocumentAuthority(<redacted>)")
    }
}

impl<R> std::fmt::Debug for DocumentBinding<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DocumentBinding(<redacted>)")
    }
}

fn local_document(url: &Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match (url.scheme(), url.host_str()) {
        ("tauri", Some("localhost")) => url.port().is_none(),
        ("http" | "https", Some("tauri.localhost")) => url.port().is_none(),
        ("http" | "https", Some("localhost" | "127.0.0.1" | "[::1]")) => {
            cfg!(debug_assertions)
        }
        _ => false,
    }
}

impl<R> DocumentAuthority<R> {
    /// Called by the serialized backend window lifecycle, not an IPC command.
    pub(crate) fn new(
        registry: Arc<RunRegistry<R>>,
        mut expected_url: Url,
    ) -> Result<Arc<Self>, SafeError> {
        if !local_document(&expected_url) {
            return Err(error("FORBIDDEN"));
        }
        expected_url.set_fragment(None);
        let proof = uuid::Uuid::new_v4().simple().to_string();
        let caller = registry.activate_window("main")?;
        Ok(Arc::new(Self {
            registry,
            caller,
            expected_url,
            proof,
            state: Mutex::new(DocumentState {
                phase: Phase::Created,
                attached: false,
            }),
        }))
    }

    fn same_document_url(&self, url: &Url) -> bool {
        let mut actual = url.clone();
        actual.set_fragment(None);
        actual == self.expected_url
    }

    /// Only the first navigation can proceed. Even same-URL reloads revoke.
    pub(crate) fn navigation(&self, url: &Url) -> bool {
        let mut state = self.state.lock();
        if state.phase == Phase::Created && self.same_document_url(url) {
            state.phase = Phase::Navigating;
            return true;
        }
        drop(state);
        self.revoke();
        false
    }

    pub(crate) fn started(&self, url: &Url) {
        let mut state = self.state.lock();
        if matches!(state.phase, Phase::Created | Phase::Navigating) && self.same_document_url(url)
        {
            state.phase = Phase::Loading;
            return;
        }
        drop(state);
        self.revoke();
    }

    pub(crate) fn finished(&self, url: &Url) {
        let mut state = self.state.lock();
        if matches!(state.phase, Phase::Loading | Phase::Ready) && self.same_document_url(url) {
            state.phase = Phase::Ready;
            return;
        }
        drop(state);
        self.revoke();
    }

    pub(crate) fn revoke(&self) {
        self.state.lock().phase = Phase::Revoked;
        // Only this exact epoch can be revoked. A late old-window callback
        // cannot revoke a newer main window. No resource is retired or killed.
        let _ = self.registry.revoke_window(&self.caller);
    }

    pub(crate) fn attach(
        self: &Arc<Self>,
        table: &mut ResourceTable,
    ) -> Result<DocumentBinding<R>, SafeError> {
        let mut state = self.state.lock();
        if state.attached || state.phase == Phase::Revoked {
            return Err(error("FORBIDDEN"));
        }
        let witness = Arc::new(DocumentWitness);
        let witness_id = table.add_arc(witness.clone());
        state.attached = true;
        Ok(DocumentBinding {
            authority: self.clone(),
            witness,
            witness_id,
            output_routes: OutputRoutes::new(128),
        })
    }

    pub(crate) fn bootstrap(&self) -> String {
        // Replace proof first: a selected URL containing the proof placeholder
        // is data and must not undergo a second template expansion.
        let proof = serde_json::to_string(&self.proof).expect("string serialization");
        let url = serde_json::to_string(self.expected_url.as_str()).expect("string serialization");
        include_str!("document_bootstrap.js")
            .replace("__CC_DESK_DOCUMENT_PROOF__", &proof)
            .replace("__CC_DESK_DOCUMENT_URL__", &url)
    }

    #[cfg(test)]
    pub(crate) fn test_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(DOCUMENT_HEADER, self.proof.parse().unwrap());
        headers
    }

    #[cfg(test)]
    pub(crate) fn test_caller(&self) -> CallerIdentity {
        self.caller.clone()
    }
}

impl<R> DocumentBinding<R> {
    /// Admission is not a reusable capability. The coordinator/registry must
    /// revalidate the returned identity before launch effects or status access.
    pub(crate) fn admit(
        &self,
        table: &ResourceTable,
        context: &NativeContext<'_>,
        headers: &HeaderMap,
    ) -> Result<CallerIdentity, SafeError> {
        let caller = self.admit_witness(table, context, headers)?;
        self.authority.registry.check_caller(&caller)?;
        Ok(caller)
    }

    /// Check native identity without taking the registry lock. The native
    /// adapter releases its resource-table guard before checking the epoch.
    fn admit_witness(
        &self,
        table: &ResourceTable,
        context: &NativeContext<'_>,
        headers: &HeaderMap,
    ) -> Result<CallerIdentity, SafeError> {
        if context.window_label != "main"
            || context.webview_label != "main"
            || !self.authority.same_document_url(context.url)
        {
            return Err(error("FORBIDDEN"));
        }
        let witness = table
            .get::<DocumentWitness>(self.witness_id)
            .map_err(|_| error("FORBIDDEN"))?;
        if !Arc::ptr_eq(&witness, &self.witness) {
            return Err(error("FORBIDDEN"));
        }
        let mut values = headers.get_all(DOCUMENT_HEADER).iter();
        let supplied = values.next().ok_or_else(|| error("FORBIDDEN"))?;
        if values.next().is_some()
            || supplied.as_bytes().len() != 32
            || supplied.as_bytes() != self.authority.proof.as_bytes()
            || self.authority.state.lock().phase != Phase::Ready
        {
            return Err(error("FORBIDDEN"));
        }
        Ok(self.authority.caller.clone())
    }

    pub(crate) fn start_request(
        &self,
        table: &ResourceTable,
        context: &NativeContext<'_>,
        headers: &HeaderMap,
        body: &InvokeBody,
    ) -> Result<(CallerIdentity, LaunchRequest), SafeError> {
        let caller = self.admit(table, context, headers)?;
        Ok((caller, decode_start(body)?))
    }

    pub(crate) fn query_status(
        &self,
        table: &ResourceTable,
        context: &NativeContext<'_>,
        headers: &HeaderMap,
        body: &InvokeBody,
    ) -> Result<LaunchStatus, SafeError> {
        let caller = self.admit(table, context, headers)?;
        self.query_after_admission(&caller, body)
    }

    fn query_after_admission(
        &self,
        caller: &CallerIdentity,
        body: &InvokeBody,
    ) -> Result<LaunchStatus, SafeError> {
        let query: StatusQuery = decode_raw(body, MAX_QUERY_WIRE_BYTES)?;
        self.authority.registry.status(caller, &query.request_id)
    }

    #[cfg(test)]
    pub(crate) fn test_witness_id(&self) -> ResourceId {
        self.witness_id
    }
}

impl<R> Drop for DocumentBinding<R> {
    fn drop(&mut self) {
        self.authority.revoke();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusQuery {
    request_id: String,
}

fn decode_raw<T: DeserializeOwned>(body: &InvokeBody, limit: usize) -> Result<T, SafeError> {
    let InvokeBody::Raw(bytes) = body else {
        return Err(error("RAW_BODY_REQUIRED"));
    };
    if bytes.len() > limit {
        return Err(error("REQUEST_TOO_LARGE"));
    }
    // Do not format serde errors: paths, prompt fragments and user values may
    // be embedded in diagnostics. The default serde recursion limit remains.
    serde_json::from_slice(bytes).map_err(|_| error("INVALID_REQUEST"))
}

fn decode_start(body: &InvokeBody) -> Result<LaunchRequest, SafeError> {
    let request: LaunchRequest = decode_raw(body, MAX_LAUNCH_WIRE_BYTES)?;
    request.validate()?;
    Ok(request)
}
