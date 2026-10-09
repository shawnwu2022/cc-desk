//! Actual NTFS protected-role sharing, without constructing source/return authority.
use crate::version_history::{
    journal::{CapacityPlan, JournalBinding, JournalEvent, JournalStore, ManifestRole},
    windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        security::CurrentUser,
    },
};
use std::{ffi::OsStr, sync::Arc};

// 同一实际artifact句柄由writer和proof共享；并行读受同一cursor锁保护，writer退出仍拒绝外部写。
#[test]
fn HistoryRoleGuard_SharedHandleAndCursor_001() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = Arc::new(
        PrivateDirectory::create_new(
            Directory::open_absolute(temporary.path()).unwrap(),
            ComponentName::new(OsStr::new("private")).unwrap(),
            &user,
        )
        .unwrap(),
    );
    let binding = JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000211".into(),
        source_context: "00000000-0000-4000-8000-000000000212".into(),
        target_context: "00000000-0000-4000-8000-000000000213".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let mut store =
        JournalStore::create_windows_transaction(root.clone(), &binding.transaction_id).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(10, 10, 10, 4096).unwrap(),
        )
        .unwrap();
    let bytes = (0..131072).map(|n| (n % 251) as u8).collect::<Vec<_>>();
    let digest = store.retain_manifest(&bytes).unwrap();
    let generation = store
        .append(
            0,
            JournalEvent::Manifest {
                role: ManifestRole::Registration,
                digest: digest.clone(),
            },
        )
        .unwrap();
    assert!(store
        .retain_role_guard(root.clone(), &binding, generation, ManifestRole::Shortcuts)
        .is_err());
    let guard = Arc::new(
        store
            .retain_role_guard(
                root.clone(),
                &binding,
                generation,
                ManifestRole::Registration,
            )
            .unwrap(),
    );
    let mut foreign = binding.clone();
    foreign.target_context = "00000000-0000-4000-8000-000000000214".into();
    assert!(guard
        .verify_role(&foreign, ManifestRole::Registration, &root)
        .is_err());
    let mut workers = Vec::new();
    for _ in 0..4 {
        let guard = guard.clone();
        let bytes = bytes.clone();
        workers.push(std::thread::spawn(move || {
            for _ in 0..20 {
                assert_eq!(guard.read().unwrap(), bytes);
            }
        }));
    }
    for _ in 0..20 {
        assert_eq!(store.read_manifest(&digest).unwrap(), bytes);
    }
    for worker in workers {
        worker.join().unwrap();
    }
    drop(store);
    assert_eq!(guard.read().unwrap(), bytes);
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(
            temporary
                .path()
                .join("private")
                .join(format!("manifest-{digest}.json"))
        )
        .is_err());
    guard
        .verify_role(&binding, ManifestRole::Registration, &root)
        .unwrap();
}
