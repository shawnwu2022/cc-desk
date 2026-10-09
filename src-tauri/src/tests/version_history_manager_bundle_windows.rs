use super::super::files::Directory;
use super::*;
use crate::version_history::{
    journal::{CapacityPlan, JournalBinding, JournalStore},
    windows::{
        context::{probe_manager_copy_handoff, InstalledBundleManifest},
        lease::LeaseFiles,
    },
};

fn private_root(
    parent: &Arc<Directory>,
    name_text: &str,
    user: &CurrentUser,
) -> Arc<PrivateDirectory> {
    Arc::new(PrivateDirectory::create_new(parent.clone(), name(name_text).unwrap(), user).unwrap())
}
fn journal_binding(source_bundle: String) -> JournalBinding {
    JournalBinding {
        transaction_id: "11111111-1111-4111-8111-111111111111".into(),
        source_context: "22222222-2222-4222-8222-222222222222".into(),
        target_context: "33333333-3333-4333-8333-333333333333".into(),
        user_installation: "1".repeat(64),
        source_bundle,
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}
#[test]
fn HistoryManagerBundle_CompleteCopy_001() {
    for change in [
        "none",
        "modified",
        "added",
        "manager-collision",
        "foreign-transaction",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let source_root = private_root(&parent, "source", &user);
        let records = private_root(&parent, "records", &user);
        let data = private_root(&parent, "data", &user);
        for filename in [
            "cc-desk.exe",
            "ConPTY.dll",
            "OpenConsole.exe",
            "unknown-companion.bin",
        ] {
            drop(
                ManagerRecord::create(
                    source_root.clone(),
                    filename,
                    &format!("fixture {filename}"),
                    &user,
                )
                .unwrap(),
            );
        }
        let source = HeldTree::capture_private(
            source_root.directory().clone(),
            SnapshotLimits::default(),
            &user,
        )
        .unwrap();
        let source_image = source_root
            .directory()
            .open_file(name("cc-desk.exe").unwrap(), FileAccess::Read)
            .unwrap();
        let manifest: InstalledBundleManifest = serde_json::from_value(serde_json::json!({
            "schema":1, "original_image_name":"cc-desk.exe", "fenced_image_location":"1".repeat(64),
            "tree":source.manifest(),
        }))
        .unwrap();
        let source_digest = manifest.logical_digest().unwrap();
        let binding = journal_binding(source_digest.clone());
        let mut store = JournalStore::open_windows(records.clone()).unwrap();
        store
            .initialize(
                binding.clone(),
                CapacityPlan::for_effects(100, 100, 20, 4096).unwrap(),
            )
            .unwrap();
        let leases = LeaseFiles::open(records.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let shared = leases.acquire_shared(&control).unwrap();
        let mut journal = ContextJournal::new_precommit(
            &mut store,
            records,
            &control,
            &shared,
            binding.clone(),
            0,
        )
        .unwrap();
        let copied = temp.path().join("data/bundle");
        let changed_path = copied.clone();
        let _probe = probe_manager_copy_handoff(move || match change {
            "modified" => std::fs::write(
                changed_path.join("unknown-companion.bin"),
                b"intervening write",
            )
            .unwrap(),
            "added" => {
                std::fs::write(changed_path.join("unexpected.bin"), b"unexpected entry").unwrap()
            }
            "manager-collision" => {
                std::fs::write(changed_path.join(MANAGER_BASENAME), b"foreign image").unwrap()
            }
            _ => (),
        });
        let result = ManagerBundle::prepare_copy(
            &source,
            &source_image,
            source_digest,
            data.clone(),
            if change == "foreign-transaction" {
                "44444444-4444-4444-8444-444444444444"
            } else {
                &binding.transaction_id
            },
            &user,
            &mut journal,
        );
        if change == "none" {
            let manager = result.unwrap();
            let reopened =
                ManagerBundle::reopen(data, &binding.transaction_id, manager.reference(), &user)
                    .unwrap();
            manager.verify(&user).unwrap();
            reopened.verify(&user).unwrap();
            assert_eq!(
                manager.binding.complete.entries.len(),
                source.manifest().entries.len() + 1
            );
            for filename in [
                "cc-desk.exe",
                "ConPTY.dll",
                "OpenConsole.exe",
                "unknown-companion.bin",
            ] {
                assert_eq!(
                    std::fs::read(temp.path().join("source").join(filename)).unwrap(),
                    std::fs::read(copied.join(filename)).unwrap()
                );
            }
            assert_eq!(
                std::fs::read(copied.join("cc-desk.exe")).unwrap(),
                std::fs::read(copied.join(MANAGER_BASENAME)).unwrap()
            );
            assert!(std::fs::OpenOptions::new()
                .write(true)
                .open(copied.join(MANAGER_BASENAME))
                .is_err());
            assert!(leases.acquire_exclusive(&control).is_err());
        } else if change == "foreign-transaction" {
            assert!(result.is_err());
            assert!(!copied.exists());
            assert_eq!(journal.generation(), 0);
            assert!(!temp.path().join("data/manager-bundle.json").exists());
            source.verify().unwrap();
        } else {
            assert!(result.is_err());
            assert!(!temp.path().join("data/manager-bundle.json").exists());
            assert!(copied.join("ConPTY.dll").exists());
            assert!(copied.join("OpenConsole.exe").exists());
            assert!(!copied.join(MANAGER_BASENAME).exists() || change == "manager-collision");
            source.verify().unwrap();
        }
    }
}
#[test]
fn HistoryManagerBundle_ConcurrentReadmission_002() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = private_root(&parent, "manager", &user);
    let original = ManagerRecord::create(
        root.clone(),
        "record.json",
        &serde_json::json!({"schema":1,"value":"exact"}),
        &user,
    )
    .unwrap();
    let reopened = ManagerRecord::open(root, "record.json", original.reference(), &user).unwrap();
    assert_eq!(original.reference(), reopened.reference());
    assert_eq!(
        reopened.decode::<serde_json::Value>(&user).unwrap()["value"],
        "exact"
    );
    // Both read guards deny a new mutable open while allowing child reads.
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temp.path().join("manager/record.json"))
        .is_err());
    original.verify(&user).unwrap();
}
#[test]
fn HistoryManagerBundle_RecordCollision_003() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = private_root(&parent, "manager", &user);
    let original =
        ManagerRecord::create(root.clone(), "manager-launch.json", &"first", &user).unwrap();
    let expected = original.reference().clone();
    drop(original);
    assert!(
        ManagerRecord::create(root.clone(), "manager-launch.json", &"replacement", &user).is_err()
    );
    let kept = ManagerRecord::open(root, "manager-launch.json", &expected, &user).unwrap();
    assert_eq!(kept.decode::<String>(&user).unwrap(), "first");
}
#[test]
fn HistoryManagerBundle_RecordIdentity_004() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = private_root(&parent, "one", &user);
    let other = private_root(&parent, "two", &user);
    let first = ManagerRecord::create(root.clone(), "record.json", &"same", &user).unwrap();
    let second = ManagerRecord::create(other.clone(), "record.json", &"same", &user).unwrap();
    assert_eq!(first.reference().digest(), second.reference().digest());
    assert!(ManagerRecord::open(other, "record.json", first.reference(), &user).is_err());
    let expected = first.reference().clone();
    drop(first);
    std::fs::write(temp.path().join("one/record.json"), b"\"edit\"").unwrap();
    assert!(ManagerRecord::open(root, "record.json", &expected, &user).is_err());
}
#[test]
fn HistoryManagerBundle_PartialRecord_005() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = private_root(&parent, "manager", &user);
    // Model the same secured object left by an interrupted initial write.
    let initial = ManagerRecord::create(
        root.clone(),
        "manager-ready.json",
        &serde_json::json!({"schema":1}),
        &user,
    )
    .unwrap();
    drop(initial);
    std::fs::write(temp.path().join("manager/manager-ready.json"), b"{").unwrap();
    assert!(ManagerRecord::create(
        root.clone(),
        "manager-ready.json",
        &serde_json::json!({"schema":1}),
        &user
    )
    .is_err());
    let partial = ManagerRecord::observe(root, "manager-ready.json", &user).unwrap();
    assert!(partial.decode::<serde_json::Value>(&user).is_err());
    assert_eq!(
        std::fs::read(temp.path().join("manager/manager-ready.json")).unwrap(),
        b"{"
    );
}
