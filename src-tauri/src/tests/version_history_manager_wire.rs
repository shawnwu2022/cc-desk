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

// 普通安装只发布原生完整备份位置；交接不是安装成功或恢复权限。
#[test]
fn HistoryManagerWire_OrdinarySummary_003() {
    let status =
        ManagerStatus::fixture(ManagerPhase::Installing, None, vec![ManagerAction::Refresh])
            .with_ordinary_install(Some("C:\\backup\\<manual>&settings"), true)
            .unwrap();
    let actual = serde_json::to_value(&status).unwrap();
    assert_eq!(actual["phase"], "installing");
    assert_eq!(actual["allowedActions"], serde_json::json!(["refresh"]));
    assert_eq!(
        actual["ordinaryInstall"],
        serde_json::json!({
            "backupLocation": "C:\\backup\\<manual>&settings",
            "installerHandedOff": true,
            "contextPolicy": "fresh-settings-backup-manual-restore",
        })
    );
    assert!(
        serde_json::from_str::<InspectManagerRequest>("{\"backupLocation\":\"C:\\\\caller\"}")
            .is_err()
    );
}

#[test]
fn HistoryManagerWire_OrdinaryNeverSuccessOrReturn_004() {
    for phase in [
        ManagerPhase::InstalledUnconfirmed,
        ManagerPhase::HistoricalActive,
        ManagerPhase::Returning,
        ManagerPhase::Restored,
    ] {
        assert!(
            ManagerStatus::fixture(phase, None, vec![ManagerAction::Refresh])
                .with_ordinary_install(Some("C:\\backup"), true)
                .is_err()
        );
    }
    let status = ManagerStatus::fixture(
        ManagerPhase::RecoveryRequired,
        Some(ManagerBlockReason::InstallerOutcomeUnknown),
        vec![ManagerAction::Refresh, ManagerAction::ReturnToPrevious],
    )
    .with_ordinary_install(Some("C:\\backup"), false)
    .unwrap();
    assert_eq!(status.allowed_actions, vec![ManagerAction::Refresh]);
    assert!(
        ManagerStatus::fixture(ManagerPhase::Preparing, None, vec![ManagerAction::Refresh])
            .with_ordinary_install(Some(""), false)
            .is_err()
    );
    for phase in [ManagerPhase::Preparing, ManagerPhase::PreContextAborted] {
        assert!(
            ManagerStatus::fixture(phase, None, vec![ManagerAction::Refresh])
                .with_ordinary_install(Some("C:\\backup"), true)
                .is_err()
        );
    }
}

#[test]
fn HistoryManagerWire_OrdinaryFailureRetainsBackup_005() {
    let status =
        ManagerStatus::fixture(ManagerPhase::Installing, None, vec![ManagerAction::Refresh])
            .with_ordinary_install(Some("C:\\complete-backup"), false)
            .unwrap()
            .ordinary_failure(ManagerBlockReason::StorageUnavailable)
            .unwrap();
    let actual = serde_json::to_value(status).unwrap();
    assert_eq!(actual["phase"], "recovery-required");
    assert_eq!(actual["blockedReason"], "STORAGE_UNAVAILABLE");
    assert_eq!(actual["allowedActions"], serde_json::json!(["refresh"]));
    assert_eq!(
        actual["ordinaryInstall"]["backupLocation"],
        "C:\\complete-backup"
    );
    assert_eq!(actual["ordinaryInstall"]["installerHandedOff"], false);
    assert!(
        ManagerStatus::fixture(ManagerPhase::Preparing, None, vec![ManagerAction::Refresh])
            .ordinary_failure(ManagerBlockReason::StorageUnavailable)
            .is_none()
    );
}

#[test]
fn HistoryManagerWire_OrdinaryPendingAndMonotonic_006() {
    let pending =
        ManagerStatus::fixture(ManagerPhase::Preparing, None, vec![ManagerAction::Refresh])
            .with_ordinary_install(None, false)
            .unwrap();
    let actual = serde_json::to_value(&pending).unwrap();
    assert!(actual["ordinaryInstall"]["backupLocation"].is_null());
    assert_eq!(actual["ordinaryInstall"]["installerHandedOff"], false);
    assert!(pending.with_ordinary_install(None, true).is_err());
    let proven =
        ManagerStatus::fixture(ManagerPhase::Installing, None, vec![ManagerAction::Refresh])
            .with_ordinary_install(Some("C:\\backup"), true)
            .unwrap();
    let retained = proven.clone().with_ordinary_install(None, false).unwrap();
    assert_eq!(
        serde_json::to_value(retained).unwrap()["ordinaryInstall"],
        serde_json::to_value(&proven).unwrap()["ordinaryInstall"]
    );
    assert!(proven
        .with_ordinary_install(Some("C:\\other"), false)
        .is_err());
}
