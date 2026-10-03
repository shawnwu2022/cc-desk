//! Reducer admission only; operation-specific Windows effects have separate probes.
use crate::version_history::journal::{
    CapacityPlan, EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase, ManifestRole,
    Observation, ObservedResult, RegistrationSlot, RootKind, ShortcutOperation, ShortcutSlot,
    SwitchJournal,
};
fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000201".into(),
        source_context: "00000000-0000-4000-8000-000000000202".into(),
        target_context: "00000000-0000-4000-8000-000000000203".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}
fn applied(journal: &mut SwitchJournal, kind: EffectKind) {
    let id = uuid::Uuid::new_v4().to_string();
    journal
        .apply(JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: id.clone(),
                kind,
                before: "a".repeat(64),
                expected_postconditions: "b".repeat(64),
            },
        })
        .unwrap();
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
fn restoring() -> SwitchJournal {
    let mut journal = SwitchJournal::new(
        binding(),
        CapacityPlan::for_effects(100, 100, 100, 4096).unwrap(),
    )
    .unwrap();
    for role in [
        ManifestRole::SourceContext,
        ManifestRole::RetainedTargetContext,
        ManifestRole::Registration,
        ManifestRole::Shortcuts,
    ] {
        journal
            .apply(JournalEvent::Manifest {
                role,
                digest: if role == ManifestRole::Registration {
                    "b"
                } else {
                    "a"
                }
                .repeat(64),
            })
            .unwrap();
    }
    journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        })
        .unwrap();
    applied(&mut journal, EffectKind::FenceHistoricalImage);
    for root in [RootKind::Desk, RootKind::WebView] {
        applied(
            &mut journal,
            EffectKind::PreserveRoot {
                context: binding().target_context,
                root,
            },
        );
    }
    journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::Restoring,
        })
        .unwrap();
    journal
}
// 检查shortcut条目只绑定实际Shortcuts role且不能用其他manifest或在Reviewed执行。
#[test]
fn HistoryRegistrationJournal_ShortcutRole_001() {
    for operation in [
        ShortcutOperation::CreateFile,
        ShortcutOperation::WriteBytes,
        ShortcutOperation::SetPermissions,
        ShortcutOperation::RemoveFile,
    ] {
        let mut journal = restoring();
        assert_eq!(
            journal.manifest(ManifestRole::Shortcuts),
            Some("a".repeat(64).as_str())
        );
        let kind = EffectKind::RecoveryShortcutEntry {
            slot: ShortcutSlot::Desktop,
            operation,
            manifest: "a".repeat(64),
            entry_index: 0,
        };
        applied(&mut journal, kind.clone());
        let mut reviewed = SwitchJournal::new(
            binding(),
            CapacityPlan::for_effects(100, 100, 100, 4096).unwrap(),
        )
        .unwrap();
        reviewed
            .apply(JournalEvent::Manifest {
                role: ManifestRole::Shortcuts,
                digest: "a".repeat(64),
            })
            .unwrap();
        let effect = EffectSpec {
            effect_id: uuid::Uuid::new_v4().to_string(),
            kind,
            before: "a".repeat(64),
            expected_postconditions: "b".repeat(64),
        };
        assert!(reviewed.apply(JournalEvent::Intent { effect }).is_err());
        assert_eq!(
            journal.manifest(ManifestRole::Registration),
            Some("b".repeat(64).as_str())
        );
        let wrong = EffectSpec {
            effect_id: uuid::Uuid::new_v4().to_string(),
            kind: EffectKind::RecoveryShortcutEntry {
                slot: ShortcutSlot::Desktop,
                operation,
                manifest: "b".repeat(64),
                entry_index: 0,
            },
            before: "a".repeat(64),
            expected_postconditions: "b".repeat(64),
        };
        assert!(journal
            .apply(JournalEvent::Intent { effect: wrong })
            .is_err());
    }
}
// 检查完整六个产品树和shortcut仍不足以提交Restored，必须另有selective Run验证。
#[test]
fn HistoryRegistrationJournal_RunTerminal_002() {
    let mut journal = restoring();
    applied(&mut journal, EffectKind::VerifySourceBundleRestore);
    for root in [RootKind::Desk, RootKind::WebView] {
        applied(&mut journal, EffectKind::RestoreSourceRoot { root });
    }
    for slot in [
        RegistrationSlot::Uninstall,
        RegistrationSlot::Publisher,
        RegistrationSlot::DeskDirectory,
        RegistrationSlot::DeskDirectoryBackground,
        RegistrationSlot::LegacyDirectory,
        RegistrationSlot::LegacyDirectoryBackground,
    ] {
        applied(&mut journal, EffectKind::VerifyRegistrationRestore { slot });
    }
    for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
        applied(&mut journal, EffectKind::RestoreShortcut { slot });
    }
    assert!(journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::Restored
        })
        .is_err());
    applied(
        &mut journal,
        EffectKind::VerifyRegistrationRestore {
            slot: RegistrationSlot::OwnedRun,
        },
    );
    journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::Restored,
        })
        .unwrap();
}
