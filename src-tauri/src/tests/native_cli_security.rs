use crate::logger::frontend_message_summary;

#[test]
fn D26_Logging_FrontendPayloadIsAlwaysContentRedacted_001() {
    let secret = "fixture-secret --token=abc C:\\Users\\private\\repo prompt-body";
    let summary = frontend_message_summary(secret);
    assert!(!summary.contains("fixture-secret"));
    assert!(!summary.contains("--token"));
    assert!(!summary.contains("private"));
    assert!(!summary.contains("prompt-body"));
    assert!(summary.contains(&format!("bytes={}", secret.len())));
}

#[test]
fn D26_Logging_ControlAndUnicodePayloadStillCannotEscapeRedaction_002() {
    let value = "title\n[ERROR] 注入 fixture-secret\u{1b}]8;;https://evil.invalid\u{7}";
    let summary = frontend_message_summary(value);
    assert!(!summary.contains("fixture-secret"));
    assert!(!summary.contains("evil.invalid"));
    assert!(!summary.contains('\n'));
    assert!(!summary.contains('\u{1b}'));
    assert!(summary.starts_with("redacted "));
}
