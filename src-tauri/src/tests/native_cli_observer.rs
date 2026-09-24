use crate::hook_server::{
    ObserverAccept, ObserverBinding, ObserverRegistry, ObserverRun, ObserverSource,
    MAX_OBSERVER_PAYLOAD,
};

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
