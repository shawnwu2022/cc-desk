//! Reducer-only tests. These do not create native source-terminal/no-launch
//! authority or certify Windows namespace reversal.
use crate::version_history::journal::{
    CapacityPlan, EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase, JournalStore,
    ManifestRole, Observation, ObservedResult, PrivateBackupOperation, RootKind, SwitchJournal,
};

fn journal() -> SwitchJournal {
    SwitchJournal::new(
        JournalBinding {
            transaction_id: "00000000-0000-4000-8000-000000000301".into(),
            source_context: "00000000-0000-4000-8000-000000000302".into(),
            target_context: "00000000-0000-4000-8000-000000000303".into(),
            user_installation: "1".repeat(64),
            source_bundle: "2".repeat(64),
            target_package: "3".repeat(64),
            target_payload: "4".repeat(64),
            roots: "5".repeat(64),
        },
        CapacityPlan::for_effects(100, 100, 100, 4096).unwrap(),
    )
    .unwrap()
}
fn source_observation(journal: &mut SwitchJournal) {
    for role in [ManifestRole::SourceHandoffExit, ManifestRole::SourceContext] {
        journal
            .apply(JournalEvent::Manifest {
                role,
                digest: "a".repeat(64),
            })
            .unwrap();
    }
}
fn effect(
    journal: &mut SwitchJournal,
    kind: EffectKind,
    observation: Observation,
) -> (String, u64) {
    let id = uuid::Uuid::new_v4().to_string();
    journal
        .apply(JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: id.clone(),
                kind,
                before: "a".repeat(64),
                expected_postconditions: "a".repeat(64),
            },
        })
        .unwrap();
    let generation = journal.generation();
    journal
        .apply(JournalEvent::Observed {
            effect_id: id.clone(),
            intent_generation: generation,
            result: ObservedResult {
                observation,
                receipt: (observation != Observation::Unknown).then(|| "b".repeat(64)),
            },
        })
        .unwrap();
    (id, generation)
}
fn abort(journal: &mut SwitchJournal) -> Result<(), crate::cli::types::SafeError> {
    journal.apply(JournalEvent::AbortPreContext {
        receipt: "f".repeat(64),
    })
}
fn reverse_image(journal: &mut SwitchJournal, original: (String, u64), observation: Observation) {
    effect(
        journal,
        EffectKind::ReverseSourceFence {
            original_effect_id: original.0,
            original_intent_generation: original.1,
        },
        observation,
    );
}

#[test]
fn HistorySourceFailure_ObservationBeforeSealDoesNotInventContextMutation_001() {
    let mut journal = journal();
    source_observation(&mut journal);
    assert!(!journal.fixture_private_abort_eligible());
    abort(&mut journal).unwrap();
    assert_eq!(journal.phase(), JournalPhase::PreContextAborted);
}

#[test]
fn HistorySourceFailure_AppliedSourceImageNeedsExactInverse_002() {
    let mut journal = journal();
    source_observation(&mut journal);
    let original = effect(
        &mut journal,
        EffectKind::FenceSourceImage,
        Observation::Applied,
    );
    assert!(abort(&mut journal).is_err());
    reverse_image(&mut journal, original, Observation::Applied);
    abort(&mut journal).unwrap();
}

#[test]
fn HistorySourceFailure_AnyProcessIntentClosesAbortEvenNotApplied_003() {
    for kind in [
        EffectKind::InstallerCreateSuspended,
        EffectKind::InstallerResume,
        EffectKind::InstallerTerminalOutcome,
        EffectKind::HistoricalCreateSuspended,
        EffectKind::HistoricalResume,
        EffectKind::HistoricalTerminalOutcome,
        EffectKind::VerifyTargetBundle,
        EffectKind::ConfirmFirstLaunch,
    ] {
        let mut journal = journal();
        source_observation(&mut journal);
        effect(&mut journal, kind, Observation::NotApplied);
        assert!(abort(&mut journal).is_err());
    }
}

#[test]
fn HistorySourceFailure_UnknownImageOrInverseCannotClearMarker_004() {
    for inverse_unknown in [false, true] {
        let mut journal = journal();
        source_observation(&mut journal);
        let original = effect(
            &mut journal,
            EffectKind::FenceSourceImage,
            if inverse_unknown {
                Observation::Applied
            } else {
                Observation::Unknown
            },
        );
        if inverse_unknown {
            reverse_image(&mut journal, original, Observation::Unknown);
        }
        journal
            .apply(JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            })
            .unwrap();
        assert!(abort(&mut journal).is_err());
        assert!(journal.requires_reconciliation());
    }
}

#[test]
fn HistorySourceFailure_FreshStateNeverEntersPresealAbort_005() {
    for create_effect in [false, true] {
        let mut journal = journal();
        source_observation(&mut journal);
        if create_effect {
            effect(
                &mut journal,
                EffectKind::CreateFreshRoot {
                    root: RootKind::Desk,
                },
                Observation::NotApplied,
            );
        } else {
            journal
                .apply(JournalEvent::Manifest {
                    role: ManifestRole::FreshTargetContext,
                    digest: "c".repeat(64),
                })
                .unwrap();
        }
        journal
            .apply(JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            })
            .unwrap();
        assert!(abort(&mut journal).is_err());
    }
}

#[test]
fn HistorySourceFailure_SourceContextRoleAllowsOnlyProvenRootInverse_006() {
    let mut journal = journal();
    source_observation(&mut journal);
    let image = effect(
        &mut journal,
        EffectKind::FenceSourceImage,
        Observation::Applied,
    );
    let root = effect(
        &mut journal,
        EffectKind::RotateSourceRoot {
            root: RootKind::Desk,
            manifest: "c".repeat(64),
        },
        Observation::Unknown,
    );
    assert!(abort(&mut journal).is_err());
    journal
        .apply(JournalEvent::AdmitRootReverse {
            effect_id: root.0.clone(),
            intent_generation: root.1,
            current_manifest: "d".repeat(64),
            receipt: "e".repeat(64),
        })
        .unwrap();
    effect(
        &mut journal,
        EffectKind::ReverseSourceRoot {
            original_effect_id: root.0,
            original_intent_generation: root.1,
            current_manifest: "d".repeat(64),
        },
        Observation::Applied,
    );
    assert!(abort(&mut journal).is_err());
    reverse_image(&mut journal, image, Observation::Applied);
    abort(&mut journal).unwrap();
}

#[test]
fn HistorySourceFailure_PartialRetentionKeepsUnknownAndForbidsForwardReplay_007() {
    let mut journal = journal();
    source_observation(&mut journal);
    let image = effect(
        &mut journal,
        EffectKind::FenceSourceImage,
        Observation::Applied,
    );
    journal
        .apply(JournalEvent::PrivateBackupPlan {
            manifest: "c".repeat(64),
            effects: 1,
            recovery_dependencies: 140,
        })
        .unwrap();
    let plan_generation = journal.generation();
    let copy_kind = EffectKind::PrivateBackupEntry {
        plan_generation,
        operation: PrivateBackupOperation::CopyFile,
        manifest: "c".repeat(64),
        entry_index: 0,
    };
    let private = effect(&mut journal, copy_kind, Observation::Unknown);
    assert!(abort(&mut journal).is_err());
    journal
        .apply(JournalEvent::RetainSourcePartial {
            effect_id: private.0.clone(),
            intent_generation: private.1,
            partial_manifest: "d".repeat(64),
            source_manifest: "e".repeat(64),
            receipt: "f".repeat(64),
        })
        .unwrap();
    assert!(!journal.requires_reconciliation());
    assert_eq!(
        journal.effect_observation(&private.0),
        Some(Observation::Unknown)
    );
    assert!(journal
        .apply(JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: uuid::Uuid::new_v4().to_string(),
                kind: EffectKind::InstallerCreateSuspended,
                before: "a".repeat(64),
                expected_postconditions: "a".repeat(64),
            }
        })
        .is_err());
    reverse_image(&mut journal, image, Observation::Applied);
    abort(&mut journal).unwrap();
}

#[test]
fn HistorySourceFailure_PartialReceiptCannotClearSourceMutation_008() {
    for kind in [
        EffectKind::FenceSourceImage,
        EffectKind::RotateSourceRoot {
            root: RootKind::Desk,
            manifest: "c".repeat(64),
        },
    ] {
        let mut journal = journal();
        source_observation(&mut journal);
        let pending = effect(&mut journal, kind, Observation::Unknown);
        assert!(journal
            .apply(JournalEvent::RetainSourcePartial {
                effect_id: pending.0,
                intent_generation: pending.1,
                partial_manifest: "d".repeat(64),
                source_manifest: "e".repeat(64),
                receipt: "f".repeat(64),
            })
            .is_err());
        assert!(journal.requires_reconciliation());
    }
}

#[test]
fn HistorySourceFailure_ObservedPreservationAllowsReturnWithoutInventedSealedPhase_009() {
    let mut journal = journal();
    source_observation(&mut journal);
    for role in [
        ManifestRole::SourceBundle,
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
    effect(
        &mut journal,
        EffectKind::FenceSourceImage,
        Observation::Applied,
    );
    effect(
        &mut journal,
        EffectKind::VerifySourceBundleCopy,
        Observation::Applied,
    );
    let admit = |journal: &mut SwitchJournal| {
        journal.apply(JournalEvent::AdmitPreinstallReturn {
            roots: [
                (RootKind::Desk, "b".repeat(64)),
                (RootKind::WebView, "c".repeat(64)),
            ]
            .into_iter()
            .collect(),
            pending: None,
            receipt: "d".repeat(64),
        })
    };
    assert!(admit(&mut journal).is_err());
    let source_context = journal.binding().source_context.clone();
    for root in [RootKind::Desk, RootKind::WebView] {
        effect(
            &mut journal,
            EffectKind::PreserveRoot {
                context: source_context.clone(),
                root,
            },
            Observation::Applied,
        );
    }
    assert_eq!(journal.phase(), JournalPhase::Reviewed);
    admit(&mut journal).unwrap();
    assert_eq!(journal.phase(), JournalPhase::RecoveryRequired);
}

#[test]
fn HistorySourceFailure_ProtectedPartialAdmissionCannotBeAppendedFromData_010() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = JournalStore::fixture(
        cap_std::fs::Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap(),
    )
    .unwrap();
    let rejected = store.append(
        0,
        JournalEvent::RetainSourcePartial {
            effect_id: "00000000-0000-4000-8000-000000000304".into(),
            intent_generation: 0,
            partial_manifest: "d".repeat(64),
            source_manifest: "e".repeat(64),
            receipt: "f".repeat(64),
        },
    );
    assert!(rejected.is_err());
}
