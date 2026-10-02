//! Transcript eligibility only; real unchanged-source proof remains Windows UI evidence.
use crate::version_history::journal::{
    CapacityPlan, EffectKind, EffectSpec, JournalBinding, JournalEvent, ManifestRole, Observation,
    ObservedResult, SwitchJournal,
};

fn journal() -> SwitchJournal {
    SwitchJournal::new(
        JournalBinding {
            transaction_id: "00000000-0000-4000-8000-000000000111".into(),
            source_context: "00000000-0000-4000-8000-000000000112".into(),
            target_context: "00000000-0000-4000-8000-000000000113".into(),
            user_installation: "1".repeat(64),
            source_bundle: "2".repeat(64),
            target_package: "3".repeat(64),
            target_payload: "4".repeat(64),
            roots: "5".repeat(64),
        },
        CapacityPlan::for_effects(20, 20, 20, 4096).unwrap(),
    )
    .unwrap()
}
fn observed(journal: &mut SwitchJournal, kind: EffectKind, observation: Observation) {
    let id = "00000000-0000-4000-8000-000000000114".to_owned();
    journal
        .apply(JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: id.clone(),
                kind,
                before: "6".repeat(64),
                expected_postconditions: "7".repeat(64),
            },
        })
        .unwrap();
    let generation = journal.generation();
    journal
        .apply(JournalEvent::Observed {
            effect_id: id,
            intent_generation: generation,
            result: ObservedResult {
                observation,
                receipt: if observation == Observation::Unknown {
                    None
                } else {
                    Some("8".repeat(64))
                },
            },
        })
        .unwrap();
}

// 没有上下文/启动影响的健康记录可以进入真实source proof工厂，不能直接成为终态。
#[test]
fn HistoryPrivateAbort_HealthyPrivateOnly_001() {
    let mut state = journal();
    assert!(state.fixture_private_abort_eligible());
    observed(
        &mut state,
        EffectKind::VerifySourceBundleCopy,
        Observation::Applied,
    );
    assert!(state.fixture_private_abort_eligible());
}

// 未知copy验证、任何fence意图以及已发布manager都不能走较弱的活source abort。
#[test]
fn HistoryPrivateAbort_StrongerEvidenceRequired_002() {
    for (kind, result) in [
        (EffectKind::VerifySourceBundleCopy, Observation::Unknown),
        (EffectKind::FenceSourceImage, Observation::NotApplied),
        (EffectKind::FenceSourceImage, Observation::Applied),
    ] {
        let mut state = journal();
        observed(&mut state, kind, result);
        assert!(!state.fixture_private_abort_eligible());
    }
    let mut state = journal();
    state
        .apply(JournalEvent::Manifest {
            role: ManifestRole::ManagerHandoff,
            digest: "9".repeat(64),
        })
        .unwrap();
    assert!(!state.fixture_private_abort_eligible());
}
