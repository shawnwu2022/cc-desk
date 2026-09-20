use super::*;

#[test]
fn PasteTrace_ExactFramedText_001() {
    let body = "Error\n\tat Item (匿名)";
    let wire = format!("\x1b[200~{body}\x1b[201~");
    let result = summarize(&wire, Some(body));
    assert!(result.complete);
    assert_eq!(result.exact, Some(true));
    assert_eq!(result.first_difference, None);
    assert_eq!(result.body_bytes, body.len());
    assert_eq!(result.controls[0], 1);
    assert_eq!(result.controls[2], 1);
}

#[test]
fn PasteTrace_MiddleLossIsDetected_002() {
    let result = summarize("\x1b[200~headtail\x1b[201~", Some("head-MIDDLE-tail"));
    assert_eq!(result.exact, Some(false));
    assert_eq!(result.first_difference, Some(4));
    assert_eq!(result.common_suffix, Some(4));
}

#[test]
fn PasteTrace_SameLengthCorruptionIsDetected_003() {
    let result = summarize("\x1b[200~abXd\x1b[201~", Some("abcd"));
    assert_eq!(result.exact, Some(false));
    assert_eq!(result.first_difference, Some(2));
}

#[test]
fn PasteTrace_UnframedTextAndMissingMarker_004() {
    assert_eq!(
        summarize("head\ntail", Some("head\ntail")).exact,
        Some(true)
    );
    let result = summarize("\x1b[200~head", Some("head"));
    assert!(!result.complete);
    assert_eq!(result.exact, Some(false));
}

#[test]
fn PasteTrace_OrdinaryInputHasNoReference_005() {
    let result = summarize("\r\x15\x17\x08\x7f", None);
    assert_eq!(result.exact, None);
    assert_eq!(result.first_difference, None);
    // LF, CR, TAB, ESC, BS, DEL, ETX, Ctrl+U, Ctrl+W
    assert_eq!(result.controls, [0, 1, 0, 0, 1, 1, 0, 1, 1]);
}

#[test]
fn PasteTrace_DiagnosticsNeverFormatContents_006() {
    let result = summarize("sensitive-business-text", Some("sensitive-business-text"));
    let text = format!("{result:?}");
    assert!(!text.contains("sensitive-business-text"));
    assert!(!text.contains("business"));
}

#[test]
fn PasteTrace_BudgetIsFinite_007() {
    let mut budget = TraceBudget::default();
    let now = Instant::now();
    for n in 1..=256 {
        assert_eq!(budget.take(now), Some(n));
    }
    assert_eq!(budget.take(now), None);
}

#[test]
fn PasteTrace_BudgetExpires_008() {
    let mut budget = TraceBudget::default();
    let now = Instant::now();
    assert_eq!(budget.take(now), Some(1));
    assert_eq!(budget.take(now + Duration::from_secs(60)), None);
    assert_eq!(budget.take(now + Duration::from_secs(61)), None);
}
