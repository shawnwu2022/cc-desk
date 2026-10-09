//! 检查点日志协议测试；不把合成终态记录当作原生进程验收。
use crate::version_history::journal::{
    CapacityPlan, EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase, ManifestRole,
    Observation, ObservedResult, RootKind, SwitchJournal,
};

fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000301".into(),
        source_context: "00000000-0000-4000-8000-000000000302".into(),
        target_context: "00000000-0000-4000-8000-000000000303".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}
fn effect(kind: EffectKind) -> EffectSpec {
    EffectSpec {
        effect_id: uuid::Uuid::new_v4().to_string(),
        kind,
        before: "a".repeat(64),
        expected_postconditions: "b".repeat(64),
    }
}
fn applied(journal: &mut SwitchJournal, kind: EffectKind) {
    let effect = effect(kind);
    let id = effect.effect_id.clone();
    journal.apply(JournalEvent::Intent { effect }).unwrap();
    journal
        .apply(JournalEvent::Observed {
            effect_id: id,
            intent_generation: journal.generation(),
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some("c".repeat(64)),
            },
        })
        .unwrap();
}
fn restoring(omit: Option<EffectKind>) -> SwitchJournal {
    let mut journal = SwitchJournal::new(
        binding(),
        CapacityPlan::for_effects(100, 100, 100, 4096).unwrap(),
    )
    .unwrap();
    for role in [
        ManifestRole::ManagerHandoff,
        ManifestRole::SourceHandoffExit,
        ManifestRole::SourceContext,
        ManifestRole::SourceBundle,
        ManifestRole::FreshTargetContext,
        ManifestRole::RetainedTargetContext,
        ManifestRole::Registration,
        ManifestRole::Shortcuts,
    ] {
        journal
            .apply(JournalEvent::Manifest {
                role,
                digest: "a".repeat(64),
            })
            .unwrap();
    }
    for kind in [
        EffectKind::FenceSourceImage,
        EffectKind::VerifySourceBundleCopy,
        EffectKind::InstallerCreateSuspended,
        EffectKind::InstallerResume,
        EffectKind::InstallerTerminalOutcome,
        EffectKind::HistoricalCreateSuspended,
        EffectKind::HistoricalResume,
        EffectKind::HistoricalTerminalOutcome,
        EffectKind::FenceHistoricalImage,
    ] {
        if omit.as_ref() != Some(&kind) {
            applied(&mut journal, kind);
        }
    }
    for root in [RootKind::Desk, RootKind::WebView] {
        for context in [binding().source_context, binding().target_context] {
            applied(&mut journal, EffectKind::PreserveRoot { context, root });
        }
    }
    journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        })
        .unwrap();
    journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::Restoring,
        })
        .unwrap();
    journal
}
fn seal() -> JournalEvent {
    serde_json::from_value(serde_json::json!({
        "ReturnCheckpointSealed": { "checkpoint": "d".repeat(64) }
    }))
    .expect("return checkpoint event must be understood")
}
fn claim(digest: &str) -> JournalEvent {
    serde_json::from_value(serde_json::json!({
        "ReturnExecutionClaimed": {
            "checkpoint": digest,
            "attempt_id": uuid::Uuid::new_v4().to_string()
        }
    }))
    .expect("return claim event must be understood")
}

// 唯一claim以前不能开始恢复；以后仍不可再次启动受管程序。
#[test]
fn HistoryReturnCheckpoint_ClaimOnceBeforeRestore_001() {
    let mut journal = restoring(None);
    journal.apply(seal()).unwrap();
    let restore = effect(EffectKind::RestoreSourceRoot {
        root: RootKind::Desk,
    });
    assert!(journal
        .apply(JournalEvent::Intent {
            effect: restore.clone()
        })
        .is_err());
    assert!(journal.apply(claim(&"e".repeat(64))).is_err());
    journal.apply(claim(&"d".repeat(64))).unwrap();
    assert!(journal.apply(claim(&"d".repeat(64))).is_err());
    assert!(journal.apply(seal()).is_err());
    assert!(journal
        .apply(JournalEvent::Intent {
            effect: effect(EffectKind::HistoricalResume)
        })
        .is_err());
    journal
        .apply(JournalEvent::Intent { effect: restore })
        .unwrap();
}

// 普通receipt缺少任意受管终态或启动配对都不能生成检查点。
#[test]
fn HistoryReturnCheckpoint_MissingCustodyRejected_002() {
    for missing in [
        EffectKind::InstallerCreateSuspended,
        EffectKind::InstallerResume,
        EffectKind::InstallerTerminalOutcome,
        EffectKind::HistoricalCreateSuspended,
        EffectKind::HistoricalResume,
        EffectKind::HistoricalTerminalOutcome,
    ] {
        assert!(restoring(Some(missing)).apply(seal()).is_err());
    }
}

// 未完成intent及Unknown不能由checkpoint覆盖或重放。
#[test]
fn HistoryReturnCheckpoint_InterruptedOrUnknownRejected_003() {
    for unknown in [false, true] {
        let mut journal = restoring(None);
        let pending = effect(EffectKind::RestoreSourceRoot {
            root: RootKind::Desk,
        });
        let id = pending.effect_id.clone();
        journal
            .apply(JournalEvent::Intent { effect: pending })
            .unwrap();
        if unknown {
            journal
                .apply(JournalEvent::Observed {
                    effect_id: id,
                    intent_generation: journal.generation(),
                    result: ObservedResult {
                        observation: Observation::Unknown,
                        receipt: None,
                    },
                })
                .unwrap();
        }
        assert!(journal.apply(seal()).is_err());
    }
}

// claim必须紧邻seal；已记录其他状态的检查点不得重新准入。
#[test]
fn HistoryReturnCheckpoint_ChangedCheckpointRejected_004() {
    let mut journal = restoring(None);
    assert!(journal.apply(claim(&"d".repeat(64))).is_err());
    journal.apply(seal()).unwrap();
    journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        })
        .unwrap();
    assert!(journal.apply(claim(&"d".repeat(64))).is_err());
}

// 严格事件格式拒绝未知字段和无效checkpoint摘要。
#[test]
fn HistoryReturnCheckpoint_MalformedRejected_005() {
    assert!(serde_json::from_value::<JournalEvent>(serde_json::json!({
        "ReturnCheckpointSealed": { "checkpoint": "d".repeat(64), "force": true }
    }))
    .is_err());
    let event = serde_json::from_value::<JournalEvent>(serde_json::json!({
        "ReturnCheckpointSealed": { "checkpoint": "missing" }
    }))
    .expect("known event with malformed digest is rejected by reducer");
    assert!(restoring(None).apply(event).is_err());
}
