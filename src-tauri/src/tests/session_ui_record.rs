#![allow(non_snake_case)]
use crate::session_ui_record::{merge_record, SessionUiRecord};

fn record(title: &str, at: Option<u64>) -> SessionUiRecord {
    SessionUiRecord {
        runtime: "native-cli".into(),
        cli: "codex".into(),
        project_path: "/repo".into(),
        adapter_session_id: "exact-source".into(),
        native_session_id: Some("native-id".into()),
        title: title.into(),
        last_activity_at: 100,
        last_opened_at: at,
    }
}

// 旧元数据缺打开时间仍可读，写回不伪造打开时间。
#[test]
fn SessionMeta_LegacyDefault_001() {
    let old = record("Saved name", None);
    let json = serde_json::to_value(&old).unwrap();
    assert!(json.get("lastOpenedAt").is_none());
    assert_eq!(
        serde_json::from_value::<SessionUiRecord>(json).unwrap(),
        old
    );
}

// 打开时间独立保存，活动时间不替代该字段。
#[test]
fn SessionMeta_OpenTimeRoundTrip_002() {
    let saved = record("Saved name", Some(1234));
    let json = serde_json::to_value(&saved).unwrap();
    assert_eq!(json["lastOpenedAt"], 1234);
    assert_eq!(json["lastActivityAt"], 100);
    assert_eq!(
        serde_json::from_value::<SessionUiRecord>(json).unwrap(),
        saved
    );
}

// 锁内打开时间更新保留另一个窗口已保存的名称与活动值。
#[test]
fn SessionMeta_OpenKeepsRename_003() {
    let existing = record("Renamed in another window", Some(1000));
    let mut incoming = record("Stale title", Some(2000));
    incoming.last_activity_at = 9999;
    let merged = merge_record(Some(&existing), incoming, true);
    assert_eq!(merged.title, existing.title);
    assert_eq!(merged.last_activity_at, existing.last_activity_at);
    assert_eq!(merged.last_opened_at, Some(2000));
}

// 重命名写入保留另一个写入者更新的打开时间。
#[test]
fn SessionMeta_RenameKeepsTime_004() {
    let existing = record("Old name", Some(2000));
    let merged = merge_record(Some(&existing), record("New name", None), false);
    assert_eq!(merged.title, "New name");
    assert_eq!(merged.last_opened_at, Some(2000));
}

// 不同精确来源不能继承旧会话的名称或打开时间。
#[test]
fn SessionMeta_ExactIdentity_005() {
    let existing = record("Other source", Some(2000));
    let mut incoming = record("Selected source", Some(1000));
    incoming.adapter_session_id = "different-source".into();
    assert_eq!(
        merge_record(Some(&existing), incoming.clone(), true),
        incoming
    );
}
