//! D11 document admission scaffold. No registered IPC or live-window changes.
#![allow(dead_code)]

use super::profiles::error;
use super::run_registry::{LaunchStatus, RunRegistry};
use super::snapshot::CallerIdentity;
use super::types::{LaunchRequest, SafeError};
use std::sync::Arc;
use tauri::http::HeaderMap;
use tauri::ipc::InvokeBody;
use tauri::{ResourceId, ResourceTable, Url};

pub(crate) const DOCUMENT_HEADER: &str = "x-cc-desk-document";
pub(crate) const MAX_LAUNCH_WIRE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_QUERY_WIRE_BYTES: usize = 1024;

pub(crate) struct NativeContext<'a> {
    pub(crate) window_label: &'a str,
    pub(crate) webview_label: &'a str,
    pub(crate) url: &'a Url,
}

pub(crate) struct DocumentWitness;
impl tauri::Resource for DocumentWitness {}

pub(crate) struct DocumentAuthority<R> {
    registry: Arc<RunRegistry<R>>,
    caller: CallerIdentity,
    expected_url: Url,
    proof: String,
}

pub(crate) struct DocumentBinding<R> {
    authority: Arc<DocumentAuthority<R>>,
    witness: Arc<DocumentWitness>,
    witness_id: ResourceId,
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

impl<R> DocumentAuthority<R> {
    pub(crate) fn new(registry: Arc<RunRegistry<R>>, expected_url: Url) -> Result<Arc<Self>, SafeError> {
        let caller = registry.activate_window("main")?;
        Ok(Arc::new(Self {
            registry,
            caller,
            expected_url,
            proof: uuid::Uuid::new_v4().simple().to_string(),
        }))
    }

    pub(crate) fn navigation(&self, _url: &Url) -> bool {
        true
    }

    pub(crate) fn started(&self, _url: &Url) {}

    pub(crate) fn finished(&self, _url: &Url) {}

    pub(crate) fn revoke(&self) {}

    pub(crate) fn attach(self: &Arc<Self>, table: &mut ResourceTable) -> Result<DocumentBinding<R>, SafeError> {
        let witness = Arc::new(DocumentWitness);
        let witness_id = table.add_arc(witness.clone());
        Ok(DocumentBinding { authority: self.clone(), witness, witness_id })
    }

    pub(crate) fn bootstrap(&self) -> String {
        include_str!("document_bootstrap.js").to_owned()
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
    pub(crate) fn admit(&self, _table: &ResourceTable, _context: &NativeContext<'_>, _headers: &HeaderMap) -> Result<CallerIdentity, SafeError> {
        Err(error("DOCUMENT_BINDING_NOT_IMPLEMENTED"))
    }

    pub(crate) fn start_request(&self, _table: &ResourceTable, _context: &NativeContext<'_>, _headers: &HeaderMap, _body: &InvokeBody) -> Result<(CallerIdentity, LaunchRequest), SafeError> {
        Err(error("DOCUMENT_BINDING_NOT_IMPLEMENTED"))
    }

    pub(crate) fn query_status(&self, _table: &ResourceTable, _context: &NativeContext<'_>, _headers: &HeaderMap, _body: &InvokeBody) -> Result<LaunchStatus, SafeError> {
        Err(error("DOCUMENT_BINDING_NOT_IMPLEMENTED"))
    }

    #[cfg(test)]
    pub(crate) fn test_witness_id(&self) -> ResourceId {
        self.witness_id
    }
}
