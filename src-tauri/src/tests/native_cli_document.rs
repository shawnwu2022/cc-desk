use super::native_cli_routed_launch::{Fixture, Lease};
use crate::cli::document::{
    DocumentAuthority, DocumentBinding, DocumentWitness, NativeContext, DOCUMENT_HEADER,
    MAX_LAUNCH_WIRE_BYTES, MAX_QUERY_WIRE_BYTES,
};
use crate::cli::routed_launch::RoutedResource;
use crate::cli::run_registry::{LaunchPhase, LaunchStatus};
use crate::cli::types::SafeError;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::http::{HeaderMap, HeaderValue};
use tauri::ipc::InvokeBody;
use tauri::{ResourceTable, Url};

type Resource = RoutedResource<usize, Lease>;

// CallerIdentity deliberately has no Debug. Never make authority printable
// merely to satisfy Result::unwrap_err or assertion-formatting bounds.
fn rejected<T>(result: Result<T, SafeError>) -> SafeError {
    match result {
        Err(failure) => failure,
        Ok(_) => panic!("expected admission rejection"),
    }
}

struct Harness {
    f: Fixture<usize>,
    authority: Arc<DocumentAuthority<Resource>>,
    binding: DocumentBinding<Resource>,
    table: ResourceTable,
    url: Url,
    headers: HeaderMap,
}

impl Harness {
    fn new(ready: bool) -> Self {
        let mut f = Fixture::new();
        let url: Url = "http://tauri.localhost/index.html".parse().unwrap();
        let authority = DocumentAuthority::new(f.driver.registry().clone(), url.clone()).unwrap();
        f.caller = authority.test_caller();
        let headers = authority.test_headers();
        let mut table = ResourceTable::default();
        let binding = authority.attach(&mut table).unwrap();
        if ready {
            authority.started(&url);
            authority.finished(&url);
        }
        Self {
            f,
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

    fn query(&self) -> Result<LaunchStatus, SafeError> {
        self.binding.query_status(
            &self.table,
            &self.context(),
            &self.headers,
            &InvokeBody::Raw(br#"{"requestId":"owned-request"}"#.to_vec()),
        )
    }

    fn start(&self) -> LaunchStatus {
        let body = InvokeBody::Raw(serde_json::to_vec(&self.f.request).unwrap());
        let (caller, request) = self
            .binding
            .start_request(&self.table, &self.context(), &self.headers, &body)
            .unwrap();
        self.f
            .driver
            .start_routed(
                &caller,
                &request,
                || self.f.freeze(),
                |_| Ok(self.f.lease()),
                |_| Ok(42),
            )
            .unwrap()
    }
}

#[test]
fn D11_Document_RequiresInitialLoadCompletion_01() {
    let h = Harness::new(false);
    assert_eq!(h.query().unwrap_err().code, "FORBIDDEN");
    h.authority.started(&h.url);
    assert_eq!(h.query().unwrap_err().code, "FORBIDDEN");
    h.authority.finished(&h.url);
    assert_eq!(h.query().unwrap_err().code, "LAUNCH_NOT_FOUND");
    assert_eq!(h.start().phase, LaunchPhase::Running);
}

#[test]
fn D11_Document_InjectedLabelsAndExactUrlRequired_02() {
    let h = Harness::new(true);
    let foreign: Url = "http://tauri.localhost/other.html".parse().unwrap();
    let remote: Url = "https://example.invalid/index.html".parse().unwrap();
    for context in [
        NativeContext {
            window_label: "other",
            ..h.context()
        },
        NativeContext {
            webview_label: "other",
            ..h.context()
        },
        NativeContext {
            url: &foreign,
            ..h.context()
        },
        NativeContext {
            url: &remote,
            ..h.context()
        },
    ] {
        let failure = rejected(h.binding.admit(&h.table, &context, &h.headers));
        assert_eq!(failure.code, "FORBIDDEN");
    }
    let caller = h.binding.admit(&h.table, &h.context(), &h.headers).unwrap();
    assert!(caller == h.f.caller);
}

#[test]
fn D11_Document_ProofMissingWrongDuplicateAndOversizedRejected_03() {
    let h = Harness::new(true);
    let mut wrong = HeaderMap::new();
    wrong.insert(
        DOCUMENT_HEADER,
        HeaderValue::from_static("00000000000000000000000000000000"),
    );
    let mut duplicate = h.headers.clone();
    duplicate.append(DOCUMENT_HEADER, h.headers[DOCUMENT_HEADER].clone());
    let mut oversized = HeaderMap::new();
    oversized.insert(DOCUMENT_HEADER, "x".repeat(4096).parse().unwrap());
    for headers in [HeaderMap::new(), wrong, duplicate, oversized] {
        let failure = rejected(h.binding.admit(&h.table, &h.context(), &headers));
        assert_eq!(failure.code, "FORBIDDEN");
    }
    assert!(h.binding.admit(&h.table, &h.context(), &h.headers).is_ok());
}

#[test]
fn D11_Document_RemoteBootstrapCannotRotateRegistry_04() {
    let h = Harness::new(true);
    for url in [
        "https://example.invalid/",
        "file:///private/index.html",
        "http://tauri.localhost.evil.invalid/",
        "http://user@tauri.localhost/",
    ] {
        assert!(
            DocumentAuthority::new(h.f.driver.registry().clone(), url.parse().unwrap()).is_err()
        );
    }
    let failure =
        h.f.driver
            .registry()
            .status(&h.f.caller, "owned-request")
            .unwrap_err();
    assert_eq!(failure.code, "LAUNCH_NOT_FOUND");
}

#[test]
fn D11_Document_ReloadAndLateFinishNeverReauthorize_05() {
    let h = Harness::new(true);
    let status = h.start();
    h.authority.started(&h.url);
    h.authority.finished(&h.url);
    h.authority.finished(&h.url);
    assert_eq!(h.query().unwrap_err().code, "FORBIDDEN");
    let failure =
        h.f.driver
            .registry()
            .resource(&h.f.caller, &status.run)
            .unwrap_err();
    assert_eq!(failure.code, "FORBIDDEN");
    assert_eq!(h.f.drops.load(Ordering::SeqCst), 0);
}

#[test]
fn D11_Document_NavigationRevokesBeforeCompletion_06() {
    let h = Harness::new(true);
    assert!(!h.authority.navigation(&h.url));
    h.authority.finished(&h.url);
    assert_eq!(h.query().unwrap_err().code, "FORBIDDEN");
    let early = Harness::new(false);
    early.authority.finished(&early.url);
    early.authority.started(&early.url);
    early.authority.finished(&early.url);
    assert_eq!(early.query().unwrap_err().code, "FORBIDDEN");
}

#[test]
fn D11_Document_NativeResourceIdentityNotLabelOrId_07() {
    let mut h = Harness::new(true);
    let other_table = ResourceTable::default();
    let failure = rejected(h.binding.admit(&other_table, &h.context(), &h.headers));
    assert_eq!(failure.code, "FORBIDDEN");
    h.table
        .replace(h.binding.test_witness_id(), DocumentWitness);
    let failure = rejected(h.binding.admit(&h.table, &h.context(), &h.headers));
    assert_eq!(failure.code, "FORBIDDEN");
}

#[test]
fn D11_Document_BindingCannotAttachToSecondWebview_08() {
    let h = Harness::new(true);
    assert!(h.authority.attach(&mut ResourceTable::default()).is_err());
    assert_eq!(h.start().phase, LaunchPhase::Running);
}

#[test]
fn D11_Document_RawStartPreservesRequestAndRejectsForgedOwner_09() {
    let h = Harness::new(true);
    let body = InvokeBody::Raw(serde_json::to_vec(&h.f.request).unwrap());
    let (caller, request) = h
        .binding
        .start_request(&h.table, &h.context(), &h.headers, &body)
        .unwrap();
    assert!(caller == h.f.caller);
    assert_eq!(request, h.f.request);
    let mut forged = serde_json::to_value(&h.f.request).unwrap();
    forged["ownerWindowId"] = "main".into();
    let body = InvokeBody::Raw(serde_json::to_vec(&forged).unwrap());
    let failure = rejected(
        h.binding
            .start_request(&h.table, &h.context(), &h.headers, &body),
    );
    assert_eq!(failure.code, "INVALID_REQUEST");
}

#[test]
fn D11_Document_AuthenticatesBeforeBoundedRawDecode_10() {
    let h = Harness::new(true);
    let body = InvokeBody::Raw(vec![b' '; MAX_LAUNCH_WIRE_BYTES + 1]);
    let failure =
        rejected(
            h.binding
                .start_request(&h.table, &h.context(), &HeaderMap::new(), &body),
        );
    assert_eq!(failure.code, "FORBIDDEN");
    let failure = rejected(
        h.binding
            .start_request(&h.table, &h.context(), &h.headers, &body),
    );
    assert_eq!(failure.code, "REQUEST_TOO_LARGE");
    let json = InvokeBody::Json(serde_json::to_value(&h.f.request).unwrap());
    let failure = rejected(
        h.binding
            .start_request(&h.table, &h.context(), &h.headers, &json),
    );
    assert_eq!(failure.code, "RAW_BODY_REQUIRED");
    let query = InvokeBody::Raw(vec![b' '; MAX_QUERY_WIRE_BYTES + 1]);
    let failure = h
        .binding
        .query_status(&h.table, &h.context(), &h.headers, &query)
        .unwrap_err();
    assert_eq!(failure.code, "REQUEST_TOO_LARGE");
}

#[test]
fn D11_Document_QueryOnlyReturnsOriginalRetainedResult_11() {
    let h = Harness::new(true);
    let status = h.start();
    assert_eq!(h.query().unwrap(), status);
    h.f.driver.registry().mark_exited(&status.run).unwrap();
    assert_eq!(h.query().unwrap().phase, LaunchPhase::Exited);
    let unknown = InvokeBody::Raw(br#"{"requestId":"not-started"}"#.to_vec());
    let failure = h
        .binding
        .query_status(&h.table, &h.context(), &h.headers, &unknown)
        .unwrap_err();
    assert_eq!(failure.code, "LAUNCH_NOT_FOUND");
    assert_eq!(h.f.drops.load(Ordering::SeqCst), 0);
}

#[test]
fn D11_Document_RevokeDuringRoutePreventsSpawn_12() {
    let h = Harness::new(true);
    let caller = h.binding.admit(&h.table, &h.context(), &h.headers).unwrap();
    let result =
        h.f.driver
            .start_routed(
                &caller,
                &h.f.request,
                || h.f.freeze(),
                |_| {
                    h.authority.revoke();
                    Ok(h.f.lease())
                },
                |_| panic!("revoked document must not spawn"),
            )
            .unwrap();
    assert_eq!(result.phase, LaunchPhase::Cancelled);
    assert_eq!(h.f.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn D11_Document_DropRevokesWithoutDiscardingOwnedRun_13() {
    let h = Harness::new(true);
    let status = h.start();
    let resource =
        h.f.driver
            .registry()
            .resource(&h.f.caller, &status.run)
            .unwrap();
    drop(h.binding);
    let failure =
        h.f.driver
            .registry()
            .status(&h.f.caller, "owned-request")
            .unwrap_err();
    assert_eq!(failure.code, "FORBIDDEN");
    assert_eq!(h.f.drops.load(Ordering::SeqCst), 0);
    h.f.driver.registry().mark_exited(&status.run).unwrap();
    h.f.driver.registry().retire(&status.run).unwrap();
    assert_eq!(h.f.drops.load(Ordering::SeqCst), 0);
    drop(resource);
    assert_eq!(h.f.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn D11_Document_StaleDestroyCannotRevokeReplacement_14() {
    let h = Harness::new(true);
    let next = DocumentAuthority::new(h.f.driver.registry().clone(), h.url.clone()).unwrap();
    assert_ne!(
        h.headers[DOCUMENT_HEADER],
        next.test_headers()[DOCUMENT_HEADER]
    );
    assert!(h.f.caller != next.test_caller());
    next.started(&h.url);
    next.finished(&h.url);
    let mut table = ResourceTable::default();
    let binding = next.attach(&mut table).unwrap();
    h.authority.revoke();
    h.authority.finished(&h.url);
    let query = InvokeBody::Raw(br#"{"requestId":"owned-request"}"#.to_vec());
    let failure = binding
        .query_status(&table, &h.context(), &next.test_headers(), &query)
        .unwrap_err();
    assert_eq!(failure.code, "LAUNCH_NOT_FOUND");
    assert_eq!(h.query().unwrap_err().code, "FORBIDDEN");
}

#[test]
fn D11_Document_QuerySchemaAndDiagnosticsAreSafe_15() {
    let h = Harness::new(true);
    for bytes in [
        br#"{"requestId":"owned-request","epoch":"2"}"#.as_slice(),
        b"not-json-private-value",
    ] {
        let body = InvokeBody::Raw(bytes.to_vec());
        let failure = h
            .binding
            .query_status(&h.table, &h.context(), &h.headers, &body)
            .unwrap_err();
        assert_eq!(failure.code, "INVALID_REQUEST");
        assert!(!format!("{failure:?}").contains("private-value"));
    }
    let debug = format!("{:?}{:?}", h.authority, h.binding);
    assert!(!debug.contains(h.headers[DOCUMENT_HEADER].to_str().unwrap()));
    assert!(!debug.contains(h.url.as_str()));
}
