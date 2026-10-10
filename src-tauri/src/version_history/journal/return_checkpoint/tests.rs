#![allow(non_snake_case)]
use super::*;

fn fixture() -> (tempfile::TempDir, JournalStore, JournalBinding) {
    let root = tempfile::tempdir().unwrap();
    let store = JournalStore::fixture(
        Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap(),
    )
    .unwrap();
    let (store, binding) = populate(store);
    (root, store, binding)
}
fn populate(store: JournalStore) -> (JournalStore, JournalBinding) {
    populate_with_extra(store, false)
}
fn populate_with_extra(store: JournalStore, extra_launch: bool) -> (JournalStore, JournalBinding) {
    populate_until_installer(store, extra_launch, false)
}
fn populate_until_installer(
    mut store: JournalStore,
    extra_launch: bool,
    stop: bool,
) -> (JournalStore, JournalBinding) {
    let binding = JournalBinding {
        transaction_id: uuid::Uuid::new_v4().to_string(),
        source_context: uuid::Uuid::new_v4().to_string(),
        target_context: uuid::Uuid::new_v4().to_string(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 4096).unwrap(),
        )
        .unwrap();
    let digest = store
        .retain_manifest(b"synthetic observation, not native authority")
        .unwrap();
    for role in [
        ManifestRole::ManagerHandoff,
        ManifestRole::SourceHandoffExit,
        ManifestRole::SourceContext,
        ManifestRole::SourceBundle,
        ManifestRole::Registration,
        ManifestRole::Shortcuts,
    ] {
        append(
            &mut store,
            JournalEvent::Manifest {
                role,
                digest: digest.clone(),
            },
        );
    }
    for kind in [
        EffectKind::FenceSourceImage,
        EffectKind::VerifySourceBundleCopy,
    ] {
        applied(&mut store, kind, &digest);
    }
    for root in [RootKind::Desk, RootKind::WebView] {
        applied(
            &mut store,
            EffectKind::PreserveRoot {
                context: binding.source_context.clone(),
                root,
            },
            &digest,
        );
    }
    append(
        &mut store,
        JournalEvent::Phase {
            phase: JournalPhase::SourceSealed,
        },
    );
    append(
        &mut store,
        JournalEvent::Manifest {
            role: ManifestRole::FreshTargetContext,
            digest: digest.clone(),
        },
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        applied(&mut store, EffectKind::CreateFreshRoot { root }, &digest);
    }
    append(
        &mut store,
        JournalEvent::Phase {
            phase: JournalPhase::FreshReady,
        },
    );
    append(
        &mut store,
        JournalEvent::Phase {
            phase: JournalPhase::Installing,
        },
    );
    if stop {
        return (store, binding);
    }
    for kind in [
        EffectKind::InstallerCreateSuspended,
        EffectKind::InstallerResume,
        EffectKind::InstallerTerminalOutcome,
        EffectKind::VerifyTargetBundle,
    ] {
        applied(&mut store, kind, &digest);
    }
    append(
        &mut store,
        JournalEvent::Phase {
            phase: JournalPhase::InstalledUnconfirmed,
        },
    );
    for kind in [
        EffectKind::HistoricalCreateSuspended,
        EffectKind::HistoricalResume,
        EffectKind::HistoricalTerminalOutcome,
        EffectKind::FenceHistoricalImage,
    ] {
        applied(&mut store, kind, &digest);
    }
    // 实际保存路径会收窄为return-only；这不是恢复效果已发生。
    if extra_launch {
        applied(&mut store, EffectKind::HistoricalCreateSuspended, &digest);
    }
    append(
        &mut store,
        JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        },
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        let plan = LaterBackupPlan {
            root,
            source_manifest: digest.clone(),
            destination: digest.clone(),
            source_reservation: digest.clone(),
            previous_generation: None,
            previous_observation: None,
            abandoned_effect: None,
            effects: 1,
            recovery_dependencies: 128,
        };
        let receipt = store
            .retain_manifest(
                &serde_json::to_vec(&LaterBackupReceipt {
                    schema: 1,
                    anchor: anchor(&store),
                    plan: plan.clone(),
                })
                .unwrap(),
            )
            .unwrap();
        admitted(
            &mut store,
            JournalEvent::PrepareLaterBackup { plan, receipt },
        );
        let plan_generation = store.writer.as_ref().unwrap().journal.generation;
        applied(
            &mut store,
            EffectKind::PrivateBackupEntry {
                plan_generation,
                operation: PrivateBackupOperation::CopyFile,
                manifest: digest.clone(),
                entry_index: 0,
            },
            &digest,
        );
        let receipt = store
            .retain_manifest(
                &serde_json::to_vec(&LaterCompleteReceipt {
                    schema: 1,
                    anchor: anchor(&store),
                    root,
                    plan_generation,
                    copy_manifest: digest.clone(),
                })
                .unwrap(),
            )
            .unwrap();
        admitted(
            &mut store,
            JournalEvent::CompleteLaterBackup {
                root,
                plan_generation,
                copy_manifest: digest.clone(),
                receipt,
            },
        );
        applied(
            &mut store,
            EffectKind::PreserveRoot {
                context: binding.target_context.clone(),
                root,
            },
            &digest,
        );
    }
    append(
        &mut store,
        JournalEvent::Manifest {
            role: ManifestRole::RetainedTargetContext,
            digest: digest.clone(),
        },
    );
    let receipt = store
        .retain_manifest(
            &serde_json::to_vec(&BundleStartReceipt {
                schema: 1,
                anchor: anchor(&store),
                seed: digest.clone(),
                current_manifest: digest.clone(),
                effects: 1,
                recovery_dependencies: 128,
            })
            .unwrap(),
        )
        .unwrap();
    admitted(
        &mut store,
        JournalEvent::AdmitBundleStart {
            seed: digest.clone(),
            current_manifest: digest.clone(),
            effects: 1,
            recovery_dependencies: 128,
            receipt,
        },
    );
    let plan_generation = store.writer.as_ref().unwrap().journal.generation;
    applied(
        &mut store,
        EffectKind::PrivateBackupEntry {
            plan_generation,
            operation: PrivateBackupOperation::CopyFile,
            manifest: digest.clone(),
            entry_index: 0,
        },
        &digest,
    );
    append(
        &mut store,
        JournalEvent::Phase {
            phase: JournalPhase::Restoring,
        },
    );
    (store, binding)
}
fn anchor(store: &JournalStore) -> AdmissionAnchor {
    let state = store.writer.as_ref().unwrap();
    AdmissionAnchor {
        binding: state.journal.binding.clone(),
        generation: state.journal.generation,
        head: state.head.clone(),
        journal_identity: state.identity.clone(),
    }
}
fn admitted(store: &mut JournalStore, event: JournalEvent) {
    let generation = store.writer.as_ref().unwrap().journal.generation;
    store.append_admitted(generation, event).unwrap();
}
fn append(store: &mut JournalStore, event: JournalEvent) {
    let generation = store.writer.as_ref().unwrap().journal.generation;
    store.append(generation, event).unwrap();
}
fn applied(store: &mut JournalStore, kind: EffectKind, digest: &str) {
    let effect_id = uuid::Uuid::new_v4().to_string();
    append(
        store,
        JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: effect_id.clone(),
                kind,
                before: digest.into(),
                expected_postconditions: digest.into(),
            },
        },
    );
    let intent_generation = store.writer.as_ref().unwrap().journal.generation;
    let receipt = store
        .retain_effect_receipt(&effect_id, Observation::Applied, digest)
        .unwrap();
    append(
        store,
        JournalEvent::Observed {
            effect_id,
            intent_generation,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt),
            },
        },
    );
}

// 真实持久化层必须拒绝缺失、损坏、不完整及超限检查点；状态字段本身不授权。
#[test]
fn HistoryReturnCheckpoint_InvalidArtifactsNeverCommit_006() {
    for bytes in [
        None,
        Some(b"{".to_vec()),
        Some(b"{\"schema\":1}".to_vec()),
        Some(vec![b' '; 16385]),
    ] {
        let (_root, mut store, binding) = fixture();
        let digest = match bytes {
            Some(bytes) => store.retain_manifest(&bytes).unwrap(),
            None => "f".repeat(64),
        };
        let generation = store.writer.as_ref().unwrap().journal.generation;
        assert!(store
            .append_admitted(
                generation,
                JournalEvent::ReturnCheckpointSealed { checkpoint: digest }
            )
            .is_err());
        assert_eq!(
            store
                .inspect(&binding)
                .unwrap()
                .last_valid
                .unwrap()
                .generation(),
            generation
        );
    }
}

// 普通追加API即使拿到合法事件也不能代替原live owner签发。
#[test]
fn HistoryReturnCheckpoint_RawAppendCannotIssue_007() {
    let (_root, mut store, _) = fixture();
    let generation = store.writer.as_ref().unwrap().journal.generation;
    assert!(store
        .append(
            generation,
            JournalEvent::ReturnCheckpointSealed {
                checkpoint: "d".repeat(64)
            }
        )
        .is_err());
}

fn marker(
    store: &JournalStore,
    binding: &JournalBinding,
) -> crate::version_history::maintenance::ActiveContextMarker {
    crate::version_history::maintenance::ActiveContextMarker::transition_from(
        &store.inspect(binding).unwrap(),
    )
    .unwrap()
}
fn sealed(store: &mut JournalStore) -> String {
    let checkpoint = store
        .retain_return_checkpoint(b"synthetic native materials")
        .unwrap();
    let generation = store.writer.as_ref().unwrap().journal.generation;
    store
        .append_admitted(
            generation,
            JournalEvent::ReturnCheckpointSealed {
                checkpoint: checkpoint.clone(),
            },
        )
        .unwrap();
    checkpoint
}
fn reopen(root: &tempfile::TempDir) -> JournalStore {
    JournalStore::fixture(Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap())
        .unwrap()
}

// 文件、日志、marker三步缺一不可；完整封存只能获得只读检查结果。
#[test]
fn HistoryReturnCheckpoint_PersistenceBoundaries_008() {
    let (_root, mut store, binding) = fixture();
    let prior = marker(&store, &binding);
    let checkpoint = store
        .retain_return_checkpoint(b"synthetic native materials")
        .unwrap();
    assert!(store.inspect_return_checkpoint(&binding, &prior).is_err());
    let generation = store.writer.as_ref().unwrap().journal.generation;
    store
        .append_admitted(
            generation,
            JournalEvent::ReturnCheckpointSealed {
                checkpoint: checkpoint.clone(),
            },
        )
        .unwrap();
    assert!(store.inspect_return_checkpoint(&binding, &prior).is_err());
    let current = marker(&store, &binding);
    let observed = store.inspect_return_checkpoint(&binding, &current).unwrap();
    assert_eq!(observed.digest, checkpoint);
    assert_eq!(observed.materials, b"synthetic native materials");
    assert_eq!(observed.generation, generation + 1);
}

// owner释放所有存储句柄后可重新只读核验；唯一claim一旦持久化便不再准入。
#[test]
fn HistoryReturnCheckpoint_ReopenAndClaimedCrash_009() {
    let (root, mut store, binding) = fixture();
    let checkpoint = sealed(&mut store);
    let current = marker(&store, &binding);
    drop(store);
    let mut fresh = reopen(&root);
    let observed = fresh.inspect_return_checkpoint(&binding, &current).unwrap();
    fresh.bind_existing(&binding).unwrap();
    fresh
        .append_admitted(
            observed.generation,
            JournalEvent::ReturnExecutionClaimed {
                checkpoint: checkpoint.clone(),
                attempt_id: uuid::Uuid::new_v4().to_string(),
            },
        )
        .unwrap();
    let claimed = marker(&fresh, &binding);
    let generation = fresh.writer.as_ref().unwrap().journal.generation;
    assert!(fresh
        .append_admitted(
            generation,
            JournalEvent::ReturnExecutionClaimed {
                checkpoint,
                attempt_id: uuid::Uuid::new_v4().to_string(),
            }
        )
        .is_err());
    drop(fresh);
    let reopened = reopen(&root);
    assert!(reopened
        .inspect_return_checkpoint(&binding, &current)
        .is_err());
    assert!(reopened
        .inspect_return_checkpoint(&binding, &claimed)
        .is_err());
}

// 部分日志尾、检查点丢失/损坏、依赖丢失均拒绝；读取不得修尾或写回。
#[test]
fn HistoryReturnCheckpoint_DamagedRestartIsReadOnly_010() {
    for mutation in 0..4 {
        let (root, mut store, binding) = fixture();
        let checkpoint = sealed(&mut store);
        let materials = store.read_return_checkpoint(&checkpoint).unwrap().materials;
        let current = marker(&store, &binding);
        drop(store);
        let log = root.path().join("journal.log");
        match mutation {
            0 => {
                let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
                file.write_all(b"{\"interrupted\":").unwrap();
                file.sync_all().unwrap();
            }
            1 => std::fs::remove_file(root.path().join(format!("manifest-{checkpoint}.json")))
                .unwrap(),
            2 => std::fs::write(
                root.path().join(format!("manifest-{checkpoint}.json")),
                b"corrupt",
            )
            .unwrap(),
            3 => std::fs::remove_file(root.path().join(format!("manifest-{materials}.json")))
                .unwrap(),
            _ => unreachable!(),
        }
        let before = std::fs::read(&log).unwrap();
        let reopened = reopen(&root);
        assert!(reopened
            .inspect_return_checkpoint(&binding, &current)
            .is_err());
        assert_eq!(std::fs::read(&log).unwrap(), before);
    }
}

// 摘要正确也不能把其他绑定、旧head、缺角色或未知字段的记录提升为检查点。
#[test]
fn HistoryReturnCheckpoint_ForeignOrIncompleteAnchorRejected_011() {
    for mutation in 0..5 {
        let (_root, mut store, _) = fixture();
        let digest = store
            .retain_return_checkpoint(b"synthetic native materials")
            .unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&store.read_manifest(&digest).unwrap()).unwrap();
        match mutation {
            0 => {
                value["anchor"]["binding"]["transaction_id"] =
                    uuid::Uuid::new_v4().to_string().into()
            }
            1 => value["anchor"]["head"] = "f".repeat(64).into(),
            2 => {
                value["roles"]
                    .as_object_mut()
                    .unwrap()
                    .remove("SourceHandoffExit");
            }
            3 => value["force"] = true.into(),
            4 => value["schema"] = 0.into(),
            _ => unreachable!(),
        }
        let changed = store
            .retain_manifest(&serde_json::to_vec(&value).unwrap())
            .unwrap();
        let generation = store.writer.as_ref().unwrap().journal.generation;
        assert!(store
            .append_admitted(
                generation,
                JournalEvent::ReturnCheckpointSealed {
                    checkpoint: changed
                }
            )
            .is_err());
    }
}

// 重复受管启动的日志不能用一个终态覆盖整个集合。
#[test]
fn HistoryReturnCheckpoint_ExtraManagedLaunchRejected_012() {
    let root = tempfile::tempdir().unwrap();
    let store = JournalStore::fixture(
        Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap(),
    )
    .unwrap();
    let (mut store, _) = populate_with_extra(store, true);
    assert!(store
        .retain_return_checkpoint(b"synthetic native materials")
        .is_err());
}

#[cfg(windows)]
fn native_fixture() -> (
    tempfile::TempDir,
    std::sync::Arc<crate::version_history::windows::files::PrivateDirectory>,
    JournalStore,
    JournalBinding,
) {
    use crate::version_history::windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        security::CurrentUser,
    };
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = std::sync::Arc::new(
        PrivateDirectory::create_new(
            Directory::open_absolute(temporary.path()).unwrap(),
            ComponentName::new(std::ffi::OsStr::new("private")).unwrap(),
            &user,
        )
        .unwrap(),
    );
    let (store, binding) = populate(JournalStore::open_windows(root.clone()).unwrap());
    (temporary, root, store, binding)
}

// 真正NTFS保护句柄和marker持久化/重开；进程终态材料仍是协议fixture，不是整程验收。
#[cfg(windows)]
#[test]
fn HistoryReturnCheckpoint_WindowsProtectedReopen_013() {
    use crate::version_history::windows::{
        durability::MarkerStore, lease::LeaseFiles, security::CurrentUser,
    };
    let (_temporary, root, mut store, binding) = native_fixture();
    let user = CurrentUser::capture().unwrap();
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let mut markers = MarkerStore::create(
        root.clone(),
        &control,
        &marker(&store, &binding),
        &mut store,
    )
    .unwrap();
    let digest = sealed(&mut store);
    let current = marker(&store, &binding);
    markers.append(&current, &mut store).unwrap();
    assert_eq!(markers.current().unwrap(), current.encode().unwrap());
    assert!(JournalStore::open_windows(root.clone()).is_err());
    drop(markers);
    drop(store);
    drop(control);
    let control = leases.acquire_control().unwrap();
    let reopened = JournalStore::open_windows(root.clone()).unwrap();
    let markers = MarkerStore::open_existing(root, &control).unwrap().unwrap();
    let current = crate::version_history::maintenance::ActiveContextMarker::decode(
        markers.current().unwrap(),
    )
    .unwrap();
    assert_eq!(
        reopened
            .inspect_return_checkpoint(&binding, &current)
            .unwrap()
            .digest,
        digest
    );
}

// 实际持久化故障后不补seal或marker，不从不完整检查点取得准入。
#[cfg(windows)]
#[test]
fn HistoryReturnCheckpoint_WindowsPersistenceFaults_014() {
    use crate::version_history::windows::{
        durability::{
            probe_persistence_fault, MarkerStore, PersistenceBoundary, PersistenceOperation,
        },
        lease::LeaseFiles,
        security::CurrentUser,
    };
    for stage in 0..3 {
        let (_temporary, root, mut store, binding) = native_fixture();
        let user = CurrentUser::capture().unwrap();
        let leases = LeaseFiles::open(root.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let mut markers = MarkerStore::create(
            root.clone(),
            &control,
            &marker(&store, &binding),
            &mut store,
        )
        .unwrap();
        match stage {
            0 => {
                let _fault = probe_persistence_fault(
                    PersistenceOperation::Artifact,
                    PersistenceBoundary::BeforeWrite,
                );
                assert!(store
                    .retain_return_checkpoint(b"synthetic native materials")
                    .is_err());
            }
            1 => {
                let checkpoint = store
                    .retain_return_checkpoint(b"synthetic native materials")
                    .unwrap();
                let generation = store.writer.as_ref().unwrap().journal.generation;
                let _fault = probe_persistence_fault(
                    PersistenceOperation::JournalFrame,
                    PersistenceBoundary::AfterFlush,
                );
                assert!(store
                    .append_admitted(
                        generation,
                        JournalEvent::ReturnCheckpointSealed { checkpoint }
                    )
                    .is_err());
            }
            2 => {
                sealed(&mut store);
                let current = marker(&store, &binding);
                let _fault = probe_persistence_fault(
                    PersistenceOperation::MarkerFrame,
                    PersistenceBoundary::BeforeWrite,
                );
                assert!(markers.append(&current, &mut store).is_err());
            }
            _ => unreachable!(),
        }
        drop(markers);
        drop(store);
        let reopened = JournalStore::open_windows(root.clone()).unwrap();
        match MarkerStore::open_existing(root, &control) {
            Ok(Some(markers)) => {
                let current = crate::version_history::maintenance::ActiveContextMarker::decode(
                    markers.current().unwrap(),
                )
                .unwrap();
                assert!(reopened
                    .inspect_return_checkpoint(&binding, &current)
                    .is_err());
            }
            Err(_) => {}
            Ok(None) => panic!("original marker must remain retained"),
        }
    }
}

#[cfg(windows)]
#[test]
fn OrdinaryDiagnostic_PreservedBackupSurvivesUnrelatedUnknown_010() {
    let root = tempfile::tempdir().unwrap();
    let store = JournalStore::fixture(
        Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap(),
    )
    .unwrap();
    let (mut store, binding) = populate_until_installer(store, false, true);
    let digest = store
        .retain_manifest(b"synthetic ordinary creation")
        .unwrap();
    applied(&mut store, EffectKind::InstallerCreateSuspended, &digest);
    let effect_id = uuid::Uuid::new_v4().to_string();
    append(
        &mut store,
        JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: effect_id.clone(),
                kind: EffectKind::InstallerResume,
                before: digest.clone(),
                expected_postconditions: digest,
            },
        },
    );
    let intent_generation = store.writer.as_ref().unwrap().journal.generation;
    append(
        &mut store,
        JournalEvent::Observed {
            effect_id,
            intent_generation,
            result: ObservedResult {
                observation: Observation::Unknown,
                receipt: None,
            },
        },
    );
    assert!(store
        .inspect(&binding)
        .unwrap()
        .last_valid
        .unwrap()
        .requires_reconciliation());
    assert!(store
        .applied_effect_observation(&EffectKind::VerifySourceBundleCopy)
        .is_err());
    let (_, bytes) = store
        .ordinary_backup_observation(&EffectKind::VerifySourceBundleCopy)
        .unwrap();
    assert_eq!(bytes, b"synthetic observation, not native authority");
    assert!(store
        .ordinary_backup_observation(&EffectKind::InstallerCreateSuspended)
        .is_ok());
    assert!(store
        .ordinary_backup_observation(&EffectKind::InstallerResume)
        .is_err());
    assert!(store
        .ordinary_backup_observation(&EffectKind::FenceSourceImage)
        .is_err());
}

#[test]
fn OrdinaryDiagnostic_ExactAppliedReceiptOnly_011() {
    let binding = JournalBinding {
        transaction_id: uuid::Uuid::new_v4().to_string(),
        source_context: uuid::Uuid::new_v4().to_string(),
        target_context: uuid::Uuid::new_v4().to_string(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let mut state = SwitchJournal::new(
        binding.clone(),
        CapacityPlan::for_effects(100, 100, 100, 4096).unwrap(),
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    state.effects.insert(
        id.clone(),
        EffectRecord {
            spec: EffectSpec {
                effect_id: id.clone(),
                kind: EffectKind::VerifySourceBundleCopy,
                before: "6".repeat(64),
                expected_postconditions: "7".repeat(64),
            },
            intent_generation: 3,
            result: Some(ObservedResult {
                observation: Observation::Applied,
                receipt: Some("8".repeat(64)),
            }),
        },
    );
    state.pending = Some("unrelated installer ambiguity".into());
    assert!(
        select_applied_observation(&state, &EffectKind::VerifySourceBundleCopy, false).is_err()
    );
    let effect =
        select_applied_observation(&state, &EffectKind::VerifySourceBundleCopy, true).unwrap();
    let mut receipt = EffectReceipt {
        schema: 1,
        transaction_id: binding.transaction_id,
        effect_id: id.clone(),
        intent_generation: 3,
        expected_postconditions: "7".repeat(64),
        observation: Observation::Applied,
        observed_manifest: "9".repeat(64),
    };
    validate_applied_receipt(
        &state.binding,
        effect,
        effect.result.as_ref().unwrap(),
        &receipt,
    )
    .unwrap();
    receipt.intent_generation += 1;
    assert!(validate_applied_receipt(
        &state.binding,
        effect,
        effect.result.as_ref().unwrap(),
        &receipt
    )
    .is_err());
    receipt.intent_generation -= 1;
    receipt.transaction_id = uuid::Uuid::new_v4().to_string();
    assert!(validate_applied_receipt(
        &state.binding,
        effect,
        effect.result.as_ref().unwrap(),
        &receipt
    )
    .is_err());
    assert!(select_applied_observation(&state, &EffectKind::FenceSourceImage, true).is_err());
    state
        .effects
        .get_mut(&id)
        .unwrap()
        .result
        .as_mut()
        .unwrap()
        .observation = Observation::Unknown;
    assert!(select_applied_observation(&state, &EffectKind::VerifySourceBundleCopy, true).is_err());
}
