use super::*;
use serde_json::{json, Value};

fn fixture_record() -> Value {
    json!({"schema":"codex-identity-v1","run":format!("run-{}", "a".repeat(32)),
        "generation":1,"event":"sessionStart","outcome":"adopted","reason":"first-id",
        "before":null,"after":"01a18afd-4f1f-7df1-98b7-93534352dc84"})
}

#[test]
fn frontend_identity_schema_rejects_forgery_and_private_fields() {
    let mut budget = FrontendIdentityBudget::default();
    let valid = fixture_record();
    let line = budget.format("main", &valid.to_string()).unwrap();
    let output: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(output["layer"], "frontend-untrusted-identity");
    assert!(output.get("schema").is_none());
    for (key, value) in [
        ("layer", json!("backend-authenticated")),
        ("prompt", json!("PRIVATE_BODY")),
        ("path", json!("PRIVATE_PATH")),
        ("argv", json!(["PRIVATE_ARG"])),
        ("token", json!("PRIVATE_TOKEN")),
        ("schema", json!("anything")),
        ("run", json!("PRIVATE_PATH")),
        ("generation", json!(0)),
        ("generation", json!(4294967296u64)),
        ("generation", json!(-1)),
        ("event", json!("PRIVATE_EVENT")),
        ("reason", json!("PRIVATE_REASON")),
        ("outcome", json!("backend-authenticated")),
        ("before", json!("PRIVATE_SECRET")),
        ("after", json!("01A18AFD-4F1F-7DF1-98B7-93534352DC84")),
    ] {
        let mut input = valid.clone();
        input[key] = value;
        assert!(budget.format("main", &input.to_string()).is_none(), "{key}");
    }
    assert!(budget.format("other-window", &valid.to_string()).is_none());
    assert!(budget
        .format("main", &format!("{}{}", valid, " ".repeat(1024)))
        .is_none());
    let duplicate = valid
        .to_string()
        .replacen('{', "{\"schema\":\"codex-identity-v1\",", 1);
    assert!(budget.format("main", &duplicate).is_none());
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("before");
    assert!(budget.format("main", &missing.to_string()).is_none());
    let mut impossible = valid.clone();
    impossible["reason"] = "identity-mismatch".into();
    assert!(budget.format("main", &impossible.to_string()).is_none());
    assert_eq!(budget.total, 1);
    assert_eq!(
        frontend_message_summary(&valid.to_string()),
        format!("redacted bytes={}", valid.to_string().len())
    );
}

#[test]
fn frontend_identity_budget_cannot_be_bypassed_by_rotating_runs() {
    let mut budget = FrontendIdentityBudget::default();
    let first = fixture_record().to_string();
    assert!(budget.format("main", &first).is_some());
    assert!(budget.format("main", &first).is_none());
    for i in 0..100 {
        let mut input = fixture_record();
        input["after"] = uuid::Uuid::from_u128(i + 1).to_string().into();
        let _ = budget.format("main", &input.to_string());
    }
    assert_eq!(budget.total, 16);
    for i in 0..300 {
        let mut input = fixture_record();
        input["run"] = format!("run-{:032x}", i + 1).into();
        let _ = budget.format("main", &input.to_string());
    }
    assert_eq!(budget.total, 128);
    assert!(budget.runs.len() <= 128);
}

#[test]
#[ignore = "owns the process-global logger; run exact in a separate test process"]
fn frontend_identity_local_file_fixture() {
    use crate::observer_registry::{ObserverRegistry, ObserverRun, ObserverSource};
    let directory = tempfile::Builder::new()
        .prefix("cc-desk-owned-identity-log-")
        .tempdir()
        .unwrap();
    let path = directory.path().join("diagnostic.log");
    *LOGGER.app_log.lock().unwrap() = Some(File::create(&path).unwrap());
    log::set_logger(&LOGGER).expect("this fixture requires its own test process");
    log::set_max_level(log::LevelFilter::Info);
    let registry = ObserverRegistry::new();
    let binding = registry
        .mint(
            ObserverRun {
                run_id: format!("run-{}", "a".repeat(32)),
                generation: 1,
            },
            ObserverSource::CodexHook,
        )
        .unwrap();
    registry.accept_event(&binding, "fixture-start", &serde_json::to_vec(&json!({"hook_event_name":"SessionStart", "session_id":"01a18afd-4f1f-7df1-98b7-93534352dc84", "prompt":"PRIVATE_PAYLOAD"})).unwrap()).unwrap();
    log_frontend_message("main", "info", &fixture_record().to_string());
    log_frontend_message(
        "main",
        "info",
        "PRIVATE_TOKEN C:/PRIVATE_PATH --argv=PRIVATE_BODY",
    );
    let mut forged = fixture_record();
    forged["layer"] = "backend-authenticated".into();
    forged["prompt"] = "PRIVATE_INJECTION".into();
    log_frontend_message("main", "info", &forged.to_string());
    *LOGGER.app_log.lock().unwrap() = None;
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"layer\":\"backend-receipt\""));
    assert!(text.contains("\"layer\":\"frontend-untrusted-identity\""));
    assert!(text.contains("\"frontendAdoption\":\"unknown\""));
    assert!(text.contains("01a18afd-4f1f-7df1-98b7-93534352dc84"));
    assert!(text.contains("[Frontend] redacted bytes="));
    assert!(!text.contains("PRIVATE"));
    println!("OWNED_FILE_READBACK_PASS path={}\n{}", path.display(), text);
    let _retained_evidence = directory.keep();
}
