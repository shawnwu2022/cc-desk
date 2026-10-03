//! Real held-object dependencies of the initial source session. These do not
//! impersonate a registered source or an actual BrowserProcessExited receipt.
use super::*;
use crate::version_history::journal::{CapacityPlan, JournalEvent};

// 检查真实缺失的Desk根保留父目录和精确名称，文件类型错误不能当作缺失。
#[test]
fn HistorySourceSession_DeskAbsence_001() {
    let temporary = tempfile::tempdir().unwrap();
    let home = Directory::open_absolute(temporary.path()).unwrap();
    let absent = capture_desk_root(home.clone()).unwrap();
    assert!(matches!(&absent, HeldRoot::Absent { parent, name }
        if parent.identity() == home.identity() && name.os_string() == OsStr::new(".cc-box")));
    std::fs::write(temporary.path().join(".cc-box"), b"not a directory").unwrap();
    assert!(absent.location_identity().is_err());
    assert!(capture_desk_root(home).is_err());
}

// 检查临时只读句柄释放后取得同对象DELETE能力，并对未知文件执行durable M0读取。
#[test]
fn HistorySourceSession_DeskDurable_002() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir(temporary.path().join(".cc-box")).unwrap();
    std::fs::write(
        temporary.path().join(".cc-box/unknown.bin"),
        b"retained bytes",
    )
    .unwrap();
    let home = Directory::open_absolute(temporary.path()).unwrap();
    let expected = {
        let observed = home.open_directory(component(".cc-box").unwrap()).unwrap();
        observed.identity().clone()
    };
    let desk = capture_desk_root(home.clone()).unwrap();
    let HeldRoot::Present(root) = &desk else {
        panic!("Desk root must remain present")
    };
    assert_eq!(root.identity(), &expected);
    root.require_renameable().unwrap();
    let context = HeldContext::capture_durable(
        desk,
        HeldRoot::observe(home, component("actual-udf").unwrap()).unwrap(),
        SnapshotLimits::default(),
    )
    .unwrap();
    context.verify_durable().unwrap();
    assert!(context
        .tree(RootKind::Desk)
        .manifest()
        .entries
        .iter()
        .any(|entry| entry.metadata.path == "unknown.bin"
            && entry.sha256.as_deref() == Some(sha256(b"retained bytes").as_str())));
}

// 检查旧Desk读取者导致DELETE准入冲突时，源会话不会错误地产生缺失快照。
#[test]
fn HistorySourceSession_DeskReader_003() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir(temporary.path().join(".cc-box")).unwrap();
    let home = Directory::open_absolute(temporary.path()).unwrap();
    let reader = home.open_directory(component(".cc-box").unwrap()).unwrap();
    assert!(capture_desk_root(home.clone()).is_err());
    reader.recheck().unwrap();
    drop(reader);
    assert!(matches!(
        capture_desk_root(home).unwrap(),
        HeldRoot::Present(_)
    ));
}

// 检查活跃的真实源进程即使身份一致也不能成为终止证明。
#[test]
fn HistorySourceSession_LiveSource_004() {
    let source = ExactProcess::capture_observed(std::process::id()).unwrap();
    let expected = source.identity().clone();
    assert_eq!(
        require_source_terminal(&source, &expected)
            .unwrap_err()
            .code,
        "HISTORY_SOURCE_EXIT_UNCONFIRMED"
    );
    assert!(source.terminal(0).unwrap().is_none());
}

// 检查关闭旧shared句柄后重新取得独占锁，仍有其他真实shared持有者时失败。
#[test]
fn HistorySourceSession_NewExclusive_005() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let control = installation.acquire_control().unwrap();
    let mut source = Some(installation.leases().acquire_shared(&control).unwrap());
    let other = installation.leases().acquire_shared(&control).unwrap();
    assert!(acquire_new_exclusive(&installation, &control, &mut source).is_err());
    assert!(source.is_none());
    other.verify_root(installation.root()).unwrap();
    drop(other);
    let mut source = Some(installation.leases().acquire_shared(&control).unwrap());
    let exclusive = acquire_new_exclusive(&installation, &control, &mut source).unwrap();
    assert!(source.is_none());
    assert!(installation.leases().acquire_shared(&control).is_err());
    exclusive.verify_root(installation.root()).unwrap();
}

// 检查真实源写入者要求同一根、当前marker和唯一退出代次，陈旧检查点不能继续。
#[test]
fn HistorySourceSession_MarkerBinding_006() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let control = installation.acquire_control().unwrap();
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
    let mut store = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &binding.transaction_id,
    )
    .unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(32, 32, 32, 4096).unwrap(),
        )
        .unwrap();
    let initial = ActiveContextMarker::transition_from(&store.inspect(&binding).unwrap()).unwrap();
    drop(MarkerStore::create(installation.root().clone(), &control, &initial, &mut store).unwrap());
    // Only role storage is exercised here. These bytes cannot be decoded into
    // ManagerChildAdmission or SourceHandoffTerminal by their actual factories.
    let handoff = store
        .retain_manifest(b"storage-only handoff fixture")
        .unwrap();
    let generation = store
        .append(
            0,
            JournalEvent::Manifest {
                role: ManifestRole::ManagerHandoff,
                digest: handoff,
            },
        )
        .unwrap();
    publish_copy_checkpoint(&installation, &control, &mut store, &binding).unwrap();
    drop(store);
    let (mut store, observed) = open_bound_writer(&installation, &control, &binding, None).unwrap();
    assert_eq!(observed, generation);
    let exit = store.retain_manifest(b"storage-only exit fixture").unwrap();
    let exit_generation = store
        .append(
            generation,
            JournalEvent::Manifest {
                role: ManifestRole::SourceHandoffExit,
                digest: exit,
            },
        )
        .unwrap();
    drop(store);
    assert!(open_bound_writer(&installation, &control, &binding, Some(exit_generation)).is_err());
    let mut store = JournalStore::open_windows_transaction(
        installation.root().clone(),
        &binding.transaction_id,
    )
    .unwrap();
    store.bind_existing(&binding).unwrap();
    publish_copy_checkpoint(&installation, &control, &mut store, &binding).unwrap();
    drop(store);
    let (mut store, observed) =
        open_bound_writer(&installation, &control, &binding, Some(exit_generation)).unwrap();
    assert_eq!(observed, exit_generation);
    let source = store
        .retain_manifest(b"unrelated later advancement")
        .unwrap();
    store
        .append(
            exit_generation,
            JournalEvent::Manifest {
                role: ManifestRole::SourceBundle,
                digest: source,
            },
        )
        .unwrap();
    publish_copy_checkpoint(&installation, &control, &mut store, &binding).unwrap();
    drop(store);
    assert!(open_bound_writer(&installation, &control, &binding, Some(exit_generation)).is_err());
    let mut changed = binding.clone();
    changed.roots = "6".repeat(64);
    assert!(open_bound_writer(&installation, &control, &changed, None).is_err());
}

// 检查真实子进程wait后只接受其精确身份，另一个仍活跃的进程不能替代。
#[test]
fn HistorySourceSession_ExactTerminal_007() {
    let temporary = tempfile::tempdir().unwrap();
    let marker = temporary.path().join("source-marker");
    let release = temporary.path().join("release-source");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::version_history_windows::HistoryWindows_ProcessWorker_013",
            "--ignored",
        ])
        .env("CC_DESK_HISTORY_PROBE_MARKER", &marker)
        .env("CC_DESK_HISTORY_PROBE_RELEASE", &release)
        .spawn()
        .unwrap();
    let source = ExactProcess::capture_observed(child.id()).unwrap();
    let expected = source.identity().clone();
    assert!(require_source_terminal(&source, &expected).is_err());
    std::fs::write(release, b"release").unwrap();
    assert!(child.wait().unwrap().success());
    require_source_terminal(&source, &expected).unwrap();
    let other = ExactProcess::capture_observed(std::process::id()).unwrap();
    assert!(require_source_terminal(&source, other.identity()).is_err());
    assert!(require_source_terminal(&other, &expected).is_err());
}
