//! Actual secured NTFS persistence; no installer/context admission is constructed.
use crate::version_history::{
    journal::{
        CapacityPlan, JournalBinding, JournalEvent, JournalPhase, JournalStore, ManifestRole,
    },
    maintenance::ActiveContextMarker,
    windows::{
        durability::{
            probe_persistence_fault, MarkerStore, PersistenceBoundary, PersistenceOperation,
        },
        files::{ComponentName, Directory, PrivateDirectory},
        lease::LeaseFiles,
        security::CurrentUser,
    },
};
use std::{ffi::OsStr, sync::Arc};

fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000101".into(),
        source_context: "00000000-0000-4000-8000-000000000102".into(),
        target_context: "00000000-0000-4000-8000-000000000103".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}
fn private_root(path: &std::path::Path) -> Arc<PrivateDirectory> {
    let user = CurrentUser::capture().unwrap();
    Arc::new(
        PrivateDirectory::create_new(
            Directory::open_absolute(path).unwrap(),
            ComponentName::new(OsStr::new("private")).unwrap(),
            &user,
        )
        .unwrap(),
    )
}
fn capacity() -> CapacityPlan {
    CapacityPlan::for_effects(10, 10, 10, 4096).unwrap()
}

// 稳定控制根下每次切换使用独立UUID日志；重开缺失日志不能创建空日志冒充恢复。
#[test]
fn HistoryJournalWindows_NamedTransactions_012() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let original = binding();
    assert!(
        JournalStore::open_windows_transaction(root.clone(), &original.transaction_id).is_err()
    );
    let mut first =
        JournalStore::create_windows_transaction(root.clone(), &original.transaction_id).unwrap();
    first.initialize(original.clone(), capacity()).unwrap();
    let mut next = original.clone();
    next.transaction_id = "00000000-0000-4000-8000-000000000104".into();
    let mut second =
        JournalStore::create_windows_transaction(root.clone(), &next.transaction_id).unwrap();
    assert!(second.initialize(original.clone(), capacity()).is_err());
    second.initialize(next.clone(), capacity()).unwrap();
    assert!(second.bind_existing(&original).is_err());
    let before = std::fs::read(
        temporary
            .path()
            .join(format!("private/journal-{}.log", original.transaction_id)),
    )
    .unwrap();
    drop(first);
    let mut reopened =
        JournalStore::open_windows_transaction(root, &original.transaction_id).unwrap();
    reopened.bind_existing(&original).unwrap();
    assert_eq!(
        std::fs::read(
            temporary
                .path()
                .join(format!("private/journal-{}.log", original.transaction_id))
        )
        .unwrap(),
        before
    );
}

// 只有同根原终态完整检查点和新reviewed日志可发布下一次切换，原证据永不重置。
#[test]
fn HistoryJournalWindows_TerminalRollover_013() {
    use crate::version_history::journal::PreContextAbortProof;
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let original = binding();
    let mut old =
        JournalStore::create_windows_transaction(root.clone(), &original.transaction_id).unwrap();
    old.initialize(original.clone(), capacity()).unwrap();
    let initial = ActiveContextMarker::transition_from(&old.inspect(&original).unwrap()).unwrap();
    let mut markers = MarkerStore::create(root.clone(), &control, &initial, &mut old).unwrap();
    let mut next = original.clone();
    next.transaction_id = "00000000-0000-4000-8000-000000000104".into();
    next.source_context = "00000000-0000-4000-8000-000000000105".into();
    next.target_context = "00000000-0000-4000-8000-000000000106".into();
    let mut new =
        JournalStore::create_windows_transaction(root.clone(), &next.transaction_id).unwrap();
    new.initialize(next.clone(), capacity()).unwrap();
    let next_marker = ActiveContextMarker::transition_from(&new.inspect(&next).unwrap()).unwrap();
    assert!(markers
        .append_successor(&next_marker, &mut old, &mut new)
        .is_err());
    let digest = old
        .retain_manifest(b"test-only unchanged source proof")
        .unwrap();
    let proof = PreContextAbortProof::fixture(
        &old.inspect(&original).unwrap(),
        std::array::from_fn(|_| digest.clone()),
    );
    old.abort_pre_context(&proof).unwrap();
    let terminal =
        ActiveContextMarker::pre_context_aborted(&old.inspect(&original).unwrap()).unwrap();
    markers.append(&terminal, &mut old).unwrap();
    let marker_path = temporary.path().join("private/active-context.log");
    let prior = std::fs::read(&marker_path).unwrap();
    assert!(markers.append(&next_marker, &mut new).is_err());
    markers
        .append_successor(&next_marker, &mut old, &mut new)
        .unwrap();
    assert!(std::fs::read(&marker_path).unwrap().starts_with(&prior));
    drop(markers);
    let reopened = MarkerStore::open_existing(root, &control).unwrap().unwrap();
    assert_eq!(reopened.current().unwrap(), next_marker.encode().unwrap());
    assert!(!old.inspect(&original).unwrap().blocked);
}

// 检查真实私有句柄写入、超过1MiB的manifest、重开链验证和第二writer拒绝。
#[test]
fn HistoryJournalWindows_Reopen_001() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let bytes = vec![b'x'; 1024 * 1024 + 1];
    let digest = store.retain_manifest(&bytes).unwrap();
    store
        .append(
            0,
            JournalEvent::Manifest {
                role: ManifestRole::SourceBundle,
                digest: digest.clone(),
            },
        )
        .unwrap();
    assert_eq!(store.read_manifest(&digest).unwrap(), bytes);
    assert!(JournalStore::open_windows(root.clone()).is_err());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.path().join("private/journal.log"))
        .is_err());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(
            temporary
                .path()
                .join(format!("private/manifest-{digest}.json"))
        )
        .is_err());
    drop(store);
    let mut reopened = JournalStore::open_windows(root).unwrap();
    reopened.bind_existing(&binding()).unwrap();
    reopened
        .append(
            1,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    let inspection = reopened.inspect(&binding()).unwrap();
    assert!(!inspection.blocked);
    assert_eq!(inspection.last_valid.unwrap().generation(), 2);
}

// 检查实际写入边界失败保留原前缀，原writer不能续写，半帧不能重绑有效前缀。
#[test]
fn HistoryJournalWindows_TornAppend_002() {
    for boundary in [
        PersistenceBoundary::PartialWrite,
        PersistenceBoundary::AfterWrite,
        PersistenceBoundary::FlushCall,
        PersistenceBoundary::AfterFlush,
        PersistenceBoundary::Readback,
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = private_root(temporary.path());
        let mut store = JournalStore::open_windows(root.clone()).unwrap();
        store.initialize(binding(), capacity()).unwrap();
        let path = temporary.path().join("private/journal.log");
        let before = std::fs::read(&path).unwrap();
        let fault = probe_persistence_fault(PersistenceOperation::JournalFrame, boundary);
        assert!(store
            .append(
                0,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired
                }
            )
            .is_err());
        drop(fault);
        let after = std::fs::read(&path).unwrap();
        assert!(after.starts_with(&before));
        assert!(after.len() > before.len());
        assert!(store
            .append(
                0,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired
                }
            )
            .is_err());
        assert!(store.inspect(&binding()).unwrap().blocked);
        drop(store);
        let mut reopened = JournalStore::open_windows(root).unwrap();
        if boundary == PersistenceBoundary::PartialWrite {
            assert!(reopened.inspect(&binding()).unwrap().blocked);
            assert!(reopened.bind_existing(&binding()).is_err());
        } else {
            // Re-reading a complete frame may reconcile persistence, never replay it.
            reopened.bind_existing(&binding()).unwrap();
            assert_eq!(
                reopened
                    .inspect(&binding())
                    .unwrap()
                    .last_valid
                    .unwrap()
                    .generation(),
                1
            );
            assert!(reopened
                .append(
                    0,
                    JournalEvent::Phase {
                        phase: JournalPhase::RecoveryRequired
                    }
                )
                .is_err());
        }
        assert_eq!(std::fs::read(&path).unwrap(), after);
    }
}

// 检查artifact半写保留原文件并阻止重试覆盖，不将残留文件解释为有效manifest。
#[test]
fn HistoryJournalWindows_TornArtifact_003() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let bytes = b"complete retained artifact";
    let digest = crate::version_history::verified_package::sha256(bytes);
    let fault = probe_persistence_fault(
        PersistenceOperation::Artifact,
        PersistenceBoundary::PartialWrite,
    );
    assert!(store.retain_manifest(bytes).is_err());
    drop(fault);
    let path = temporary
        .path()
        .join(format!("private/manifest-{digest}.json"));
    let partial = std::fs::read(&path).unwrap();
    assert!(!partial.is_empty() && partial.len() < bytes.len());
    assert!(store.retain_manifest(bytes).is_err());
    drop(store);
    let mut reopened = JournalStore::open_windows(root).unwrap();
    reopened.bind_existing(&binding()).unwrap();
    assert!(reopened.retain_manifest(bytes).is_err());
    assert!(reopened
        .append(
            0,
            JournalEvent::Manifest {
                role: ManifestRole::SourceBundle,
                digest
            }
        )
        .is_err());
    assert_eq!(std::fs::read(path).unwrap(), partial);
}

// 检查marker只能在同根control lease下持久化精确journal检查点，且旧代不能覆盖新代。
#[test]
fn HistoryJournalWindows_MarkerChain_004() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    assert!(MarkerStore::open_existing(root.clone(), &control)
        .unwrap()
        .is_none());
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let initial = store.inspect(&binding()).unwrap();
    let marker = ActiveContextMarker::transition_from(&initial).unwrap();
    let mut markers = MarkerStore::create(root.clone(), &control, &marker, &mut store).unwrap();
    assert_eq!(markers.current().unwrap(), marker.encode().unwrap());
    assert!(markers.append(&marker, &mut store).is_err());
    store
        .append(
            0,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    let next = store.inspect(&binding()).unwrap();
    let next_marker = ActiveContextMarker::transition_from(&next).unwrap();
    assert!(markers.append(&marker, &mut store).is_err());
    markers.append(&next_marker, &mut store).unwrap();
    drop(markers);
    let reopened = MarkerStore::open_existing(root.clone(), &control)
        .unwrap()
        .unwrap();
    assert_eq!(reopened.current().unwrap(), next_marker.encode().unwrap());
    let other_temporary = tempfile::tempdir().unwrap();
    let other_root = private_root(other_temporary.path());
    assert!(MarkerStore::open_existing(other_root, &control).is_err());
}

// 检查marker半写或完整写后回执丢失均保留已有marker；半帧禁止回退先前终态。
#[test]
fn HistoryJournalWindows_TornMarker_005() {
    for boundary in [
        PersistenceBoundary::PartialWrite,
        PersistenceBoundary::FlushCall,
        PersistenceBoundary::Readback,
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = private_root(temporary.path());
        let user = CurrentUser::capture().unwrap();
        let leases = LeaseFiles::open(root.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let mut store = JournalStore::open_windows(root.clone()).unwrap();
        store.initialize(binding(), capacity()).unwrap();
        let initial = store.inspect(&binding()).unwrap();
        let marker = ActiveContextMarker::transition_from(&initial).unwrap();
        let mut markers = MarkerStore::create(root.clone(), &control, &marker, &mut store).unwrap();
        let path = temporary.path().join("private/active-context.log");
        let before = std::fs::read(&path).unwrap();
        store
            .append(
                0,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired,
                },
            )
            .unwrap();
        let next = store.inspect(&binding()).unwrap();
        let marker = ActiveContextMarker::transition_from(&next).unwrap();
        let fault = probe_persistence_fault(PersistenceOperation::MarkerFrame, boundary);
        assert!(markers.append(&marker, &mut store).is_err());
        assert!(markers.current().is_err());
        drop(fault);
        let after = std::fs::read(&path).unwrap();
        assert!(after.starts_with(&before) && after.len() > before.len());
        assert!(markers.append(&marker, &mut store).is_err());
        drop(markers);
        let read = MarkerStore::open_existing(root, &control);
        if boundary == PersistenceBoundary::PartialWrite {
            assert!(read.is_err());
        } else {
            assert_eq!(
                read.unwrap().unwrap().current().unwrap(),
                marker.encode().unwrap()
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), after);
    }
}

// 检查同一句柄flush后被改变的字节无法通过真实readback，且损坏frame阻止重绑。
#[test]
fn HistoryJournalWindows_Readback_006() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let fault = probe_persistence_fault(
        PersistenceOperation::JournalFrame,
        PersistenceBoundary::ChangedReadback,
    );
    assert!(store
        .append(
            0,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
    drop(fault);
    drop(store);
    let mut reopened = JournalStore::open_windows(root).unwrap();
    assert!(reopened.inspect(&binding()).unwrap().blocked);
    assert!(reopened.bind_existing(&binding()).is_err());
}

// 检查immutable artifact预算按唯一digest计数，在创建第三个文件前拒绝并保留前两份。
#[test]
fn HistoryJournalWindows_Budget_007() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let mut store = JournalStore::fixture_windows_dependency_limit(root, 2).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let first = store.retain_manifest(b"first").unwrap();
    let second = store.retain_manifest(b"second").unwrap();
    assert_eq!(store.retain_manifest(b"first").unwrap(), first);
    assert_eq!(store.fixture_dependency_count(), 2);
    assert!(store.retain_manifest(b"third").is_err());
    let third = crate::version_history::verified_package::sha256(b"third");
    assert!(!temporary
        .path()
        .join(format!("private/manifest-{third}.json"))
        .exists());
    assert_eq!(store.read_manifest(&first).unwrap(), b"first");
    assert_eq!(store.read_manifest(&second).unwrap(), b"second");
    assert!(store
        .retain_manifest(&vec![0; 32 * 1024 * 1024 + 1])
        .is_err());
    assert!(store.retain_manifest(b"").is_err());
}

// 检查marker的外来事务/伪造检查点/创建碰撞均不能覆盖已存在记录。
#[test]
fn HistoryJournalWindows_MarkerBinding_008() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let inspection = store.inspect(&binding()).unwrap();
    let fabricated = ActiveContextMarker::transition(binding(), 99, "9".repeat(64)).unwrap();
    assert!(MarkerStore::create(root.clone(), &control, &fabricated, &mut store).is_err());
    assert!(!temporary.path().join("private/active-context.log").exists());
    let marker = ActiveContextMarker::transition_from(&inspection).unwrap();
    let mut markers = MarkerStore::create(root.clone(), &control, &marker, &mut store).unwrap();
    assert!(MarkerStore::create(root.clone(), &control, &marker, &mut store).is_err());
    let other_temp = tempfile::tempdir().unwrap();
    let other_root = private_root(other_temp.path());
    let mut other_binding = binding();
    other_binding.transaction_id = uuid::Uuid::new_v4().to_string();
    let mut other = JournalStore::open_windows(other_root).unwrap();
    other.initialize(other_binding.clone(), capacity()).unwrap();
    other
        .append(
            0,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    let foreign = other.inspect(&other_binding).unwrap();
    let foreign_marker = ActiveContextMarker::transition_from(&foreign).unwrap();
    assert!(markers.append(&foreign_marker, &mut other).is_err());
    assert_eq!(markers.current().unwrap(), marker.encode().unwrap());
}

// 检查空marker文件属于未完成写入，不能被解释为不存在；损坏链不能回退旧记录。
#[test]
fn HistoryJournalWindows_EmptyMarker_009() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let path = temporary.path().join("private/active-context.log");
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let inspection = store.inspect(&binding()).unwrap();
    let marker = ActiveContextMarker::transition_from(&inspection).unwrap();
    let fault = probe_persistence_fault(
        PersistenceOperation::MarkerFrame,
        PersistenceBoundary::BeforeWrite,
    );
    assert!(MarkerStore::create(root.clone(), &control, &marker, &mut store).is_err());
    drop(fault);
    assert!(MarkerStore::open_existing(root, &control).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"");
}

// 检查真实artifact的硬链接计数漂移阻止后续frame，而不是只相信缓存digest。
#[test]
fn HistoryJournalWindows_AliasDrift_010() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let mut store = JournalStore::open_windows(root).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let digest = store.retain_manifest(b"protected prerequisite").unwrap();
    store
        .append(
            0,
            JournalEvent::Manifest {
                role: ManifestRole::SourceBundle,
                digest: digest.clone(),
            },
        )
        .unwrap();
    let log = temporary.path().join("private/journal.log");
    let before = std::fs::read(&log).unwrap();
    std::fs::hard_link(
        temporary
            .path()
            .join(format!("private/manifest-{digest}.json")),
        temporary.path().join("alias"),
    )
    .unwrap();
    assert!(store
        .append(
            1,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
    assert_eq!(std::fs::read(log).unwrap(), before);
    assert!(store.inspect(&binding()).unwrap().blocked);
}

// 检查Windows后端复用forward/recovery独立容量，正向满额仍可记录recovery状态。
#[test]
fn HistoryJournalWindows_RecoveryLane_011() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let mut store = JournalStore::open_windows(root).unwrap();
    store
        .initialize(binding(), CapacityPlan::for_effects(1, 1, 1, 4096).unwrap())
        .unwrap();
    let digest = store.retain_manifest(b"fixture manifest").unwrap();
    for (generation, role) in [
        ManifestRole::SourceBundle,
        ManifestRole::Registration,
        ManifestRole::Shortcuts,
        ManifestRole::SourceContext,
    ]
    .into_iter()
    .enumerate()
    {
        store
            .append(
                generation as u64,
                JournalEvent::Manifest {
                    role,
                    digest: digest.clone(),
                },
            )
            .unwrap();
    }
    let path = temporary.path().join("private/journal.log");
    let full = std::fs::read(&path).unwrap();
    assert!(store
        .append(
            4,
            JournalEvent::Manifest {
                role: ManifestRole::FreshTargetContext,
                digest
            }
        )
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), full);
    store
        .append(
            4,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    assert_eq!(
        store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        5
    );
}

// 检查真实持久化receipt绑定原transaction/effect/intent代数，错误代数不能推进journal。
#[test]
fn HistoryJournalWindows_Receipt_012() {
    use crate::version_history::journal::{EffectKind, EffectSpec, Observation, ObservedResult};
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let before = store.retain_manifest(b"fixture before-state").unwrap();
    let expected = store.retain_manifest(b"fixture expected-state").unwrap();
    let observed = store.retain_manifest(b"fixture observed-state").unwrap();
    let effect_id = uuid::Uuid::new_v4().to_string();
    store
        .append(
            0,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: effect_id.clone(),
                    kind: EffectKind::VerifySourceBundleCopy,
                    before,
                    expected_postconditions: expected,
                },
            },
        )
        .unwrap();
    let receipt = store
        .retain_effect_receipt(&effect_id, Observation::Applied, &observed)
        .unwrap();
    assert!(store
        .append(
            1,
            JournalEvent::Observed {
                effect_id: effect_id.clone(),
                intent_generation: 2,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt.clone())
                }
            }
        )
        .is_err());
    store
        .append(
            1,
            JournalEvent::Observed {
                effect_id,
                intent_generation: 1,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt),
                },
            },
        )
        .unwrap();
    drop(store);
    let mut reopened = JournalStore::open_windows(root).unwrap();
    reopened.bind_existing(&binding()).unwrap();
    let inspected = reopened.inspect(&binding()).unwrap();
    assert!(!inspected.blocked);
    let journal = inspected.last_valid.unwrap();
    assert_eq!(journal.generation(), 2);
    assert!(!journal.requires_reconciliation());
}

// 检查完全相同binding的另一根journal不能授权本根marker创建或追加。
#[test]
fn HistoryJournalWindows_ForeignRoot_013() {
    let a = tempfile::tempdir().unwrap();
    let a_root = private_root(a.path());
    let mut a_store = JournalStore::open_windows(a_root).unwrap();
    a_store.initialize(binding(), capacity()).unwrap();
    let a_inspection = a_store.inspect(&binding()).unwrap();
    let a_marker = ActiveContextMarker::transition_from(&a_inspection).unwrap();
    let b = tempfile::tempdir().unwrap();
    let b_root = private_root(b.path());
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(b_root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    assert!(MarkerStore::create(b_root.clone(), &control, &a_marker, &mut a_store).is_err());
    assert!(!b.path().join("private/active-context.log").exists());
    let mut b_store = JournalStore::open_windows(b_root.clone()).unwrap();
    b_store.initialize(binding(), capacity()).unwrap();
    let b_inspection = b_store.inspect(&binding()).unwrap();
    let b_marker = ActiveContextMarker::transition_from(&b_inspection).unwrap();
    let mut markers = MarkerStore::create(b_root, &control, &b_marker, &mut b_store).unwrap();
    a_store
        .append(
            0,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    let advanced = a_store.inspect(&binding()).unwrap();
    let marker = ActiveContextMarker::transition_from(&advanced).unwrap();
    assert!(markers.append(&marker, &mut a_store).is_err());
    assert_eq!(markers.current().unwrap(), b_marker.encode().unwrap());
}

// 检查journal前进或不确定写入后，旧inspection不能授权marker发布。
#[test]
fn HistoryJournalWindows_StaleHead_014() {
    for uncertain in [false, true] {
        for append_existing in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let root = private_root(temporary.path());
            let user = CurrentUser::capture().unwrap();
            let leases = LeaseFiles::open(root.clone(), &user).unwrap();
            let control = leases.acquire_control().unwrap();
            let mut store = JournalStore::open_windows(root.clone()).unwrap();
            store.initialize(binding(), capacity()).unwrap();
            let initial = store.inspect(&binding()).unwrap();
            let initial_marker = ActiveContextMarker::transition_from(&initial).unwrap();
            let mut markers = if append_existing {
                Some(
                    MarkerStore::create(root.clone(), &control, &initial_marker, &mut store)
                        .unwrap(),
                )
            } else {
                None
            };
            store
                .append(
                    0,
                    JournalEvent::Phase {
                        phase: JournalPhase::RecoveryRequired,
                    },
                )
                .unwrap();
            let old = store.inspect(&binding()).unwrap();
            let old_marker = ActiveContextMarker::transition_from(&old).unwrap();
            let fault = uncertain.then(|| {
                probe_persistence_fault(
                    PersistenceOperation::JournalFrame,
                    PersistenceBoundary::AfterFlush,
                )
            });
            let result = store.append(
                1,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired,
                },
            );
            assert_eq!(result.is_err(), uncertain);
            drop(fault);
            if let Some(markers) = &mut markers {
                // Generation1 follows the retained generation0 marker, but the
                // actual journal advanced (or was poisoned) at generation2.
                assert!(markers.append(&old_marker, &mut store).is_err());
                assert_eq!(markers.current().unwrap(), initial_marker.encode().unwrap());
            } else {
                assert!(MarkerStore::create(root, &control, &old_marker, &mut store).is_err());
                assert!(!temporary.path().join("private/active-context.log").exists());
            }
        }
    }
}

// 检查完整artifact在flush失败或后续读回失败后保留，重开验证可引用该完整文件。
#[test]
fn HistoryJournalWindows_RecoverArtifact_015() {
    for boundary in [
        PersistenceBoundary::FlushCall,
        PersistenceBoundary::AfterWrite,
        PersistenceBoundary::AfterFlush,
        PersistenceBoundary::Readback,
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = private_root(temporary.path());
        let mut store = JournalStore::open_windows(root.clone()).unwrap();
        store.initialize(binding(), capacity()).unwrap();
        let bytes = b"complete artifact after a lost persistence receipt";
        let digest = crate::version_history::verified_package::sha256(bytes);
        let fault = probe_persistence_fault(PersistenceOperation::Artifact, boundary);
        assert!(store.retain_manifest(bytes).is_err());
        if boundary == PersistenceBoundary::FlushCall {
            assert_eq!(
                fault.flush_error(),
                Some(5),
                "the same-object read-only handle must fail FlushFileBuffers with access denied"
            );
        }
        drop(fault);
        let path = temporary
            .path()
            .join(format!("private/manifest-{digest}.json"));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(store.retain_manifest(bytes).is_err());
        drop(store);
        let mut reopened = JournalStore::open_windows(root).unwrap();
        reopened.bind_existing(&binding()).unwrap();
        assert_eq!(reopened.retain_manifest(bytes).unwrap(), digest);
        reopened
            .append(
                0,
                JournalEvent::Manifest {
                    role: ManifestRole::SourceBundle,
                    digest,
                },
            )
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(!reopened.inspect(&binding()).unwrap().blocked);
    }
}

// 检查持有journal期间发生的文件身份漂移在发布前复核，旧inspection不能消除poison。
#[test]
fn HistoryJournalWindows_LiveCheckpoint_016() {
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let artifact = store.retain_manifest(b"held prerequisite").unwrap();
    let inspection = store.inspect(&binding()).unwrap();
    let marker = ActiveContextMarker::transition_from(&inspection).unwrap();
    std::fs::hard_link(
        temporary
            .path()
            .join(format!("private/manifest-{artifact}.json")),
        temporary.path().join("alias"),
    )
    .unwrap();
    assert!(MarkerStore::create(root.clone(), &control, &marker, &mut store).is_err());
    assert!(MarkerStore::create(root, &control, &marker, &mut store).is_err());
    assert!(!temporary.path().join("private/active-context.log").exists());
}

// 检查early-abort完整frame回执丢失后仍需重开验证，旧transition marker不能提前放行。
#[test]
fn HistoryJournalWindows_AbortReceipt_017() {
    use crate::version_history::journal::{JournalPhase, PreContextAbortProof};
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.initialize(binding(), capacity()).unwrap();
    let digest = store
        .retain_manifest(b"fixture complete unchanged-source observations")
        .unwrap();
    let initial = store.inspect(&binding()).unwrap();
    let transition = ActiveContextMarker::transition_from(&initial).unwrap();
    let mut markers = MarkerStore::create(root.clone(), &control, &transition, &mut store).unwrap();
    let proof = PreContextAbortProof::fixture(&initial, std::array::from_fn(|_| digest.clone()));
    let fault = probe_persistence_fault(
        PersistenceOperation::JournalFrame,
        PersistenceBoundary::AfterWrite,
    );
    assert!(store.abort_pre_context(&proof).is_err());
    drop(fault);
    assert!(store.inspect(&binding()).unwrap().blocked);
    assert_eq!(markers.current().unwrap(), transition.encode().unwrap());
    drop(store);
    let mut store = JournalStore::open_windows(root).unwrap();
    store.bind_existing(&binding()).unwrap();
    let inspected = store.inspect(&binding()).unwrap();
    assert_eq!(
        inspected.last_valid.as_ref().unwrap().phase(),
        JournalPhase::PreContextAborted
    );
    let aborted = ActiveContextMarker::pre_context_aborted(&inspected).unwrap();
    markers.append(&aborted, &mut store).unwrap();
    assert_eq!(markers.current().unwrap(), aborted.encode().unwrap());
}

// 检查Windows真实持久化compensation约束、later留存和完整return终态保持原Unknown。
#[test]
fn HistoryJournalWindows_ReturnOnly_018() {
    use crate::version_history::journal::{
        EffectKind, EffectSpec, Observation, ObservedResult, RegistrationSlot, RootKind,
        ShortcutSlot, UnknownCompensationProof,
    };
    fn observed(
        store: &mut JournalStore,
        generation: &mut u64,
        kind: EffectKind,
        before: &str,
        after: &str,
    ) {
        let id = uuid::Uuid::new_v4().to_string();
        *generation = store
            .append(
                *generation,
                JournalEvent::Intent {
                    effect: EffectSpec {
                        effect_id: id.clone(),
                        kind,
                        before: before.into(),
                        expected_postconditions: after.into(),
                    },
                },
            )
            .unwrap();
        let intent = *generation;
        let receipt = store
            .retain_effect_receipt(&id, Observation::Applied, after)
            .unwrap();
        *generation = store
            .append(
                *generation,
                JournalEvent::Observed {
                    effect_id: id,
                    intent_generation: intent,
                    result: ObservedResult {
                        observation: Observation::Applied,
                        receipt: Some(receipt),
                    },
                },
            )
            .unwrap();
    }
    let temporary = tempfile::tempdir().unwrap();
    let root = private_root(temporary.path());
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding(),
            CapacityPlan::for_effects(100, 100, 10, 4096).unwrap(),
        )
        .unwrap();
    let before = store
        .retain_manifest(b"fixture owned before-state")
        .unwrap();
    let after = store
        .retain_manifest(b"fixture retained after-state")
        .unwrap();
    let initial = store.inspect(&binding()).unwrap();
    let transition = ActiveContextMarker::transition_from(&initial).unwrap();
    let mut markers = MarkerStore::create(root.clone(), &control, &transition, &mut store).unwrap();
    let mut generation = 0;
    for role in [
        ManifestRole::SourceContext,
        ManifestRole::SourceBundle,
        ManifestRole::Registration,
        ManifestRole::Shortcuts,
    ] {
        generation = store
            .append(
                generation,
                JournalEvent::Manifest {
                    role,
                    digest: after.clone(),
                },
            )
            .unwrap();
    }
    for kind in [
        EffectKind::VerifySourceBundleCopy,
        EffectKind::FenceSourceImage,
        EffectKind::PreserveRoot {
            context: binding().source_context.clone(),
            root: RootKind::Desk,
        },
        EffectKind::PreserveRoot {
            context: binding().source_context,
            root: RootKind::WebView,
        },
    ] {
        observed(&mut store, &mut generation, kind, &before, &after);
    }
    generation = store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::SourceSealed,
            },
        )
        .unwrap();
    for root in [RootKind::Desk, RootKind::WebView] {
        observed(
            &mut store,
            &mut generation,
            EffectKind::CreateFreshRoot { root },
            &before,
            &after,
        );
    }
    generation = store
        .append(
            generation,
            JournalEvent::Manifest {
                role: ManifestRole::FreshTargetContext,
                digest: after.clone(),
            },
        )
        .unwrap();
    for phase in [JournalPhase::FreshReady, JournalPhase::Installing] {
        generation = store
            .append(generation, JournalEvent::Phase { phase })
            .unwrap();
    }
    let id = uuid::Uuid::new_v4().to_string();
    generation = store
        .append(
            generation,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: id.clone(),
                    kind: EffectKind::InstallerResume,
                    before: before.clone(),
                    expected_postconditions: after.clone(),
                },
            },
        )
        .unwrap();
    let proof = UnknownCompensationProof::fixture(
        &store.inspect(&binding()).unwrap(),
        std::array::from_fn(|_| before.clone()),
    );
    assert!(store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restoring
            }
        )
        .is_err());
    generation = store.compensate_unknown(&proof).unwrap();
    assert!(store
        .append(
            generation,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: uuid::Uuid::new_v4().to_string(),
                    kind: EffectKind::InstallerResume,
                    before: before.clone(),
                    expected_postconditions: after.clone()
                }
            }
        )
        .is_err());
    assert_eq!(
        store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .effect_observation(&id),
        Some(Observation::Unknown)
    );
    observed(
        &mut store,
        &mut generation,
        EffectKind::FenceHistoricalImage,
        &before,
        &after,
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        observed(
            &mut store,
            &mut generation,
            EffectKind::PreserveRoot {
                context: binding().target_context,
                root,
            },
            &before,
            &after,
        );
    }
    generation = store
        .append(
            generation,
            JournalEvent::Manifest {
                role: ManifestRole::RetainedTargetContext,
                digest: after.clone(),
            },
        )
        .unwrap();
    generation = store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restoring,
            },
        )
        .unwrap();
    observed(
        &mut store,
        &mut generation,
        EffectKind::VerifySourceBundleRestore,
        &before,
        &after,
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        observed(
            &mut store,
            &mut generation,
            EffectKind::RestoreSourceRoot { root },
            &before,
            &after,
        );
    }
    for slot in [
        RegistrationSlot::Uninstall,
        RegistrationSlot::Publisher,
        RegistrationSlot::DeskDirectory,
        RegistrationSlot::DeskDirectoryBackground,
        RegistrationSlot::LegacyDirectory,
        RegistrationSlot::LegacyDirectoryBackground,
        RegistrationSlot::OwnedRun,
    ] {
        observed(
            &mut store,
            &mut generation,
            EffectKind::VerifyRegistrationRestore { slot },
            &before,
            &after,
        );
    }
    for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
        observed(
            &mut store,
            &mut generation,
            EffectKind::RestoreShortcut { slot },
            &before,
            &after,
        );
    }
    store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restored,
            },
        )
        .unwrap();
    drop(store);
    let mut store = JournalStore::open_windows(root).unwrap();
    store.bind_existing(&binding()).unwrap();
    let inspection = store.inspect(&binding()).unwrap();
    assert!(inspection
        .last_valid
        .as_ref()
        .unwrap()
        .has_historical_uncertainty());
    assert_eq!(
        inspection
            .last_valid
            .as_ref()
            .unwrap()
            .effect_observation(&id),
        Some(Observation::Unknown)
    );
    let restored = ActiveContextMarker::restored(&inspection).unwrap();
    markers.append(&restored, &mut store).unwrap();
    assert_eq!(markers.current().unwrap(), restored.encode().unwrap());
}
