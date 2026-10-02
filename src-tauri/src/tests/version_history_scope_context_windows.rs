//! Actual NTFS M0-to-exclusion admission; fixtures never change host environment.
use crate::cli::environment::EnvMap;
use crate::version_history::{
    journal::{CapacityPlan, JournalBinding, JournalStore, RootKind},
    maintenance::SnapshotBoundary,
    snapshot::SnapshotLimits,
    windows::{
        context::{ContextJournal, HeldContext, HeldRoot, PrivateTreeCopy},
        fence::ImageFence,
        files::{ComponentName, Directory, FileAccess, PrivateDirectory},
        lease::LeaseFiles,
        scope::{ConfiguredInventory, ScopeBlock, ScopeInputs},
        security::CurrentUser,
    },
};
use std::{
    ffi::{OsStr, OsString},
    sync::Arc,
};

fn name(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
}
struct Fixture {
    temporary: tempfile::TempDir,
    user: CurrentUser,
    home: Arc<Directory>,
    desk: Arc<PrivateDirectory>,
    installation: Arc<Directory>,
    recovery: Arc<PrivateDirectory>,
}
impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let home = Directory::open_absolute(temporary.path()).unwrap();
        let desk = Arc::new(
            PrivateDirectory::create_renameable_new(home.clone(), name(".cc-box"), &user).unwrap(),
        );
        let recovery =
            Arc::new(PrivateDirectory::create_new(home.clone(), name("recovery"), &user).unwrap());
        std::fs::create_dir(temporary.path().join("installation")).unwrap();
        let installation = home.open_directory(name("installation")).unwrap();
        Self {
            temporary,
            user,
            home,
            desk,
            installation,
            recovery,
        }
    }
    fn environment(&self) -> EnvMap {
        EnvMap::from([(
            OsString::from("USERPROFILE"),
            self.temporary.path().as_os_str().to_owned(),
        )])
    }
    fn context(&self, durable: bool) -> HeldContext {
        let desk = HeldRoot::Present(self.desk.directory().clone());
        let udf = HeldRoot::observe(self.home.clone(), name("actual-udf")).unwrap();
        if durable {
            HeldContext::capture_durable(desk, udf, SnapshotLimits::default()).unwrap()
        } else {
            HeldContext::capture(desk, udf, SnapshotLimits::default()).unwrap()
        }
    }
    fn fill(&self) {
        std::fs::write(
            self.temporary.path().join(".cc-box").join("config.json"),
            b"{}",
        )
        .unwrap();
        std::fs::write(
            self.temporary.path().join(".cc-box").join("unknown.bin"),
            b"unknown retained data",
        )
        .unwrap();
    }
}
fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: "11111111-1111-4111-8111-111111111111".into(),
        source_context: "22222222-2222-4222-8222-222222222222".into(),
        target_context: "33333333-3333-4333-8333-333333333333".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}

// 检查实际durable M0解码避免旧scope reader的WRITE共享冲突，消费后同根旋转不留下隐藏子句柄。
#[test]
fn HistoryScopeContext_DurableRotation_001() {
    let fixture = Fixture::new();
    fixture.fill();
    let inputs = ScopeInputs::capture(&fixture.temporary.path().join(".cc-box"));
    // The DELETE-capable root already excludes an independent ordinary root open.
    assert!(inputs.is_err());
    let mut context = fixture.context(true);
    let inventory = ConfiguredInventory::fixture_for_context(
        &mut context,
        fixture.temporary.path(),
        fixture.environment(),
    )
    .unwrap();
    let exclusions = inventory
        .into_exclusions(fixture.installation.clone(), fixture.recovery.clone())
        .unwrap();
    exclusions.verify_context(&context).unwrap();
    let source_location = exclusions.source_root_identity(RootKind::Desk).to_owned();
    let source_config = exclusions.configuration_identity().to_owned();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let copies = Arc::new(
        PrivateDirectory::create_new(
            fixture.recovery.directory().clone(),
            name("copies"),
            &fixture.user,
        )
        .unwrap(),
    );
    let quarantine = Arc::new(
        PrivateDirectory::create_new(
            fixture.recovery.directory().clone(),
            name("quarantine"),
            &fixture.user,
        )
        .unwrap(),
    );
    let mut store = JournalStore::open_windows(fixture.recovery.clone()).unwrap();
    store
        .initialize(
            binding(),
            CapacityPlan::for_effects(100, 100, 20, 4096).unwrap(),
        )
        .unwrap();
    let leases = LeaseFiles::open(fixture.recovery.clone(), &fixture.user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    std::fs::write(fixture.temporary.path().join("image.exe"), b"fixture image").unwrap();
    let file = fixture
        .home
        .open_file(name("image.exe"), FileAccess::Read)
        .unwrap();
    let identity = file.identity().clone();
    let digest = file.digest().unwrap();
    drop(file);
    let fence =
        ImageFence::acquire(fixture.home.clone(), name("image.exe"), &identity, &digest).unwrap();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.recovery.clone(),
        &exclusive,
        binding(),
        0,
    )
    .unwrap();
    let mut copy = PrivateTreeCopy::new(copies, name("desk-copy"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    copy.rotate_context_root(
        &mut context,
        RootKind::Desk,
        fixture.home.clone(),
        name(".cc-box"),
        quarantine,
        name("original"),
        &boundary,
        &fence,
        &fixture.user,
        &mut journal,
    )
    .unwrap();
    exclusions.verify_external().unwrap();
    assert!(exclusions.verify_context(&context).is_err());
    std::fs::create_dir(fixture.temporary.path().join(".cc-box")).unwrap();
    std::fs::write(
        fixture.temporary.path().join(".cc-box").join("config.json"),
        b"{\"later\":true}",
    )
    .unwrap();
    std::fs::create_dir(fixture.temporary.path().join("actual-udf")).unwrap();
    exclusions.verify().unwrap();
    assert_eq!(
        exclusions.source_root_identity(RootKind::Desk),
        source_location
    );
    assert_eq!(exclusions.configuration_identity(), source_config);
}

// 检查无durability、错误Desk位置、非DELETE根不能铸造生产排除能力。
#[test]
fn HistoryScopeContext_AdmissionRefusals_002() {
    let fixture = Fixture::new();
    let mut context = fixture.context(false);
    assert!(ConfiguredInventory::fixture_for_context(
        &mut context,
        fixture.temporary.path(),
        fixture.environment()
    )
    .is_err());
    drop(context);
    let mut context = fixture.context(true);
    let wrong = fixture.temporary.path().join("elsewhere");
    std::fs::create_dir(&wrong).unwrap();
    assert!(
        ConfiguredInventory::fixture_for_context(&mut context, &wrong, fixture.environment())
            .is_err()
    );
    drop(context);
    let ordinary = fixture.home.open_directory(name("installation")).unwrap();
    let mut context = HeldContext::capture_durable(
        HeldRoot::Present(ordinary),
        HeldRoot::observe(fixture.home.clone(), name("actual-udf")).unwrap(),
        SnapshotLimits::default(),
    )
    .unwrap();
    assert!(ConfiguredInventory::fixture_for_context(
        &mut context,
        fixture.temporary.path(),
        fixture.environment()
    )
    .is_err());
}

// 检查完整M0中的大小写文件名按Windows ordinal解释；损坏配置与目录冒充文件明确拒绝。
#[test]
fn HistoryScopeContext_ExactInputKinds_003() {
    for (entry, directory, contents, accepted) in [
        ("CONFIG.JSON", false, b"{}".as_slice(), true),
        ("config.json", false, b"{broken".as_slice(), false),
        ("config.json", true, b"".as_slice(), false),
    ] {
        let fixture = Fixture::new();
        let path = fixture.temporary.path().join(".cc-box").join(entry);
        if directory {
            std::fs::create_dir(path).unwrap();
        } else {
            std::fs::write(path, contents).unwrap();
        }
        let mut context = fixture.context(true);
        assert_eq!(
            ConfiguredInventory::fixture_for_context(
                &mut context,
                fixture.temporary.path(),
                fixture.environment()
            )
            .is_ok(),
            accepted
        );
    }
}

// 检查配置/Legacy派生外部guard包含精确缺失对象；后来出现或实际根碰撞会拒绝。
#[test]
fn HistoryScopeContext_ExternalDriftAndOverlap_004() {
    let fixture = Fixture::new();
    let mut context = fixture.context(true);
    let inventory = ConfiguredInventory::fixture_for_context(
        &mut context,
        fixture.temporary.path(),
        fixture.environment(),
    )
    .unwrap();
    let exclusions = inventory
        .into_exclusions(fixture.installation.clone(), fixture.recovery.clone())
        .unwrap();
    std::fs::create_dir(fixture.temporary.path().join(".claude")).unwrap();
    assert_eq!(
        exclusions.verify_external().err(),
        Some(ScopeBlock::InputChanged)
    );
    drop(context);
    let fixture = Fixture::new();
    let config = serde_json::json!({"claudeEnvVars":{"CLAUDE_CONFIG_DIR":fixture.temporary.path().join("recovery")}});
    std::fs::write(
        fixture.temporary.path().join(".cc-box").join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let mut context = fixture.context(true);
    let inventory = ConfiguredInventory::fixture_for_context(
        &mut context,
        fixture.temporary.path(),
        fixture.environment(),
    )
    .unwrap();
    assert_eq!(
        inventory
            .into_exclusions(fixture.installation.clone(), fixture.recovery.clone())
            .err()
            .unwrap(),
        ScopeBlock::Overlap
    );
}

// 检查原Desk完全缺失的M0绑定；消费之前新增根不能把旧absence当成当前证据。
#[test]
fn HistoryScopeContext_AbsenceAndStaleM0_005() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let home = Directory::open_absolute(temporary.path()).unwrap();
    let recovery =
        Arc::new(PrivateDirectory::create_new(home.clone(), name("recovery"), &user).unwrap());
    std::fs::create_dir(temporary.path().join("installation")).unwrap();
    let installation = home.open_directory(name("installation")).unwrap();
    let env = EnvMap::from([(
        OsString::from("USERPROFILE"),
        temporary.path().as_os_str().to_owned(),
    )]);
    for stale in [false, true] {
        let mut context = HeldContext::capture_durable(
            HeldRoot::observe(home.clone(), name(".cc-box")).unwrap(),
            HeldRoot::observe(home.clone(), name("udf")).unwrap(),
            SnapshotLimits::default(),
        )
        .unwrap();
        let inventory =
            ConfiguredInventory::fixture_for_context(&mut context, temporary.path(), env.clone())
                .unwrap();
        if stale {
            std::fs::create_dir(temporary.path().join(".cc-box")).unwrap();
        }
        let result = inventory.into_exclusions(installation.clone(), recovery.clone());
        assert_eq!(result.is_err(), stale);
    }
}

// 检查同内容的另一完整context不能替代已消费的实际M0，新输入出现也会撤销借用的admission。
#[test]
fn HistoryScopeContext_ForeignContextAndNewInput_006() {
    let fixture = Fixture::new();
    let mut context = fixture.context(true);
    let inventory = ConfiguredInventory::fixture_for_context(
        &mut context,
        fixture.temporary.path(),
        fixture.environment(),
    )
    .unwrap();
    let exclusions = inventory
        .into_exclusions(fixture.installation.clone(), fixture.recovery.clone())
        .unwrap();
    let other = Fixture::new();
    let foreign = other.context(true);
    assert!(exclusions.verify_context(&foreign).is_err());
    exclusions.verify_context(&context).unwrap();
    let inventory = ConfiguredInventory::fixture_for_context(
        &mut context,
        fixture.temporary.path(),
        fixture.environment(),
    )
    .unwrap();
    std::fs::write(
        fixture
            .temporary
            .path()
            .join(".cc-box")
            .join("projects.json"),
        b"{}",
    )
    .unwrap();
    assert_eq!(inventory.recheck().err(), Some(ScopeBlock::InputChanged));
    assert!(inventory
        .into_exclusions(fixture.installation.clone(), fixture.recovery.clone())
        .is_err());
}
