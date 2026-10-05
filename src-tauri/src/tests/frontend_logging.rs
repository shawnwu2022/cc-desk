use crate::logger::frontend_message_summary;

// 检查凭证、路径和提示正文只产生字节数摘要，不保留输入内容。
#[test]
fn FrontendLogging_RedactsPayload_001() {
    let secret = "fixture-secret --token=abc C:\\Users\\private\\repo prompt-body";
    let summary = frontend_message_summary(secret);
    assert!(!summary.contains("fixture-secret"));
    assert!(!summary.contains("--token"));
    assert!(!summary.contains("private"));
    assert!(!summary.contains("prompt-body"));
    assert_eq!(summary, format!("redacted bytes={}", secret.len()));
}

// 检查 Unicode、换行和终端控制序列不能注入日志或泄露正文。
#[test]
fn FrontendLogging_RedactsControls_002() {
    let value = "title\n[ERROR] 注入 fixture-secret\u{1b}]8;;https://evil.invalid\u{7}";
    let summary = frontend_message_summary(value);
    assert!(!summary.contains("fixture-secret"));
    assert!(!summary.contains("evil.invalid"));
    assert!(!summary.contains('\n'));
    assert!(!summary.contains('\u{1b}'));
    assert!(summary.starts_with("redacted "));
    assert_eq!(summary, format!("redacted bytes={}", value.len()));
}
