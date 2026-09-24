use crate::hook_config::deployment_id;
use crate::hook_events::HookPayload;
use crate::hook_server::{
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
        .attach(current.clone(), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(), ObserverSource::ClaudeHook)
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
        .attach(current.clone(), "cccccccccccccccccccccccccccccccc".to_string(), ObserverSource::ClaudeHook)
        .unwrap();
    let next = registry
        .attach(current.clone(), "dddddddddddddddddddddddddddddddd".to_string(), ObserverSource::ClaudeHook)
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
        ("event-json", br#"{"hook_event_name":"Stop","secret":"private-value""#.as_slice()),
        ("event-name", br#"{"hook_event_name":"MadeUp","secret":"private-value"}"#.as_slice()),
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
    assert_eq!(
        binding.capability,
        "0123456789abcdef0123456789abcdef"
    );
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
    assert!(SCRIPT.contains("-H @<("));
    assert!(!SCRIPT.contains("-H \"X-CC-Desk-Capability:"));
}
