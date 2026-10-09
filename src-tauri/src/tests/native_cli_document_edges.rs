use crate::cli::document::{
    DocumentAuthority, DocumentBinding, NativeContext, MAX_LAUNCH_WIRE_BYTES, MAX_QUERY_WIRE_BYTES,
};
use crate::cli::run_registry::{LaunchStatus, RunRegistry};
use crate::cli::types::SafeError;
use std::sync::Arc;
use tauri::http::HeaderMap;
use tauri::ipc::InvokeBody;
use tauri::{ResourceTable, Url};

struct Document {
    registry: Arc<RunRegistry<usize>>,
    authority: Arc<DocumentAuthority<usize>>,
    binding: DocumentBinding<usize>,
    table: ResourceTable,
    url: Url,
    headers: HeaderMap,
}

impl Document {
    fn new(ready: bool) -> Self {
        let registry = Arc::new(RunRegistry::new(4));
        let url: Url = "http://tauri.localhost/index.html".parse().unwrap();
        let authority = DocumentAuthority::new(registry.clone(), url.clone()).unwrap();
        let headers = authority.test_headers();
        let mut table = ResourceTable::default();
        let binding = authority.attach(&mut table).unwrap();
        if ready {
            authority.started(&url);
            authority.finished(&url);
        }
        Self {
            registry,
            authority,
            binding,
            table,
            url,
            headers,
        }
    }

    fn context(&self) -> NativeContext<'_> {
        NativeContext {
            window_label: "main",
            webview_label: "main",
            url: &self.url,
        }
    }

    fn query(&self, body: &InvokeBody) -> Result<LaunchStatus, SafeError> {
        self.binding
            .query_status(&self.table, &self.context(), &self.headers, body)
    }
}

#[test]
fn D11_Document_EpochInvalidationPrecedesTypedDecode_16() {
    for replacement in [false, true] {
        let d = Document::new(true);
        if replacement {
            d.registry.activate_window("main").unwrap();
        } else {
            d.registry
                .revoke_window(&d.authority.test_caller())
                .unwrap();
        }
        // Deliberately do NOT deliver a document/navigation/destroy callback.
        // The original gate still says Ready, but the registry already revoked
        // its epoch. It must not authorize typed parsing with a stale proof.
        let body = InvokeBody::Raw(b"not-json-private-value".to_vec());
        assert_eq!(d.query(&body).unwrap_err().code, "FORBIDDEN");
        let body = InvokeBody::Raw(vec![b' '; MAX_LAUNCH_WIRE_BYTES + 1]);
        let result = d
            .binding
            .start_request(&d.table, &d.context(), &d.headers, &body);
        let Err(failure) = result else {
            panic!("stale authority must not admit a start request");
        };
        assert_eq!(failure.code, "FORBIDDEN");
        assert!(d.binding.admit(&d.table, &d.context(), &d.headers).is_err());
    }
}

#[test]
fn D11_Document_FirstNavigationWorksButSecondCannotRecover_17() {
    for second_navigation in [false, true] {
        let d = Document::new(false);
        assert!(d.authority.navigation(&d.url));
        if second_navigation {
            assert!(!d.authority.navigation(&d.url));
        }
        d.authority.started(&d.url);
        d.authority.finished(&d.url);
        d.authority.finished(&d.url);
        let body = InvokeBody::Raw(br#"{"requestId":"missing"}"#.to_vec());
        assert_eq!(
            d.query(&body).unwrap_err().code,
            if second_navigation {
                "FORBIDDEN"
            } else {
                "LAUNCH_NOT_FOUND"
            }
        );
    }
}

#[test]
fn D11_Document_QueryByteBoundaryIsInclusive_18() {
    let d = Document::new(true);
    let mut bytes = br#"{"requestId":"missing"}"#.to_vec();
    bytes.resize(MAX_QUERY_WIRE_BYTES, b' ');
    let body = InvokeBody::Raw(bytes.clone());
    assert_eq!(d.query(&body).unwrap_err().code, "LAUNCH_NOT_FOUND");
    bytes.push(b' ');
    assert_eq!(
        d.query(&InvokeBody::Raw(bytes)).unwrap_err().code,
        "REQUEST_TOO_LARGE"
    );
}

#[test]
fn D11_Document_ProofCannotCrossBackendInstances_19() {
    let first = Document::new(true);
    let other = Document::new(true);
    let body = InvokeBody::Raw(br#"{"requestId":"missing"}"#.to_vec());
    assert_eq!(first.query(&body).unwrap_err().code, "LAUNCH_NOT_FOUND");
    assert_eq!(other.query(&body).unwrap_err().code, "LAUNCH_NOT_FOUND");
    let failure = other
        .binding
        .query_status(&other.table, &other.context(), &first.headers, &body)
        .unwrap_err();
    assert_eq!(failure.code, "FORBIDDEN");
}
