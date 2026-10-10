//! Real isolated NTFS startup admission. No installer or user profile is used.
use crate::version_history::{
    journal::{CapacityPlan, JournalBinding, JournalStore},
    maintenance::ActiveContextMarker,
    windows::{durability::MarkerStore, startup::InstallationControl},
};

fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000701".into(),
        source_context: "00000000-0000-4000-8000-000000000702".into(),
        target_context: "00000000-0000-4000-8000-000000000703".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}

// 不论debug/release或是否注册，干净控制根的普通启动持有真实shared lease到生命周期结束。
#[test]
fn HistoryStartup_CleanAbsenceKeepsLease_001() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let admitted = installation.fixture_admit().unwrap();
    let control = installation.acquire_control().unwrap();
    assert!(installation.leases().acquire_exclusive(&control).is_err());
    assert!(admitted.retained_startup_bundle().is_err());
    drop(admitted);
    assert!(installation.leases().acquire_exclusive(&control).is_ok());
}

// marker缺失但保留了事务日志时不能创建一个新空状态绕过恢复。
#[test]
fn HistoryStartup_OrphanJournalBlocks_002() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let original = binding();
    let mut store = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &original.transaction_id,
    )
    .unwrap();
    store
        .initialize(original, CapacityPlan::for_effects(4, 4, 4, 4096).unwrap())
        .unwrap();
    drop(store);
    assert!(installation.fixture_admit().is_err());
}

// 损坏或半写marker是未知状态；不能当作NotFound放行，也不能覆盖原字节。
#[test]
fn HistoryStartup_UnreadableMarkerBlocks_003() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let path = temporary
        .path()
        .join("CCDesk-VersionControl/active-context.log");
    std::fs::write(&path, b"{\"interrupted\":").unwrap();
    assert!(installation.fixture_admit().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{\"interrupted\":");
}

// 真正transition marker在普通初始化前拒绝，即使当前没有manager进程也不能消除。
#[test]
fn HistoryStartup_TransitionBlocks_004() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let original = binding();
    let control = installation.acquire_control().unwrap();
    let mut store = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &original.transaction_id,
    )
    .unwrap();
    store
        .initialize(
            original.clone(),
            CapacityPlan::for_effects(4, 4, 4, 4096).unwrap(),
        )
        .unwrap();
    let marker = ActiveContextMarker::transition_from(&store.inspect(&original).unwrap()).unwrap();
    let persisted =
        MarkerStore::create(installation.root().clone(), &control, &marker, &mut store).unwrap();
    drop(persisted);
    drop(store);
    drop(control);
    let failed = match installation.fixture_admit() {
        Err(failed) => failed,
        Ok(_) => panic!("transition must block"),
    };
    assert_eq!(failed.code, "HISTORY_RECOVERY_REQUIRED");
}

// Tauri托管的普通启动所有者只保留可发送的bundle/租约；registry观察在准入线程结束。
#[test]
fn HistoryStartup_ManagedOwnerTraits_005() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<crate::version_history::windows::startup::OrdinaryStartup>();
}

#[test]
fn HistoryStartup_OrdinaryBackupLeavesGlobalNamespaceUntouched_006() {
    let temporary = tempfile::tempdir().unwrap();
    let global = InstallationControl::fixture(temporary.path()).unwrap();
    let control = global.acquire_control().unwrap();
    let before = std::fs::read_dir(temporary.path().join("CCDesk-VersionControl"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    let transaction = binding().transaction_id;
    let backup = global.ordinary_backup(&control, &transaction).unwrap();
    assert!(backup.is_ordinary_backup());
    assert!(!global.is_ordinary_backup());
    assert_ne!(
        backup.root().directory().identity(),
        global.root().directory().identity()
    );
    assert!(temporary
        .path()
        .join("CCDesk-VersionBackups")
        .join(&transaction)
        .is_dir());
    assert!(global.ordinary_backup(&control, &transaction).is_err());
    let after = std::fs::read_dir(temporary.path().join("CCDesk-VersionControl"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(before, after);
    drop(control);
    assert!(global.fixture_admit().is_ok());
}

fn publish_transition(installation: &InstallationControl, original: JournalBinding) {
    let control = installation.acquire_control().unwrap();
    let mut store = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &original.transaction_id,
    )
    .unwrap();
    store
        .initialize(
            original.clone(),
            CapacityPlan::for_effects(4, 4, 4, 4096).unwrap(),
        )
        .unwrap();
    let marker = ActiveContextMarker::transition_from(&store.inspect(&original).unwrap()).unwrap();
    MarkerStore::create(installation.root().clone(), &control, &marker, &mut store).unwrap();
}

#[test]
fn HistoryStartup_ManagerSelectsExactPrivateTransactionAndRejectsAmbiguity_007() {
    let temporary = tempfile::tempdir().unwrap();
    let global = InstallationControl::fixture(temporary.path()).unwrap();
    let original = binding();
    let control = global.acquire_control().unwrap();
    let backup = global
        .ordinary_backup(&control, &original.transaction_id)
        .unwrap();
    drop(control);
    assert!(global
        .fixture_open_for_manager(&original.transaction_id)
        .is_err());
    publish_transition(&backup, original.clone());
    let reopened = global
        .fixture_open_for_manager(&original.transaction_id)
        .unwrap();
    assert!(reopened.is_ordinary_backup());
    assert_eq!(
        reopened.root().directory().identity(),
        backup.root().directory().identity()
    );
    assert!(global
        .fixture_open_for_manager("00000000-0000-4000-8000-000000000799")
        .is_err());
    publish_transition(&global, original.clone());
    assert!(global
        .fixture_open_for_manager(&original.transaction_id)
        .is_err());
}

#[test]
fn HistoryStartup_GlobalCustodyRetainsBothActualLeasesUntilHandoff_008() {
    use crate::version_history::windows::startup::GlobalLeaseCustody;
    let temporary = tempfile::tempdir().unwrap();
    let global = InstallationControl::fixture(temporary.path()).unwrap();
    let custody = GlobalLeaseCustody::acquire(global.clone()).unwrap();
    custody.verify().unwrap();
    assert!(global.acquire_control().is_err());
    custody.release_at_installer_handoff().unwrap();
    let control = global.acquire_control().unwrap();
    assert!(global.leases().acquire_exclusive(&control).is_ok());
}

#[test]
fn HistoryStartup_OrdinaryBackupRejectsExistingGlobalEvidence_009() {
    use crate::version_history::windows::startup::GlobalLeaseCustody;
    for evidence in ["active-context.log", "orphan.json"] {
        let temporary = tempfile::tempdir().unwrap();
        let global = InstallationControl::fixture(temporary.path()).unwrap();
        let control = global.acquire_control().unwrap();
        let path = temporary
            .path()
            .join("CCDesk-VersionControl")
            .join(evidence);
        std::fs::write(&path, b"preserved global evidence").unwrap();
        let failure = global
            .ordinary_backup(&control, &binding().transaction_id)
            .err()
            .unwrap();
        assert_eq!(failure.code, "HISTORY_ORDINARY_EXISTING_RECOVERY");
        assert!(!temporary.path().join("CCDesk-VersionBackups").exists());
        drop(control);
        let failure = GlobalLeaseCustody::acquire(global).err().unwrap();
        assert_eq!(failure.code, "HISTORY_ORDINARY_EXISTING_RECOVERY");
        assert_eq!(std::fs::read(path).unwrap(), b"preserved global evidence");
    }
}

#[test]
fn HistoryStartup_GlobalCustodyWaitsForActualSourceLifetime_010() {
    use crate::version_history::windows::startup::GlobalLeaseCustody;
    let temporary = tempfile::tempdir().unwrap();
    let global = InstallationControl::fixture(temporary.path()).unwrap();
    let source = global.fixture_admit().unwrap();
    let control = global.acquire_control().unwrap();
    let backup = global
        .ordinary_backup(&control, &binding().transaction_id)
        .unwrap();
    let backup_control = backup.acquire_control().unwrap();
    let backup_shared = backup.leases().acquire_shared(&backup_control).unwrap();
    drop(control);
    assert!(GlobalLeaseCustody::acquire(global.clone()).is_err());
    drop(source);
    let custody = GlobalLeaseCustody::acquire(global).unwrap();
    custody.verify().unwrap();
    assert!(backup.leases().acquire_exclusive(&backup_control).is_err());
    drop(backup_shared);
    assert!(backup.leases().acquire_exclusive(&backup_control).is_ok());
    custody.release_at_installer_handoff().unwrap();
}

#[test]
fn HistoryStartup_GlobalCustodyRechecksEvidenceBeforeHandoff_011() {
    use crate::version_history::windows::startup::GlobalLeaseCustody;
    let temporary = tempfile::tempdir().unwrap();
    let global = InstallationControl::fixture(temporary.path()).unwrap();
    let custody = GlobalLeaseCustody::acquire(global.clone()).unwrap();
    let path = temporary
        .path()
        .join("CCDesk-VersionControl/active-context.log");
    std::fs::write(&path, b"new global evidence").unwrap();
    assert_eq!(
        custody.verify().unwrap_err().code,
        "HISTORY_ORDINARY_EXISTING_RECOVERY"
    );
    assert_eq!(
        custody.release_at_installer_handoff().unwrap_err().code,
        "HISTORY_ORDINARY_EXISTING_RECOVERY"
    );
    assert_eq!(std::fs::read(path).unwrap(), b"new global evidence");
}

#[test]
fn HistoryStartup_OrdinaryDataRetainedInsideSiblingBackupStore_012() {
    use crate::version_history::windows::startup::TransactionDataRoot;
    let temporary = tempfile::tempdir().unwrap();
    let global = InstallationControl::fixture(temporary.path()).unwrap();
    let global_control = global.acquire_control().unwrap();
    let backup = global
        .ordinary_backup(&global_control, &binding().transaction_id)
        .unwrap();
    let control = backup.acquire_control().unwrap();
    let data =
        TransactionDataRoot::create(backup.clone(), &control, &binding().transaction_id).unwrap();
    let expected = temporary
        .path()
        .join("CCDesk-VersionBackups")
        .join(format!("CCDesk-VersionData-{}", binding().transaction_id));
    assert!(expected.is_dir());
    let displayed = std::path::PathBuf::from(backup.ordinary_backup_location().unwrap());
    assert!(
        displayed.join(&binding().transaction_id).is_dir(),
        "display includes protected journal and marker"
    );
    assert!(
        displayed
            .join(format!("CCDesk-VersionData-{}", binding().transaction_id))
            .is_dir(),
        "display includes full data copies"
    );
    assert!(!temporary
        .path()
        .join(format!("CCDesk-VersionData-{}", binding().transaction_id))
        .exists());
    let reopened = TransactionDataRoot::reopen(
        backup,
        &control,
        &binding().transaction_id,
        data.reference().clone(),
    )
    .unwrap();
    assert_eq!(
        reopened.root().directory().identity(),
        data.root().directory().identity()
    );
    global.require_ordinary_global_context().unwrap();
}
