//! Actual private-copy readmission probes on disposable local NTFS roots.
//! These do not synthesize source/process authority or certify a full switch.
use crate::version_history::{
    journal::{CapacityPlan, JournalBinding, JournalStore, RootKind},
    snapshot::SnapshotLimits,
    windows::{
        context::{
            probe_copy_failure, ContextJournal, CopyFault, HeldContext, HeldRoot, PrivateTreeCopy,
        },
        files::{ComponentName, Directory, PrivateDirectory},
        lease::LeaseFiles,
        security::CurrentUser,
    },
};
use std::{ffi::OsStr, sync::Arc};

fn name(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
}
fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000401".into(),
        source_context: "00000000-0000-4000-8000-000000000402".into(),
        target_context: "00000000-0000-4000-8000-000000000403".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}

#[test]
fn HistorySourcePartialWindows_ActualPartialBytesAreRetainedWithoutReplay_001() {
    for fault in [CopyFault::BeforeCreate, CopyFault::AfterWrite] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let source =
            PrivateDirectory::create_renameable_new(parent.clone(), name("source"), &user).unwrap();
        let records =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("records"), &user).unwrap());
        let data =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("data"), &user).unwrap());
        let payload = vec![42u8; 131_072];
        std::fs::write(temp.path().join("source/payload.bin"), &payload).unwrap();
        let context = HeldContext::capture_durable(
            HeldRoot::Present(source.directory().clone()),
            HeldRoot::observe(parent, name("absent-udf")).unwrap(),
            SnapshotLimits::default(),
        )
        .unwrap();
        let files = LeaseFiles::open(records.clone(), &user).unwrap();
        let control = files.acquire_control().unwrap();
        let exclusive = files.acquire_exclusive(&control).unwrap();
        let mut store = JournalStore::open_windows(records.clone()).unwrap();
        store
            .initialize(
                binding(),
                CapacityPlan::for_effects(100, 100, 20, 4096).unwrap(),
            )
            .unwrap();
        let generation = store
            .inspect(&binding())
            .unwrap()
            .last_valid
            .unwrap()
            .generation();
        let mut journal =
            ContextJournal::new(&mut store, records, &exclusive, binding(), generation).unwrap();
        let mut copy = PrivateTreeCopy::new(data.clone(), name("partial"));
        let probe = probe_copy_failure(fault);
        assert!(copy
            .copy_from(context.tree(RootKind::Desk), &user, &mut journal)
            .is_err());
        drop(probe);
        let plan = copy.source_plan_generation().unwrap();
        assert!(!copy.has_source_rotation());
        assert!(copy
            .observe_partial_for_source_abort(&data, plan + 1, &user)
            .is_err());
        copy.observe_partial_for_source_abort(&data, plan, &user)
            .unwrap();
        let observed = copy.partial_source_observation(&data, plan, &user).unwrap();
        assert!(!observed.is_empty());
        assert!(copy.manifest().is_err());
        assert!(copy
            .copy_from(context.tree(RootKind::Desk), &user, &mut journal)
            .is_err());
        context.verify_durable().unwrap();
        match fault {
            CopyFault::BeforeCreate => assert!(!temp.path().join("data/partial").exists()),
            CopyFault::AfterWrite => assert_eq!(
                std::fs::read(temp.path().join("data/partial/payload.bin")).unwrap(),
                payload[..65536]
            ),
            _ => unreachable!(),
        }
    }
}
