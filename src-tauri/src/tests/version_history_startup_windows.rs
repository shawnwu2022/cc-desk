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
