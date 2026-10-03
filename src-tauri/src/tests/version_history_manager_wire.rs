use crate::version_history::manager_types::{
    InspectManagerRequest, ManagerAction, ManagerActionRequest, ManagerBlockReason, ManagerPhase,
    ManagerStatus,
};

// 使用真正Rust序列化固定前端契约；这些测试状态不是OS安装/恢复证据。
#[test]
fn HistoryManagerWire_Serialization_001() {
    use ManagerAction::*;
    let actual = serde_json::json!({
        "preparing": ManagerStatus::fixture(ManagerPhase::Preparing, Some(ManagerBlockReason::SourceStillRunning), vec![Refresh]),
        "installing": ManagerStatus::fixture(ManagerPhase::Installing, None, vec![Refresh]),
        "installedUnconfirmed": ManagerStatus::fixture(ManagerPhase::InstalledUnconfirmed, None, vec![Refresh, ConfirmHistoricalVersion, ReturnToPrevious]),
        "historicalActive": ManagerStatus::fixture(ManagerPhase::HistoricalActive, None, vec![Refresh, ReturnToPrevious]),
        "returning": ManagerStatus::fixture(ManagerPhase::Returning, None, vec![Refresh]),
        "recoveryRequired": ManagerStatus::fixture(ManagerPhase::RecoveryRequired, Some(ManagerBlockReason::InstallerOutcomeUnknown), vec![Refresh, ReturnToPrevious]),
        "restored": ManagerStatus::fixture(ManagerPhase::Restored, None, vec![Refresh]),
        "preContextAborted": ManagerStatus::fixture(ManagerPhase::PreContextAborted, None, vec![Refresh]),
    });
    let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/version-manager-wire.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}

// 命令只有读取或精确代数，没有外来事务、URL、路径、PID或布尔proof输入。
#[test]
fn HistoryManagerWire_NoCallerAuthority_002() {
    assert!(serde_json::from_str::<InspectManagerRequest>("{}").is_ok());
    assert!(
        serde_json::from_str::<InspectManagerRequest>("{\"transactionId\":\"other\"}").is_err()
    );
    let request =
        serde_json::from_str::<ManagerActionRequest>("{\"expectedGeneration\":\"17\"}").unwrap();
    assert_eq!(request.expected_generation.get(), 17);
    for body in [
        "{\"expectedGeneration\":17}",
        "{\"expectedGeneration\":\"017\"}",
        "{\"expectedGeneration\":\"17\",\"sourceExited\":true}",
        "{\"expectedGeneration\":\"17\",\"path\":\"C:\\\\private\"}",
    ] {
        assert!(serde_json::from_str::<ManagerActionRequest>(body).is_err());
    }
}
