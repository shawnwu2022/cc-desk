//! Actual local NTFS capture/copy/re-admission probes. Only disposable fixture
//! directories are affected; these are not installed-application acceptance.
use crate::version_history::{
    journal::{CapacityPlan, JournalBinding, JournalStore, RootKind},
    maintenance::SnapshotBoundary,
    snapshot::{capture_context, ContextReader, SnapshotLimits},
    windows::{
        context::{
            probe_after_guard_release, probe_after_root_rotation, probe_copy_failure,
            ContextJournal, CopyFault, HeldBundle, HeldContext, HeldRoot, PrivateTreeCopy,
        },
        fence::ImageFence,
        files::{ComponentName, Directory, FileAccess, PrivateDirectory},
        lease::{ExclusiveLease, LeaseFiles},
        security::CurrentUser,
    },
};
use parking_lot::Mutex;
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path, sync::Arc};

fn name(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
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
struct Fixture {
    temp: tempfile::TempDir,
    user: CurrentUser,
    parent: Arc<Directory>,
    source: Arc<PrivateDirectory>,
    records: Arc<PrivateDirectory>,
    copies: Arc<PrivateDirectory>,
    quarantine: Arc<PrivateDirectory>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let source = Arc::new(
            PrivateDirectory::create_renameable_new(parent.clone(), name("desk"), &user).unwrap(),
        );
        let records =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("records"), &user).unwrap());
        let copies =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("copies"), &user).unwrap());
        let quarantine = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("quarantine"), &user).unwrap(),
        );
        Self {
            temp,
            user,
            parent,
            source,
            records,
            copies,
            quarantine,
        }
    }
    fn context(&self) -> HeldContext {
        HeldContext::capture(
            HeldRoot::Present(self.source.directory().clone()),
            HeldRoot::observe(self.parent.clone(), name("udf")).unwrap(),
            SnapshotLimits::default(),
        )
        .unwrap()
    }
    fn journal(&self) -> JournalStore {
        let mut journal = JournalStore::open_windows(self.records.clone()).unwrap();
        journal
            .initialize(
                binding(),
                CapacityPlan::for_effects(100, 100, 20, 4096).unwrap(),
            )
            .unwrap();
        journal
    }
    fn lease(&self) -> ExclusiveLease {
        let files = LeaseFiles::open(self.records.clone(), &self.user).unwrap();
        let control = files.acquire_control().unwrap();
        files.acquire_exclusive(&control).unwrap()
    }
    fn image(&self) -> ImageFence {
        std::fs::write(self.temp.path().join("image.exe"), b"fenced fixture image").unwrap();
        let file = self
            .parent
            .open_file(name("image.exe"), FileAccess::Read)
            .unwrap();
        let identity = file.identity().clone();
        let digest = file.digest().unwrap();
        drop(file);
        ImageFence::acquire(self.parent.clone(), name("image.exe"), &identity, &digest).unwrap()
    }
    fn fill(&self) {
        std::fs::create_dir_all(self.temp.path().join("desk/disabled/skills")).unwrap();
        std::fs::create_dir(self.temp.path().join("desk/empty")).unwrap();
        std::fs::write(
            self.temp.path().join("desk/providers.json"),
            b"private provider settings",
        )
        .unwrap();
        std::fs::write(
            self.temp.path().join("desk/disabled/skills/用户.md"),
            b"user authored disabled skill",
        )
        .unwrap();
        std::fs::write(self.temp.path().join("desk/unknown.bin"), [0, 1, 255]).unwrap();
    }
}
fn set_dacl(path: &Path, sddl: &str) {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SetFileSecurityW,
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
    };
    use windows_core::PCWSTR;
    let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            1,
            &mut descriptor,
            None,
        )
        .unwrap();
        assert!(SetFileSecurityW(
            PCWSTR(path.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor
        )
        .as_bool());
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
}
fn source_acl(user: &CurrentUser, directory: bool, extra: &str) -> String {
    let flags = if directory { "OICI" } else { "" };
    format!(
        "D:P(A;{flags};FA;;;{})(A;{flags};FA;;;SY)(A;{flags};FA;;;BA){extra}",
        user.sid_text()
    )
}

// 检查完整实际句柄捕获不筛除 provider、禁用内容、未知文件和空目录，并保留 UDF 缺失。
#[test]
fn HistoryContextWindows_CompleteCapture_001() {
    let fixture = Fixture::new();
    fixture.fill();
    let mut context = fixture.context();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let manifest = capture_context(
        &boundary,
        &binding().source_context,
        &mut context,
        SnapshotLimits::default(),
    )
    .unwrap();
    let names: Vec<_> = manifest.roots[0]
        .entries
        .iter()
        .map(|entry| entry.metadata.path.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "",
            "disabled",
            "disabled/skills",
            "disabled/skills/用户.md",
            "empty",
            "providers.json",
            "unknown.bin"
        ]
    );
    assert!(manifest.roots[1].entries.is_empty());
    context.verify().unwrap();
    assert!(std::fs::write(
        fixture.temp.path().join("desk/providers.json"),
        b"overwritten"
    )
    .is_err());
}

// 检查 source 与 copy 有不同对象身份和独立私有 ACL，原权限映射仍完整保留。
#[test]
fn HistoryContextWindows_PrivateCompleteCopy_002() {
    let fixture = Fixture::new();
    fixture.fill();
    let original_acl = source_acl(&fixture.user, false, "");
    set_dacl(
        &fixture.temp.path().join("desk/providers.json"),
        &original_acl,
    );
    let context = fixture.context();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("desk-backup"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    copy.verify(&fixture.user).unwrap();
    let manifest = copy.manifest().unwrap();
    for (source, copied) in manifest.source.entries.iter().zip(&manifest.copy.entries) {
        assert_eq!(source.metadata.path, copied.metadata.path);
        assert_eq!(source.sha256, copied.sha256);
        assert_ne!(
            source.metadata.object_identity,
            copied.metadata.object_identity
        );
    }
    let original = manifest
        .source
        .entries
        .iter()
        .find(|entry| entry.metadata.path == "providers.json")
        .unwrap();
    let retained = manifest
        .copy
        .entries
        .iter()
        .find(|entry| entry.metadata.path == "providers.json")
        .unwrap();
    assert_ne!(original.metadata.permissions, retained.metadata.permissions);
    assert_eq!(
        std::fs::read(
            fixture
                .temp
                .path()
                .join("copies/desk-backup/disabled/skills/用户.md")
        )
        .unwrap(),
        b"user authored disabled skill"
    );
    assert!(journal.generation() >= 14);
}

// 检查空目录与不存在根保持不同语义，复制不存在根不会创建空目录代替。
#[test]
fn HistoryContextWindows_EmptyAndAbsent_003() {
    let fixture = Fixture::new();
    let context = fixture.context();
    assert_eq!(context.tree(RootKind::Desk).manifest().entries.len(), 1);
    assert!(context
        .tree(RootKind::WebView)
        .manifest()
        .entries
        .is_empty());
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut empty = PrivateTreeCopy::new(fixture.copies.clone(), name("empty"));
    empty
        .copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    let mut absent = PrivateTreeCopy::new(fixture.copies.clone(), name("missing"));
    absent
        .copy_from(context.tree(RootKind::WebView), &fixture.user, &mut journal)
        .unwrap();
    assert!(fixture.temp.path().join("copies/empty").is_dir());
    assert!(!fixture.temp.path().join("copies/missing").exists());
    absent.verify(&fixture.user).unwrap();
    std::fs::create_dir(fixture.temp.path().join("udf")).unwrap();
    assert!(context.verify().is_err());
}

// 检查字节/数量预算、硬链接和命名数据流均阻止整个捕获，不产生删减清单。
#[test]
fn HistoryContextWindows_UnsupportedAndBounded_004() {
    for variant in [
        "bytes",
        "entries",
        "hardlink",
        "stream",
        "overlap",
        "wrong-type",
    ] {
        let mut fixture = Fixture::new();
        fixture.fill();
        let mut limits = SnapshotLimits::default();
        let hardlink_identity = (variant == "hardlink").then(|| {
            fixture
                .source
                .directory()
                .open_file(name("unknown.bin"), FileAccess::Read)
                .unwrap()
                .identity()
                .clone()
        });
        match variant {
            "bytes" => limits.max_bytes = 1,
            "entries" => limits.max_entries = 2,
            "hardlink" => {
                // Construct the hostile fixture before pinning its rotating
                // parent. The DELETE-capable root guard conflicts with Win32's
                // hardlink setup open; rejection must come from capture itself.
                let expected_root = fixture.source.directory().identity().clone();
                let rotating = std::mem::replace(&mut fixture.source, fixture.records.clone());
                drop(rotating);
                std::fs::hard_link(
                    fixture.temp.path().join("desk/unknown.bin"),
                    fixture.temp.path().join("desk/alias"),
                )
                .unwrap();
                fixture.source = Arc::new(
                    PrivateDirectory::open_existing(
                        fixture.parent.clone(),
                        name("desk"),
                        &fixture.user,
                    )
                    .unwrap(),
                );
                assert_eq!(fixture.source.directory().identity(), &expected_root);
            }
            "stream" => std::fs::write(
                fixture.temp.path().join("desk/unknown.bin:retained-data"),
                b"cannot omit",
            )
            .unwrap(),
            "wrong-type" => {
                std::fs::write(fixture.temp.path().join("udf"), b"file is not absence").unwrap()
            }
            _ => (),
        }
        let udf = if variant == "overlap" {
            Ok(HeldRoot::Present(fixture.source.directory().clone()))
        } else {
            HeldRoot::observe(fixture.parent.clone(), name("udf"))
        };
        let result = udf.and_then(|udf| {
            HeldContext::capture(
                HeldRoot::Present(fixture.source.directory().clone()),
                udf,
                limits,
            )
        });
        assert!(result.is_err(), "{variant}");
        drop(result);
        if let Some(expected) = hardlink_identity {
            // Remove only the hostile alias created by this fixture, then
            // prove the same normal read guard admits the unchanged object.
            std::fs::remove_file(fixture.temp.path().join("desk/alias")).unwrap();
            let admitted = fixture.context();
            admitted.verify().unwrap();
            let file = fixture
                .source
                .directory()
                .open_file(name("unknown.bin"), FileAccess::Read)
                .unwrap();
            assert_eq!(file.identity(), &expected);
        }
        assert_eq!(
            std::fs::read(fixture.temp.path().join("desk/unknown.bin")).unwrap(),
            [0, 1, 255]
        );
    }
}

// 检查新增项和 ACL 漂移使既有 held inventory 失效。
#[test]
fn HistoryContextWindows_InventoryAndAclDrift_005() {
    for variant in ["entry", "acl"] {
        let fixture = Fixture::new();
        fixture.fill();
        let mut context = fixture.context();
        match variant {
            "entry" => std::fs::write(fixture.temp.path().join("desk/added"), b"new data").unwrap(),
            _ => set_dacl(
                &fixture.temp.path().join("desk/providers.json"),
                &source_acl(&fixture.user, false, ""),
            ),
        }
        assert!(context.inventory().is_err(), "{variant}");
        assert!(fixture.temp.path().join("desk/providers.json").exists());
    }
}

// 检查每个复制故障保留源和已创建私有对象，未知回执不允许自动重试。
#[test]
fn HistoryContextWindows_CopyFailurePreserves_006() {
    for fault in [
        CopyFault::AfterWrite,
        CopyFault::AfterFlush,
        CopyFault::BeforeReceipt,
    ] {
        let fixture = Fixture::new();
        fixture.fill();
        let context = fixture.context();
        let lease = fixture.lease();
        let mut store = fixture.journal();
        let mut journal =
            ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
        let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("partial"));
        let _fault = probe_copy_failure(fault);
        assert!(copy
            .copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
            .is_err());
        assert!(copy.manifest().is_err());
        assert!(copy
            .copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
            .is_err());
        assert!(fixture.temp.path().join("copies/partial").is_dir());
        context.verify().unwrap();
        drop(journal);
        assert!(store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .requires_reconciliation());
    }
}

// 检查已占用备份目录不能覆盖，也不能删除调用方已有内容。
#[test]
fn HistoryContextWindows_CreateNewCollision_007() {
    let fixture = Fixture::new();
    fixture.fill();
    let context = fixture.context();
    std::fs::create_dir(fixture.temp.path().join("copies/occupied")).unwrap();
    std::fs::write(
        fixture.temp.path().join("copies/occupied/keep"),
        b"owned by someone else",
    )
    .unwrap();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("occupied"));
    assert!(copy
        .copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .is_err());
    assert_eq!(
        std::fs::read(fixture.temp.path().join("copies/occupied/keep")).unwrap(),
        b"owned by someone else"
    );
    context.verify().unwrap();
}

// 检查复制使用真实同源 Windows journal 和 lifetime lease，拒绝其他根及旧 generation。
#[test]
fn HistoryContextWindows_JournalLeaseBinding_008() {
    let fixture = Fixture::new();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    assert!(ContextJournal::new(&mut store, fixture.copies.clone(), &lease, binding(), 0).is_err());
    let foreign = LeaseFiles::open(fixture.copies.clone(), &fixture.user).unwrap();
    let control = foreign.acquire_control().unwrap();
    let other_lease = foreign.acquire_exclusive(&control).unwrap();
    assert!(ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &other_lease,
        binding(),
        0
    )
    .is_err());
    assert!(
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 1).is_err()
    );
    let mut other_binding = binding();
    other_binding.source_bundle = "a".repeat(64);
    assert!(ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        other_binding,
        0
    )
    .is_err());
}

// 检查独立 bundle 类型通过同一独占映像句柄读取，并保留所有伴随文件。
#[test]
fn HistoryContextWindows_FencedBundle_009() {
    let fixture = Fixture::new();
    let bundle_path = fixture.temp.path().join("bundle");
    std::fs::create_dir(&bundle_path).unwrap();
    for (name, bytes) in [
        ("cc-desk.exe", "executable"),
        ("conpty.dll", "conpty"),
        ("OpenConsole.exe", "host"),
        ("unknown-companion", "unknown"),
    ] {
        std::fs::write(bundle_path.join(name), bytes).unwrap();
    }
    let root = fixture.parent.open_directory(name("bundle")).unwrap();
    let image = root
        .open_file(name("cc-desk.exe"), FileAccess::Read)
        .unwrap();
    let id = image.identity().clone();
    let digest = image.digest().unwrap();
    drop(image);
    let fence = Arc::new(Mutex::new(
        ImageFence::acquire(root.clone(), name("cc-desk.exe"), &id, &digest).unwrap(),
    ));
    let bundle = HeldBundle::capture(
        root.clone(),
        name("cc-desk.exe"),
        fence.clone(),
        SnapshotLimits::default(),
    )
    .unwrap();
    assert_eq!(bundle.manifest().tree.entries.len(), 5);
    assert!(root
        .open_file(name("cc-desk.exe"), FileAccess::Read)
        .is_err());
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("bundle"));
    copy.copy_from(bundle.tree(), &fixture.user, &mut journal)
        .unwrap();
    copy.verify(&fixture.user).unwrap();
    assert_eq!(
        std::fs::read(fixture.temp.path().join("copies/bundle/unknown-companion")).unwrap(),
        b"unknown"
    );
    fence.lock().verify().unwrap();
}

// 检查生产复制后释放子句柄、同根改名、完整重新接纳的成功路径，以及字节/新增项/ACL/目标冲突失败均保留状态。
#[test]
fn HistoryContextWindows_RotationReadmission_010() {
    for variant in [
        "same",
        "bytes",
        "entry",
        "acl",
        "quarantine-collision",
        "fresh-collision",
    ] {
        let fixture = Fixture::new();
        fixture.fill();
        let mut context = fixture.context();
        let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
        let lease = fixture.lease();
        let mut store = fixture.journal();
        let image = fixture.image();
        let mut journal =
            ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
        let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("desk-backup"));
        copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
            .unwrap();
        let before = copy.manifest().unwrap().source.clone();
        let path = fixture.temp.path().to_owned();
        let acl = source_acl(&fixture.user, false, "");
        let _probe = probe_after_guard_release(move || match variant {
            "bytes" => std::fs::write(
                path.join("desk/providers.json"),
                b"new bytes during the gap",
            )
            .unwrap(),
            "entry" => std::fs::write(path.join("desk/new-entry"), b"new during the gap").unwrap(),
            "acl" => set_dacl(&path.join("desk/providers.json"), &acl),
            "quarantine-collision" => {
                std::fs::create_dir(path.join("quarantine/retained")).unwrap();
                std::fs::write(path.join("quarantine/retained/keep"), b"other owner").unwrap();
            }
            _ => (),
        });
        let path = fixture.temp.path().to_owned();
        let _fresh = if variant == "fresh-collision" {
            Some(probe_after_root_rotation(move || {
                std::fs::create_dir(path.join("desk")).unwrap();
                std::fs::write(path.join("desk/keep"), b"late user work").unwrap();
            }))
        } else {
            None
        };
        let result = copy.rotate_context_root(
            &mut context,
            RootKind::Desk,
            fixture.parent.clone(),
            name("desk"),
            fixture.quarantine.clone(),
            name("retained"),
            &boundary,
            &image,
            &fixture.user,
            &mut journal,
        );
        if variant == "same" {
            let proof = result.unwrap();
            proof.verify(&context, &copy, &fixture.user).unwrap();
            assert_eq!(proof.root(), RootKind::Desk);
            assert_eq!(proof.receipt_manifest().len(), 64);
            assert!(!fixture.temp.path().join("desk").exists());
            assert_eq!(
                before.entries,
                context.tree(RootKind::Desk).manifest().entries
            );
            assert_ne!(
                before.location_identity,
                context.tree(RootKind::Desk).manifest().location_identity
            );
        } else {
            assert!(result.is_err(), "{variant}");
            assert!(copy
                .rotate_context_root(
                    &mut context,
                    RootKind::Desk,
                    fixture.parent.clone(),
                    name("desk"),
                    fixture.quarantine.clone(),
                    name("retained"),
                    &boundary,
                    &image,
                    &fixture.user,
                    &mut journal
                )
                .is_err());
            if variant == "fresh-collision" {
                assert_eq!(
                    std::fs::read(fixture.temp.path().join("desk/keep")).unwrap(),
                    b"late user work"
                );
            }
            if variant == "quarantine-collision" {
                assert_eq!(
                    std::fs::read(fixture.temp.path().join("quarantine/retained/keep")).unwrap(),
                    b"other owner"
                );
                assert!(fixture.temp.path().join("desk/providers.json").exists());
            } else {
                assert!(fixture
                    .temp
                    .path()
                    .join("quarantine/retained/providers.json")
                    .exists());
            }
        }
        copy.verify(&fixture.user).unwrap();
        image.verify().unwrap();
        assert_eq!(
            std::fs::read(
                fixture
                    .temp
                    .path()
                    .join("copies/desk-backup/providers.json")
            )
            .unwrap(),
            b"private provider settings"
        );
    }
}

// 检查 normal 用户/SYSTEM/Administrators 原权限和正确的仅继承 Creator Owner 可以保留，其他主体有效授权会在旋转前拒绝。
#[test]
fn HistoryContextWindows_SourceConfidentiality_011() {
    for variant in [
        "system-admin",
        "creator-owner-inherit-only",
        "world-effective",
        "creator-owner-effective",
        "world-inherit-only",
    ] {
        let fixture = Fixture::new();
        fixture.fill();
        let extra = match variant {
            "creator-owner-inherit-only" => "(A;OICIIO;FA;;;CO)",
            "world-effective" => "(A;;FR;;;WD)",
            "creator-owner-effective" => "(A;;FR;;;CO)",
            "world-inherit-only" => "(A;OICIIO;FR;;;WD)",
            _ => "",
        };
        set_dacl(
            &fixture.temp.path().join("desk"),
            &source_acl(&fixture.user, true, extra),
        );
        // Apply a normal protected source-file ACL; it differs from the backup's
        // sole-current-user ACL, and is retained byte-for-byte in source metadata.
        set_dacl(
            &fixture.temp.path().join("desk/providers.json"),
            &source_acl(&fixture.user, false, ""),
        );
        let mut context = fixture.context();
        let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
        let lease = fixture.lease();
        let mut store = fixture.journal();
        let image = fixture.image();
        let mut journal =
            ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
        let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("private-backup"));
        copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
            .unwrap();
        let generation = journal.generation();
        let result = copy.rotate_context_root(
            &mut context,
            RootKind::Desk,
            fixture.parent.clone(),
            name("desk"),
            fixture.quarantine.clone(),
            name("retained"),
            &boundary,
            &image,
            &fixture.user,
            &mut journal,
        );
        if matches!(variant, "system-admin" | "creator-owner-inherit-only") {
            result
                .unwrap()
                .verify(&context, &copy, &fixture.user)
                .unwrap();
        } else {
            assert!(result.is_err(), "{variant}");
            assert_eq!(
                journal.generation(),
                generation,
                "must block before rotation intent"
            );
            assert!(fixture.temp.path().join("desk/providers.json").exists());
            assert!(!fixture.temp.path().join("quarantine/retained").exists());
        }
        copy.verify(&fixture.user).unwrap();
    }
}

// 检查私有副本 ACL 漂移即使没有字节改变仍拒绝验证，不能用原权限覆盖 copy 权限。
#[test]
fn HistoryContextWindows_CopyAclDrift_012() {
    let fixture = Fixture::new();
    fixture.fill();
    let context = fixture.context();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("backup"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    set_dacl(
        &fixture.temp.path().join("copies/backup/providers.json"),
        &source_acl(&fixture.user, false, "(A;;FR;;;WD)"),
    );
    assert!(copy.verify(&fixture.user).is_err());
    assert_eq!(
        std::fs::read(fixture.temp.path().join("copies/backup/providers.json")).unwrap(),
        b"private provider settings"
    );
    context.verify().unwrap();
}

// 检查实际映像已经隔离后，bundle 仍通过同一 fence 捕获原逻辑文件；原位置的新文件不能被采纳。
#[test]
fn HistoryContextWindows_BundleAfterImageQuarantine_013() {
    for collision in [false, true] {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.temp.path().join("bundle")).unwrap();
        std::fs::write(
            fixture.temp.path().join("bundle/cc-desk.exe"),
            b"original image",
        )
        .unwrap();
        std::fs::write(
            fixture.temp.path().join("bundle/unknown"),
            b"original companion",
        )
        .unwrap();
        let root = fixture.parent.open_directory(name("bundle")).unwrap();
        let image = root
            .open_file(name("cc-desk.exe"), FileAccess::Read)
            .unwrap();
        let id = image.identity().clone();
        let digest = image.digest().unwrap();
        drop(image);
        let mut fence =
            ImageFence::acquire(root.clone(), name("cc-desk.exe"), &id, &digest).unwrap();
        fence
            .rename_to(
                fixture.quarantine.directory().clone(),
                name("sealed-image.exe"),
            )
            .unwrap();
        if collision {
            std::fs::write(
                fixture.temp.path().join("bundle/CC-DESK.EXE"),
                b"foreign replacement",
            )
            .unwrap();
        }
        let fence = Arc::new(Mutex::new(fence));
        let result = HeldBundle::capture(
            root,
            name("cc-desk.exe"),
            fence.clone(),
            SnapshotLimits::default(),
        );
        if collision {
            assert!(result.is_err());
        } else {
            let bundle = result.unwrap();
            assert_eq!(bundle.manifest().tree.entries.len(), 3);
            let lease = fixture.lease();
            let mut store = fixture.journal();
            let mut journal =
                ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0)
                    .unwrap();
            let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("bundle"));
            copy.copy_from(bundle.tree(), &fixture.user, &mut journal)
                .unwrap();
            assert_eq!(
                std::fs::read(fixture.temp.path().join("copies/bundle/cc-desk.exe")).unwrap(),
                b"original image"
            );
            copy.verify(&fixture.user).unwrap();
        }
        fence.lock().verify().unwrap();
    }
}

// 检查两个缺失根的父目录可以互为祖先，只要完整 prospective 位置不重叠。
#[test]
fn HistoryContextWindows_NestedAbsentParents_014() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.temp.path().join("local").join("app")).unwrap();
    let nested = fixture
        .parent
        .open_directory(name("local"))
        .unwrap()
        .open_directory(name("app"))
        .unwrap();
    let mut context = HeldContext::capture(
        HeldRoot::observe(fixture.parent.clone(), name("absent-desk")).unwrap(),
        HeldRoot::observe(nested, name("absent-udf")).unwrap(),
        SnapshotLimits::default(),
    )
    .unwrap();
    assert!(context
        .inventory()
        .unwrap()
        .iter()
        .all(|root| root.entries.is_empty()));
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    capture_context(
        &boundary,
        &binding().source_context,
        &mut context,
        SnapshotLimits::default(),
    )
    .unwrap();
}

// 检查复制的完整 dependency/record 预算在首个创建前拒绝，并保留独立恢复容量。
#[test]
fn HistoryContextWindows_CopyCapacityBeforeEffects_015() {
    for limit in [159, 160] {
        let fixture = Fixture::new();
        std::fs::write(fixture.temp.path().join("desk/data"), b"kept").unwrap();
        let context = fixture.context();
        let lease = fixture.lease();
        let mut store =
            JournalStore::fixture_windows_dependency_limit(fixture.records.clone(), limit).unwrap();
        store
            .initialize(
                binding(),
                CapacityPlan::for_effects(100, 100, 20, 4096).unwrap(),
            )
            .unwrap();
        let mut journal =
            ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
        let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("bounded"));
        let result = copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal);
        if limit == 159 {
            assert!(result.is_err());
            assert_eq!(journal.generation(), 0);
            assert!(!fixture.temp.path().join("copies/bounded").exists());
        } else {
            result.unwrap();
            copy.verify(&fixture.user).unwrap();
        }
        context.verify().unwrap();
    }
}

fn fixture_abort(store: &mut JournalStore, context: &HeldContext, image: &ImageFence) {
    use crate::version_history::journal::{JournalPhase, PreContextAbortProof};
    context.verify().unwrap();
    image.verify().unwrap();
    // The terminal registration/bundle/quiescence factory is explicitly mocked
    // here. Actual root/copy/reverse and Windows journal IO remain production.
    let digests = [
        store.retain_manifest(b"fixture unchanged bundle").unwrap(),
        store
            .retain_manifest(&context.tree(RootKind::Desk).manifest().encode().unwrap())
            .unwrap(),
        store
            .retain_manifest(b"fixture unchanged registration")
            .unwrap(),
        store
            .retain_manifest(b"fixture positive quiescence")
            .unwrap(),
    ];
    let inspection = store.inspect(&binding()).unwrap();
    let proof = PreContextAbortProof::fixture(&inspection, digests);
    store.abort_pre_context(&proof).unwrap();
    assert_eq!(
        store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .phase(),
        JournalPhase::PreContextAborted
    );
}

// 检查完成或未知的私有副本均可通过完整 abort proof 终止，部分备份保留且 Unknown 不被伪造为 Applied。
#[test]
fn HistoryContextWindows_CopyOnlyAbort_016() {
    for failed in [false, true] {
        let fixture = Fixture::new();
        fixture.fill();
        let context = fixture.context();
        let image = fixture.image();
        let lease = fixture.lease();
        let mut store = fixture.journal();
        let mut journal =
            ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
        let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("retained"));
        let _failure = failed.then(|| probe_copy_failure(CopyFault::AfterWrite));
        assert_eq!(
            copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
                .is_err(),
            failed
        );
        drop(journal);
        let before = store.inspect(&binding()).unwrap();
        let pending = before
            .last_valid
            .as_ref()
            .unwrap()
            .pending_effect()
            .map(|effect| effect.effect_id.clone());
        fixture_abort(&mut store, &context, &image);
        if let Some(id) = pending {
            assert_eq!(
                store
                    .inspect(&binding())
                    .unwrap()
                    .last_valid
                    .unwrap()
                    .effect_observation(&id),
                Some(crate::version_history::journal::Observation::Unknown)
            );
        }
        assert!(fixture.temp.path().join("copies/retained").is_dir());
        assert_eq!(
            std::fs::read(fixture.temp.path().join("desk/providers.json")).unwrap(),
            b"private provider settings"
        );
    }
}

// 检查旋转发生字节漂移后，恢复移动实际当前根回原位置，保留 C0 和新增字节而不覆盖为旧版本。
#[test]
fn HistoryContextWindows_ReversePreservesCurrentData_017() {
    let fixture = Fixture::new();
    fixture.fill();
    let mut context = fixture.context();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let image = fixture.image();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("backup"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    let path = fixture.temp.path().join("desk/providers.json");
    let _probe = probe_after_guard_release(move || {
        std::fs::write(path, b"new user data in the gap").unwrap()
    });
    assert!(copy
        .rotate_context_root(
            &mut context,
            RootKind::Desk,
            fixture.parent.clone(),
            name("desk"),
            fixture.quarantine.clone(),
            name("retained"),
            &boundary,
            &image,
            &fixture.user,
            &mut journal
        )
        .is_err());
    let returned = copy
        .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    returned.verify(&context).unwrap();
    assert_eq!(
        std::fs::read(fixture.temp.path().join("desk/providers.json")).unwrap(),
        b"new user data in the gap"
    );
    assert_eq!(
        std::fs::read(fixture.temp.path().join("copies/backup/providers.json")).unwrap(),
        b"private provider settings"
    );
    drop(journal);
    fixture_abort(&mut store, &context, &image);
}

// 检查 inverse rename 后回执丢失可通过原位同对象的正向重新观测完成，绝不再执行 rename。
#[test]
fn HistoryContextWindows_ReverseReceiptLoss_018() {
    let fixture = Fixture::new();
    fixture.fill();
    let mut context = fixture.context();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let image = fixture.image();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("backup"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    copy.rotate_context_root(
        &mut context,
        RootKind::Desk,
        fixture.parent.clone(),
        name("desk"),
        fixture.quarantine.clone(),
        name("retained"),
        &boundary,
        &image,
        &fixture.user,
        &mut journal,
    )
    .unwrap();
    let _failure = probe_copy_failure(CopyFault::AfterReverseMove);
    assert!(copy
        .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
        .is_err());
    assert!(fixture.temp.path().join("desk/providers.json").exists());
    copy.reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
        .unwrap()
        .verify(&context)
        .unwrap();
    drop(journal);
    fixture_abort(&mut store, &context, &image);
}

// 检查只读早期 bundle/备份允许持源 shared lease，而旋转必须等待新的 exclusive lease。
#[test]
fn HistoryContextWindows_PrecommitCopyOnly_019() {
    let fixture = Fixture::new();
    fixture.fill();
    let mut context = fixture.context();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let image = fixture.image();
    let files = LeaseFiles::open(fixture.records.clone(), &fixture.user).unwrap();
    let control = files.acquire_control().unwrap();
    let shared = files.acquire_shared(&control).unwrap();
    assert!(files.acquire_exclusive(&control).is_err());
    let mut store = fixture.journal();
    let mut journal = ContextJournal::new_precommit(
        &mut store,
        fixture.records.clone(),
        &control,
        &shared,
        binding(),
        0,
    )
    .unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("precommit"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    let generation = journal.generation();
    assert!(copy
        .rotate_context_root(
            &mut context,
            RootKind::Desk,
            fixture.parent.clone(),
            name("desk"),
            fixture.quarantine.clone(),
            name("retained"),
            &boundary,
            &image,
            &fixture.user,
            &mut journal
        )
        .is_err());
    assert_eq!(journal.generation(), generation);
    copy.verify(&fixture.user).unwrap();
    context.verify().unwrap();
}

// 检查正常无 DELETE 的目录 guard 在写入旋转意图/释放子级 guard 前即被拒绝。
#[test]
fn HistoryContextWindows_RenameCapabilityPreflight_020() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.temp.path().join("ordinary")).unwrap();
    std::fs::write(fixture.temp.path().join("ordinary/data"), b"original").unwrap();
    let ordinary = fixture.parent.open_directory(name("ordinary")).unwrap();
    let mut context = HeldContext::capture(
        HeldRoot::Present(ordinary),
        HeldRoot::observe(fixture.parent.clone(), name("udf")).unwrap(),
        SnapshotLimits::default(),
    )
    .unwrap();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let image = fixture.image();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("ordinary"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    let generation = journal.generation();
    assert!(copy
        .rotate_context_root(
            &mut context,
            RootKind::Desk,
            fixture.parent.clone(),
            name("ordinary"),
            fixture.quarantine.clone(),
            name("retained"),
            &boundary,
            &image,
            &fixture.user,
            &mut journal
        )
        .is_err());
    assert_eq!(journal.generation(), generation);
    context.verify().unwrap();
}

// 检查 journal 及私有副本重新打开后仍只能逆转同一已记录根，不能换父目录或重放前向操作。
#[test]
fn HistoryContextWindows_ReopenRotation_021() {
    let fixture = Fixture::new();
    fixture.fill();
    let mut context = fixture.context();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let image = fixture.image();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("backup"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    let path = fixture.temp.path().join("desk/new");
    let _probe =
        probe_after_guard_release(move || std::fs::write(path, b"new retained data").unwrap());
    assert!(copy
        .rotate_context_root(
            &mut context,
            RootKind::Desk,
            fixture.parent.clone(),
            name("desk"),
            fixture.quarantine.clone(),
            name("retained"),
            &boundary,
            &image,
            &fixture.user,
            &mut journal
        )
        .is_err());
    let generation = journal.generation();
    drop(journal);
    let effect_id = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .pending_effect()
        .unwrap()
        .effect_id
        .clone();
    drop(copy);
    drop(store);
    let mut store = JournalStore::open_windows(fixture.records.clone()).unwrap();
    store.bind_existing(&binding()).unwrap();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    assert!(PrivateTreeCopy::reopen_rotation(
        fixture.copies.clone(),
        fixture.quarantine.directory().clone(),
        fixture.quarantine.clone(),
        &effect_id,
        &fixture.user,
        SnapshotLimits::default(),
        &mut journal
    )
    .is_err());
    let mut reopened = PrivateTreeCopy::reopen_rotation(
        fixture.copies.clone(),
        fixture.parent.clone(),
        fixture.quarantine.clone(),
        &effect_id,
        &fixture.user,
        SnapshotLimits::default(),
        &mut journal,
    )
    .unwrap();
    reopened
        .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
        .unwrap()
        .verify(&context)
        .unwrap();
    assert_eq!(
        std::fs::read(fixture.temp.path().join("desk/new")).unwrap(),
        b"new retained data"
    );
    drop(journal);
    fixture_abort(&mut store, &context, &image);
}

// 检查两个源根可逐个返回；其中一个原位冲突时也不能覆盖它，另一个可安全恢复。
#[test]
fn HistoryContextWindows_TwoRootReverse_022() {
    for collision in [false, true] {
        let fixture = Fixture::new();
        fixture.fill();
        let udf = PrivateDirectory::create_renameable_new(
            fixture.parent.clone(),
            name("udf"),
            &fixture.user,
        )
        .unwrap();
        std::fs::write(fixture.temp.path().join("udf/state"), b"webview original").unwrap();
        let mut context = HeldContext::capture(
            HeldRoot::Present(fixture.source.directory().clone()),
            HeldRoot::Present(udf.directory().clone()),
            SnapshotLimits::default(),
        )
        .unwrap();
        let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
        let image = fixture.image();
        let lease = fixture.lease();
        let mut store = fixture.journal();
        let mut journal =
            ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
        let mut desk_copy = PrivateTreeCopy::new(fixture.copies.clone(), name("desk-backup"));
        desk_copy
            .copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
            .unwrap();
        let mut udf_copy = PrivateTreeCopy::new(fixture.copies.clone(), name("udf-backup"));
        udf_copy
            .copy_from(context.tree(RootKind::WebView), &fixture.user, &mut journal)
            .unwrap();
        desk_copy
            .rotate_context_root(
                &mut context,
                RootKind::Desk,
                fixture.parent.clone(),
                name("desk"),
                fixture.quarantine.clone(),
                name("desk-old"),
                &boundary,
                &image,
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        let path = fixture.temp.path().join("udf/state");
        let _probe =
            probe_after_guard_release(move || std::fs::write(path, b"new webview bytes").unwrap());
        assert!(udf_copy
            .rotate_context_root(
                &mut context,
                RootKind::WebView,
                fixture.parent.clone(),
                name("udf"),
                fixture.quarantine.clone(),
                name("udf-old"),
                &boundary,
                &image,
                &fixture.user,
                &mut journal
            )
            .is_err());
        if collision {
            std::fs::create_dir(fixture.temp.path().join("udf")).unwrap();
            std::fs::write(fixture.temp.path().join("udf/keep"), b"foreign new root").unwrap();
        }
        let returned = udf_copy.reverse_context_root(
            &mut context,
            &boundary,
            &image,
            &fixture.user,
            &mut journal,
        );
        assert_eq!(returned.is_err(), collision);
        desk_copy
            .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
            .unwrap()
            .verify(&context)
            .unwrap();
        if collision {
            assert_eq!(
                std::fs::read(fixture.temp.path().join("udf/keep")).unwrap(),
                b"foreign new root"
            );
            assert_eq!(
                std::fs::read(fixture.temp.path().join("quarantine/udf-old/state")).unwrap(),
                b"new webview bytes"
            );
            drop(journal);
            let digests =
                ["a", "b", "c", "d"].map(|value| store.retain_manifest(value.as_bytes()).unwrap());
            let proof = crate::version_history::journal::PreContextAbortProof::fixture(
                &store.inspect(&binding()).unwrap(),
                digests,
            );
            assert!(store.abort_pre_context(&proof).is_err());
        } else {
            drop(journal);
            fixture_abort(&mut store, &context, &image);
        }
    }
}

fn descriptor_sid_pair(descriptor: &[u8]) -> (String, String, u16) {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::{
            Authorization::ConvertSidToStringSidW, GetSecurityDescriptorControl,
            GetSecurityDescriptorGroup, GetSecurityDescriptorOwner, PSECURITY_DESCRIPTOR, PSID,
        },
    };
    use windows_core::{BOOL, PWSTR};
    let mut aligned = vec![0u32; descriptor.len().div_ceil(4)];
    unsafe {
        std::ptr::copy_nonoverlapping(
            descriptor.as_ptr(),
            aligned.as_mut_ptr().cast(),
            descriptor.len(),
        );
    }
    unsafe {
        let descriptor = PSECURITY_DESCRIPTOR(aligned.as_mut_ptr().cast());
        let mut owner = PSID::default();
        let mut group = PSID::default();
        let mut defaulted = BOOL(0);
        GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted).unwrap();
        GetSecurityDescriptorGroup(descriptor, &mut group, &mut defaulted).unwrap();
        let mut control = 0;
        let mut revision = 0;
        GetSecurityDescriptorControl(descriptor, &mut control, &mut revision).unwrap();
        let convert = |sid| {
            let mut value = PWSTR::null();
            ConvertSidToStringSidW(sid, &mut value).unwrap();
            let text = value.to_string().unwrap();
            let _ = LocalFree(Some(HLOCAL(value.0.cast())));
            text
        };
        (convert(owner), convert(group), control)
    }
}
fn actual_sid_pair(path: &Path) -> (String, String) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::{LocalFree, HANDLE, HLOCAL},
        Security::{
            Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT},
            GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        },
    };
    use windows_core::PWSTR;
    let file = std::fs::File::open(path).unwrap();
    unsafe {
        let mut owner = PSID::default();
        let mut group = PSID::default();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION,
            Some(&mut owner),
            Some(&mut group),
            None,
            None,
            Some(&mut descriptor),
        )
        .ok()
        .unwrap();
        let convert = |sid| {
            let mut value = PWSTR::null();
            ConvertSidToStringSidW(sid, &mut value).unwrap();
            let text = value.to_string().unwrap();
            let _ = LocalFree(Some(HLOCAL(value.0.cast())));
            text
        };
        let result = (convert(owner), convert(group));
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        result
    }
}

fn token_default_owner() -> String {
    use std::{
        mem::size_of,
        os::windows::io::{FromRawHandle, OwnedHandle},
    };
    use windows::Win32::{
        Foundation::{LocalFree, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, IsValidSid, TokenOwner,
            TOKEN_OWNER, TOKEN_QUERY,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    use windows_core::PWSTR;
    unsafe {
        let mut raw = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw).unwrap();
        let _token = OwnedHandle::from_raw_handle(raw.0);
        let mut length = 0;
        let sizing = GetTokenInformation(raw, TokenOwner, None, 0, &mut length);
        assert_eq!(
            sizing.unwrap_err().code(),
            ERROR_INSUFFICIENT_BUFFER.to_hresult()
        );
        assert!((size_of::<TOKEN_OWNER>()..=65536).contains(&(length as usize)));
        let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        GetTokenInformation(
            raw,
            TokenOwner,
            Some(buffer.as_mut_ptr().cast()),
            length,
            &mut length,
        )
        .unwrap();
        let owner = &*buffer.as_ptr().cast::<TOKEN_OWNER>();
        assert!(IsValidSid(owner.Owner).as_bool());
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(owner.Owner, &mut text).unwrap();
        let result = text.to_string().unwrap();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        result
    }
}

// 检查实际新建子目录/文件继承 Creator Owner 规则后的真实 owner/group 与精确 descriptor 在复制和旋转后保持。
#[test]
fn HistoryContextWindows_InheritedOwnerGroup_023() {
    use crate::version_history::snapshot::PermissionRecord;
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.temp.path().join("desk/inherited")).unwrap();
    set_dacl(
        &fixture.temp.path().join("desk/inherited"),
        &source_acl(&fixture.user, true, "(A;OICIIO;FA;;;CO)"),
    );
    std::fs::create_dir(fixture.temp.path().join("desk/inherited/child")).unwrap();
    let file_path = fixture.temp.path().join("desk/inherited/child/data");
    std::fs::write(&file_path, b"created after inheritance was configured").unwrap();
    let (owner, group) = actual_sid_pair(&file_path);
    // Elevated Windows tokens can choose Administrators as the default object
    // owner. Compare with the actual token default, then preserve owner/group
    // and the complete inherited descriptor exactly throughout capture/rotation.
    assert_eq!(owner, token_default_owner());
    let mut context = fixture.context();
    let original = context
        .tree(RootKind::Desk)
        .manifest()
        .entries
        .iter()
        .find(|entry| entry.metadata.path == "inherited/child/data")
        .unwrap()
        .metadata
        .permissions
        .clone();
    let PermissionRecord::Windows { descriptor, .. } = &original else {
        panic!("Windows descriptor required")
    };
    let (captured_owner, captured_group, control) = descriptor_sid_pair(descriptor);
    assert_eq!((captured_owner, captured_group), (owner, group));
    assert_eq!(
        control & windows::Win32::Security::SE_DACL_PROTECTED.0,
        0,
        "new child should carry inherited permissions"
    );
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let image = fixture.image();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("backup"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    copy.rotate_context_root(
        &mut context,
        RootKind::Desk,
        fixture.parent.clone(),
        name("desk"),
        fixture.quarantine.clone(),
        name("retained"),
        &boundary,
        &image,
        &fixture.user,
        &mut journal,
    )
    .unwrap()
    .verify(&context, &copy, &fixture.user)
    .unwrap();
    let retained = context
        .tree(RootKind::Desk)
        .manifest()
        .entries
        .iter()
        .find(|entry| entry.metadata.path == "inherited/child/data")
        .unwrap();
    assert_eq!(retained.metadata.permissions, original);
}

// 检查逻辑 bundle 身份忽略新文件 ID 和根位置，但仍绑定权限、路径和字节。
#[test]
fn HistoryContextWindows_LogicalBundleIdentity_024() {
    let fixture = Fixture::new();
    let mut bundles = Vec::new();
    for directory in ["first-bundle", "second-bundle"] {
        std::fs::create_dir(fixture.temp.path().join(directory)).unwrap();
        std::fs::write(
            fixture.temp.path().join(directory).join("cc-desk.exe"),
            b"same original bytes",
        )
        .unwrap();
        let root = fixture.parent.open_directory(name(directory)).unwrap();
        let file = root
            .open_file(name("cc-desk.exe"), FileAccess::Read)
            .unwrap();
        let identity = file.identity().clone();
        let digest = file.digest().unwrap();
        drop(file);
        let fence = Arc::new(Mutex::new(
            ImageFence::acquire(root.clone(), name("cc-desk.exe"), &identity, &digest).unwrap(),
        ));
        bundles.push(
            HeldBundle::capture(root, name("cc-desk.exe"), fence, SnapshotLimits::default())
                .unwrap(),
        );
    }
    assert_ne!(bundles[0].manifest().tree, bundles[1].manifest().tree);
    assert_eq!(
        bundles[0].manifest().logical_digest().unwrap(),
        bundles[1].manifest().logical_digest().unwrap()
    );
    let mut changed = bundles[1].manifest().clone();
    changed.tree.entries[1].sha256 = Some("0".repeat(64));
    assert_ne!(
        bundles[0].manifest().logical_digest().unwrap(),
        changed.logical_digest().unwrap()
    );
}

// 检查不存在 DACL 不是普通权限记录，必须拒绝整个源捕获。
#[test]
fn HistoryContextWindows_NullAclRejected_025() {
    let fixture = Fixture::new();
    fixture.fill();
    set_dacl(
        &fixture.temp.path().join("desk/providers.json"),
        "D:NO_ACCESS_CONTROL",
    );
    assert!(HeldContext::capture(
        HeldRoot::Present(fixture.source.directory().clone()),
        HeldRoot::observe(fixture.parent.clone(), name("udf")).unwrap(),
        SnapshotLimits::default()
    )
    .is_err());
    assert_eq!(
        std::fs::read(fixture.temp.path().join("desk/providers.json")).unwrap(),
        b"private provider settings"
    );
}

// 检查 C1 写入/刷新/回执失败后，实时和重新打开恢复均保留旧部分副本与 Unknown，创建独立 C1 后返回。
#[test]
fn HistoryContextWindows_RecoveryCopyFailure_026() {
    use crate::version_history::journal::Observation;
    for reopen in [false, true] {
        for fault in [
            CopyFault::BeforeCreate,
            CopyFault::AfterWrite,
            CopyFault::AfterFlush,
            CopyFault::BeforeReceipt,
        ] {
            let fixture = Fixture::new();
            fixture.fill();
            let mut context = fixture.context();
            let boundary =
                SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
            let image = fixture.image();
            let lease = fixture.lease();
            let mut store = fixture.journal();
            let mut journal =
                ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0)
                    .unwrap();
            let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("backup"));
            copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
                .unwrap();
            let added = fixture.temp.path().join("desk/new-data");
            let _gap = probe_after_guard_release(move || {
                std::fs::write(added, b"current user bytes").unwrap()
            });
            assert!(copy
                .rotate_context_root(
                    &mut context,
                    RootKind::Desk,
                    fixture.parent.clone(),
                    name("desk"),
                    fixture.quarantine.clone(),
                    name("retained"),
                    &boundary,
                    &image,
                    &fixture.user,
                    &mut journal
                )
                .is_err());
            let generation = journal.generation();
            drop(journal);
            let rotation = store
                .inspect(&binding())
                .unwrap()
                .last_valid
                .unwrap()
                .pending_effect()
                .unwrap()
                .effect_id
                .clone();
            let mut journal = ContextJournal::new(
                &mut store,
                fixture.records.clone(),
                &lease,
                binding(),
                generation,
            )
            .unwrap();
            let failure = probe_copy_failure(fault);
            assert!(copy
                .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
                .is_err());
            drop(failure);
            let generation = journal.generation();
            drop(journal);
            let failed = store
                .inspect(&binding())
                .unwrap()
                .last_valid
                .unwrap()
                .pending_effect()
                .unwrap()
                .effect_id
                .clone();
            let (plan_generation, old_plan) =
                store.context_root_backup(&rotation).unwrap().unwrap();
            let (_, failed_generation) = store.context_pending().unwrap().unwrap();
            for wrong_generation in [false, true] {
                let mut invalid = old_plan.clone();
                invalid.prior_plan_generation = Some(plan_generation);
                invalid.preserved_manifest = Some("a".repeat(64));
                invalid.abandoned_effect = Some((failed.clone(), failed_generation));
                if wrong_generation {
                    invalid.original_intent_generation += 1;
                } else {
                    invalid.abandoned_effect.as_mut().unwrap().1 += 1;
                }
                let event = crate::version_history::journal::JournalEvent::PrepareRootBackup {
                    plan: invalid,
                    receipt: "b".repeat(64),
                };
                assert!(store
                    .inspect(&binding())
                    .unwrap()
                    .last_valid
                    .unwrap()
                    .apply(event.clone())
                    .is_err());
                assert!(store.append(generation, event).is_err());
            }
            std::fs::write(
                fixture
                    .temp
                    .path()
                    .join("quarantine/retained/after-failed-copy"),
                b"more current data",
            )
            .unwrap();
            let prior: Vec<_> = std::fs::read_dir(fixture.temp.path().join("copies"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            if reopen {
                drop(copy);
                drop(store);
                store = JournalStore::open_windows(fixture.records.clone()).unwrap();
                store.bind_existing(&binding()).unwrap();
                let mut journal = ContextJournal::new(
                    &mut store,
                    fixture.records.clone(),
                    &lease,
                    binding(),
                    generation,
                )
                .unwrap();
                copy = PrivateTreeCopy::reopen_rotation(
                    fixture.copies.clone(),
                    fixture.parent.clone(),
                    fixture.quarantine.clone(),
                    &rotation,
                    &fixture.user,
                    SnapshotLimits::default(),
                    &mut journal,
                )
                .unwrap();
            }
            let mut journal = ContextJournal::new(
                &mut store,
                fixture.records.clone(),
                &lease,
                binding(),
                generation,
            )
            .unwrap();
            // Complete the new C1 but stop before any inverse intent, so the
            // stale-plan refusal cannot pass merely because an inverse exists.
            std::fs::create_dir(fixture.temp.path().join("desk")).unwrap();
            assert!(copy
                .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
                .is_err());
            assert!(prior.iter().all(|path| path.is_dir()));
            assert_eq!(
                std::fs::read(fixture.temp.path().join("quarantine/retained/new-data")).unwrap(),
                b"current user bytes"
            );
            assert_eq!(
                std::fs::read(
                    fixture
                        .temp
                        .path()
                        .join("quarantine/retained/after-failed-copy")
                )
                .unwrap(),
                b"more current data"
            );
            assert_eq!(
                std::fs::read(fixture.temp.path().join("copies/backup/providers.json")).unwrap(),
                b"private provider settings"
            );
            let generation = journal.generation();
            drop(journal);
            let (current_plan_generation, current_plan) =
                store.context_root_backup(&rotation).unwrap().unwrap();
            assert!(current_plan_generation > plan_generation);
            assert_eq!(current_plan.effects, old_plan.effects + 1);
            assert!(store.context_pending().unwrap().is_none());
            assert!(store.context_inverse(&rotation).unwrap().1.is_none());
            // The old plan's final index was never attempted. It must remain
            // unusable after replan, including after reconstructing the reducer.
            let pending_spec = store.context_rotation(&failed).unwrap().0;
            let crate::version_history::journal::EffectKind::PrivateBackupEntry {
                entry_index, ..
            } = pending_spec.kind
            else {
                panic!("expected C1 copy effect")
            };
            assert!(entry_index < old_plan.effects - 1);
            let stale = crate::version_history::journal::JournalEvent::Intent {
                effect: crate::version_history::journal::EffectSpec {
                    effect_id: uuid::Uuid::new_v4().to_string(),
                    kind: crate::version_history::journal::EffectKind::PrivateBackupEntry {
                        plan_generation,
                        operation:
                            crate::version_history::journal::PrivateBackupOperation::CopyFile,
                        manifest: old_plan.current_manifest.clone(),
                        entry_index: old_plan.effects - 1,
                    },
                    before: pending_spec.before,
                    expected_postconditions: pending_spec.expected_postconditions,
                },
            };
            for reopen_journal in [false, true] {
                if reopen_journal {
                    drop(store);
                    store = JournalStore::open_windows(fixture.records.clone()).unwrap();
                    store.bind_existing(&binding()).unwrap();
                }
                assert!(store
                    .inspect(&binding())
                    .unwrap()
                    .last_valid
                    .unwrap()
                    .apply(stale.clone())
                    .is_err());
                assert!(store.append(generation, stale.clone()).is_err());
                assert_eq!(
                    store
                        .inspect(&binding())
                        .unwrap()
                        .last_valid
                        .unwrap()
                        .generation(),
                    generation
                );
            }
            std::fs::remove_dir(fixture.temp.path().join("desk")).unwrap();
            let mut journal = ContextJournal::new(
                &mut store,
                fixture.records.clone(),
                &lease,
                binding(),
                generation,
            )
            .unwrap();
            copy.reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
                .unwrap()
                .verify(&context)
                .unwrap();
            assert_eq!(
                std::fs::read(fixture.temp.path().join("desk/after-failed-copy")).unwrap(),
                b"more current data"
            );
            drop(journal);
            fixture_abort(&mut store, &context, &image);
            assert_eq!(
                store
                    .inspect(&binding())
                    .unwrap()
                    .last_valid
                    .unwrap()
                    .effect_observation(&failed),
                Some(Observation::Unknown)
            );
        }
    }
}

// 检查原位冲突后新增源数据可重新观测，旧完整 C1 保留且新 C1 绑定当前内容。
#[test]
fn HistoryContextWindows_RecoveryCopyDrift_027() {
    let fixture = Fixture::new();
    fixture.fill();
    let mut context = fixture.context();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let image = fixture.image();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("backup"));
    copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    copy.rotate_context_root(
        &mut context,
        RootKind::Desk,
        fixture.parent.clone(),
        name("desk"),
        fixture.quarantine.clone(),
        name("retained"),
        &boundary,
        &image,
        &fixture.user,
        &mut journal,
    )
    .unwrap();
    std::fs::create_dir(fixture.temp.path().join("desk")).unwrap();
    assert!(copy
        .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
        .is_err());
    let prior: Vec<_> = std::fs::read_dir(fixture.temp.path().join("copies"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    std::fs::write(
        fixture
            .temp
            .path()
            .join("quarantine/retained/arrived-later"),
        b"later current data",
    )
    .unwrap();
    std::fs::remove_dir(fixture.temp.path().join("desk")).unwrap();
    copy.reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
        .unwrap()
        .verify(&context)
        .unwrap();
    assert!(prior.iter().all(|path| path.is_dir()));
    assert_eq!(
        std::fs::read(fixture.temp.path().join("desk/arrived-later")).unwrap(),
        b"later current data"
    );
    drop(journal);
    fixture_abort(&mut store, &context, &image);
}

// 检查增长的第一个 C1 不能消耗另一个未返回根的恢复预算，拒绝前无新副本且第二根仍可返回。
#[test]
fn HistoryContextWindows_RecoveryBudgetIsolation_028() {
    let fixture = Fixture::new();
    std::fs::write(fixture.temp.path().join("desk/state"), b"desk original").unwrap();
    let udf =
        PrivateDirectory::create_renameable_new(fixture.parent.clone(), name("udf"), &fixture.user)
            .unwrap();
    std::fs::write(fixture.temp.path().join("udf/state"), b"webview original").unwrap();
    let mut context = HeldContext::capture(
        HeldRoot::Present(fixture.source.directory().clone()),
        HeldRoot::Present(udf.directory().clone()),
        SnapshotLimits::default(),
    )
    .unwrap();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let image = fixture.image();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut desk = PrivateTreeCopy::new(fixture.copies.clone(), name("desk-backup"));
    let mut webview = PrivateTreeCopy::new(fixture.copies.clone(), name("udf-backup"));
    desk.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
        .unwrap();
    webview
        .copy_from(context.tree(RootKind::WebView), &fixture.user, &mut journal)
        .unwrap();
    desk.rotate_context_root(
        &mut context,
        RootKind::Desk,
        fixture.parent.clone(),
        name("desk"),
        fixture.quarantine.clone(),
        name("desk-old"),
        &boundary,
        &image,
        &fixture.user,
        &mut journal,
    )
    .unwrap();
    webview
        .rotate_context_root(
            &mut context,
            RootKind::WebView,
            fixture.parent.clone(),
            name("udf"),
            fixture.quarantine.clone(),
            name("udf-old"),
            &boundary,
            &image,
            &fixture.user,
            &mut journal,
        )
        .unwrap();
    for index in 0..18 {
        std::fs::write(
            fixture
                .temp
                .path()
                .join(format!("quarantine/desk-old/added-{index}")),
            b"preserve growth",
        )
        .unwrap();
    }
    let generation = journal.generation();
    drop(journal);
    // Allow exactly the expanded first copy plus terminal allowance; exclude
    // the still-outstanding second root's reservation. It must be rejected.
    let limit = store.fixture_dependency_count() + 2 + (20 * 4 + 16) + 128;
    drop(store);
    let mut store =
        JournalStore::fixture_windows_dependency_limit(fixture.records.clone(), limit).unwrap();
    store.bind_existing(&binding()).unwrap();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    assert!(desk
        .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
        .is_err());
    assert_eq!(
        std::fs::read_dir(fixture.temp.path().join("copies"))
            .unwrap()
            .count(),
        2
    );
    assert!(fixture
        .temp
        .path()
        .join("quarantine/desk-old/added-17")
        .exists());
    webview
        .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
        .unwrap()
        .verify(&context)
        .unwrap();
    assert_eq!(
        std::fs::read(fixture.temp.path().join("udf/state")).unwrap(),
        b"webview original"
    );
    assert!(!fixture.temp.path().join("desk").exists());
    drop(journal);
    let digests =
        ["a", "b", "c", "d"].map(|value| store.retain_manifest(value.as_bytes()).unwrap());
    let proof = crate::version_history::journal::PreContextAbortProof::fixture(
        &store.inspect(&binding()).unwrap(),
        digests,
    );
    assert!(store.abort_pre_context(&proof).is_err());
}

// 检查持有写入权限的同一源对象完成真实 flush/readback，普通只读捕获不能冒充持久性证据。
#[test]
fn HistoryContextWindows_DurableCapture_029() {
    let fixture = Fixture::new();
    fixture.fill();
    let readonly = fixture.context();
    assert!(readonly.verify_durable().is_err());
    drop(readonly);
    let durable = HeldContext::capture_durable(
        HeldRoot::Present(fixture.source.directory().clone()),
        HeldRoot::observe(fixture.parent.clone(), name("udf")).unwrap(),
        SnapshotLimits::default(),
    )
    .unwrap();
    durable.verify_durable().unwrap();
    assert_eq!(
        std::fs::read(fixture.temp.path().join("desk/providers.json")).unwrap(),
        b"private provider settings"
    );
}

fn fixture_external_effect(
    store: &mut JournalStore,
    kind: crate::version_history::journal::EffectKind,
) {
    use crate::version_history::journal::{EffectSpec, JournalEvent, Observation, ObservedResult};
    // Only unrelated bundle/process/registration proof is mocked by this helper.
    // Context effects below always use the production held Windows executor.
    let observed = store
        .retain_manifest(b"fixture external subsystem evidence")
        .unwrap();
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    let effect_id = uuid::Uuid::new_v4().to_string();
    let generation = store
        .append(
            generation,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: effect_id.clone(),
                    kind,
                    before: observed.clone(),
                    expected_postconditions: observed.clone(),
                },
            },
        )
        .unwrap();
    let receipt = store
        .retain_effect_receipt(&effect_id, Observation::Applied, &observed)
        .unwrap();
    store
        .append(
            generation,
            JournalEvent::Observed {
                effect_id,
                intent_generation: generation,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt),
                },
            },
        )
        .unwrap();
}
fn fixture_sealed_context(
    fixture: &Fixture,
    present_udf: bool,
) -> (
    crate::version_history::windows::context::RetainedContextRoots,
    SnapshotBoundary,
    ImageFence,
    ExclusiveLease,
    JournalStore,
) {
    use crate::version_history::{
        journal::{EffectKind, JournalEvent, JournalPhase, ManifestRole},
        windows::context::RetainedContextRoots,
    };
    use std::collections::BTreeMap;
    fixture.fill();
    let udf = present_udf.then(|| {
        PrivateDirectory::create_renameable_new(fixture.parent.clone(), name("udf"), &fixture.user)
            .unwrap()
    });
    if present_udf {
        std::fs::write(fixture.temp.path().join("udf/state"), b"original webview").unwrap();
    }
    let mut context = HeldContext::capture_durable(
        HeldRoot::Present(fixture.source.directory().clone()),
        udf.as_ref().map_or_else(
            || HeldRoot::observe(fixture.parent.clone(), name("udf")).unwrap(),
            |root| HeldRoot::Present(root.directory().clone()),
        ),
        SnapshotLimits::default(),
    )
    .unwrap();
    let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
    let snapshot = capture_context(
        &boundary,
        &binding().source_context,
        &mut context,
        SnapshotLimits::default(),
    )
    .unwrap();
    let image = fixture.image();
    let lease = fixture.lease();
    let mut store = fixture.journal();
    let mut journal =
        ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
    let mut copies = BTreeMap::new();
    let mut readmitted = BTreeMap::new();
    for (kind, original, retained, backup) in [
        (RootKind::Desk, "desk", "desk-old", "desk-backup"),
        (RootKind::WebView, "udf", "udf-old", "udf-backup"),
    ] {
        let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name(backup));
        copy.copy_from(context.tree(kind), &fixture.user, &mut journal)
            .unwrap();
        if !context.tree(kind).manifest().entries.is_empty() {
            readmitted.insert(
                kind,
                copy.rotate_context_root(
                    &mut context,
                    kind,
                    fixture.parent.clone(),
                    name(original),
                    fixture.quarantine.clone(),
                    name(retained),
                    &boundary,
                    &image,
                    &fixture.user,
                    &mut journal,
                )
                .unwrap(),
            );
        }
        copies.insert(kind, copy);
    }
    drop(journal);
    let originals = RetainedContextRoots::admit(
        context,
        copies,
        readmitted,
        snapshot,
        &boundary,
        &fixture.user,
    )
    .unwrap();
    let digest = store
        .retain_manifest(&originals.snapshot().encode().unwrap())
        .unwrap();
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    let generation = store
        .append(
            generation,
            JournalEvent::Manifest {
                role: ManifestRole::SourceContext,
                digest,
            },
        )
        .unwrap();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    originals
        .record_preserved(&boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    drop(journal);
    for role in [
        ManifestRole::SourceBundle,
        ManifestRole::Registration,
        ManifestRole::Shortcuts,
    ] {
        let digest = store
            .retain_manifest(b"fixture other subsystem manifest")
            .unwrap();
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        store
            .append(generation, JournalEvent::Manifest { role, digest })
            .unwrap();
    }
    fixture_external_effect(&mut store, EffectKind::VerifySourceBundleCopy);
    fixture_external_effect(&mut store, EffectKind::FenceSourceImage);
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::SourceSealed,
            },
        )
        .unwrap();
    (originals, boundary, image, lease, store)
}

// 检查完整原始根/缺失根保留后才创建全新的空 Desk/UDF，原始字节与权限继续由同一对象保留。
#[test]
fn HistoryContextWindows_FreshRoots_030() {
    use crate::version_history::windows::context::FreshContextRoots;
    for present_udf in [false, true] {
        let fixture = Fixture::new();
        let (originals, boundary, image, lease, mut store) =
            fixture_sealed_context(&fixture, present_udf);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        fresh.verify(&originals, &fixture.user).unwrap();
        assert!(!fresh
            .manifest_bytes(&originals, &fixture.user)
            .unwrap()
            .is_empty());
        assert_eq!(
            std::fs::read_dir(fixture.temp.path().join("desk"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            std::fs::read_dir(fixture.temp.path().join("udf"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            std::fs::read(
                fixture
                    .temp
                    .path()
                    .join("quarantine/desk-old/providers.json")
            )
            .unwrap(),
            b"private provider settings"
        );
        assert!(fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .is_err());
    }
}

// 检查 fresh 创建前、创建后和回执前失败均保留原始树及实际创建对象，不能重试 create。
#[test]
fn HistoryContextWindows_FreshFailurePreserves_031() {
    use crate::version_history::windows::context::FreshContextRoots;
    for fault in [
        CopyFault::BeforeFreshCreate,
        CopyFault::AfterFreshCreate,
        CopyFault::BeforeFreshReceipt,
    ] {
        let fixture = Fixture::new();
        let (originals, boundary, image, lease, mut store) = fixture_sealed_context(&fixture, true);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        let _fault = probe_copy_failure(fault);
        assert!(fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .is_err());
        assert!(fresh.verify(&originals, &fixture.user).is_err());
        originals.verify(&fixture.user).unwrap();
        assert_eq!(
            fixture.temp.path().join("desk").exists(),
            fault != CopyFault::BeforeFreshCreate
        );
        assert!(!fixture.temp.path().join("udf").exists());
        let generation = journal.generation();
        assert!(fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .is_err());
        assert_eq!(generation, journal.generation());
    }
}

fn fixture_begin_context_restore(
    store: &mut JournalStore,
    later: &crate::version_history::windows::context::LaterContextRoots,
    originals: &crate::version_history::windows::context::RetainedContextRoots,
    user: &CurrentUser,
) {
    use crate::version_history::journal::{JournalEvent, JournalPhase, ManifestRole};
    let bytes = later.manifest_bytes(originals, user).unwrap();
    let digest = store.retain_manifest(&bytes).unwrap();
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    let generation = store
        .append(
            generation,
            JournalEvent::Manifest {
                role: ManifestRole::RetainedTargetContext,
                digest,
            },
        )
        .unwrap();
    store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restoring,
            },
        )
        .unwrap();
}
fn fixture_finish_context_restore(store: &mut JournalStore) {
    use crate::version_history::journal::{
        EffectKind, JournalEvent, JournalPhase, RegistrationSlot, ShortcutSlot,
    };
    fixture_external_effect(store, EffectKind::VerifySourceBundleRestore);
    for slot in [
        RegistrationSlot::Uninstall,
        RegistrationSlot::Publisher,
        RegistrationSlot::DeskDirectory,
        RegistrationSlot::DeskDirectoryBackground,
        RegistrationSlot::LegacyDirectory,
        RegistrationSlot::LegacyDirectoryBackground,
        RegistrationSlot::OwnedRun,
    ] {
        fixture_external_effect(store, EffectKind::VerifyRegistrationRestore { slot });
    }
    for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
        fixture_external_effect(store, EffectKind::RestoreShortcut { slot });
    }
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    store
        .append(
            generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restored,
            },
        )
        .unwrap();
}

// 检查 fresh 成功或未知均可保留实际后续数据并恢复同一原始根，原始缺失状态也准确恢复。
#[test]
fn HistoryContextWindows_PreinstallContextReturn_032() {
    use crate::version_history::windows::context::{ContextRestoration, FreshContextRoots};
    for (present_udf, fault) in [
        (false, None),
        (true, Some(CopyFault::BeforeFreshCreate)),
        (true, Some(CopyFault::AfterFreshCreate)),
        (true, Some(CopyFault::BeforeFreshReceipt)),
    ] {
        let fixture = Fixture::new();
        let (originals, boundary, image, lease, mut store) =
            fixture_sealed_context(&fixture, present_udf);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        let injected = fault.map(probe_copy_failure);
        assert_eq!(
            fresh
                .create(&originals, &boundary, &image, &fixture.user, &mut journal)
                .is_err(),
            fault.is_some()
        );
        drop(injected);
        if fixture.temp.path().join("desk").exists() {
            std::fs::write(
                fixture.temp.path().join("desk/later-data"),
                b"retain actual later data",
            )
            .unwrap();
        }
        let mut later = fresh
            .observe_for_return(
                &originals,
                fixture.quarantine.clone(),
                &boundary,
                &fixture.user,
            )
            .unwrap();
        later
            .admit_preinstall_return(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        later
            .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        let generation = journal.generation();
        drop(journal);
        let roots = store.inspect(&binding()).unwrap().last_valid.unwrap();
        assert_eq!(roots.generation(), generation);
        fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut returning = ContextRestoration::new(originals, later).unwrap();
        returning
            .restore(&boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        let restored = returning.finish(&fixture.user).unwrap();
        restored.verify(&fixture.user).unwrap();
        assert_eq!(
            std::fs::read(fixture.temp.path().join("desk/providers.json")).unwrap(),
            b"private provider settings"
        );
        assert_eq!(fixture.temp.path().join("udf").exists(), present_udf);
        assert!(!fixture.temp.path().join("desk/later-data").exists());
        drop(journal);
        fixture_finish_context_restore(&mut store);
        restored.verify(&fixture.user).unwrap();
    }
}

// 检查实际后来根/原始根 rename 后丢失回执只重新观察同一对象，不再次执行 rename。
#[test]
fn HistoryContextWindows_ContextMoveReceiptLoss_033() {
    use crate::version_history::windows::context::{ContextRestoration, FreshContextRoots};
    for fault in [CopyFault::AfterLaterMove, CopyFault::AfterSourceRestoreMove] {
        let fixture = Fixture::new();
        let (originals, boundary, image, lease, mut store) = fixture_sealed_context(&fixture, true);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        std::fs::write(
            fixture.temp.path().join("desk/new-data"),
            b"new retained bytes",
        )
        .unwrap();
        let mut later = fresh
            .observe_for_return(
                &originals,
                fixture.quarantine.clone(),
                &boundary,
                &fixture.user,
            )
            .unwrap();
        later
            .admit_preinstall_return(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        let injected = (fault == CopyFault::AfterLaterMove).then(|| probe_copy_failure(fault));
        if injected.is_some() {
            assert!(later
                .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
                .is_err());
        }
        drop(injected);
        later
            .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        drop(journal);
        fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut returning = ContextRestoration::new(originals, later).unwrap();
        let injected =
            (fault == CopyFault::AfterSourceRestoreMove).then(|| probe_copy_failure(fault));
        if injected.is_some() {
            assert!(returning
                .restore(&boundary, &image, &fixture.user, &mut journal)
                .is_err());
        }
        drop(injected);
        returning
            .restore(&boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        returning
            .finish(&fixture.user)
            .unwrap()
            .verify(&fixture.user)
            .unwrap();
        assert_eq!(
            std::fs::read(fixture.temp.path().join("desk/providers.json")).unwrap(),
            b"private provider settings"
        );
        drop(journal);
        fixture_finish_context_restore(&mut store);
    }
}

// 检查后来上下文私有复制失败后保留旧部分副本与 Unknown，重新计划独立副本后仍能返回原始根。
#[test]
fn HistoryContextWindows_LaterCopyFailure_034() {
    use crate::version_history::windows::context::{ContextRestoration, FreshContextRoots};
    for (fault, reopen) in [
        (CopyFault::AfterWrite, false),
        (CopyFault::BeforeReceipt, true),
    ] {
        let fixture = Fixture::new();
        let (mut originals, boundary, image, lease, mut store) =
            fixture_sealed_context(&fixture, false);
        drop(fixture.source);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        std::fs::write(fixture.temp.path().join("desk/later"), b"later user data").unwrap();
        std::fs::write(
            fixture.temp.path().join("desk/zz-unattempted"),
            b"unattempted retained data",
        )
        .unwrap();
        let mut later = fresh
            .observe_for_return(
                &originals,
                fixture.quarantine.clone(),
                &boundary,
                &fixture.user,
            )
            .unwrap();
        later
            .admit_preinstall_return(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        let injected = probe_copy_failure(fault);
        assert!(later
            .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
            .is_err());
        drop(injected);
        std::fs::write(
            fixture.temp.path().join("desk/arrived"),
            b"additional later data",
        )
        .unwrap();
        let retained_before: Vec<_> = std::fs::read_dir(fixture.temp.path().join("copies"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        let generation = journal.generation();
        drop(journal);
        let (stale_generation, stale_plan) =
            store.context_later_backup(RootKind::Desk).unwrap().unwrap();
        let unknown = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .pending_effect()
            .unwrap()
            .effect_id
            .clone();
        if reopen {
            use crate::version_history::windows::context::{
                LaterContextRoots, RetainedContextRoots,
            };
            use std::collections::BTreeMap;
            drop(later);
            drop(originals);
            drop(store);
            store = JournalStore::open_windows(fixture.records.clone()).unwrap();
            store.bind_existing(&binding()).unwrap();
            let mut journal = ContextJournal::new(
                &mut store,
                fixture.records.clone(),
                &lease,
                binding(),
                generation,
            )
            .unwrap();
            originals = RetainedContextRoots::reopen_observation(
                BTreeMap::from([
                    (RootKind::Desk, fixture.parent.clone()),
                    (RootKind::WebView, fixture.parent.clone()),
                ]),
                fixture.copies.clone(),
                fixture.quarantine.clone(),
                &fixture.user,
                &mut journal,
            )
            .unwrap();
            later = LaterContextRoots::reopen_observation(
                &originals,
                fixture.quarantine.clone(),
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        }
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        later
            .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        assert!(retained_before.iter().all(|path| path.exists()));
        drop(journal);
        assert_eq!(
            store
                .inspect(&binding())
                .unwrap()
                .last_valid
                .unwrap()
                .effect_observation(&unknown),
            Some(crate::version_history::journal::Observation::Unknown)
        );
        {
            use crate::version_history::journal::{
                EffectKind, EffectSpec, JournalEvent, PrivateBackupOperation,
            };
            let evidence = store
                .retain_manifest(b"stale later copy cannot authorize an unattempted entry")
                .unwrap();
            let stale = JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: uuid::Uuid::new_v4().to_string(),
                    kind: EffectKind::PrivateBackupEntry {
                        plan_generation: stale_generation,
                        operation: PrivateBackupOperation::CopyFile,
                        manifest: stale_plan.source_manifest,
                        entry_index: stale_plan.effects - 1,
                    },
                    before: evidence.clone(),
                    expected_postconditions: evidence,
                },
            };
            let mut state = store.inspect(&binding()).unwrap().last_valid.unwrap();
            assert!(state.apply(stale.clone()).is_err());
            assert!(store.append(state.generation(), stale.clone()).is_err());
            drop(store);
            store = JournalStore::open_windows(fixture.records.clone()).unwrap();
            store.bind_existing(&binding()).unwrap();
            let current = store
                .inspect(&binding())
                .unwrap()
                .last_valid
                .unwrap()
                .generation();
            assert!(store.append(current, stale).is_err());
        }
        fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut returning = ContextRestoration::new(originals, later).unwrap();
        returning
            .restore(&boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        returning
            .finish(&fixture.user)
            .unwrap()
            .verify(&fixture.user)
            .unwrap();
        assert!(!fixture.temp.path().join("desk/arrived").exists());
        drop(journal);
        fixture_finish_context_restore(&mut store);
    }
}

// 检查逐树 flush 证据在 guard gap 失败时失效，当前字节必须重新 flush 后才可恢复持久性声明。
#[test]
fn HistoryContextWindows_DurableReadmission_035() {
    use std::{cell::RefCell, os::windows::fs::OpenOptionsExt, rc::Rc};
    for block_readmission in [false, true] {
        let fixture = Fixture::new();
        fixture.fill();
        let mut context = HeldContext::capture_durable(
            HeldRoot::Present(fixture.source.directory().clone()),
            HeldRoot::observe(fixture.parent.clone(), name("udf")).unwrap(),
            SnapshotLimits::default(),
        )
        .unwrap();
        context.verify_durable().unwrap();
        let boundary = SnapshotBoundary::fixture_with_roots(binding(), context.root_identities());
        let image = fixture.image();
        let lease = fixture.lease();
        let mut store = fixture.journal();
        let mut journal =
            ContextJournal::new(&mut store, fixture.records.clone(), &lease, binding(), 0).unwrap();
        let mut copy = PrivateTreeCopy::new(fixture.copies.clone(), name("durable-backup"));
        copy.copy_from(context.tree(RootKind::Desk), &fixture.user, &mut journal)
            .unwrap();
        let path = fixture.temp.path().join("desk/providers.json");
        let reader = Rc::new(RefCell::new(None));
        let hold = reader.clone();
        let _probe = probe_after_guard_release(move || {
            std::fs::write(&path, b"actual changed bytes after guard release").unwrap();
            if block_readmission {
                *hold.borrow_mut() = Some(
                    std::fs::OpenOptions::new()
                        .read(true)
                        .share_mode(5)
                        .open(&path)
                        .unwrap(),
                );
            }
        });
        let rotation = copy.rotate_context_root(
            &mut context,
            RootKind::Desk,
            fixture.parent.clone(),
            name("desk"),
            fixture.quarantine.clone(),
            name("durable-retained"),
            &boundary,
            &image,
            &fixture.user,
            &mut journal,
        );
        eprintln!(
            "HISTORY_DURABLE_ROTATION blocked_reader={} original={} quarantined={} error_kind={:?}",
            block_readmission,
            fixture.temp.path().join("desk").exists(),
            fixture
                .temp
                .path()
                .join("quarantine/durable-retained")
                .exists(),
            rotation.as_ref().err().map(std::io::Error::kind),
        );
        assert!(rotation.is_err());
        if block_readmission {
            assert!(context.verify_durable().is_err());
        } else {
            // Changed current contents were actually captured using fresh writable
            // handles and flushed; this is no assertion of equality to original C0.
            context.verify_durable().unwrap();
        }
        let moved = fixture
            .temp
            .path()
            .join("quarantine/durable-retained")
            .exists();
        assert_eq!(fixture.temp.path().join("desk").exists(), !moved);
        if !block_readmission {
            assert!(
                moved,
                "the byte-drift case must exercise a real inverse move"
            );
        } else {
            // The actual outside reader still denies a writable flush handle.
            // This must fail readmission whether the forward rename happened
            // or NTFS rejected it while that descendant handle was open.
            assert!(copy
                .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
                .is_err());
            assert!(context.verify_durable().is_err());
        }
        reader.borrow_mut().take();
        if moved {
            let _failure = probe_copy_failure(CopyFault::AfterReverseMove);
            let failure = copy
                .reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
                .err()
                .expect("the actual inverse move must reach its injected failure");
            assert_eq!(failure.to_string(), "injected copy boundary failure");
            assert!(context.verify_durable().is_err());
        }
        // A root that never left its original slot has no inverse move to
        // fault. Its real retry must still flush current bytes and retain C1.
        copy.reverse_context_root(&mut context, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        context.verify_durable().unwrap();
        assert_eq!(
            std::fs::read(fixture.temp.path().join("desk/providers.json")).unwrap(),
            b"actual changed bytes after guard release"
        );
        assert_eq!(
            std::fs::read(
                fixture
                    .temp
                    .path()
                    .join("copies/durable-backup/providers.json")
            )
            .unwrap(),
            b"private provider settings"
        );
        copy.verify(&fixture.user).unwrap();
        drop(journal);
        fixture_abort(&mut store, &context, &image);
    }
}

// 检查独立 return boundary 的真实缺失 image / fence 路径，丢失根改名回执跨重启后只重新观测。
#[test]
fn HistoryContextWindows_NormalReturnReopen_036() {
    use crate::version_history::{
        journal::{EffectKind, JournalEvent, JournalPhase},
        windows::{
            context::{
                ContextRestoration, FreshContextRoots, LaterContextRoots, RetainedContextRoots,
            },
            coordinator_evidence::ReturnBoundary,
        },
    };
    use std::collections::BTreeMap;
    for fault in [
        None,
        Some(CopyFault::AfterLaterMove),
        Some(CopyFault::AfterSourceRestoreMove),
    ] {
        let fixture = Fixture::new();
        let (originals, source_boundary, source_image, lease, mut store) =
            fixture_sealed_context(&fixture, false);
        // Model actual process loss: the fixture must not retain an extra DELETE
        // root guard after all executor owners have been dropped for restart.
        drop(fixture.source);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        fresh
            .create(
                &originals,
                &source_boundary,
                &source_image,
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        std::fs::write(
            fixture.temp.path().join("desk/later-work"),
            b"preserved later work",
        )
        .unwrap();
        let mut later = fresh
            .observe_for_return(
                &originals,
                fixture.quarantine.clone(),
                &source_boundary,
                &fixture.user,
            )
            .unwrap();
        drop(journal);
        // Only unrelated process facts are fixtures. This prior launch intent
        // permanently disqualifies the source-only pre-install return route.
        fixture_external_effect(&mut store, EffectKind::InstallerCreateSuspended);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        store
            .append(
                generation,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired,
                },
            )
            .unwrap();
        fixture_external_effect(&mut store, EffectKind::FenceHistoricalImage);
        std::fs::create_dir(fixture.temp.path().join("current-installation")).unwrap();
        let installation = fixture
            .parent
            .open_directory(name("current-installation"))
            .unwrap();
        let current_fence = if fault.is_none() {
            std::fs::write(
                fixture.temp.path().join("current-installation/current.exe"),
                b"current fixture image",
            )
            .unwrap();
            let image = installation
                .open_file(name("current.exe"), FileAccess::Read)
                .unwrap();
            let id = image.identity().clone();
            let digest = image.digest().unwrap();
            drop(image);
            Some(Arc::new(Mutex::new(
                ImageFence::acquire(installation.clone(), name("current.exe"), &id, &digest)
                    .unwrap(),
            )))
        } else {
            None
        };
        let return_boundary = ReturnBoundary::fixture(
            SnapshotBoundary::fixture_with_roots(binding(), later.root_identities()),
            installation,
            name("current.exe"),
            current_fence,
        )
        .unwrap();
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        assert!(later
            .admit_preinstall_return(
                &originals,
                &source_boundary,
                &source_image,
                &fixture.user,
                &mut journal
            )
            .is_err());
        assert!(later
            .preserve(
                &originals,
                &source_boundary,
                &source_image,
                &fixture.user,
                &mut journal
            )
            .is_err());
        if fault.is_some() {
            let generation = journal.generation();
            let replacement = fixture.temp.path().join("current-installation/current.exe");
            std::fs::write(&replacement, b"foreign image occupies the verified absence").unwrap();
            assert!(later
                .preserve_after_exit(&originals, &return_boundary, &fixture.user, &mut journal)
                .is_err());
            assert_eq!(generation, journal.generation());
            std::fs::remove_file(replacement).unwrap();
        }
        let injection = (fault == Some(CopyFault::AfterLaterMove))
            .then(|| probe_copy_failure(CopyFault::AfterLaterMove));
        if injection.is_some() {
            assert!(later
                .preserve_after_exit(&originals, &return_boundary, &fixture.user, &mut journal)
                .is_err());
        }
        drop(injection);
        if fault == Some(CopyFault::AfterLaterMove) {
            let moved = std::fs::read_dir(fixture.temp.path().join("quarantine"))
                .unwrap()
                .filter_map(Result::ok)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("later-root-")
                })
                .unwrap()
                .path();
            std::fs::write(
                moved.join("arrived-after-move"),
                b"new data requires latest B copy after uncertain move A",
            )
            .unwrap();
        }
        let generation = journal.generation();
        drop(journal);
        drop(later);
        drop(originals);
        drop(store);
        let mut store = JournalStore::open_windows(fixture.records.clone()).unwrap();
        store.bind_existing(&binding()).unwrap();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let parents = BTreeMap::from([
            (RootKind::Desk, fixture.parent.clone()),
            (RootKind::WebView, fixture.parent.clone()),
        ]);
        let originals = RetainedContextRoots::reopen_observation(
            parents.clone(),
            fixture.copies.clone(),
            fixture.quarantine.clone(),
            &fixture.user,
            &mut journal,
        )
        .unwrap();
        let mut later = LaterContextRoots::reopen_observation(
            &originals,
            fixture.quarantine.clone(),
            &fixture.user,
            &mut journal,
        )
        .unwrap();
        later
            .preserve_after_exit(&originals, &return_boundary, &fixture.user, &mut journal)
            .unwrap();
        drop(journal);
        fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut returning = ContextRestoration::new(originals, later).unwrap();
        let injection = (fault == Some(CopyFault::AfterSourceRestoreMove))
            .then(|| probe_copy_failure(CopyFault::AfterSourceRestoreMove));
        if injection.is_some() {
            assert!(returning
                .restore_after_exit(&return_boundary, &fixture.user, &mut journal)
                .is_err());
        }
        drop(injection);
        let generation = journal.generation();
        drop(journal);
        drop(returning);
        drop(store);
        let mut store = JournalStore::open_windows(fixture.records.clone()).unwrap();
        store.bind_existing(&binding()).unwrap();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let originals = RetainedContextRoots::reopen_observation(
            parents,
            fixture.copies.clone(),
            fixture.quarantine.clone(),
            &fixture.user,
            &mut journal,
        )
        .unwrap();
        let later = LaterContextRoots::reopen_observation(
            &originals,
            fixture.quarantine.clone(),
            &fixture.user,
            &mut journal,
        )
        .unwrap();
        let mut returning =
            ContextRestoration::reopen_observation(originals, later, &fixture.user, &mut journal)
                .unwrap();
        returning
            .restore_after_exit(&return_boundary, &fixture.user, &mut journal)
            .unwrap();
        let restored = returning.finish(&fixture.user).unwrap();
        restored.verify(&fixture.user).unwrap();
        assert_eq!(
            std::fs::read(fixture.temp.path().join("desk/providers.json")).unwrap(),
            b"private provider settings"
        );
        assert!(!fixture.temp.path().join("udf").exists());
        assert!(std::fs::read_dir(fixture.temp.path().join("quarantine"))
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("later-root-")
                && entry.path().join("later-work").exists()));
        if fault == Some(CopyFault::AfterLaterMove) {
            let moved = std::fs::read_dir(fixture.temp.path().join("quarantine"))
                .unwrap()
                .filter_map(Result::ok)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("later-root-")
                        && entry.path().join("arrived-after-move").exists()
                })
                .unwrap()
                .path();
            assert_eq!(
                std::fs::read(moved.join("arrived-after-move")).unwrap(),
                b"new data requires latest B copy after uncertain move A"
            );
        }
        drop(journal);
        if fault == Some(CopyFault::AfterLaterMove) {
            let histories = store.context_later_history(RootKind::Desk).unwrap();
            assert_eq!(histories.len(), 2);
            assert!(histories.iter().all(|record| record.complete.is_some()));
        }
        fixture_finish_context_restore(&mut store);
    }
}

// 检查任何已记录 installer/historical launch intent（含 NotApplied 与 Unknown）均禁止提前返回通道。
#[test]
fn HistoryContextWindows_PreinstallLaunchRefusal_037() {
    use crate::version_history::{
        journal::{EffectKind, EffectSpec, JournalEvent, Observation, ObservedResult},
        windows::context::FreshContextRoots,
    };
    for observation in [
        Observation::Applied,
        Observation::NotApplied,
        Observation::Unknown,
    ] {
        let fixture = Fixture::new();
        let (originals, boundary, image, lease, mut store) =
            fixture_sealed_context(&fixture, false);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        let later = fresh
            .observe_for_return(
                &originals,
                fixture.quarantine.clone(),
                &boundary,
                &fixture.user,
            )
            .unwrap();
        let generation = journal.generation();
        drop(journal);
        let evidence = store
            .retain_manifest(b"fixture prior launch attempt")
            .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let generation = store
            .append(
                generation,
                JournalEvent::Intent {
                    effect: EffectSpec {
                        effect_id: id.clone(),
                        kind: EffectKind::HistoricalCreateSuspended,
                        before: evidence.clone(),
                        expected_postconditions: evidence.clone(),
                    },
                },
            )
            .unwrap();
        let receipt = if observation == Observation::Unknown {
            None
        } else {
            Some(
                store
                    .retain_effect_receipt(&id, observation, &evidence)
                    .unwrap(),
            )
        };
        let current = store
            .append(
                generation,
                JournalEvent::Observed {
                    effect_id: id,
                    intent_generation: generation,
                    result: ObservedResult {
                        observation,
                        receipt,
                    },
                },
            )
            .unwrap();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            current,
        )
        .unwrap();
        assert!(later
            .admit_preinstall_return(&originals, &boundary, &image, &fixture.user, &mut journal)
            .is_err());
        assert_eq!(current, journal.generation());
        originals.verify(&fixture.user).unwrap();
    }
}

// 检查目的地冲突与保留原树新增条目均在原对象恢复前拒绝，不删除用户内容或用 C0 覆盖。
#[test]
fn HistoryContextWindows_ReturnConflicts_038() {
    use crate::version_history::windows::context::{ContextRestoration, FreshContextRoots};
    for original_drift in [false, true] {
        let fixture = Fixture::new();
        let (originals, boundary, image, lease, mut store) = fixture_sealed_context(&fixture, true);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        let mut later = fresh
            .observe_for_return(
                &originals,
                fixture.quarantine.clone(),
                &boundary,
                &fixture.user,
            )
            .unwrap();
        later
            .admit_preinstall_return(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        later
            .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        drop(journal);
        fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut returning = ContextRestoration::new(originals, later).unwrap();
        let user_path = if original_drift {
            fixture
                .temp
                .path()
                .join("quarantine/desk-old/new-original-data")
        } else {
            std::fs::create_dir(fixture.temp.path().join("desk")).unwrap();
            fixture.temp.path().join("desk/foreign")
        };
        std::fs::write(&user_path, b"never overwrite or delete this work").unwrap();
        assert!(returning
            .restore(&boundary, &image, &fixture.user, &mut journal)
            .is_err());
        assert_eq!(generation, journal.generation());
        assert_eq!(
            std::fs::read(&user_path).unwrap(),
            b"never overwrite or delete this work"
        );
        assert_eq!(
            std::fs::read(
                fixture
                    .temp
                    .path()
                    .join("quarantine/desk-old/providers.json")
            )
            .unwrap(),
            b"private provider settings"
        );
        assert_eq!(
            std::fs::read(
                fixture
                    .temp
                    .path()
                    .join("copies/desk-backup/providers.json")
            )
            .unwrap(),
            b"private provider settings"
        );
    }
}

// 检查历史部分副本与缺失完成记录的已确认完整副本，跨重试/重启必须保留原 ID、字节、权限与完整旧快照。
#[test]
fn HistoryContextWindows_LaterHistoryIntegrity_039() {
    use crate::version_history::windows::context::{
        ContextRestoration, FreshContextRoots, LaterContextRoots, RetainedContextRoots,
    };
    use std::collections::BTreeMap;
    for variant in [
        "snapshot-bytes",
        "snapshot-missing",
        "snapshot-live-add",
        "unpublished-bytes",
        "unpublished-positive",
    ] {
        let fixture = Fixture::new();
        let (originals, boundary, image, lease, mut store) =
            fixture_sealed_context(&fixture, false);
        drop(fixture.source);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
        fresh
            .create(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        std::fs::write(fixture.temp.path().join("desk/later"), b"known later bytes").unwrap();
        std::fs::write(fixture.temp.path().join("desk/z"), b"last copied entry").unwrap();
        let mut later = fresh
            .observe_for_return(
                &originals,
                fixture.quarantine.clone(),
                &boundary,
                &fixture.user,
            )
            .unwrap();
        later
            .admit_preinstall_return(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        let unpublished = variant.starts_with("unpublished");
        let injected = probe_copy_failure(if unpublished {
            CopyFault::BeforeLaterComplete
        } else {
            CopyFault::AfterWrite
        });
        assert!(later
            .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
            .is_err());
        drop(injected);
        let generation = journal.generation();
        drop(journal);
        let history = store.context_later_history(RootKind::Desk).unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].complete.is_none());
        if unpublished {
            assert_eq!(history[0].applied.len(), 3);
        }
        let selector: serde_json::Value =
            serde_json::from_slice(&store.read_manifest(&history[0].plan.destination).unwrap())
                .unwrap();
        let old = fixture
            .temp
            .path()
            .join("copies")
            .join(selector["name"].as_str().unwrap());
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        if !unpublished {
            later
                .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
                .unwrap();
        }
        if variant == "snapshot-live-add" {
            std::fs::write(
                old.join("new-foreign-entry"),
                b"detect changed old namespace",
            )
            .unwrap();
            assert!(later.verify_preserved(&originals, &fixture.user).is_err());
            assert!(later.manifest_bytes(&originals, &fixture.user).is_err());
            assert_eq!(
                std::fs::read(old.join("later")).unwrap(),
                b"known later bytes"
            );
            continue;
        }
        drop(journal);
        if !unpublished {
            fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
        }
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        drop(later);
        drop(originals);
        drop(store);
        if variant == "snapshot-missing" {
            std::fs::remove_dir_all(&old).unwrap();
        } else if variant != "unpublished-positive" {
            std::fs::write(old.join("later"), b"changed known private material").unwrap();
        }
        let mut store = JournalStore::open_windows(fixture.records.clone()).unwrap();
        store.bind_existing(&binding()).unwrap();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let parents = BTreeMap::from([
            (RootKind::Desk, fixture.parent.clone()),
            (RootKind::WebView, fixture.parent.clone()),
        ]);
        let originals = RetainedContextRoots::reopen_observation(
            parents,
            fixture.copies.clone(),
            fixture.quarantine.clone(),
            &fixture.user,
            &mut journal,
        )
        .unwrap();
        let reopened = LaterContextRoots::reopen_observation(
            &originals,
            fixture.quarantine.clone(),
            &fixture.user,
            &mut journal,
        );
        if variant != "unpublished-positive" {
            assert!(reopened.is_err(), "{variant}");
            assert_eq!(generation, journal.generation());
            assert_eq!(
                std::fs::read(
                    fixture
                        .temp
                        .path()
                        .join("quarantine/desk-old/providers.json")
                )
                .unwrap(),
                b"private provider settings"
            );
            continue;
        }
        let mut later = reopened.unwrap();
        later
            .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        assert_eq!(
            std::fs::read(old.join("later")).unwrap(),
            b"known later bytes"
        );
        drop(journal);
        let history = store.context_later_history(RootKind::Desk).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history[0].complete.is_none());
        assert!(history[1].complete.is_some());
        fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal = ContextJournal::new(
            &mut store,
            fixture.records.clone(),
            &lease,
            binding(),
            generation,
        )
        .unwrap();
        let mut returning = ContextRestoration::new(originals, later).unwrap();
        returning
            .restore(&boundary, &image, &fixture.user, &mut journal)
            .unwrap();
        returning
            .finish(&fixture.user)
            .unwrap()
            .verify(&fixture.user)
            .unwrap();
        drop(journal);
        fixture_finish_context_restore(&mut store);
    }
}

// 检查根均已保留后新副本 B 完成但完成记录未发布，不能凭内存中的成功副本提前绑定返回清单。
#[test]
fn HistoryContextWindows_LaterCompletionPublication_040() {
    use crate::version_history::windows::context::{ContextRestoration, FreshContextRoots};
    let fixture = Fixture::new();
    let (originals, boundary, image, lease, mut store) = fixture_sealed_context(&fixture, false);
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
    fresh
        .create(&originals, &boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    std::fs::write(fixture.temp.path().join("desk/later-data"), b"later data A").unwrap();
    let mut later = fresh
        .observe_for_return(
            &originals,
            fixture.quarantine.clone(),
            &boundary,
            &fixture.user,
        )
        .unwrap();
    later
        .admit_preinstall_return(&originals, &boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    later
        .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    let retained = std::fs::read_dir(fixture.temp.path().join("quarantine"))
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("later-root-")
                && entry.path().join("later-data").exists()
        })
        .unwrap()
        .path();
    std::fs::write(retained.join("arrived"), b"new data requires B").unwrap();
    let injected = probe_copy_failure(CopyFault::BeforeLaterComplete);
    assert!(later
        .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
        .is_err());
    drop(injected);
    assert!(later.manifest_bytes(&originals, &fixture.user).is_err());
    let generation = journal.generation();
    drop(journal);
    let history = store.context_later_history(RootKind::Desk).unwrap();
    assert_eq!(history.len(), 2);
    assert!(history[0].complete.is_some());
    assert!(history[1].complete.is_none());
    assert_eq!(history[1].applied.len(), 3);
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    later
        .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    later.manifest_bytes(&originals, &fixture.user).unwrap();
    drop(journal);
    assert_eq!(
        store.context_later_history(RootKind::Desk).unwrap().len(),
        2
    );
    fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    let mut returning = ContextRestoration::new(originals, later).unwrap();
    returning
        .restore(&boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    returning
        .finish(&fixture.user)
        .unwrap()
        .verify(&fixture.user)
        .unwrap();
    assert_eq!(
        std::fs::read(retained.join("arrived")).unwrap(),
        b"new data requires B"
    );
    drop(journal);
    fixture_finish_context_restore(&mut store);
}

// 检查第一个根的副本已完成而第二个根 AfterWrite 失败，跨根验证前先完整重新观测第二个部分副本。
#[test]
fn HistoryContextWindows_SecondRootCopyRetry_041() {
    use crate::version_history::windows::context::{ContextRestoration, FreshContextRoots};
    let fixture = Fixture::new();
    let (originals, boundary, image, lease, mut store) = fixture_sealed_context(&fixture, false);
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    let mut fresh = FreshContextRoots::new(&originals, &binding()).unwrap();
    fresh
        .create(&originals, &boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    // Desk has only its empty directory, so AfterWrite occurs in actual UDF.
    std::fs::write(
        fixture.temp.path().join("udf/browser-store"),
        b"second root user data",
    )
    .unwrap();
    let mut later = fresh
        .observe_for_return(
            &originals,
            fixture.quarantine.clone(),
            &boundary,
            &fixture.user,
        )
        .unwrap();
    later
        .admit_preinstall_return(&originals, &boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    let injected = probe_copy_failure(CopyFault::AfterWrite);
    assert!(later
        .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
        .is_err());
    drop(injected);
    let generation = journal.generation();
    drop(journal);
    let desk = store.context_later_history(RootKind::Desk).unwrap();
    let udf = store.context_later_history(RootKind::WebView).unwrap();
    assert_eq!(desk.len(), 1);
    assert!(desk[0].complete.is_some());
    assert_eq!(udf.len(), 1);
    assert!(udf[0].complete.is_none());
    assert_eq!(udf[0].applied.len(), 1);
    let selector: serde_json::Value =
        serde_json::from_slice(&store.read_manifest(&udf[0].plan.destination).unwrap()).unwrap();
    let partial = fixture
        .temp
        .path()
        .join("copies")
        .join(selector["name"].as_str().unwrap());
    let pending = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .pending_effect()
        .unwrap()
        .effect_id
        .clone();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    // Keep the exact same executor and partial writable file owner alive.
    later
        .preserve(&originals, &boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    assert_eq!(
        std::fs::read(partial.join("browser-store")).unwrap(),
        b"second root user data"
    );
    drop(journal);
    assert_eq!(
        store.context_later_history(RootKind::Desk).unwrap().len(),
        1
    );
    assert_eq!(
        store
            .context_later_history(RootKind::WebView)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .effect_observation(&pending),
        Some(crate::version_history::journal::Observation::Unknown)
    );
    fixture_begin_context_restore(&mut store, &later, &originals, &fixture.user);
    let generation = store
        .inspect(&binding())
        .unwrap()
        .last_valid
        .unwrap()
        .generation();
    let mut journal = ContextJournal::new(
        &mut store,
        fixture.records.clone(),
        &lease,
        binding(),
        generation,
    )
    .unwrap();
    let mut returning = ContextRestoration::new(originals, later).unwrap();
    returning
        .restore(&boundary, &image, &fixture.user, &mut journal)
        .unwrap();
    returning
        .finish(&fixture.user)
        .unwrap()
        .verify(&fixture.user)
        .unwrap();
    assert!(!fixture.temp.path().join("udf").exists());
    assert_eq!(
        std::fs::read(partial.join("browser-store")).unwrap(),
        b"second root user data"
    );
    drop(journal);
    fixture_finish_context_restore(&mut store);
}
