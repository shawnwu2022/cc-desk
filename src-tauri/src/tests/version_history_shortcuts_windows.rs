//! Disposable NTFS product-link probes. These do not install CC Desk, create a
//! manager launcher, or supply installed-product acceptance evidence.
use crate::version_history::{
    journal::{
        CapacityPlan, EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase,
        JournalStore, ManifestRole, Observation, ObservedResult, RootKind, ShortcutSlot,
    },
    windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        lease::LeaseFiles,
        security::CurrentUser,
        shortcuts::{
            probe_shortcut_fault, probe_shortcut_resolver, HeldProductShortcuts,
            RetainedProductShortcuts, ShortcutFault, ShortcutJournal, ShortcutState,
            MAX_SHORTCUT_BYTES,
        },
    },
};
use std::{ffi::OsStr, sync::Arc};

fn name(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
}
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

fn set_dacl(path: &std::path::Path, sddl: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SetFileSecurityW,
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
    };
    let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            windows_core::PCWSTR(sddl.as_ptr()),
            1,
            &mut descriptor,
            None,
        )
        .unwrap();
        assert!(SetFileSecurityW(
            windows_core::PCWSTR(path.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor
        )
        .as_bool());
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
}

// Journal-only prerequisites for a disposable link test, never a production
// admission factory or evidence that a real installer/context was restored.
fn enter_restore(store: &mut JournalStore, mut generation: u64) -> u64 {
    let fixture = store
        .retain_manifest(b"shortcut fixture prerequisite")
        .unwrap();
    for role in [
        ManifestRole::SourceContext,
        ManifestRole::RetainedTargetContext,
    ] {
        generation = store
            .append(
                generation,
                JournalEvent::Manifest {
                    role,
                    digest: fixture.clone(),
                },
            )
            .unwrap();
    }
    generation = store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    for kind in [
        EffectKind::FenceHistoricalImage,
        EffectKind::PreserveRoot {
            context: binding().target_context,
            root: RootKind::Desk,
        },
        EffectKind::PreserveRoot {
            context: binding().target_context,
            root: RootKind::WebView,
        },
    ] {
        let id = uuid::Uuid::new_v4().to_string();
        generation = store
            .append(
                generation,
                JournalEvent::Intent {
                    effect: EffectSpec {
                        effect_id: id.clone(),
                        kind,
                        before: fixture.clone(),
                        expected_postconditions: fixture.clone(),
                    },
                },
            )
            .unwrap();
        let receipt = store
            .retain_effect_receipt(&id, Observation::Applied, &fixture)
            .unwrap();
        generation = store
            .append(
                generation,
                JournalEvent::Observed {
                    effect_id: id,
                    intent_generation: generation,
                    result: ObservedResult {
                        observation: Observation::Applied,
                        receipt: Some(receipt),
                    },
                },
            )
            .unwrap();
    }
    store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restoring,
            },
        )
        .unwrap()
}

// 检查实际链接的完整字节和权限私有保留，后续冲突先保留再恢复，另一位置缺失不被创建。
#[test]
fn HistoryShortcuts_RestoreConflict_001() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let desktop =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
    let programs =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap());
    let records = Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
    let link = temp.path().join("desktop/CC Desk.lnk");
    std::fs::write(&link, [0, 1, 255, 10]).unwrap();
    set_dacl(
        &link,
        &format!("D:P(A;;FA;;;{})(A;;FR;;;SY)", user.sid_text()),
    );
    let captured = HeldProductShortcuts::capture_at([
        desktop.directory().clone(),
        programs.directory().clone(),
    ])
    .unwrap();
    assert!(std::fs::write(&link, b"blocked writer").is_err());
    let leases = LeaseFiles::open(records.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let mut store = JournalStore::open_windows(records.clone()).unwrap();
    store
        .initialize(
            binding(),
            CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
        )
        .unwrap();
    let (retained, generation) = {
        let mut journal =
            ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
        let retained = captured.retain(&mut journal).unwrap();
        (retained, journal.generation())
    };
    let source = retained.state(ShortcutSlot::Desktop).clone();
    assert!(std::fs::write(
        temp.path()
            .join(format!("records/manifest-{}.json", retained.digest())),
        b"replace protected evidence"
    )
    .is_err());
    std::fs::write(&link, b"later user-created shortcut").unwrap();
    set_dacl(&link, &format!("D:P(A;;FA;;;{})", user.sid_text()));
    let generation = enter_restore(&mut store, generation);
    let mut journal = ShortcutJournal::new(
        &mut store,
        records.clone(),
        &exclusive,
        binding(),
        generation,
    )
    .unwrap();
    let receipt = retained
        .restore(ShortcutSlot::Desktop, &mut journal)
        .unwrap();
    let generation = journal.generation();
    drop(journal);
    let conflict: serde_json::Value =
        serde_json::from_slice(&store.read_manifest(receipt.preserved_current()).unwrap()).unwrap();
    assert_eq!(
        conflict["state"]["Present"]["bytes"],
        serde_json::json!(b"later user-created shortcut".to_vec())
    );
    assert_eq!(std::fs::read(&link).unwrap(), [0, 1, 255, 10]);
    let current = HeldProductShortcuts::capture_at([
        desktop.directory().clone(),
        programs.directory().clone(),
    ])
    .unwrap();
    assert!(current
        .state(ShortcutSlot::Desktop)
        .matches_restored_content_and_permissions(&source));
    assert_eq!(
        current.state(ShortcutSlot::StartMenu),
        &ShortcutState::Absent
    );
    drop(current);
    receipt.verify().unwrap();
    drop(receipt);
    let mut journal =
        ShortcutJournal::new(&mut store, records, &exclusive, binding(), generation).unwrap();
    assert!(retained
        .restore(ShortcutSlot::Desktop, &mut journal)
        .is_err());
    assert_eq!(std::fs::read(&link).unwrap(), [0, 1, 255, 10]);
}

// 检查已保留的真实缺失状态删除后续链接前保留其完整内容，重开后也不能重放。
#[test]
fn HistoryShortcuts_RestoreAbsence_002() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let desktop =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
    let programs =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap());
    let records = Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
    let destinations = [desktop.directory().clone(), programs.directory().clone()];
    let captured = HeldProductShortcuts::capture_at(destinations.clone()).unwrap();
    let leases = LeaseFiles::open(records.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let mut store = JournalStore::open_windows(records.clone()).unwrap();
    store
        .initialize(
            binding(),
            CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
        )
        .unwrap();
    let mut journal =
        ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
    let retained = captured.retain(&mut journal).unwrap();
    let digest = retained.digest().to_owned();
    let generation = journal.generation();
    drop(journal);
    let generation = enter_restore(&mut store, generation);
    let path = temp.path().join("programs/CC Desk.lnk");
    std::fs::write(&path, b"post-install product shortcut").unwrap();
    let mut journal = ShortcutJournal::new(
        &mut store,
        records.clone(),
        &exclusive,
        binding(),
        generation,
    )
    .unwrap();
    let receipt = retained
        .restore(ShortcutSlot::StartMenu, &mut journal)
        .unwrap();
    let generation = journal.generation();
    drop(journal);
    assert!(!path.exists());
    let conflict: serde_json::Value =
        serde_json::from_slice(&store.read_manifest(receipt.preserved_current()).unwrap()).unwrap();
    assert_eq!(
        conflict["state"]["Present"]["bytes"],
        serde_json::json!(b"post-install product shortcut".to_vec())
    );
    receipt.verify().unwrap();
    drop(receipt);
    drop(retained);
    drop(store);
    let mut reopened = JournalStore::open_windows(records.clone()).unwrap();
    reopened.bind_existing(&binding()).unwrap();
    let restored =
        RetainedProductShortcuts::reopen_at(&reopened, &binding(), &digest, destinations).unwrap();
    std::fs::write(&path, b"new conflict after first restore").unwrap();
    let mut journal =
        ShortcutJournal::new(&mut reopened, records, &exclusive, binding(), generation).unwrap();
    assert!(restored
        .restore(ShortcutSlot::StartMenu, &mut journal)
        .is_err());
    assert_eq!(
        std::fs::read(path).unwrap(),
        b"new conflict after first restore"
    );
}

// 检查部分字节和完成写入后丢失观察均保留源与冲突，挂起结果阻止同进程及重开重放。
#[test]
fn HistoryShortcuts_UnknownNoReplay_003() {
    for fault in [
        ShortcutFault::PartialWrite,
        ShortcutFault::AfterWrite,
        ShortcutFault::AfterAttributes,
        ShortcutFault::AfterPermissions,
        ShortcutFault::AfterRemove,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let desktop =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
        let programs = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap(),
        );
        let records =
            Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
        let destinations = [desktop.directory().clone(), programs.directory().clone()];
        let path = temp.path().join("desktop/CC Desk.lnk");
        if fault != ShortcutFault::AfterRemove {
            std::fs::write(&path, b"original source bytes").unwrap();
            if fault == ShortcutFault::AfterAttributes {
                use std::os::windows::ffi::OsStrExt;
                use windows::Win32::Storage::FileSystem::{
                    SetFileAttributesW, FILE_ATTRIBUTE_NORMAL,
                };
                let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                unsafe {
                    SetFileAttributesW(windows_core::PCWSTR(path.as_ptr()), FILE_ATTRIBUTE_NORMAL)
                        .unwrap();
                }
            }
        }
        let captured = HeldProductShortcuts::capture_at(destinations.clone()).unwrap();
        let leases = LeaseFiles::open(records.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let exclusive = leases.acquire_exclusive(&control).unwrap();
        let mut store = JournalStore::open_windows(records.clone()).unwrap();
        store
            .initialize(
                binding(),
                CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
            )
            .unwrap();
        let mut journal =
            ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
        let retained = captured.retain(&mut journal).unwrap();
        let digest = retained.digest().to_owned();
        let generation = journal.generation();
        drop(journal);
        let generation = enter_restore(&mut store, generation);
        std::fs::write(&path, b"later conflict").unwrap();
        let mut journal = ShortcutJournal::new(
            &mut store,
            records.clone(),
            &exclusive,
            binding(),
            generation,
        )
        .unwrap();
        let probe = probe_shortcut_fault(fault);
        assert!(retained
            .restore(ShortcutSlot::Desktop, &mut journal)
            .is_err());
        drop(probe);
        let generation = journal.generation();
        drop(journal);
        let after = std::fs::read(&path).ok();
        if fault == ShortcutFault::AfterAttributes {
            use std::os::windows::ffi::OsStrExt;
            use windows::Win32::Storage::FileSystem::{GetFileAttributesW, FILE_ATTRIBUTE_NORMAL};
            let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            assert_eq!(
                unsafe { GetFileAttributesW(windows_core::PCWSTR(path.as_ptr())) },
                FILE_ATTRIBUTE_NORMAL.0
            );
        }
        let inspected = store.inspect(&binding()).unwrap();
        let state = inspected.last_valid.unwrap();
        assert!(state.requires_reconciliation());
        if fault == ShortcutFault::AfterAttributes {
            let pending = state.pending_effect().unwrap();
            assert!(matches!(
                pending.kind,
                EffectKind::RecoveryShortcutEntry {
                    operation: crate::version_history::journal::ShortcutOperation::SetAttributes,
                    ..
                }
            ));
            assert_eq!(
                state.effect_observation(&pending.effect_id),
                Some(Observation::Unknown)
            );
        }
        assert!(store.read_manifest(&digest).is_ok());
        drop(retained);
        drop(store);
        let mut reopened = JournalStore::open_windows(records.clone()).unwrap();
        reopened.bind_existing(&binding()).unwrap();
        let retained =
            RetainedProductShortcuts::reopen_at(&reopened, &binding(), &digest, destinations)
                .unwrap();
        let mut journal =
            ShortcutJournal::new(&mut reopened, records, &exclusive, binding(), generation)
                .unwrap();
        assert!(retained
            .restore(ShortcutSlot::Desktop, &mut journal)
            .is_err());
        assert_eq!(std::fs::read(path).ok(), after);
    }
}

// 检查真实目录替换被旧句柄拒绝，不能用新的相同路径重建源权限。
#[test]
fn HistoryShortcuts_ChangedParent_004() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let desktop = Arc::new(
        PrivateDirectory::create_renameable_new(parent.clone(), name("desktop"), &user).unwrap(),
    );
    let programs =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap());
    let captured = HeldProductShortcuts::capture_at([
        desktop.directory().clone(),
        programs.directory().clone(),
    ])
    .unwrap();
    desktop
        .directory()
        .rename_to(parent, name("moved-desktop"))
        .unwrap();
    std::fs::create_dir(temp.path().join("desktop")).unwrap();
    std::fs::write(temp.path().join("desktop/CC Desk.lnk"), b"replacement").unwrap();
    assert!(captured.verify().is_err());
    assert_eq!(
        std::fs::read(temp.path().join("desktop/CC Desk.lnk")).unwrap(),
        b"replacement"
    );
}

// 检查受限大小和命名数据流不能进入完整快捷方式清单。
#[test]
fn HistoryShortcuts_BoundedStreams_005() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("desktop")).unwrap();
    std::fs::create_dir(temp.path().join("programs")).unwrap();
    let destinations = [
        Directory::open_absolute(&temp.path().join("desktop")).unwrap(),
        Directory::open_absolute(&temp.path().join("programs")).unwrap(),
    ];
    let path = temp.path().join("desktop/CC Desk.lnk");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_SHORTCUT_BYTES as u64 + 1).unwrap();
    drop(file);
    assert!(HeldProductShortcuts::capture_at(destinations.clone()).is_err());
    std::fs::write(&path, b"link").unwrap();
    std::fs::write(
        temp.path().join("desktop/CC Desk.lnk:hidden"),
        b"must preserve or refuse",
    )
    .unwrap();
    assert!(HeldProductShortcuts::capture_at(destinations).is_err());
}

// 检查原有链接被安装器删除后，使用新建文件、完整写入、精确权限三个独立效果恢复。
#[test]
fn HistoryShortcuts_RecreateMissing_006() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let desktop =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
    let programs =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap());
    let records = Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
    let path = temp.path().join("programs/CC Desk.lnk");
    std::fs::write(&path, b"original product shortcut").unwrap();
    set_dacl(
        &path,
        &format!("D:P(A;;FA;;;{})(A;;FR;;;SY)", user.sid_text()),
    );
    let captured = HeldProductShortcuts::capture_at([
        desktop.directory().clone(),
        programs.directory().clone(),
    ])
    .unwrap();
    let original = captured.state(ShortcutSlot::StartMenu).clone();
    let leases = LeaseFiles::open(records.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let mut store = JournalStore::open_windows(records.clone()).unwrap();
    store
        .initialize(
            binding(),
            CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
        )
        .unwrap();
    let mut journal =
        ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
    let retained = captured.retain(&mut journal).unwrap();
    let generation = journal.generation();
    drop(journal);
    std::fs::remove_file(&path).unwrap();
    let generation = enter_restore(&mut store, generation);
    let mut journal =
        ShortcutJournal::new(&mut store, records, &exclusive, binding(), generation).unwrap();
    let restored = retained
        .restore(ShortcutSlot::StartMenu, &mut journal)
        .unwrap();
    restored.verify().unwrap();
    assert_eq!(
        journal.generation() - generation,
        10,
        "create, content, attributes, permission and aggregate verification each need intent/observed frames"
    );
    let current = HeldProductShortcuts::capture_at([
        desktop.directory().clone(),
        programs.directory().clone(),
    ])
    .unwrap();
    assert!(current
        .state(ShortcutSlot::StartMenu)
        .matches_restored_content_and_permissions(&original));
}

// 检查私有冲突证据发生部分写入时不允许触碰原位置，并保持源证据可读。
#[test]
fn HistoryShortcuts_RetentionFails_007() {
    use crate::version_history::windows::durability::{
        probe_persistence_fault, PersistenceBoundary, PersistenceOperation,
    };
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let desktop =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
    let programs =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap());
    let records = Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
    let captured = HeldProductShortcuts::capture_at([
        desktop.directory().clone(),
        programs.directory().clone(),
    ])
    .unwrap();
    let leases = LeaseFiles::open(records.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let mut store = JournalStore::open_windows(records.clone()).unwrap();
    store
        .initialize(
            binding(),
            CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
        )
        .unwrap();
    let mut journal =
        ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
    let retained = captured.retain(&mut journal).unwrap();
    let generation = journal.generation();
    drop(journal);
    let generation = enter_restore(&mut store, generation);
    let path = temp.path().join("desktop/CC Desk.lnk");
    std::fs::write(&path, b"unretained conflict must survive").unwrap();
    let mut journal =
        ShortcutJournal::new(&mut store, records, &exclusive, binding(), generation).unwrap();
    let fault = probe_persistence_fault(
        PersistenceOperation::Artifact,
        PersistenceBoundary::PartialWrite,
    );
    assert!(retained
        .restore(ShortcutSlot::Desktop, &mut journal)
        .is_err());
    drop(fault);
    drop(journal);
    assert_eq!(
        std::fs::read(path).unwrap(),
        b"unretained conflict must survive"
    );
    assert!(store.read_manifest(retained.digest()).is_ok());
}

// 检查实际继承且未保护的 DACL 恢复成功，完整 owner/group/DACL 字节与来源完全相同。
#[test]
fn HistoryShortcuts_InheritedDacl_008() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let desktop =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
    let programs =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap());
    let records = Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
    let path = temp.path().join("desktop/CC Desk.lnk");
    std::fs::write(&path, b"inherited source shortcut").unwrap();
    let captured = HeldProductShortcuts::capture_at([
        desktop.directory().clone(),
        programs.directory().clone(),
    ])
    .unwrap();
    let original = captured.state(ShortcutSlot::Desktop).clone();
    let ShortcutState::Present { descriptor, .. } = &original else {
        panic!("source shortcut must exist");
    };
    let control = u16::from_le_bytes([descriptor[2], descriptor[3]]);
    assert_eq!(
        control & 0x1000,
        0,
        "fixture source DACL must be unprotected"
    );
    let dacl = u32::from_le_bytes(descriptor[16..20].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes([descriptor[dacl + 4], descriptor[dacl + 5]]);
    let mut ace = dacl + 8;
    let mut inherited = false;
    for _ in 0..count {
        inherited |= descriptor[ace + 1] & 0x10 != 0;
        ace += u16::from_le_bytes([descriptor[ace + 2], descriptor[ace + 3]]) as usize;
    }
    assert!(inherited, "fixture must exercise a real inherited ACE");
    let leases = LeaseFiles::open(records.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let mut store = JournalStore::open_windows(records.clone()).unwrap();
    store
        .initialize(
            binding(),
            CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
        )
        .unwrap();
    let mut journal =
        ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
    let retained = captured.retain(&mut journal).unwrap();
    let generation = journal.generation();
    drop(journal);
    std::fs::write(&path, b"different protected shortcut").unwrap();
    set_dacl(&path, &format!("D:P(A;;FA;;;{})", user.sid_text()));
    let generation = enter_restore(&mut store, generation);
    let mut journal =
        ShortcutJournal::new(&mut store, records, &exclusive, binding(), generation).unwrap();
    let receipt = retained
        .restore(ShortcutSlot::Desktop, &mut journal)
        .unwrap();
    receipt.verify().unwrap();
    let actual = HeldProductShortcuts::capture_at([
        desktop.directory().clone(),
        programs.directory().clone(),
    ])
    .unwrap();
    assert!(
        actual
            .state(ShortcutSlot::Desktop)
            .matches_restored_content_and_permissions(&original),
        "unprotected inherited owner/group/DACL and bytes must exactly match the source"
    );
}

// 检查解析器在最终确认前或收据创建后改指其他实际目录时拒绝旧目录证据，不修改用户 known-folder 配置。
#[test]
fn HistoryShortcuts_ResolverChange_009() {
    for before_final in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let desktop =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
        let programs = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap(),
        );
        let redirected = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("redirected"), &user).unwrap(),
        );
        let records =
            Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
        std::fs::write(
            temp.path().join("redirected/CC Desk.lnk"),
            b"new known-folder shortcut",
        )
        .unwrap();
        let resolver =
            probe_shortcut_resolver([desktop.directory().clone(), programs.directory().clone()]);
        let captured = HeldProductShortcuts::capture_current_user().unwrap();
        let leases = LeaseFiles::open(records.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let exclusive = leases.acquire_exclusive(&control).unwrap();
        let mut store = JournalStore::open_windows(records.clone()).unwrap();
        store
            .initialize(
                binding(),
                CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
            )
            .unwrap();
        let mut journal =
            ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
        let retained = captured.retain(&mut journal).unwrap();
        let generation = journal.generation();
        drop(journal);
        let generation = enter_restore(&mut store, generation);
        let mut journal =
            ShortcutJournal::new(&mut store, records, &exclusive, binding(), generation).unwrap();
        if before_final {
            resolver.replace_before_final(ShortcutSlot::Desktop, redirected.directory().clone());
            assert!(
                retained
                    .restore(ShortcutSlot::Desktop, &mut journal)
                    .is_err(),
                "changed known-folder mapping must prevent final Applied"
            );
            drop(journal);
            let inspected = store.inspect(&binding()).unwrap();
            let state = inspected.last_valid.unwrap();
            assert!(state.requires_reconciliation());
            assert!(matches!(
                state.pending_effect().unwrap().kind,
                EffectKind::RestoreShortcut {
                    slot: ShortcutSlot::Desktop
                }
            ));
        } else {
            let receipt = retained
                .restore(ShortcutSlot::Desktop, &mut journal)
                .unwrap();
            receipt.verify().unwrap();
            resolver.replace(ShortcutSlot::Desktop, redirected.directory().clone());
            assert!(
                receipt.verify().is_err(),
                "receipt cannot certify a stale known-folder mapping"
            );
        }
        assert_eq!(
            std::fs::read(temp.path().join("redirected/CC Desk.lnk")).unwrap(),
            b"new known-folder shortcut"
        );
        assert!(!temp.path().join("desktop/CC Desk.lnk").exists());
    }
}

// 检查重新打开的原始快捷方式及真实缺失状态被接受，返回后没有遗留读句柄阻碍安装器替换。
#[test]
fn HistoryShortcuts_VerifyOriginal_010() {
    for present in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let desktop =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
        let programs = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap(),
        );
        let records =
            Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
        let path = temp.path().join("desktop/CC Desk.lnk");
        if present {
            std::fs::write(&path, b"unchanged original shortcut").unwrap();
        }
        let _resolver =
            probe_shortcut_resolver([desktop.directory().clone(), programs.directory().clone()]);
        let captured = HeldProductShortcuts::capture_current_user().unwrap();
        let leases = LeaseFiles::open(records.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let exclusive = leases.acquire_exclusive(&control).unwrap();
        let mut store = JournalStore::open_windows(records.clone()).unwrap();
        store
            .initialize(
                binding(),
                CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
            )
            .unwrap();
        let mut journal =
            ShortcutJournal::new(&mut store, records, &exclusive, binding(), 0).unwrap();
        let retained = captured.retain(&mut journal).unwrap();
        retained.verify_original().unwrap();
        if present {
            let writable = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("verification must release the temporary source read guard");
            drop(writable);
            retained.verify_original().unwrap();
            std::fs::rename(&path, temp.path().join("desktop/installer-replaced.lnk"))
                .expect("verification must not retain a delete-blocking source guard");
        } else {
            std::fs::write(&path, b"installer-created shortcut").unwrap();
        }
        assert!(
            !temp.path().join("programs/CC Desk.lnk").exists(),
            "verifying source absence must not create a shortcut"
        );
    }
}

// 检查字节、属性、权限、原文件缺失、原缺失位置新增文件及解析器变化均拒绝原状态证明。
#[test]
fn HistoryShortcuts_OriginalDrift_011() {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        SetFileAttributesW, FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_NORMAL,
    };
    for drift in [
        "bytes",
        "attributes",
        "descriptor",
        "missing",
        "appeared",
        "resolver",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let desktop =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
        let programs = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap(),
        );
        let redirected = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("redirected"), &user).unwrap(),
        );
        let records =
            Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
        let path = temp.path().join("desktop/CC Desk.lnk");
        std::fs::write(&path, b"original shortcut").unwrap();
        set_dacl(&path, &format!("D:P(A;;FA;;;{})", user.sid_text()));
        let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            SetFileAttributesW(
                windows_core::PCWSTR(path_wide.as_ptr()),
                FILE_ATTRIBUTE_NORMAL,
            )
        }
        .unwrap();
        let resolver =
            probe_shortcut_resolver([desktop.directory().clone(), programs.directory().clone()]);
        let captured = HeldProductShortcuts::capture_current_user().unwrap();
        let leases = LeaseFiles::open(records.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let exclusive = leases.acquire_exclusive(&control).unwrap();
        let mut store = JournalStore::open_windows(records.clone()).unwrap();
        store
            .initialize(
                binding(),
                CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
            )
            .unwrap();
        let mut journal =
            ShortcutJournal::new(&mut store, records, &exclusive, binding(), 0).unwrap();
        let retained = captured.retain(&mut journal).unwrap();
        retained.verify_original().unwrap();
        match drift {
            "bytes" => std::fs::write(&path, b"changed shortcut bytes").unwrap(),
            "attributes" => unsafe {
                SetFileAttributesW(
                    windows_core::PCWSTR(path_wide.as_ptr()),
                    FILE_ATTRIBUTE_ARCHIVE,
                )
            }
            .unwrap(),
            "descriptor" => set_dacl(
                &path,
                &format!("D:P(A;;FA;;;{})(A;;FR;;;SY)", user.sid_text()),
            ),
            "missing" => std::fs::remove_file(&path).unwrap(),
            "appeared" => {
                std::fs::write(temp.path().join("programs/CC Desk.lnk"), b"new shortcut").unwrap()
            }
            "resolver" => resolver.replace(ShortcutSlot::Desktop, redirected.directory().clone()),
            _ => unreachable!(),
        }
        assert!(
            retained.verify_original().is_err(),
            "{drift} drift must reject original state"
        );
        if path.exists() {
            let writable = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("failed verification must also release temporary source readers");
            drop(writable);
        }
    }
}

// 检查NORMAL/ARCHIVE两种源属性在现有反向属性或当前缺失时都实际恢复，且冲突先保留。
#[test]
fn HistoryShortcuts_RestoreAttributes_012() {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        SetFileAttributesW, FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_NORMAL,
    };
    for (source_attributes, later_attributes) in [
        (FILE_ATTRIBUTE_NORMAL, Some(FILE_ATTRIBUTE_ARCHIVE)),
        (FILE_ATTRIBUTE_ARCHIVE, Some(FILE_ATTRIBUTE_NORMAL)),
        (FILE_ATTRIBUTE_NORMAL, None),
        (FILE_ATTRIBUTE_ARCHIVE, None),
    ] {
        eprintln!(
            "shortcut-attribute-case: source={:x}; later={:x?}",
            source_attributes.0,
            later_attributes.map(|attributes| attributes.0),
        );
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let desktop =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
        let programs = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap(),
        );
        let records =
            Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
        let destinations = [desktop.directory().clone(), programs.directory().clone()];
        let path = temp.path().join("desktop").join("CC Desk.lnk");
        std::fs::write(&path, b"exact source shortcut").unwrap();
        let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            SetFileAttributesW(windows_core::PCWSTR(wide.as_ptr()), source_attributes).unwrap();
        }
        let captured = HeldProductShortcuts::capture_at(destinations.clone()).unwrap();
        let original = captured.state(ShortcutSlot::Desktop).clone();
        let leases = LeaseFiles::open(records.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let exclusive = leases.acquire_exclusive(&control).unwrap();
        let mut store = JournalStore::open_windows(records.clone()).unwrap();
        store
            .initialize(
                binding(),
                CapacityPlan::for_effects(30, 30, 20, 4096).unwrap(),
            )
            .unwrap();
        let mut journal =
            ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
        let retained = captured.retain(&mut journal).unwrap();
        let generation = journal.generation();
        drop(journal);
        if let Some(attributes) = later_attributes {
            // Only the attribute differs; equal bytes must not skip this effect.
            std::fs::write(&path, b"exact source shortcut").unwrap();
            unsafe {
                SetFileAttributesW(windows_core::PCWSTR(wide.as_ptr()), attributes).unwrap();
            }
        } else {
            std::fs::remove_file(&path).unwrap();
        }
        let current = HeldProductShortcuts::capture_at(destinations.clone()).unwrap();
        let before = current.state(ShortcutSlot::Desktop).clone();
        drop(current);
        let generation = enter_restore(&mut store, generation);
        let mut journal =
            ShortcutJournal::new(&mut store, records, &exclusive, binding(), generation).unwrap();
        let receipt = retained
            .restore(ShortcutSlot::Desktop, &mut journal)
            .unwrap();
        receipt.verify().unwrap();
        let frames = journal.generation() - generation;
        assert_eq!(frames, if later_attributes.is_some() { 8 } else { 10 });
        drop(journal);
        let current = HeldProductShortcuts::capture_at(destinations).unwrap();
        assert!(current
            .state(ShortcutSlot::Desktop)
            .matches_restored_content_and_permissions(&original));
        let preserved: serde_json::Value =
            serde_json::from_slice(&store.read_manifest(receipt.preserved_current()).unwrap())
                .unwrap();
        assert_eq!(preserved["state"], serde_json::to_value(&before).unwrap());
    }
}

// 检查新增属性步骤已进入恢复容量计算，不足时在第一个修改intent前拒绝。
#[test]
fn HistoryShortcuts_AttributeCapacity_013() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let desktop =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("desktop"), &user).unwrap());
    let programs =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("programs"), &user).unwrap());
    let records = Arc::new(PrivateDirectory::create_new(parent, name("records"), &user).unwrap());
    let destinations = [desktop.directory().clone(), programs.directory().clone()];
    let path = temp.path().join("desktop").join("CC Desk.lnk");
    std::fs::write(&path, b"source").unwrap();
    let captured = HeldProductShortcuts::capture_at(destinations.clone()).unwrap();
    let leases = LeaseFiles::open(records.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let mut store = JournalStore::open_windows(records.clone()).unwrap();
    store
        .initialize(
            binding(),
            CapacityPlan::for_effects(30, 1, 10, 4096).unwrap(),
        )
        .unwrap();
    let mut journal =
        ShortcutJournal::new(&mut store, records.clone(), &exclusive, binding(), 0).unwrap();
    let retained = captured.retain(&mut journal).unwrap();
    let generation = journal.generation();
    drop(journal);
    std::fs::write(&path, b"later must remain").unwrap();
    let observed = HeldProductShortcuts::capture_at(destinations.clone()).unwrap();
    let before = observed.state(ShortcutSlot::Desktop).clone();
    drop(observed);
    let generation = enter_restore(&mut store, generation);
    let mut journal =
        ShortcutJournal::new(&mut store, records, &exclusive, binding(), generation).unwrap();
    assert!(retained
        .restore(ShortcutSlot::Desktop, &mut journal)
        .is_err());
    assert_eq!(journal.generation(), generation);
    drop(journal);
    assert!(store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .pending_effect()
        .is_none());
    let current = HeldProductShortcuts::capture_at(destinations).unwrap();
    assert_eq!(current.state(ShortcutSlot::Desktop), &before);
}

// 检查生产描述符设置器在独立 NTFS 文件上恢复继承与保护权限，仅允许系统正向设置继承标记。
#[test]
fn HistorySecurity_ReadbackProbe_001() {
    use crate::version_history::windows::{
        security::restored_file_descriptor_matches,
        shortcuts::{
            probe_apply_descriptor, probe_capture_descriptor, probe_descriptor_difference,
        },
    };
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = Arc::new(PrivateDirectory::create_new(parent, name("probe"), &user).unwrap());
    root.verify(&user).unwrap();
    let mut outcomes = Vec::new();
    for case in ["inherited", "protected"] {
        let path = temp.path().join("probe").join(case);
        std::fs::write(&path, b"disposable descriptor readback probe").unwrap();
        if case == "protected" {
            set_dacl(
                &path,
                &format!("D:P(A;;FA;;;{})(A;;FR;;;SY)", user.sid_text()),
            );
        }
        let expected = probe_capture_descriptor(&std::fs::File::open(&path).unwrap()).unwrap();
        set_dacl(&path, &format!("D:P(A;;FA;;;{})", user.sid_text()));
        let file = OpenOptions::new()
            .access_mode(FILE_ALL_ACCESS.0)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let before = probe_capture_descriptor(&file).unwrap();
        assert_ne!(before, expected, "probe must restore a changed descriptor");
        let restored = probe_apply_descriptor(&file, &expected);
        let actual = probe_capture_descriptor(&file).unwrap();
        probe_descriptor_difference(case, &expected, &actual);
        outcomes.push((
            case,
            restored.is_ok(),
            restored_file_descriptor_matches(&expected, &actual),
        ));
        drop(file);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"disposable descriptor readback probe",
            "descriptor restoration must leave file contents intact"
        );
    }
    root.verify(&user).unwrap();
    assert!(
        outcomes
            .iter()
            .all(|(_, restored, matches)| *restored && *matches),
        "each descriptor must satisfy the restoration contract; (case, setter_success, readback_matches)={outcomes:?}"
    );
}

// 检查仅恢复后的 DACL 自动继承标记可从未设置变为设置，其余每个字节和控制位仍须一致。
#[test]
fn HistorySecurity_RestoreContract_002() {
    use crate::version_history::windows::security::restored_file_descriptor_matches;
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            PSECURITY_DESCRIPTOR, SE_DACL_AUTO_INHERITED,
        },
    };

    // A complete descriptor includes owner/group and ordered DACL/SACL ACEs.
    // Conversion does not install an audit ACL or require audit privileges.
    let sddl: Vec<u16> = "O:SYG:BAD:P(A;;FA;;;SY)(A;;FR;;;BA)S:P(AU;SA;FR;;;WD)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let expected = unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let mut length = 0;
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            windows_core::PCWSTR(sddl.as_ptr()),
            1,
            &mut descriptor,
            Some(&mut length),
        )
        .unwrap();
        let bytes = std::slice::from_raw_parts(descriptor.0.cast::<u8>(), length as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        bytes
    };
    let control = u16::from_le_bytes([expected[2], expected[3]]);
    assert_eq!(control & SE_DACL_AUTO_INHERITED.0, 0);
    let mut restored = expected.clone();
    restored[2..4].copy_from_slice(&(control | SE_DACL_AUTO_INHERITED.0).to_le_bytes());
    assert_ne!(
        expected, restored,
        "raw capture/equality must retain OS evidence"
    );
    assert!(restored_file_descriptor_matches(&expected, &expected));
    assert!(restored_file_descriptor_matches(&expected, &restored));
    assert!(restored_file_descriptor_matches(&restored, &restored));
    assert!(
        !restored_file_descriptor_matches(&restored, &expected),
        "clearing AUTO_INHERITED is not the observed SetSecurityInfo transition"
    );
    // Includes owner/group SIDs, ACE access masks/SIDs/inheritance flags/order,
    // ACL sizes and offsets, DACL/SACL PRESENT, and both PROTECTED control bits.
    for offset in 0..restored.len() {
        for bit in 0..8 {
            if offset == 3 && bit == 2 {
                continue;
            }
            let mut changed = restored.clone();
            changed[offset] ^= 1 << bit;
            assert!(
                !restored_file_descriptor_matches(&expected, &changed),
                "restoration accepted another change at byte {offset}, bit {bit}"
            );
        }
    }
    let mut extended = restored.clone();
    extended.push(0);
    assert!(!restored_file_descriptor_matches(&expected, &extended));
    assert!(!restored_file_descriptor_matches(
        &expected,
        &restored[..19]
    ));
}

// 检查只读快捷方式比较仍区分系统继承标记，恢复比较仅允许正向的系统转换。
#[test]
fn HistorySecurity_ShortcutContract_003() {
    use crate::version_history::windows::shortcuts::probe_capture_descriptor;
    use windows::Win32::Security::SE_DACL_AUTO_INHERITED;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("shortcut-contract");
    std::fs::write(&path, b"shortcut fixture").unwrap();
    let user = CurrentUser::capture().unwrap();
    set_dacl(&path, &format!("D:P(A;;FA;;;{})", user.sid_text()));
    let descriptor = probe_capture_descriptor(&std::fs::File::open(&path).unwrap()).unwrap();
    let original = ShortcutState::Present {
        identity: Directory::open_absolute(temp.path())
            .unwrap()
            .identity()
            .clone(),
        attributes: windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ARCHIVE.0,
        bytes: b"shortcut fixture".to_vec(),
        sha256: "fixture digest".into(),
        descriptor,
    };
    let mut restored = original.clone();
    let ShortcutState::Present { descriptor, .. } = &mut restored else {
        unreachable!()
    };
    let control = u16::from_le_bytes([descriptor[2], descriptor[3]]);
    descriptor[2..4].copy_from_slice(&(control | SE_DACL_AUTO_INHERITED.0).to_le_bytes());
    assert_ne!(restored, original);
    assert!(!restored.same_content_and_permissions(&original));
    assert!(restored.matches_restored_content_and_permissions(&original));
    assert!(!original.matches_restored_content_and_permissions(&restored));
    for field in 0..3 {
        let mut changed = restored.clone();
        let ShortcutState::Present {
            bytes,
            sha256,
            attributes,
            ..
        } = &mut changed
        else {
            unreachable!()
        };
        match field {
            0 => bytes.push(0),
            1 => sha256.push('0'),
            2 => *attributes ^= windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY.0,
            _ => unreachable!(),
        }
        assert!(!changed.matches_restored_content_and_permissions(&original));
    }
}
