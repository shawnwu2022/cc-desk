use crate::cli::activity_diagnostics::{Capture, Stage, Status, TraceId};
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::WireU64;

fn owner(epoch: u64) -> CallerIdentity {
    CallerIdentity {
        instance_id: "synthetic-instance".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse(&epoch.to_string()).unwrap(),
    }
}

#[test]
fn ActivityDiagnostics_GrantRevokesOldHandles_001() {
    let sink = Capture::default();
    let caller = owner(1);
    let grant = sink.start(&caller, 1).unwrap();
    let trace = sink
        .trace(&caller, Some(&format!("{grant}:{}", "0".repeat(32))))
        .unwrap();
    trace.record(Stage::Backend, Status::Started);
    assert!(sink
        .trace(&owner(2), Some(&format!("{grant}:{}", "0".repeat(32))))
        .is_none());
    assert!(sink.stop(&caller, 2));
    trace.record(Stage::Provider, Status::Ok);
    assert_eq!(sink.snapshot(&caller).unwrap().events.len(), 1);
    assert!(sink.start(&caller, 1).is_none());
    let next = sink.start(&caller, 3).unwrap();
    assert_ne!(grant, next);
    trace.record(Stage::Provider, Status::Ok);
    assert!(sink.snapshot(&caller).unwrap().events.is_empty());
}

#[test]
fn ActivityDiagnostics_BoundedAndFixedTrace_002() {
    let sink = Capture::default();
    let caller = owner(1);
    let grant = sink.start(&caller, 1).unwrap();
    assert!(TraceId::parse("0").is_none());
    assert!(sink
        .trace(&caller, Some(&format!("{grant}:{}", "A".repeat(32))))
        .is_none());
    let trace = sink
        .trace(&caller, Some(&format!("{grant}:{}", "a".repeat(32))))
        .unwrap();
    for _ in 0..600 {
        trace.record(Stage::Provider, Status::Ok);
    }
    let report = sink.snapshot(&caller).unwrap();
    assert_eq!(report.events.len(), 256);
    assert_eq!(report.dropped, 344);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains(&grant));
    assert!(!json.contains("synthetic-instance"));
    assert!(json.len() < 32 * 1024);
}

#[test]
fn ActivityDiagnostics_DuplicateHeader_003() {
    use tauri::http::{HeaderMap, HeaderValue};
    let sink = Capture::default();
    let caller = owner(1);
    let grant = sink.start(&caller, 1).unwrap();
    let direct = HeaderValue::from_str(&format!("{grant}:{}", "a".repeat(32))).unwrap();
    let mut headers = HeaderMap::new();
    headers.append("x-cc-desk-diag-trace", direct.clone());
    assert!(sink.trace_from_headers(&caller, &headers).is_some());
    headers.append("x-cc-desk-diag-trace", direct);
    assert!(sink.trace_from_headers(&caller, &headers).is_none());
    sink.seal();
    assert!(sink.start(&caller, 2).is_none());
    assert!(sink.snapshot(&caller).is_none());
}
