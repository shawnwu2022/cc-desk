//! Foundation probes only. OS process/image/ACL durability acceptance is separate.
use crate::version_history::compatibility::{admit_data_mode, ReviewedDataMode};
use crate::version_history::journal::{
    CapacityPlan, EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase, JournalStore,
    Observation, ObservedResult, RootKind, SwitchJournal,
};
use crate::version_history::maintenance::{AdmissionGate, RuntimeKind, StartupControlLease};
use crate::version_history::snapshot::{
    capture_context, verify_context, ContextReader, EntryMetadata, EntryType, PermissionRecord,
    RootInventory, SnapshotLimits,
};
use cap_std::fs::Dir;
use std::collections::BTreeMap;
use std::io::{self, Cursor, Read};
use std::sync::{Arc, Barrier};

fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000001".into(),
        source_context: "00000000-0000-4000-8000-000000000002".into(),
        target_context: "00000000-0000-4000-8000-000000000003".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}
fn effect() -> EffectSpec {
    EffectSpec {
        effect_id: "00000000-0000-4000-8000-000000000004".into(),
        kind: EffectKind::PreserveRoot {
            context: binding().source_context,
            root: RootKind::Desk,
        },
        before: crate::version_history::verified_package::sha256(b"before"),
        expected_postconditions: crate::version_history::verified_package::sha256(b"after"),
    }
}
fn capacity() -> CapacityPlan {
    CapacityPlan::for_effects(1000, 1000, 64, 4096).unwrap()
}
fn store(root: &tempfile::TempDir) -> JournalStore {
    let store = JournalStore::fixture(
        Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap(),
    )
    .unwrap();
    store.retain_manifest(b"before").unwrap();
    store.retain_manifest(b"after").unwrap();
    store
}

// 活动、排队及未知PTY必须保持阻塞；从旧registry移除不是reap证据。
#[test]
fn HistoryTransaction_PendingAndUnknown_001() {
    let gate = AdmissionGate::new();
    let start = gate.begin_start(RuntimeKind::Legacy).unwrap();
    assert!(gate.freeze(&binding().transaction_id).is_err());
    let child = start.child_created();
    assert!(gate.freeze(&binding().transaction_id).is_err());
    drop(child);
    assert!(gate.freeze(&binding().transaction_id).is_err());
}

// 只有实际waiter观察的reap解除直接子进程；输出仍draining不代表进程存活。
#[test]
fn HistoryTransaction_ReapedWhileDraining_002() {
    let gate = AdmissionGate::new();
    gate.begin_start(RuntimeKind::Native)
        .unwrap()
        .child_created()
        .reaped();
    let freeze = gate.freeze(&binding().transaction_id).unwrap();
    assert!(gate.begin_start(RuntimeKind::Legacy).is_err());
    freeze.release_review().unwrap();
    gate.begin_start(RuntimeKind::Legacy)
        .unwrap()
        .no_child_created();
}

// wait失败及丢失的pending所有权均不能误报空闲。
#[test]
fn HistoryTransaction_WaitFailure_003() {
    let gate = AdmissionGate::new();
    gate.begin_start(RuntimeKind::Native)
        .unwrap()
        .child_created()
        .wait_failed();
    assert!(gate.freeze(&binding().transaction_id).is_err());
    let other = AdmissionGate::new();
    drop(other.begin_start(RuntimeKind::Legacy).unwrap());
    assert!(other.freeze(&binding().transaction_id).is_err());
}

// 原子零owner检查与启动竞争，恰好一方获准，不能留下新启动绕过freeze。
#[test]
fn HistoryTransaction_AdmissionRace_004() {
    for _ in 0..32 {
        let gate = AdmissionGate::new();
        let barrier = Arc::new(Barrier::new(2));
        let other = gate.clone();
        let ready = barrier.clone();
        let worker = std::thread::spawn(move || {
            ready.wait();
            other.begin_start(RuntimeKind::Native)
        });
        barrier.wait();
        let frozen = gate.freeze(&binding().transaction_id);
        let start = worker.join().unwrap();
        assert_ne!(frozen.is_ok(), start.is_ok());
    }
}

// committed不能走review取消，丢弃freeze也不能隐式重新准入。
#[test]
fn HistoryTransaction_CommittedFreeze_005() {
    let gate = AdmissionGate::new();
    let mut freeze = gate.freeze(&binding().transaction_id).unwrap();
    freeze.mark_committed().unwrap();
    assert!(freeze.release_review().is_err());
    assert!(gate.begin_start(RuntimeKind::Native).is_err());
}

// 新版与旧版共享状态没有已审查规则，不能按版本号猜测兼容。
#[test]
fn HistoryTransaction_FreshOnly_006() {
    assert!(admit_data_mode(ReviewedDataMode::FreshSettings).is_ok());
    assert!(admit_data_mode(ReviewedDataMode::KeepCurrentData).is_err());
}

// 每个effect先持久化intent；未知结果使下一次破坏动作保持封锁。
#[test]
fn HistoryTransaction_UnknownIsNotReplay_007() {
    let mut journal = SwitchJournal::new(binding(), capacity()).unwrap();
    journal
        .apply(JournalEvent::Intent { effect: effect() })
        .unwrap();
    assert!(journal.pending_effect().is_some());
    assert!(journal
        .apply(JournalEvent::Intent { effect: effect() })
        .is_err());
    journal
        .apply(JournalEvent::Observed {
            effect_id: effect().effect_id,
            intent_generation: 1,
            result: ObservedResult {
                observation: Observation::Unknown,
                receipt: None,
            },
        })
        .unwrap();
    assert!(journal.requires_reconciliation());
    assert!(journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::HistoricalActive
        })
        .is_err());
}

// observed结果须对应原始intent generation，不能串入另一次effect。
#[test]
fn HistoryTransaction_ForeignEffectGeneration_008() {
    let mut journal = SwitchJournal::new(binding(), capacity()).unwrap();
    journal
        .apply(JournalEvent::Intent { effect: effect() })
        .unwrap();
    assert!(journal
        .apply(JournalEvent::Observed {
            effect_id: effect().effect_id,
            intent_generation: 2,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some("8".repeat(64))
            },
        })
        .is_err());
    assert!(journal.pending_effect().is_some());
}

// 不允许宣称跨文件原子恢复，最终放行要由全部effect及两份manifest完成。
#[test]
fn HistoryTransaction_NoPrematureCompletion_009() {
    let mut journal = SwitchJournal::new(binding(), capacity()).unwrap();
    assert!(journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::Restored
        })
        .is_err());
    assert!(journal
        .apply(JournalEvent::Phase {
            phase: JournalPhase::HistoricalActive
        })
        .is_err());
}

// append-only frame保留前一代；失败后不得覆盖、截短或盲目追加。
#[test]
fn HistoryTransaction_TornTailRetained_010() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    disk.append(0, JournalEvent::Intent { effect: effect() })
        .unwrap();
    drop(disk);
    use std::io::Write;
    std::fs::File::options()
        .append(true)
        .open(root.path().join("journal.log"))
        .unwrap()
        .write_all(b"{torn")
        .unwrap();
    let mut disk = store(&root);
    let recovered = disk.inspect(&binding()).unwrap();
    assert!(recovered.blocked);
    assert_eq!(recovered.last_valid.unwrap().generation(), 1);
    assert!(disk
        .append(
            1,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
    assert!(std::fs::read(root.path().join("journal.log"))
        .unwrap()
        .ends_with(b"{torn"));
}

// 严格generation/transaction/前序摘要阻止旧请求或串线记录。
#[test]
fn HistoryTransaction_WrongBindingAndGeneration_011() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    assert!(disk
        .append(1, JournalEvent::Intent { effect: effect() })
        .is_err());
    let mut wrong = binding();
    wrong.target_context = "00000000-0000-4000-8000-000000000099".into();
    assert!(disk.inspect(&wrong).unwrap().blocked);
}

// 只读恢复可重复，不能把未观察effect变成成功，也不能重复执行。
#[test]
fn HistoryTransaction_IdempotentInspection_012() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    disk.append(0, JournalEvent::Intent { effect: effect() })
        .unwrap();
    for _ in 0..4 {
        let status = disk.inspect(&binding()).unwrap();
        assert!(!status.blocked);
        let journal = status.last_valid.unwrap();
        assert_eq!(journal.generation(), 1);
        assert!(journal.requires_reconciliation());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 4);
}

// 单写者锁身份位于轮转根之外；第二个writer不能并行写journal。
#[test]
fn HistoryTransaction_OneWriter_013() {
    let root = tempfile::tempdir().unwrap();
    let _disk = store(&root);
    let dir = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    assert!(JournalStore::fixture(dir).is_err());
}

// manifest按完整字节摘要create-new保留；损坏不能被自动覆盖。
#[test]
fn HistoryTransaction_ImmutableManifest_014() {
    let root = tempfile::tempdir().unwrap();
    let disk = store(&root);
    let digest = disk.retain_manifest(b"complete context manifest").unwrap();
    assert_eq!(
        disk.read_manifest(&digest).unwrap(),
        b"complete context manifest"
    );
    std::fs::write(
        root.path().join(format!("manifest-{digest}.json")),
        b"foreign",
    )
    .unwrap();
    assert!(disk.read_manifest(&digest).is_err());
    assert!(disk.retain_manifest(b"complete context manifest").is_err());
}

// 记录在intent/result任何边界截断后，恢复只报告证据，从不调用执行器。
#[test]
fn HistoryTransaction_EveryRecordBoundary_015() {
    for observed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        {
            let mut disk = store(&root);
            disk.initialize(binding(), capacity()).unwrap();
            disk.append(0, JournalEvent::Intent { effect: effect() })
                .unwrap();
            if observed {
                let receipt = disk
                    .retain_effect_receipt(
                        &effect().effect_id,
                        Observation::Applied,
                        &effect().expected_postconditions,
                    )
                    .unwrap();
                disk.append(
                    1,
                    JournalEvent::Observed {
                        effect_id: effect().effect_id,
                        intent_generation: 1,
                        result: ObservedResult {
                            observation: Observation::Applied,
                            receipt: Some(receipt),
                        },
                    },
                )
                .unwrap();
            }
        }
        let disk = store(&root);
        let recovered = disk.inspect(&binding()).unwrap().last_valid.unwrap();
        assert_eq!(recovered.requires_reconciliation(), !observed);
    }
}

// 测试边界返回完整枚举和实际字节，生产capture仍负责范围、限制和哈希验证。
#[derive(Clone)]
struct FixtureContext {
    roots: Vec<RootInventory>,
    bytes: BTreeMap<(RootKind, String), Vec<u8>>,
    fail_read: bool,
}
impl FixtureContext {
    fn full() -> Self {
        let mut desk = RootInventory::fixture(RootKind::Desk, "desk-object");
        let mut bytes = BTreeMap::new();
        for directory in ["", "disabled", "disabled/skills", "disabled/agents"] {
            desk.entries.push(node(directory, EntryType::Directory, 0));
        }
        for (path, content) in [
            ("providers.json", b"secret legacy config".as_slice()),
            ("disabled/skills/user.md", b"user-authored skill".as_slice()),
            ("disabled/agents/user.md", b"user-authored agent".as_slice()),
            ("unknown.bin", &[0, 255, 7][..]),
        ] {
            desk.entries
                .push(node(path, EntryType::File, content.len() as u64));
            bytes.insert((RootKind::Desk, path.into()), content.to_vec());
        }
        let webview = RootInventory::fixture(RootKind::WebView, "webview-parent/absent");
        Self {
            roots: vec![desk, webview],
            bytes,
            fail_read: false,
        }
    }
}
fn node(path: &str, kind: EntryType, size: u64) -> EntryMetadata {
    EntryMetadata {
        path: path.into(),
        kind,
        size,
        object_identity: format!("object:{path}"),
        link_count: 1,
        permissions: PermissionRecord::Unix { mode: 0o700 },
    }
}
impl ContextReader for FixtureContext {
    fn inventory(&mut self) -> io::Result<Vec<RootInventory>> {
        Ok(self.roots.clone())
    }
    fn open_file(&mut self, root: RootKind, entry: &EntryMetadata) -> io::Result<Box<dyn Read>> {
        if self.fail_read {
            return Err(io::Error::other("injected short copy / disk fault"));
        }
        Ok(Box::new(Cursor::new(
            self.bytes.get(&(root, entry.path.clone())).unwrap().clone(),
        )))
    }
}
// 完整保留providers、disabled用户内容、未知文件以及UDF不存在这一事实。
#[test]
fn HistoryTransaction_CompleteContexts_016() {
    let mut context = FixtureContext::full();
    let boundary = crate::version_history::maintenance::SnapshotBoundary::fixture(binding());
    let manifest = capture_context(
        &boundary,
        &binding().source_context,
        &mut context,
        SnapshotLimits::default(),
    )
    .unwrap();
    assert_eq!(manifest.roots[0].entries.len(), 8);
    assert!(manifest.roots[1].entries.is_empty());
    verify_context(
        &boundary,
        &manifest,
        &mut context,
        SnapshotLimits::default(),
    )
    .unwrap();
}

// 文件新增、删减、字节或权限变更均破坏精确快照，不能静默删除外部写入。
#[test]
fn HistoryTransaction_ContextChanges_017() {
    let boundary = crate::version_history::maintenance::SnapshotBoundary::fixture(binding());
    let original = FixtureContext::full();
    let manifest = capture_context(
        &boundary,
        &binding().source_context,
        &mut original.clone(),
        SnapshotLimits::default(),
    )
    .unwrap();
    for mutation in 0..4 {
        let mut changed = original.clone();
        match mutation {
            0 => {
                changed
                    .bytes
                    .get_mut(&(RootKind::Desk, "unknown.bin".into()))
                    .unwrap()[0] = 1;
            }
            1 => {
                changed.roots[0].entries.pop();
            }
            2 => {
                changed.roots[0].entries[0].permissions = PermissionRecord::Unix { mode: 0o777 };
            }
            _ => {
                changed.roots[1]
                    .entries
                    .push(node("", EntryType::Directory, 0));
            }
        }
        assert!(verify_context(
            &boundary,
            &manifest,
            &mut changed,
            SnapshotLimits::default()
        )
        .is_err());
    }
}

// reparse、符号链接、硬链接、特殊文件、路径逃逸和limits一律阻止admission。
#[test]
fn HistoryTransaction_UnsafeTree_018() {
    let boundary = crate::version_history::maintenance::SnapshotBoundary::fixture(binding());
    for mutation in 0..5 {
        let mut context = FixtureContext::full();
        match mutation {
            0 => context.roots[0].entries[4].kind = EntryType::LinkOrReparse,
            1 => context.roots[0].entries[4].link_count = 2,
            2 => context.roots[0].entries[4].kind = EntryType::Other,
            3 => context.roots[0].entries[4].path = "../outside".into(),
            _ => context.roots[0].entries[4].path = "a:stream".into(),
        }
        assert!(capture_context(
            &boundary,
            &binding().source_context,
            &mut context,
            SnapshotLimits::default()
        )
        .is_err());
    }
    let limits = SnapshotLimits {
        max_bytes: 1,
        ..SnapshotLimits::default()
    };
    assert!(capture_context(
        &boundary,
        &binding().source_context,
        &mut FixtureContext::full(),
        limits
    )
    .is_err());
}

// 两个实际根缺失/重叠或与shared CLI/project根重叠不能降级到部分备份。
#[test]
fn HistoryTransaction_RootBinding_019() {
    let boundary = crate::version_history::maintenance::SnapshotBoundary::fixture(binding());
    for mutation in 0..3 {
        let mut context = FixtureContext::full();
        match mutation {
            0 => {
                context.roots.pop();
            }
            1 => {
                context.roots[1].root = RootKind::Desk;
            }
            _ => {
                context.roots[0].overlaps_shared_data = true;
            }
        }
        assert!(capture_context(
            &boundary,
            &binding().source_context,
            &mut context,
            SnapshotLimits::default()
        )
        .is_err());
    }
}

// 中途IO失败或短读不发布manifest，重试只读检查也不能误认完整复制。
#[test]
fn HistoryTransaction_PartialCopy_020() {
    let boundary = crate::version_history::maintenance::SnapshotBoundary::fixture(binding());
    let mut broken = FixtureContext::full();
    broken.fail_read = true;
    assert!(capture_context(
        &boundary,
        &binding().source_context,
        &mut broken,
        SnapshotLimits::default()
    )
    .is_err());
    broken.fail_read = false;
    broken
        .bytes
        .get_mut(&(RootKind::Desk, "unknown.bin".into()))
        .unwrap()
        .pop();
    assert!(capture_context(
        &boundary,
        &binding().source_context,
        &mut broken,
        SnapshotLimits::default()
    )
    .is_err());
}

// 整个枚举在读取前后保持同一身份，否则不能承诺快照一致性。
#[test]
fn HistoryTransaction_ConcurrentMutation_021() {
    struct Moving(FixtureContext, usize);
    impl ContextReader for Moving {
        fn inventory(&mut self) -> io::Result<Vec<RootInventory>> {
            self.1 += 1;
            if self.1 > 1 {
                self.0.roots[0].entries[0].object_identity = "replaced".into();
            }
            self.0.inventory()
        }
        fn open_file(
            &mut self,
            root: RootKind,
            entry: &EntryMetadata,
        ) -> io::Result<Box<dyn Read>> {
            self.0.open_file(root, entry)
        }
    }
    let boundary = crate::version_history::maintenance::SnapshotBoundary::fixture(binding());
    assert!(capture_context(
        &boundary,
        &binding().source_context,
        &mut Moving(FixtureContext::full(), 0),
        SnapshotLimits::default()
    )
    .is_err());
}

// 保存manifest后替换身份、路径或摘要不能取得恢复权威。
#[test]
fn HistoryTransaction_ManifestBytes_022() {
    let boundary = crate::version_history::maintenance::SnapshotBoundary::fixture(binding());
    let manifest = capture_context(
        &boundary,
        &binding().source_context,
        &mut FixtureContext::full(),
        SnapshotLimits::default(),
    )
    .unwrap();
    let bytes = manifest.encode().unwrap();
    let digest = manifest.digest().unwrap();
    assert!(crate::version_history::snapshot::SnapshotManifest::decode(
        &bytes,
        &digest,
        &binding()
    )
    .is_ok());
    let mut foreign = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
    foreign["context_id"] = serde_json::Value::String(binding().target_context);
    assert!(crate::version_history::snapshot::SnapshotManifest::decode(
        &serde_json::to_vec(&foreign).unwrap(),
        &digest,
        &binding()
    )
    .is_err());
}

// effect前后不可变manifest缺失或损坏时，持久化层不能接纳它的intent。
#[test]
fn HistoryTransaction_EffectManifestRequired_023() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    std::fs::write(
        root.path()
            .join(format!("manifest-{}.json", effect().before)),
        b"foreign",
    )
    .unwrap();
    assert!(disk
        .append(0, JournalEvent::Intent { effect: effect() })
        .is_err());
    assert_eq!(
        disk.inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        0
    );
}

// 共享lease必须在读取marker前持有，普通启动期间仍持有，manager不能绕过第二实例。
#[test]
fn HistoryTransaction_StartupLeaseRace_024() {
    use crate::version_history::maintenance::{
        decide_startup, MarkerRead, SharedStartupLease, StartupDecision,
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("stable-admission.lock");
    let shared = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    shared.try_lock_shared().unwrap();
    let contender = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let lease = SharedStartupLease::fixture_with_guard(binding(), Box::new(shared));
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &lease,
            MarkerRead::Absent
        ),
        StartupDecision::Ordinary
    );
    assert!(contender.try_lock().is_err());
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &lease,
            MarkerRead::Unreadable
        ),
        StartupDecision::RecoveryOnly
    );
    drop(lease);
    contender.try_lock().unwrap();
}

// manager消失/锁释放不能覆盖durable marker，备份源程序也不能打开历史context。
#[test]
fn HistoryTransaction_PersistentMarker_025() {
    use crate::version_history::maintenance::{
        decide_startup, ActiveContextMarker, MarkerRead, SharedStartupLease, StartupDecision,
    };
    let marker = ActiveContextMarker::transition(binding(), 4, "9".repeat(64))
        .unwrap()
        .encode()
        .unwrap();
    let lease = SharedStartupLease::fixture(binding(), false);
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &lease,
            MarkerRead::Present {
                bytes: &marker,
                journal: None
            }
        ),
        StartupDecision::RecoveryOnly
    );
    let backup = SharedStartupLease::fixture(binding(), true);
    // 干净的marker缺失不要求历史切换scope；真实shared lease仍覆盖普通启动。
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &backup,
            MarkerRead::Absent
        ),
        StartupDecision::Ordinary
    );
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &lease,
            MarkerRead::Present {
                bytes: b"{corrupt",
                journal: None
            }
        ),
        StartupDecision::RecoveryOnly
    );
}

// 伪造completed字段或未验证journal不足以清除启动barrier。
#[test]
fn HistoryTransaction_NoUiStartupProof_026() {
    use crate::version_history::maintenance::{
        decide_startup, MarkerRead, SharedStartupLease, StartupDecision,
    };
    let lease = SharedStartupLease::fixture(binding(), false);
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &lease,
            MarkerRead::Present {
                bytes: br#"{"completed":true}"#,
                journal: None
            }
        ),
        StartupDecision::RecoveryOnly
    );
}

// source退出不是config完成证据：先冻结新写入，等已准入权威写操作返回完成。
#[test]
fn HistoryTransaction_MutationDrain_027() {
    let gate = AdmissionGate::new();
    let writer = gate.begin_mutation().unwrap();
    let mut freeze = gate.freeze(&binding().transaction_id).unwrap();
    assert!(gate.begin_mutation().is_err());
    assert!(freeze.mark_committed().is_err());
    writer.completed_authoritative_write();
    freeze.mark_committed().unwrap();
    assert!(freeze.release_review().is_err());
}

// 错误/panic/丢失writer票据不可用Drop假装已完成，避免保存半截JSON后快照。
#[test]
fn HistoryTransaction_UnknownMutation_028() {
    let gate = AdmissionGate::new();
    drop(gate.begin_mutation().unwrap());
    let mut freeze = gate.freeze(&binding().transaction_id).unwrap();
    assert!(freeze.mark_committed().is_err());
}

// 真实临时目录覆盖完整枚举与原始字节；这不是Windows ACL/共享handle证明。
#[cfg(unix)]
#[test]
fn HistoryTransaction_RealFilesystem_029() {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    struct DiskContext {
        desk: std::path::PathBuf,
        webview: std::path::PathBuf,
    }
    fn enumerate(
        root: &std::path::Path,
        current: &std::path::Path,
        entries: &mut Vec<EntryMetadata>,
    ) {
        let metadata = std::fs::symlink_metadata(current).unwrap();
        let kind = if metadata.file_type().is_symlink() {
            EntryType::LinkOrReparse
        } else if metadata.is_dir() {
            EntryType::Directory
        } else if metadata.is_file() {
            EntryType::File
        } else {
            EntryType::Other
        };
        entries.push(EntryMetadata {
            path: current.strip_prefix(root).unwrap().to_str().unwrap().into(),
            kind,
            size: if kind == EntryType::File {
                metadata.len()
            } else {
                0
            },
            object_identity: format!("{}:{}", metadata.dev(), metadata.ino()),
            link_count: metadata.nlink(),
            permissions: PermissionRecord::Unix {
                mode: metadata.mode(),
            },
        });
        if kind == EntryType::Directory {
            for entry in std::fs::read_dir(current).unwrap() {
                enumerate(root, &entry.unwrap().path(), entries);
            }
        }
    }
    impl ContextReader for DiskContext {
        fn inventory(&mut self) -> io::Result<Vec<RootInventory>> {
            let mut roots = vec![
                RootInventory::fixture(RootKind::Desk, "desk-object"),
                RootInventory::fixture(RootKind::WebView, "webview-parent/absent"),
            ];
            enumerate(&self.desk, &self.desk, &mut roots[0].entries);
            if self.webview.exists() {
                enumerate(&self.webview, &self.webview, &mut roots[1].entries);
            }
            Ok(roots)
        }
        fn open_file(
            &mut self,
            root: RootKind,
            entry: &EntryMetadata,
        ) -> io::Result<Box<dyn Read>> {
            let path = if root == RootKind::Desk {
                &self.desk
            } else {
                &self.webview
            }
            .join(&entry.path);
            let file = std::fs::File::options()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)?;
            let metadata = file.metadata()?;
            if format!("{}:{}", metadata.dev(), metadata.ino()) != entry.object_identity
                || metadata.nlink() != 1
            {
                return Err(io::Error::other("fixture identity changed"));
            }
            Ok(Box::new(file))
        }
    }
    let root = tempfile::tempdir().unwrap();
    let desk = root.path().join("desk");
    std::fs::create_dir_all(desk.join("disabled/skills")).unwrap();
    std::fs::write(desk.join("disabled/skills/custom.md"), b"real skill").unwrap();
    std::fs::write(desk.join("providers.json"), &[0, 255, 2]).unwrap();
    let mut source = DiskContext {
        desk: desk.clone(),
        webview: root.path().join("absent-udf"),
    };
    let boundary = crate::version_history::maintenance::SnapshotBoundary::fixture(binding());
    let manifest = capture_context(
        &boundary,
        &binding().source_context,
        &mut source,
        SnapshotLimits::default(),
    )
    .unwrap();
    verify_context(&boundary, &manifest, &mut source, SnapshotLimits::default()).unwrap();
    std::fs::hard_link(
        desk.join("providers.json"),
        root.path().join("external-link"),
    )
    .unwrap();
    assert!(capture_context(
        &boundary,
        &binding().source_context,
        &mut source,
        SnapshotLimits::default()
    )
    .is_err());
    std::fs::remove_file(root.path().join("external-link")).unwrap();
    std::os::unix::fs::symlink(root.path(), desk.join("escape")).unwrap();
    assert!(capture_context(
        &boundary,
        &binding().source_context,
        &mut source,
        SnapshotLimits::default()
    )
    .is_err());
}

fn record_event(disk: &mut JournalStore, generation: &mut u64, event: JournalEvent) {
    *generation = disk.append(*generation, event).unwrap();
}
fn record_effect(disk: &mut JournalStore, generation: &mut u64, kind: EffectKind) {
    let mut step = effect();
    step.effect_id = uuid::Uuid::new_v4().hyphenated().to_string();
    step.kind = kind;
    record_event(
        disk,
        generation,
        JournalEvent::Intent {
            effect: step.clone(),
        },
    );
    let intent_generation = *generation;
    let receipt = disk
        .retain_effect_receipt(
            &step.effect_id,
            Observation::Applied,
            &step.expected_postconditions,
        )
        .unwrap();
    record_event(
        disk,
        generation,
        JournalEvent::Observed {
            effect_id: step.effect_id,
            intent_generation,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt),
            },
        },
    );
}

fn record_exact_effect(disk: &mut JournalStore, generation: &mut u64, spec: EffectSpec) -> u64 {
    record_event(
        disk,
        generation,
        JournalEvent::Intent {
            effect: spec.clone(),
        },
    );
    let intent = *generation;
    let receipt = disk
        .retain_effect_receipt(
            &spec.effect_id,
            Observation::Applied,
            &spec.expected_postconditions,
        )
        .unwrap();
    record_event(
        disk,
        generation,
        JournalEvent::Observed {
            effect_id: spec.effect_id,
            intent_generation: intent,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt),
            },
        },
    );
    intent
}

fn prepare_compensation(
    disk: &mut JournalStore,
    kind: EffectKind,
    unknown: bool,
) -> (u64, EffectSpec) {
    use crate::version_history::journal::ManifestRole;
    let mut generation = 0;
    for role in [
        ManifestRole::SourceContext,
        ManifestRole::SourceBundle,
        ManifestRole::Registration,
        ManifestRole::Shortcuts,
    ] {
        record_event(
            disk,
            &mut generation,
            JournalEvent::Manifest {
                role,
                digest: effect().expected_postconditions,
            },
        );
    }
    record_effect(disk, &mut generation, EffectKind::VerifySourceBundleCopy);
    record_effect(disk, &mut generation, EffectKind::FenceSourceImage);
    for root in [RootKind::Desk, RootKind::WebView] {
        record_effect(
            disk,
            &mut generation,
            EffectKind::PreserveRoot {
                context: binding().source_context,
                root,
            },
        );
    }
    record_event(
        disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::SourceSealed,
        },
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        record_effect(disk, &mut generation, EffectKind::CreateFreshRoot { root });
    }
    record_event(
        disk,
        &mut generation,
        JournalEvent::Manifest {
            role: ManifestRole::FreshTargetContext,
            digest: effect().expected_postconditions,
        },
    );
    record_event(
        disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::FreshReady,
        },
    );
    record_event(
        disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::Installing,
        },
    );
    let mut step = effect();
    step.effect_id = uuid::Uuid::new_v4().to_string();
    step.kind = kind;
    record_event(
        disk,
        &mut generation,
        JournalEvent::Intent {
            effect: step.clone(),
        },
    );
    if unknown {
        let intent = generation;
        record_event(
            disk,
            &mut generation,
            JournalEvent::Observed {
                effect_id: step.effect_id.clone(),
                intent_generation: intent,
                result: ObservedResult {
                    observation: Observation::Unknown,
                    receipt: None,
                },
            },
        );
    }
    (generation, step)
}

// 检查未触及context的真实journal终态不需要伪造snapshot便能形成独立abort marker。
#[test]
fn HistoryTransaction_EarlyAbort_051() {
    use crate::version_history::journal::PreContextAbortProof;
    use crate::version_history::maintenance::{
        decide_startup, ActiveContextMarker, MarkerRead, SharedStartupLease, StartupDecision,
    };
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let inspection = disk.inspect(&binding()).unwrap();
    let evidence =
        PreContextAbortProof::fixture(&inspection, std::array::from_fn(|_| effect().before));
    disk.abort_pre_context(&evidence).unwrap();
    let inspection = disk.inspect(&binding()).unwrap();
    assert_eq!(
        inspection.last_valid.as_ref().unwrap().phase(),
        JournalPhase::PreContextAborted
    );
    assert!(ActiveContextMarker::restored(&inspection).is_err());
    let marker = ActiveContextMarker::pre_context_aborted(&inspection)
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &SharedStartupLease::fixture(binding(), false),
            MarkerRead::Present {
                bytes: &marker,
                journal: Some(&inspection)
            }
        ),
        StartupDecision::Ordinary
    );
    assert!(disk
        .append(
            1,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
}

// 检查每个原image fence必须由绑定原effect/代数/观测状态的逆操作正向撤销。
#[test]
fn HistoryTransaction_AbortFence_052() {
    use crate::version_history::journal::PreContextAbortProof;
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let mut generation = 0;
    let mut fence = effect();
    fence.kind = EffectKind::FenceSourceImage;
    let intent = record_exact_effect(&mut disk, &mut generation, fence.clone());
    let proof = PreContextAbortProof::fixture(
        &disk.inspect(&binding()).unwrap(),
        std::array::from_fn(|_| effect().before),
    );
    assert!(disk.abort_pre_context(&proof).is_err());
    let reverse = EffectSpec {
        effect_id: uuid::Uuid::new_v4().to_string(),
        kind: EffectKind::ReverseSourceFence {
            original_effect_id: fence.effect_id,
            original_intent_generation: intent,
        },
        before: fence.expected_postconditions,
        expected_postconditions: fence.before,
    };
    let mut wrong = reverse.clone();
    wrong.expected_postconditions = effect().expected_postconditions;
    assert!(disk
        .append(generation, JournalEvent::Intent { effect: wrong })
        .is_err());
    record_exact_effect(&mut disk, &mut generation, reverse);
    let proof = PreContextAbortProof::fixture(
        &disk.inspect(&binding()).unwrap(),
        std::array::from_fn(|_| effect().before),
    );
    disk.abort_pre_context(&proof).unwrap();
}

// 检查Reviewed中的context/installer/generic写入intent即使NotApplied也不能使用早期abort。
#[test]
fn HistoryTransaction_AbortReject_053() {
    use crate::version_history::journal::{
        FilesystemOperation, PreContextAbortProof, RegistrationOperation, RegistrationSlot,
    };
    let kinds = [
        EffectKind::CreateFreshRoot {
            root: RootKind::Desk,
        },
        EffectKind::InstallerCreateSuspended,
        EffectKind::InstallerResume,
        EffectKind::HistoricalResume,
        EffectKind::FilesystemEntry {
            operation: FilesystemOperation::CopyFile,
            manifest: effect().before,
            entry_index: 0,
        },
        EffectKind::RegistrationEntry {
            slot: RegistrationSlot::Uninstall,
            operation: RegistrationOperation::SetValue,
            manifest: effect().before,
            entry_index: 0,
        },
    ];
    for kind in kinds {
        for observation in [
            None,
            Some(Observation::Unknown),
            Some(Observation::NotApplied),
            Some(Observation::Applied),
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut disk = store(&root);
            disk.initialize(binding(), capacity()).unwrap();
            let mut step = effect();
            step.kind = kind.clone();
            disk.append(
                0,
                JournalEvent::Intent {
                    effect: step.clone(),
                },
            )
            .unwrap();
            if let Some(observation) = observation {
                let receipt = if observation == Observation::Unknown {
                    None
                } else {
                    Some(
                        disk.retain_effect_receipt(
                            &step.effect_id,
                            observation,
                            if observation == Observation::NotApplied {
                                &step.before
                            } else {
                                &step.expected_postconditions
                            },
                        )
                        .unwrap(),
                    )
                };
                disk.append(
                    1,
                    JournalEvent::Observed {
                        effect_id: step.effect_id,
                        intent_generation: 1,
                        result: ObservedResult {
                            observation,
                            receipt,
                        },
                    },
                )
                .unwrap();
            }
            let proof = PreContextAbortProof::fixture(
                &disk.inspect(&binding()).unwrap(),
                std::array::from_fn(|_| effect().before),
            );
            assert!(disk.abort_pre_context(&proof).is_err());
        }
    }
}

// 检查丢失resume/terminal回执有无Unknown frame都保留Unknown，并禁止重放或改写历史结果。
#[test]
fn HistoryTransaction_Compensate_054() {
    use crate::version_history::journal::UnknownCompensationProof;
    for unknown in [false, true] {
        for kind in [
            EffectKind::InstallerResume,
            EffectKind::InstallerTerminalOutcome,
            EffectKind::HistoricalResume,
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut disk = store(&root);
            disk.initialize(binding(), capacity()).unwrap();
            let (_, step) = prepare_compensation(&mut disk, kind, unknown);
            let proof = UnknownCompensationProof::fixture(
                &disk.inspect(&binding()).unwrap(),
                std::array::from_fn(|_| effect().before),
            );
            let generation = disk.compensate_unknown(&proof).unwrap();
            let inspection = disk.inspect(&binding()).unwrap();
            let journal = inspection.last_valid.as_ref().unwrap();
            assert_eq!(
                journal.effect_observation(&step.effect_id),
                Some(Observation::Unknown)
            );
            assert!(journal.has_historical_uncertainty());
            assert!(!journal.requires_reconciliation());
            let mut replay = step.clone();
            replay.effect_id = uuid::Uuid::new_v4().to_string();
            assert!(disk
                .append(generation, JournalEvent::Intent { effect: replay })
                .is_err());
            assert!(disk
                .retain_effect_receipt(
                    &step.effect_id,
                    Observation::Applied,
                    &step.expected_postconditions
                )
                .is_err());
            drop(disk);
            let mut disk = store(&root);
            disk.bind_existing(&binding()).unwrap();
            assert_eq!(
                disk.inspect(&binding())
                    .unwrap()
                    .last_valid
                    .unwrap()
                    .effect_observation(&step.effect_id),
                Some(Observation::Unknown)
            );
        }
    }
}

// 检查compensation后的普通和generic写入在append及原始frame重放边界都被拒绝。
#[test]
fn HistoryTransaction_ReturnOnly_055() {
    use crate::version_history::journal::{
        FilesystemOperation, RegistrationOperation, RegistrationSlot, UnknownCompensationProof,
    };
    for kind in [
        EffectKind::InstallerCreateSuspended,
        EffectKind::HistoricalResume,
        EffectKind::CreateFreshRoot {
            root: RootKind::Desk,
        },
        EffectKind::FilesystemEntry {
            operation: FilesystemOperation::Rename,
            manifest: effect().before,
            entry_index: 0,
        },
        EffectKind::RegistrationEntry {
            slot: RegistrationSlot::Uninstall,
            operation: RegistrationOperation::SetValue,
            manifest: effect().before,
            entry_index: 0,
        },
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut disk = store(&root);
        disk.initialize(binding(), capacity()).unwrap();
        prepare_compensation(&mut disk, EffectKind::InstallerResume, false);
        let proof = UnknownCompensationProof::fixture(
            &disk.inspect(&binding()).unwrap(),
            std::array::from_fn(|_| effect().before),
        );
        let generation = disk.compensate_unknown(&proof).unwrap();
        let mut step = effect();
        step.effect_id = uuid::Uuid::new_v4().to_string();
        step.kind = kind;
        let event = JournalEvent::Intent { effect: step };
        assert!(disk.append(generation, event.clone()).is_err());
        let mut replay = disk.inspect(&binding()).unwrap().last_valid.unwrap();
        assert!(replay.apply(event.clone()).is_err());
        drop(disk);
        let path = root.path().join("journal.log");
        let bytes = std::fs::read(&path).unwrap();
        let last: serde_json::Value = serde_json::from_slice(
            bytes
                .split(|b| *b == b'\n')
                .rfind(|s| !s.is_empty())
                .unwrap(),
        )
        .unwrap();
        let record = serde_json::json!({"schema":2,"binding":binding(),"generation":generation+1,"previous":last["digest"],"lane":"Recovery","event":event});
        let canonical = format!("{{\"schema\":2,\"binding\":{},\"generation\":{},\"previous\":{},\"lane\":\"Recovery\",\"event\":{}}}",
            serde_json::to_string(&binding()).unwrap(), generation + 1,
            serde_json::to_string(&last["digest"]).unwrap(), serde_json::to_string(&event).unwrap());
        let digest = crate::version_history::verified_package::sha256(canonical.as_bytes());
        let mut forged =
            serde_json::to_vec(&serde_json::json!({"record":record,"digest":digest})).unwrap();
        forged.push(b'\n');
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(&forged)
            .unwrap();
        let mut disk = store(&root);
        assert!(disk.inspect(&binding()).unwrap().blocked);
        assert!(disk.bind_existing(&binding()).is_err());
    }
}

// 检查未知installer之后保留later context再恢复，终态仍保留原Unknown且不重放动作。
#[test]
fn HistoryTransaction_CompensatedReturn_056() {
    use crate::version_history::journal::{
        FilesystemOperation, ManifestRole, RegistrationOperation, RegistrationSlot, ShortcutSlot,
        UnknownCompensationProof,
    };
    use crate::version_history::maintenance::ActiveContextMarker;
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let (_, original) =
        prepare_compensation(&mut disk, EffectKind::InstallerTerminalOutcome, false);
    let proof = UnknownCompensationProof::fixture(
        &disk.inspect(&binding()).unwrap(),
        std::array::from_fn(|_| effect().before),
    );
    let mut generation = disk.compensate_unknown(&proof).unwrap();
    let mut wrong = effect();
    wrong.effect_id = uuid::Uuid::new_v4().to_string();
    wrong.kind = EffectKind::RecoveryFilesystemEntry {
        context: binding().source_context,
        operation: FilesystemOperation::Rename,
        manifest: effect().expected_postconditions,
        entry_index: 0,
    };
    assert!(disk
        .append(generation, JournalEvent::Intent { effect: wrong })
        .is_err());
    record_effect(&mut disk, &mut generation, EffectKind::FenceHistoricalImage);
    record_effect(
        &mut disk,
        &mut generation,
        EffectKind::RecoveryFilesystemEntry {
            context: binding().target_context,
            operation: FilesystemOperation::CopyFile,
            manifest: effect().before,
            entry_index: 0,
        },
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::PreserveRoot {
                context: binding().target_context,
                root,
            },
        );
    }
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Manifest {
            role: ManifestRole::RetainedTargetContext,
            digest: effect().expected_postconditions,
        },
    );
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::Restoring,
        },
    );
    record_effect(
        &mut disk,
        &mut generation,
        EffectKind::RecoveryFilesystemEntry {
            context: binding().source_context,
            operation: FilesystemOperation::CopyFile,
            manifest: effect().expected_postconditions,
            entry_index: 0,
        },
    );
    record_effect(
        &mut disk,
        &mut generation,
        EffectKind::RecoveryRegistrationEntry {
            slot: RegistrationSlot::Uninstall,
            operation: RegistrationOperation::SetValue,
            manifest: effect().expected_postconditions,
            entry_index: 0,
        },
    );
    record_effect(
        &mut disk,
        &mut generation,
        EffectKind::VerifySourceBundleRestore,
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::RestoreSourceRoot { root },
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
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::VerifyRegistrationRestore { slot },
        );
    }
    for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::RestoreShortcut { slot },
        );
    }
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::Restored,
        },
    );
    drop(disk);
    let mut disk = store(&root);
    disk.bind_existing(&binding()).unwrap();
    let inspection = disk.inspect(&binding()).unwrap();
    let journal = inspection.last_valid.as_ref().unwrap();
    assert_eq!(journal.phase(), JournalPhase::Restored);
    assert!(journal.has_historical_uncertainty());
    assert_eq!(
        journal.effect_observation(&original.effect_id),
        Some(Observation::Unknown)
    );
    ActiveContextMarker::restored(&inspection).unwrap();
    assert!(ActiveContextMarker::pre_context_aborted(&inspection).is_err());
}

// 检查特权事件不能借普通append提交，证据必须匹配原journal对象、最新代数和真实artifact。
#[test]
fn HistoryTransaction_AdmissionBinding_057() {
    use crate::version_history::journal::PreContextAbortProof;
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let proof = PreContextAbortProof::fixture(
        &disk.inspect(&binding()).unwrap(),
        std::array::from_fn(|_| effect().before),
    );
    assert!(disk
        .append(
            0,
            JournalEvent::AbortPreContext {
                receipt: effect().before
            }
        )
        .is_err());
    let other_root = tempfile::tempdir().unwrap();
    let mut other = store(&other_root);
    other.initialize(binding(), capacity()).unwrap();
    assert!(other.abort_pre_context(&proof).is_err());
    disk.append(
        0,
        JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        },
    )
    .unwrap();
    assert!(disk.abort_pre_context(&proof).is_err());
    let missing = PreContextAbortProof::fixture(
        &disk.inspect(&binding()).unwrap(),
        std::array::from_fn(|_| "0".repeat(64)),
    );
    assert!(disk.abort_pre_context(&missing).is_err());
    assert_eq!(
        disk.inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        1
    );
}

// 检查两类特权恢复事件直接使用保留lane，正向记录恰满也无需插入普通phase。
#[test]
fn HistoryTransaction_AdmissionLane_058() {
    use crate::version_history::journal::{PreContextAbortProof, UnknownCompensationProof};
    for compensation in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut disk = store(&root);
        let plan = if compensation {
            CapacityPlan::for_effects(6, 20, 3, 4096)
        } else {
            CapacityPlan::for_effects(1, 1, 1, 4096)
        }
        .unwrap();
        disk.initialize(binding(), plan).unwrap();
        if compensation {
            let (generation, _) =
                prepare_compensation(&mut disk, EffectKind::InstallerResume, false);
            assert_eq!(generation, 21);
            let proof = UnknownCompensationProof::fixture(
                &disk.inspect(&binding()).unwrap(),
                std::array::from_fn(|_| effect().before),
            );
            disk.compensate_unknown(&proof).unwrap();
        } else {
            let mut generation = 0;
            record_effect(
                &mut disk,
                &mut generation,
                EffectKind::VerifySourceBundleCopy,
            );
            record_effect(
                &mut disk,
                &mut generation,
                EffectKind::VerifySourceBundleCopy,
            );
            let proof = PreContextAbortProof::fixture(
                &disk.inspect(&binding()).unwrap(),
                std::array::from_fn(|_| effect().before),
            );
            disk.abort_pre_context(&proof).unwrap();
        }
        let bytes = std::fs::read(root.path().join("journal.log")).unwrap();
        let last: serde_json::Value = serde_json::from_slice(
            bytes
                .split(|b| *b == b'\n')
                .rfind(|s| !s.is_empty())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(last["record"]["lane"], "Recovery");
        assert!(!disk.inspect(&binding()).unwrap().blocked);
    }
}

// 检查context effect的未知结果或缺少完整source留存不能借compensation跳过安全前置条件。
#[test]
fn HistoryTransaction_CompensationReject_059() {
    use crate::version_history::journal::{FilesystemOperation, UnknownCompensationProof};
    for complete_source in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut disk = store(&root);
        disk.initialize(binding(), capacity()).unwrap();
        if complete_source {
            prepare_compensation(
                &mut disk,
                EffectKind::FilesystemEntry {
                    operation: FilesystemOperation::Rename,
                    manifest: effect().before,
                    entry_index: 0,
                },
                true,
            );
        } else {
            let mut step = effect();
            step.kind = EffectKind::InstallerResume;
            disk.append(0, JournalEvent::Intent { effect: step })
                .unwrap();
        }
        let inspection = disk.inspect(&binding()).unwrap();
        let generation = inspection.last_valid.as_ref().unwrap().generation();
        let proof = UnknownCompensationProof::fixture(
            &inspection,
            std::array::from_fn(|_| effect().before),
        );
        assert!(disk.compensate_unknown(&proof).is_err());
        let current = disk.inspect(&binding()).unwrap();
        assert_eq!(
            current.last_valid.as_ref().unwrap().generation(),
            generation
        );
        assert!(current.last_valid.unwrap().requires_reconciliation());
    }
}

// 只有完整source/target留存、每个registry/shortcut恢复及精确终代marker才放行。
#[test]
fn HistoryTransaction_CompleteReturn_030() {
    use crate::version_history::journal::{ManifestRole, RegistrationSlot, ShortcutSlot};
    use crate::version_history::maintenance::{
        decide_startup, ActiveContextMarker, MarkerRead, SharedStartupLease, StartupDecision,
    };
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let mut generation = 0;
    for role in [
        ManifestRole::SourceContext,
        ManifestRole::SourceBundle,
        ManifestRole::Registration,
        ManifestRole::Shortcuts,
    ] {
        record_event(
            &mut disk,
            &mut generation,
            JournalEvent::Manifest {
                role,
                digest: effect().expected_postconditions,
            },
        );
    }
    record_effect(
        &mut disk,
        &mut generation,
        EffectKind::VerifySourceBundleCopy,
    );
    record_effect(&mut disk, &mut generation, EffectKind::FenceSourceImage);
    for root in [RootKind::Desk, RootKind::WebView] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::PreserveRoot {
                context: binding().source_context,
                root,
            },
        );
    }
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::SourceSealed,
        },
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::CreateFreshRoot { root },
        );
    }
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Manifest {
            role: ManifestRole::FreshTargetContext,
            digest: effect().expected_postconditions,
        },
    );
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::FreshReady,
        },
    );
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::Installing,
        },
    );
    for kind in [
        EffectKind::InstallerCreateSuspended,
        EffectKind::InstallerResume,
        EffectKind::InstallerTerminalOutcome,
        EffectKind::VerifyTargetBundle,
    ] {
        record_effect(&mut disk, &mut generation, kind);
    }
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::InstalledUnconfirmed,
        },
    );
    record_effect(&mut disk, &mut generation, EffectKind::ConfirmFirstLaunch);
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::HistoricalActive,
        },
    );
    record_effect(&mut disk, &mut generation, EffectKind::FenceHistoricalImage);
    for root in [RootKind::Desk, RootKind::WebView] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::PreserveRoot {
                context: binding().target_context,
                root,
            },
        );
    }
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Manifest {
            role: ManifestRole::RetainedTargetContext,
            digest: effect().expected_postconditions,
        },
    );
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::Restoring,
        },
    );
    record_effect(
        &mut disk,
        &mut generation,
        EffectKind::VerifySourceBundleRestore,
    );
    for root in [RootKind::Desk, RootKind::WebView] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::RestoreSourceRoot { root },
        );
    }
    for slot in [
        RegistrationSlot::Uninstall,
        RegistrationSlot::Publisher,
        RegistrationSlot::DeskDirectory,
        RegistrationSlot::DeskDirectoryBackground,
        RegistrationSlot::LegacyDirectory,
        RegistrationSlot::OwnedRun,
    ] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::VerifyRegistrationRestore { slot },
        );
    }
    for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::RestoreShortcut { slot },
        );
    }
    assert!(disk
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restored
            }
        )
        .is_err());
    record_effect(
        &mut disk,
        &mut generation,
        EffectKind::VerifyRegistrationRestore {
            slot: RegistrationSlot::LegacyDirectoryBackground,
        },
    );
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::Restored,
        },
    );
    let read = disk.inspect(&binding()).unwrap();
    let marker = ActiveContextMarker::restored(&read)
        .unwrap()
        .encode()
        .unwrap();
    let lease = SharedStartupLease::fixture(binding(), false);
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &lease,
            MarkerRead::Present {
                bytes: &marker,
                journal: Some(&read)
            }
        ),
        StartupDecision::Ordinary
    );
    assert_eq!(
        decide_startup(
            &StartupControlLease::fixture(binding()),
            &lease,
            MarkerRead::Present {
                bytes: &marker,
                journal: None
            }
        ),
        StartupDecision::RecoveryOnly
    );
}

// 文件写完但目录flush失败也是unknown，不可在同一writer内重新使用看似完整的字节。
#[test]
fn HistoryTransaction_DurabilityFailure_031() {
    struct FailingFlush;
    impl crate::version_history::maintenance::DirectoryDurability for FailingFlush {
        fn sync_directory(&self, _directory: &Dir) -> io::Result<()> {
            Err(io::Error::other("injected disk-full / flush failure"))
        }
    }
    let root = tempfile::tempdir().unwrap();
    let directory = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let mut disk =
        JournalStore::fixture_with_durability(directory, Box::new(FailingFlush)).unwrap();
    assert!(disk
        .retain_manifest(b"complete bytes, uncertain persistence")
        .is_err());
    assert!(disk
        .retain_manifest(b"complete bytes, uncertain persistence")
        .is_err());
    assert!(disk.initialize(binding(), capacity()).is_err());
    assert!(disk.inspect(&binding()).unwrap().blocked);
}

// 每个文件/registry值的effect只能引用保留的manifest entry，不接受任意路径。
#[test]
fn HistoryTransaction_IndividualEffectBinding_032() {
    use crate::version_history::journal::FilesystemOperation;
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let mut entry = effect();
    entry.kind = EffectKind::FilesystemEntry {
        operation: FilesystemOperation::CopyFile,
        manifest: "f".repeat(64),
        entry_index: 3,
    };
    assert!(disk
        .append(0, JournalEvent::Intent { effect: entry })
        .is_err());
    assert_eq!(
        disk.inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        0
    );
}

// 独立短control锁序列化marker发布与普通startup，不升级仍由source持有的shared锁。
#[test]
fn HistoryTransaction_ControlLeaseOrdering_033() {
    use crate::version_history::maintenance::{
        decide_startup, ActiveContextMarker, MarkerRead, SharedStartupLease, StartupDecision,
    };
    let root = tempfile::tempdir().unwrap();
    let control_path = root.path().join("control.lock");
    let lifetime_path = root.path().join("lifetime.lock");
    let control = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&control_path)
        .unwrap();
    control.try_lock().unwrap();
    let shared = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&lifetime_path)
        .unwrap();
    shared.try_lock_shared().unwrap();
    let source = SharedStartupLease::fixture_with_guard(binding(), Box::new(shared));
    let source_control = StartupControlLease::fixture_with_guard(binding(), Box::new(control));
    let publisher_control = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&control_path)
        .unwrap();
    assert!(publisher_control.try_lock().is_err());
    assert_eq!(
        decide_startup(&source_control, &source, MarkerRead::Absent),
        StartupDecision::Ordinary
    );
    drop(source_control);
    publisher_control.try_lock().unwrap();
    let marker = ActiveContextMarker::transition(binding(), 0, "9".repeat(64))
        .unwrap()
        .encode()
        .unwrap();
    let publisher = StartupControlLease::fixture_with_guard(binding(), Box::new(publisher_control));
    let manager = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&lifetime_path)
        .unwrap();
    assert!(manager.try_lock().is_err());
    drop(publisher);
    let new_control_file = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&control_path)
        .unwrap();
    new_control_file.try_lock().unwrap();
    let new_shared = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&lifetime_path)
        .unwrap();
    new_shared.try_lock_shared().unwrap();
    let late = SharedStartupLease::fixture_with_guard(binding(), Box::new(new_shared));
    let new_control =
        StartupControlLease::fixture_with_guard(binding(), Box::new(new_control_file));
    assert_eq!(
        decide_startup(
            &new_control,
            &late,
            MarkerRead::Present {
                bytes: &marker,
                journal: None
            }
        ),
        StartupDecision::RecoveryOnly
    );
    drop(late);
    drop(new_control);
    assert!(manager.try_lock().is_err());
    drop(source);
    manager.try_lock().unwrap();
}

// 未引用的manifest也不能通过链接把外部文件混入私有恢复目录。
#[cfg(unix)]
#[test]
fn HistoryTransaction_LinkedJournalEntry_034() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    std::os::unix::fs::symlink(
        root.path().join("journal.lock"),
        root.path()
            .join(format!("manifest-{}.json", "f".repeat(64))),
    )
    .unwrap();
    assert!(disk.inspect(&binding()).unwrap().blocked);
}

// intent只含可预知postconditions，创建后新增PID/creation-time/job/file identity进入独立receipt。
#[test]
fn HistoryTransaction_DynamicReceipt_035() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let mut launch = effect();
    launch.kind = EffectKind::InstallerCreateSuspended;
    disk.append(
        0,
        JournalEvent::Intent {
            effect: launch.clone(),
        },
    )
    .unwrap();
    let actual = disk.retain_manifest(br#"{"pid":8321,"creationTime":173491823,"job":"owned-job-1","fileIdentity":"new-object-92"}"#).unwrap();
    assert_ne!(actual, launch.expected_postconditions);
    let receipt = disk
        .retain_effect_receipt(&launch.effect_id, Observation::Applied, &actual)
        .unwrap();
    disk.append(
        1,
        JournalEvent::Observed {
            effect_id: launch.effect_id,
            intent_generation: 1,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt),
            },
        },
    )
    .unwrap();
    assert!(!disk
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .requires_reconciliation());
}

// 正确哈希但错误effect或intent generation的动态receipt也不能串线。
#[test]
fn HistoryTransaction_ReceiptBinding_036() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    disk.append(0, JournalEvent::Intent { effect: effect() })
        .unwrap();
    let receipt = disk
        .retain_effect_receipt(
            &effect().effect_id,
            Observation::Applied,
            &effect().expected_postconditions,
        )
        .unwrap();
    let mut foreign: serde_json::Value =
        serde_json::from_slice(&disk.read_manifest(&receipt).unwrap()).unwrap();
    foreign["intent_generation"] = 9.into();
    let foreign_hash = disk
        .retain_manifest(&serde_json::to_vec(&foreign).unwrap())
        .unwrap();
    assert!(disk
        .append(
            1,
            JournalEvent::Observed {
                effect_id: effect().effect_id,
                intent_generation: 1,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(foreign_hash)
                }
            }
        )
        .is_err());
    assert_eq!(
        disk.inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        1
    );
}

// 100k快照entries乘以各copy/ACL/return effect会超过journal容量，必须在首个intent前拒绝。
#[test]
fn HistoryTransaction_CapacityAdmission_037() {
    assert!(CapacityPlan::for_effects(100_000, 100_000, 64, 4096).is_err());
    // Record count alone fits, but the reserved byte total exceeds 64 MiB.
    assert!(CapacityPlan::for_effects(30_000, 1, 1, 2048).is_err());
    let root = tempfile::tempdir().unwrap();
    let directory = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let mut disk = JournalStore::fixture_with_limits(directory, 20, 32 * 1024).unwrap();
    assert!(disk.initialize(binding(), capacity()).is_err());
    assert_eq!(
        std::fs::metadata(root.path().join("journal.log"))
            .unwrap()
            .len(),
        0
    );
}

// 正向工作不能用掉保留的recovery预算，拒绝追加前文件长度与generation都不变。
#[test]
fn HistoryTransaction_RecoveryCapacityReserved_038() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    let small = CapacityPlan::for_effects(1, 1, 1, 4096).unwrap();
    disk.initialize(binding(), small).unwrap();
    let mut generation = 0;
    record_effect(&mut disk, &mut generation, EffectKind::FenceSourceImage);
    record_effect(
        &mut disk,
        &mut generation,
        EffectKind::VerifySourceBundleCopy,
    );
    let before = std::fs::metadata(root.path().join("journal.log"))
        .unwrap()
        .len();
    let mut extra = effect();
    extra.effect_id = uuid::Uuid::new_v4().to_string();
    assert!(disk
        .append(generation, JournalEvent::Intent { effect: extra })
        .is_err());
    assert_eq!(
        std::fs::metadata(root.path().join("journal.log"))
            .unwrap()
            .len(),
        before
    );
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        },
    );
    record_effect(&mut disk, &mut generation, EffectKind::FenceHistoricalImage);
    record_event(
        &mut disk,
        &mut generation,
        JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        },
    );
    let full = std::fs::metadata(root.path().join("journal.log"))
        .unwrap()
        .len();
    assert!(disk
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
    assert_eq!(
        std::fs::metadata(root.path().join("journal.log"))
            .unwrap()
            .len(),
        full
    );
    assert_eq!(
        disk.inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        generation
    );
}

// 缓存writer每次增量验证/提交；大量effect不触发逐append全量replay或clone状态。
#[test]
fn HistoryTransaction_IncrementalWriter_039() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let replayed = disk.fixture_replay_count();
    let mut generation = 0;
    for _ in 0..128 {
        record_effect(
            &mut disk,
            &mut generation,
            EffectKind::VerifySourceBundleCopy,
        );
    }
    assert_eq!(disk.fixture_replay_count(), replayed);
    let read = disk.inspect(&binding()).unwrap();
    assert_eq!(read.last_valid.unwrap().generation(), 256);
    assert_eq!(disk.fixture_replay_count() - replayed, 257);
}

// 在关闭后改变已提交旧frame，重新打开不能让缓存或新generation掩盖损坏。
#[test]
fn HistoryTransaction_CorruptPrefix_040() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut disk = store(&root);
        disk.initialize(binding(), capacity()).unwrap();
        let mut generation = 0;
        record_effect(&mut disk, &mut generation, EffectKind::FenceSourceImage);
    }
    let path = root.path().join("journal.log");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[10] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let mut disk = store(&root);
    assert!(disk.bind_existing(&binding()).is_err());
    assert!(disk.inspect(&binding()).unwrap().blocked);
    assert!(disk
        .append(
            2,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
}

// 非Windows仅测试路径每次校验完整字节；同长中部损坏、恢复mtime也不能骗过缓存。
#[cfg(unix)]
#[test]
fn HistoryTransaction_ForeignMiddleMutation_041() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let mut generation = 0;
    record_effect(&mut disk, &mut generation, EffectKind::FenceSourceImage);
    let path = root.path().join("journal.log");
    let previous = std::fs::metadata(&path).unwrap().modified().unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let offset = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 20;
    bytes[offset] ^= 1;
    std::fs::write(&path, &bytes).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(previous))
        .unwrap();
    assert!(disk
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(disk.inspect(&binding()).unwrap().blocked);
}

// Windows增量信任来自实际持有的拒绝write/delete共享handle，而不是mtime/prefix猜测。
#[cfg(windows)]
#[test]
fn HistoryTransaction_WindowsHeldJournal_042() {
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let path = root.path().join("journal.log");
    assert!(std::fs::File::options().write(true).open(&path).is_err());
    assert!(std::fs::rename(&path, root.path().join("moved.log")).is_err());
    disk.append(0, JournalEvent::Intent { effect: effect() })
        .unwrap();
    assert_eq!(
        disk.inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        1
    );
}

// receipt已落盘但Observed frame尚未提交时，重启仍保持unknown且不自动采用或重跑。
#[test]
fn HistoryTransaction_ReceiptCrashBoundary_043() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut disk = store(&root);
        disk.initialize(binding(), capacity()).unwrap();
        disk.append(0, JournalEvent::Intent { effect: effect() })
            .unwrap();
        disk.retain_effect_receipt(
            &effect().effect_id,
            Observation::Applied,
            &effect().expected_postconditions,
        )
        .unwrap();
    }
    let mut disk = store(&root);
    disk.bind_existing(&binding()).unwrap();
    let read = disk.inspect(&binding()).unwrap();
    assert_eq!(read.last_valid.as_ref().unwrap().generation(), 1);
    assert!(read.last_valid.unwrap().requires_reconciliation());
    assert!(disk
        .append(1, JournalEvent::Intent { effect: effect() })
        .is_err());
}

// 精确回归：旧SourceContext manifest损坏后，Phase或无关Intent都不能被缓存writer确认。
#[cfg(unix)]
#[test]
fn HistoryTransaction_CachedManifestCorruption_044() {
    use crate::version_history::journal::ManifestRole;
    for unrelated_intent in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut disk = store(&root);
        disk.initialize(binding(), capacity()).unwrap();
        let manifest = disk
            .retain_manifest(b"original recovery prerequisite")
            .unwrap();
        disk.append(
            0,
            JournalEvent::Manifest {
                role: ManifestRole::SourceContext,
                digest: manifest.clone(),
            },
        )
        .unwrap();
        std::fs::write(
            root.path().join(format!("manifest-{manifest}.json")),
            b"tampered recovery prerequisite",
        )
        .unwrap();
        let before = std::fs::read(root.path().join("journal.log")).unwrap();
        let next = if unrelated_intent {
            JournalEvent::Intent { effect: effect() }
        } else {
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            }
        };
        assert!(disk.append(1, next).is_err());
        assert_eq!(
            std::fs::read(root.path().join("journal.log")).unwrap(),
            before
        );
        assert!(disk.inspect(&binding()).unwrap().blocked);
    }
}

// receipt wrapper与其observed manifest都是传递依赖，任一损坏必须阻止后续无关frame。
#[cfg(unix)]
#[test]
fn HistoryTransaction_CachedReceiptCorruption_045() {
    for corrupt_receipt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut disk = store(&root);
        disk.initialize(binding(), capacity()).unwrap();
        disk.append(0, JournalEvent::Intent { effect: effect() })
            .unwrap();
        let observed = disk
            .retain_manifest(b"new observed object identity")
            .unwrap();
        let receipt = disk
            .retain_effect_receipt(&effect().effect_id, Observation::Applied, &observed)
            .unwrap();
        disk.append(
            1,
            JournalEvent::Observed {
                effect_id: effect().effect_id,
                intent_generation: 1,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt.clone()),
                },
            },
        )
        .unwrap();
        let changed = if corrupt_receipt { receipt } else { observed };
        std::fs::write(
            root.path().join(format!("manifest-{changed}.json")),
            b"foreign",
        )
        .unwrap();
        let before = std::fs::read(root.path().join("journal.log")).unwrap();
        assert!(disk
            .append(
                2,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired
                }
            )
            .is_err());
        assert_eq!(
            std::fs::read(root.path().join("journal.log")).unwrap(),
            before
        );
    }
}

// 同一依赖去重；保护handle预算耗尽在写frame前失败，不借关闭旧依赖腾出位置。
#[test]
fn HistoryTransaction_DependencyHandleBudget_046() {
    use crate::version_history::journal::ManifestRole;
    let root = tempfile::tempdir().unwrap();
    let dir = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let mut disk = JournalStore::fixture_with_dependency_limit(dir, 2).unwrap();
    disk.initialize(binding(), capacity()).unwrap();
    let first = disk.retain_manifest(b"first").unwrap();
    let second = disk.retain_manifest(b"second").unwrap();
    let third = disk.retain_manifest(b"third").unwrap();
    disk.append(
        0,
        JournalEvent::Manifest {
            role: ManifestRole::SourceContext,
            digest: first.clone(),
        },
    )
    .unwrap();
    disk.append(
        1,
        JournalEvent::Manifest {
            role: ManifestRole::SourceBundle,
            digest: first,
        },
    )
    .unwrap();
    assert_eq!(disk.fixture_dependency_count(), 1);
    disk.append(
        2,
        JournalEvent::Manifest {
            role: ManifestRole::Registration,
            digest: second,
        },
    )
    .unwrap();
    let before = std::fs::read(root.path().join("journal.log")).unwrap();
    assert!(disk
        .append(
            3,
            JournalEvent::Manifest {
                role: ManifestRole::Shortcuts,
                digest: third
            }
        )
        .is_err());
    assert_eq!(disk.fixture_dependency_count(), 2);
    assert_eq!(
        std::fs::read(root.path().join("journal.log")).unwrap(),
        before
    );
    assert_eq!(
        disk.inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        3
    );
}

// Windows旧依赖、receipt及observed文件在writer生命期和重开后均拒绝普通write/delete。
#[cfg(windows)]
#[test]
fn HistoryTransaction_WindowsProtectedDependencies_047() {
    use crate::version_history::journal::ManifestRole;
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let original = disk.retain_manifest(b"source context").unwrap();
    disk.append(
        0,
        JournalEvent::Manifest {
            role: ManifestRole::SourceContext,
            digest: original.clone(),
        },
    )
    .unwrap();
    disk.append(1, JournalEvent::Intent { effect: effect() })
        .unwrap();
    let observed = disk.retain_manifest(b"actual created object").unwrap();
    let receipt = disk
        .retain_effect_receipt(&effect().effect_id, Observation::Applied, &observed)
        .unwrap();
    disk.append(
        2,
        JournalEvent::Observed {
            effect_id: effect().effect_id,
            intent_generation: 2,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt.clone()),
            },
        },
    )
    .unwrap();
    let assert_held = || {
        for digest in [&original, &receipt, &observed] {
            let path = root.path().join(format!("manifest-{digest}.json"));
            assert!(std::fs::File::options().write(true).open(&path).is_err());
            assert!(std::fs::remove_file(&path).is_err());
            assert!(std::fs::rename(&path, root.path().join("moved-manifest")).is_err());
        }
    };
    assert_held();
    disk.append(
        3,
        JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        },
    )
    .unwrap();
    drop(disk);
    let mut reopened = store(&root);
    reopened.bind_existing(&binding()).unwrap();
    assert_held();
    assert_eq!(
        reopened
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        4
    );
}

// 已被其他writer打开的manifest不能成为被确认frame的依赖，失败时log保持原样。
#[cfg(windows)]
#[test]
fn HistoryTransaction_DependencySharingDenial_048() {
    use crate::version_history::journal::ManifestRole;
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let manifest = disk
        .retain_manifest(b"unprotected pending manifest")
        .unwrap();
    let _foreign_writer = std::fs::File::options()
        .write(true)
        .open(root.path().join(format!("manifest-{manifest}.json")))
        .unwrap();
    let before = std::fs::read(root.path().join("journal.log")).unwrap();
    assert!(disk
        .append(
            0,
            JournalEvent::Manifest {
                role: ManifestRole::SourceContext,
                digest: manifest
            }
        )
        .is_err());
    assert_eq!(
        std::fs::read(root.path().join("journal.log")).unwrap(),
        before
    );
    assert_eq!(
        disk.inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation(),
        0
    );
}

// 重开必须重建全部保护集合；预算不足不能仅凭已验证log缓存跳过保护。
#[test]
fn HistoryTransaction_ReopenDependencyBudget_049() {
    use crate::version_history::journal::ManifestRole;
    let root = tempfile::tempdir().unwrap();
    {
        let mut disk = store(&root);
        disk.initialize(binding(), capacity()).unwrap();
        for (index, role) in [
            ManifestRole::SourceContext,
            ManifestRole::SourceBundle,
            ManifestRole::Registration,
        ]
        .into_iter()
        .enumerate()
        {
            let digest = disk
                .retain_manifest(format!("manifest {index}").as_bytes())
                .unwrap();
            disk.append(index as u64, JournalEvent::Manifest { role, digest })
                .unwrap();
        }
    }
    let dir = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let mut disk = JournalStore::fixture_with_dependency_limit(dir, 2).unwrap();
    let before = std::fs::read(root.path().join("journal.log")).unwrap();
    assert!(disk.bind_existing(&binding()).is_err());
    assert!(disk.inspect(&binding()).unwrap().blocked);
    assert!(disk
        .append(
            3,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
    assert_eq!(
        std::fs::read(root.path().join("journal.log")).unwrap(),
        before
    );
}

// Unix路径替换为相同内容的新对象也会失去原始依赖identity，不能只比较hash。
#[cfg(unix)]
#[test]
fn HistoryTransaction_DependencyObjectReplacement_050() {
    use crate::version_history::journal::ManifestRole;
    let root = tempfile::tempdir().unwrap();
    let mut disk = store(&root);
    disk.initialize(binding(), capacity()).unwrap();
    let digest = disk.retain_manifest(b"same bytes").unwrap();
    disk.append(
        0,
        JournalEvent::Manifest {
            role: ManifestRole::SourceContext,
            digest: digest.clone(),
        },
    )
    .unwrap();
    let path = root.path().join(format!("manifest-{digest}.json"));
    std::fs::rename(&path, root.path().join("preserved-old-object")).unwrap();
    std::fs::write(&path, b"same bytes").unwrap();
    let before = std::fs::read(root.path().join("journal.log")).unwrap();
    assert!(disk
        .append(
            1,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired
            }
        )
        .is_err());
    assert_eq!(
        std::fs::read(root.path().join("journal.log")).unwrap(),
        before
    );
}
