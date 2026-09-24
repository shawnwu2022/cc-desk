use crate::hook_config::deployment_id;
use crate::hook_events::HookPayload;
use crate::observer_registry::{
    parse_observer_headers, ObserverAccept, ObserverBinding, ObserverRegistry, ObserverRun,
    ObserverSource, MAX_OBSERVER_PAYLOAD,
};
use axum::http::HeaderMap;

fn run(id: &str, generation: u32) -> ObserverRun {
    ObserverRun {
        run_id: id.to_string(),
        generation,
    }
}

fn payload(name: &str) -> Vec<u8> {
    format!(
        r#"{{"hook_event_name":"{name}","session_id":"native-session","cwd":"/untrusted/payload/path"}}"#
    )
    .into_bytes()
}

#[test]
fn D13_Observer_CapabilityRunReplayAndPayloadBoundary_001() {
    let registry = ObserverRegistry::new();
    let current = run("run-current", 2);
    let binding = registry
        .attach(
            current.clone(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ObserverSource::ClaudeHook,
        )
        .unwrap();

    let accepted = registry
        .accept_event(&binding, "event-1", &payload("UserPromptSubmit"))
        .unwrap();
    assert!(matches!(accepted, ObserverAccept::Accepted(_)));

    let duplicate = registry
        .accept_event(&binding, "event-1", &payload("Stop"))
        .unwrap();
    assert_eq!(duplicate, ObserverAccept::Duplicate);

    let forged = ObserverBinding {
        run: current.clone(),
        capability: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        source: ObserverSource::ClaudeHook,
    };
    assert_eq!(
        registry
            .accept_event(&forged, "event-2", &payload("Stop"))
            .unwrap_err()
            .code,
        "OBSERVER_FORBIDDEN"
    );

    let stale = ObserverBinding {
        run: run("run-current", 1),
        capability: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        source: ObserverSource::ClaudeHook,
    };
    assert_eq!(
        registry
            .accept_event(&stale, "event-3", &payload("Stop"))
            .unwrap_err()
            .code,
        "OBSERVER_FORBIDDEN"
    );

    let mut oversized = vec![b' '; MAX_OBSERVER_PAYLOAD + 1];
    oversized[0] = b'{';
    assert_eq!(
        registry
            .accept_event(&binding, "event-4", &oversized)
            .unwrap_err()
            .code,
        "OBSERVER_PAYLOAD_TOO_LARGE"
    );
}

#[test]
fn D13_Observer_ReattachRevokesOldCapabilityAndSourcesAreExplicit_002() {
    let registry = ObserverRegistry::new();
    let current = run("run-current", 7);
    let old = registry
        .attach(
            current.clone(),
            "cccccccccccccccccccccccccccccccc".to_string(),
            ObserverSource::ClaudeHook,
        )
        .unwrap();
    let next = registry
        .attach(
            current.clone(),
            "dddddddddddddddddddddddddddddddd".to_string(),
            ObserverSource::ClaudeHook,
        )
        .unwrap();

    assert_eq!(
        registry
            .accept_event(&old, "old-event", &payload("SessionStart"))
            .unwrap_err()
            .code,
        "OBSERVER_FORBIDDEN"
    );
    assert!(matches!(
        registry
            .accept_event(&next, "new-event", &payload("SessionStart"))
            .unwrap(),
        ObserverAccept::Accepted(_)
    ));

    let wrong_source = ObserverBinding {
        run: current,
        capability: "dddddddddddddddddddddddddddddddd".to_string(),
        source: ObserverSource::Unknown,
    };
    assert_eq!(
        registry
            .accept_event(&wrong_source, "event-source", &payload("Stop"))
            .unwrap_err()
            .code,
        "OBSERVER_SOURCE_UNSUPPORTED"
    );
}

#[test]
fn D13_Observer_InvalidEventAndIdentifiersFailClosedWithoutRawContent_003() {
    let registry = ObserverRegistry::new();
    let binding = registry
        .attach(
            run("run-current", 3),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ObserverSource::ClaudeHook,
        )
        .unwrap();

    for (event_id, body) in [
        ("", br#"{"hook_event_name":"Stop"}"#.as_slice()),
        ("bad\nevent", br#"{"hook_event_name":"Stop"}"#.as_slice()),
        (
            "event-json",
            br#"{"hook_event_name":"Stop","secret":"private-value""#.as_slice(),
        ),
        (
            "event-name",
            br#"{"hook_event_name":"MadeUp","secret":"private-value"}"#.as_slice(),
        ),
    ] {
        let error = registry.accept_event(&binding, event_id, body).unwrap_err();
        assert!(
            matches!(
                error.code.as_str(),
                "INVALID_REQUEST" | "OBSERVER_INVALID_EVENT"
            ),
            "unexpected code {}",
            error.code
        );
        assert!(!error.to_string().contains("private-value"));
    }
}

#[test]
fn D13_Observer_AttachRequiresStrongOpaqueCapabilityAndBoundedReplayTable_004() {
    let registry = ObserverRegistry::new();
    let current = run("run-current", 9);
    for weak in ["", "short", "contains space", "contains\nnewline"] {
        let error = registry
            .attach(
                current.clone(),
                weak.to_string(),
                ObserverSource::ClaudeHook,
            )
            .unwrap_err();
        assert_eq!(error.code, "INVALID_REQUEST");
    }

    let binding = registry
        .attach(
            current,
            "0123456789abcdef0123456789abcdef".to_string(),
            ObserverSource::ClaudeHook,
        )
        .unwrap();
    for index in 0..ObserverRegistry::MAX_SEEN_EVENTS {
        assert!(matches!(
            registry
                .accept_event(
                    &binding,
                    &format!("event-{index}"),
                    &payload("UserPromptSubmit")
                )
                .unwrap(),
            ObserverAccept::Accepted(_)
        ));
    }
    assert_eq!(
        registry
            .accept_event(&binding, "event-over-capacity", &payload("Stop"))
            .unwrap_err()
            .code,
        "OBSERVER_EVENT_CAPACITY"
    );
}

#[test]
fn D13_Observer_HeadersBindExactRunCapabilitySourceAndEvent_005() {
    let mut headers = HeaderMap::new();
    headers.insert("x-cc-desk-run", "run-current".parse().unwrap());
    headers.insert("x-cc-desk-generation", "7".parse().unwrap());
    headers.insert(
        "x-cc-desk-capability",
        "0123456789abcdef0123456789abcdef".parse().unwrap(),
    );
    headers.insert("x-cc-desk-event", "event-7".parse().unwrap());
    headers.insert("x-cc-desk-observer-source", "claude-hook".parse().unwrap());

    let (binding, event_id) = parse_observer_headers(&headers).unwrap();
    assert_eq!(binding.run, run("run-current", 7));
    assert_eq!(binding.capability, "0123456789abcdef0123456789abcdef");
    assert_eq!(binding.source, ObserverSource::ClaudeHook);
    assert_eq!(event_id, "event-7");

    headers.insert("x-cc-desk-generation", "07".parse().unwrap());
    assert_eq!(
        parse_observer_headers(&headers).unwrap_err().code,
        "INVALID_REQUEST"
    );
}

#[test]
fn D13_Observer_MintedCapabilitiesAreOpaquePerRun_006() {
    let registry = ObserverRegistry::new();
    let first = registry
        .mint(run("first", 1), ObserverSource::ClaudeHook)
        .unwrap();
    let second = registry
        .mint(run("second", 1), ObserverSource::ClaudeHook)
        .unwrap();

    assert_eq!(first.capability.len(), 32);
    assert_eq!(second.capability.len(), 32);
    assert_ne!(first.capability, second.capability);
    assert!(first
        .capability
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit()));
    assert!(second
        .capability
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn D13_Observer_ValidatedHookPayloadNeverPublishesGuessedActivity_007() {
    let registry = ObserverRegistry::new();
    let binding = registry
        .attach(
            run("run-current", 4),
            "0123456789abcdef0123456789abcdef".to_string(),
            ObserverSource::ClaudeHook,
        )
        .unwrap();
    let ObserverAccept::Accepted(event) = registry
        .accept_event(&binding, "event-4", &payload("UserPromptSubmit"))
        .unwrap()
    else {
        panic!("first event must be accepted");
    };

    let payload = HookPayload::from_validated(event);
    assert_eq!(payload.run_id.as_deref(), Some("run-current"));
    assert_eq!(payload.generation, Some(4));
    assert_eq!(payload.event_id.as_deref(), Some("event-4"));
    assert_eq!(payload.observer_source.as_deref(), Some("claude-hook"));
    assert_eq!(payload.state, "unknown");
    assert!(payload.pty_id.is_none());
}

#[test]
fn D13_Observer_PluginDeploymentTracksScriptAndReporterStaysBounded_008() {
    assert_ne!(
        deployment_id("plugin", "hooks", "report-a"),
        deployment_id("plugin", "hooks", "report-b")
    );

    const SCRIPT: &str = include_str!("../../plugin/scripts/report-hook.sh");
    assert!(SCRIPT.contains("/observer"));
    assert!(SCRIPT.contains("CC_DESK_OBSERVER_CAPABILITY"));
    assert!(SCRIPT.contains("CC_DESK_OBSERVER_RUN"));
    assert!(SCRIPT.contains("CC_DESK_OBSERVER_GENERATION"));
    assert!(SCRIPT.contains("--max-time 3"));
    assert!(SCRIPT.contains("--config -"));
    assert!(!SCRIPT.contains("-H @<("));
    assert!(!SCRIPT.contains("-H \"X-CC-Desk-Capability:"));
}

#[test]
fn D13_Observer_ReattachingSameCapabilityMustNotResetReplay_009() {
    let registry = ObserverRegistry::new();
    let first = registry
        .mint(run("same", 1), ObserverSource::ClaudeHook)
        .unwrap();
    registry
        .accept_event(&first, "once", &payload("Stop"))
        .unwrap();
    let again = registry
        .attach(first.run.clone(), first.capability.clone(), first.source)
        .unwrap();
    assert_eq!(
        registry
            .accept_event(&again, "once", &payload("Stop"))
            .unwrap(),
        ObserverAccept::Duplicate
    );
}

#[test]
fn D13_Observer_DuplicateHeadersAreRejected_010() {
    let mut headers = HeaderMap::new();
    for (key, val) in [
        ("x-cc-desk-run", "run"),
        ("x-cc-desk-generation", "1"),
        ("x-cc-desk-capability", "0123456789abcdef0123456789abcdef"),
        ("x-cc-desk-event", "event"),
        ("x-cc-desk-observer-source", "claude-hook"),
    ] {
        headers.insert(key, val.parse().unwrap());
    }
    headers.append(
        "x-cc-desk-capability",
        "ffffffffffffffffffffffffffffffff".parse().unwrap(),
    );
    assert!(parse_observer_headers(&headers).is_err());
}

#[test]
fn D13_Observer_PromptAndErrorBodyNeverReachFrontend_011() {
    let registry = ObserverRegistry::new();
    let binding = registry
        .mint(run("redaction", 1), ObserverSource::ClaudeHook)
        .unwrap();
    for (i, name) in [
        "UserPromptSubmit",
        "Stop",
        "StopFailure",
        "PostToolUseFailure",
        "Notification",
    ]
    .iter()
    .enumerate()
    {
        let body = serde_json::to_vec(&serde_json::json!({"hook_event_name":name, "session_id":"sid", "prompt":"fixture-secret", "last_assistant_message":"fixture-secret", "error":"fixture-secret", "message":"fixture-secret", "title":"fixture-secret", "env":{"TOKEN":"fixture-secret"}})).unwrap();
        let ObserverAccept::Accepted(event) = registry
            .accept_event(&binding, &format!("e{i}"), &body)
            .unwrap()
        else {
            panic!("new event");
        };
        assert!(!serde_json::to_string(&HookPayload::from_validated(event))
            .unwrap()
            .contains("fixture-secret"));
    }
}

#[test]
fn D13_Observer_MalformedIdentityIsNotAcceptedAsAnOfficialEvent_012() {
    let registry = ObserverRegistry::new();
    let binding = registry
        .mint(run("invalid", 1), ObserverSource::ClaudeHook)
        .unwrap();
    for (i, body) in [
        r#"{"hook_event_name":"SessionStart","session_id":7}"#,
        r#"{"hook_event_name":"SessionStart","cwd":"/bad\u0000path"}"#,
    ]
    .iter()
    .enumerate()
    {
        assert!(registry
            .accept_event(&binding, &format!("e{i}"), body.as_bytes())
            .is_err());
    }
}

#[test]
fn D13_Observer_LeaseRevokesAndOldDropCannotRevokeReplacement_013() {
    use crate::observer_registry::ObserverDelivery;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let registry = Arc::new(ObserverRegistry::with_capacity(2));
    let alive = Arc::new(AtomicBool::new(true));
    let guard = alive.clone();
    let first = registry
        .lease(
            run("leased", 1),
            ObserverDelivery::native("main"),
            Arc::new(move || guard.load(Ordering::SeqCst)),
        )
        .unwrap();
    assert!(registry
        .accept_event(first.binding(), "before", &payload("Stop"))
        .is_ok());
    alive.store(false, Ordering::SeqCst);
    assert_eq!(
        registry
            .accept_event(first.binding(), "after", &payload("Stop"))
            .unwrap_err()
            .code,
        "OBSERVER_FORBIDDEN"
    );
    let second = registry
        .lease(
            run("leased", 1),
            ObserverDelivery::native("main"),
            Arc::new(|| true),
        )
        .unwrap();
    drop(first);
    assert!(registry
        .accept_event(second.binding(), "replacement", &payload("Stop"))
        .is_ok());
    let binding = second.binding().clone();
    drop(second);
    assert!(registry
        .accept_event(&binding, "late", &payload("Stop"))
        .is_err());
}

#[test]
fn D13_Observer_LeaseCapacityAndPublicationRecheck_014() {
    use crate::observer_registry::ObserverDelivery;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let registry = Arc::new(ObserverRegistry::with_capacity(1));
    let alive = Arc::new(AtomicBool::new(true));
    let guard = alive.clone();
    let lease = registry
        .lease(
            run("a", 1),
            ObserverDelivery::native("main"),
            Arc::new(move || guard.load(Ordering::SeqCst)),
        )
        .unwrap();
    assert!(registry
        .lease(
            run("b", 1),
            ObserverDelivery::native("main"),
            Arc::new(|| true)
        )
        .is_err());
    let _accepted = registry
        .accept_event(lease.binding(), "before", &payload("SessionStart"))
        .unwrap();
    alive.store(false, Ordering::SeqCst);
    assert!(
        registry.delivery(lease.binding()).is_err(),
        "revocation after parse must prevent publication"
    );
    drop(lease);
    assert!(registry
        .lease(
            run("b", 1),
            ObserverDelivery::native("main"),
            Arc::new(|| true)
        )
        .is_ok());
}

#[test]
fn D13_Observer_HostPreparesOnlyVerifiedPluginAndDropsAuthority_015() {
    use crate::observer_host::ObserverHost;
    use crate::observer_registry::ObserverDelivery;
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(ObserverRegistry::new());
    let host = ObserverHost::new(
        registry.clone(),
        root.path().to_path_buf(),
        Arc::new(|| Some(12345)),
    );
    assert!(host
        .prepare(
            run("prepared", 1),
            ObserverDelivery::native("main"),
            Arc::new(|| true)
        )
        .is_err());
    crate::hook_config::ensure_plugin_files_at(root.path()).unwrap();
    let prepared = host
        .prepare(
            run("prepared", 1),
            ObserverDelivery::native("main"),
            Arc::new(|| true),
        )
        .unwrap();
    let binding = prepared.lease.binding().clone();
    assert_eq!(
        prepared.environment.values[std::ffi::OsStr::new("CC_DESK_OBSERVER_RUN")],
        "prepared"
    );
    assert_eq!(
        prepared.environment.values[std::ffi::OsStr::new("CC_DESK_OBSERVER_GENERATION")],
        "1"
    );
    assert_eq!(
        prepared.environment.values[std::ffi::OsStr::new("CC_DESK_OBSERVER_CAPABILITY")],
        binding.capability.as_str()
    );
    assert_eq!(prepared.plugin_dir, root.path());
    assert!(registry.check_binding(&binding).is_ok());
    drop(prepared);
    assert!(registry.check_binding(&binding).is_err());
    std::fs::write(
        root.path().join("scripts/report-hook.sh"),
        b"wrong reporter",
    )
    .unwrap();
    assert!(host
        .prepare(
            run("prepared", 2),
            ObserverDelivery::native("main"),
            Arc::new(|| true)
        )
        .is_err());
    crate::hook_config::ensure_plugin_files_at(root.path()).unwrap();
    assert!(host
        .prepare(
            run("prepared", 3),
            ObserverDelivery::native("main"),
            Arc::new(|| true)
        )
        .is_ok());
}
