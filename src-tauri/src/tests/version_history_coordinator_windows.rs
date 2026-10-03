//! Disposable same-object coordinator boundary probes; no app/installer is run.
use super::*;

// 源映像同对象移动后旧观察必须失效；重新准入保留完整原树身份和内容。
#[test]
fn HistoryCoordinator_MovedFenceReadmission_001() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let install =
        PrivateDirectory::create_new(parent.clone(), name("install").unwrap(), &user).unwrap();
    let quarantine =
        PrivateDirectory::create_new(parent, name("quarantine").unwrap(), &user).unwrap();
    std::fs::write(
        temporary.path().join("install/cc-desk.exe"),
        b"original image",
    )
    .unwrap();
    std::fs::write(
        temporary.path().join("install/companion"),
        b"complete source",
    )
    .unwrap();
    let image = install
        .directory()
        .open_file(name("cc-desk.exe").unwrap(), FileAccess::Read)
        .unwrap();
    let identity = image.identity().clone();
    let digest = image.digest().unwrap();
    drop(image);
    let fence = Arc::new(Mutex::new(
        ImageFence::acquire(
            install.directory().clone(),
            name("cc-desk.exe").unwrap(),
            &identity,
            &digest,
        )
        .unwrap(),
    ));
    let prior = HeldBundle::capture(
        install.directory().clone(),
        name("cc-desk.exe").unwrap(),
        fence.clone(),
        SnapshotLimits::default(),
    )
    .unwrap();
    let logical = prior.manifest().logical_digest().unwrap();
    fence
        .lock()
        .rename_to(
            quarantine.directory().clone(),
            name("source-image.exe").unwrap(),
        )
        .unwrap();
    assert!(
        prior.tree().verify().is_err(),
        "old location observation cannot be reused after rename"
    );
    let after = readmit_source_bundle(
        &prior,
        install.directory().clone(),
        name("cc-desk.exe").unwrap(),
        fence.clone(),
        &logical,
    )
    .unwrap();
    assert_eq!(after.manifest().tree, prior.manifest().tree);
    after.tree().verify().unwrap();

    std::fs::write(
        temporary.path().join("install/unexpected"),
        b"new companion",
    )
    .unwrap();
    assert!(
        readmit_source_bundle(
            &prior,
            install.directory().clone(),
            name("cc-desk.exe").unwrap(),
            fence.clone(),
            &logical
        )
        .is_err(),
        "new content cannot be hidden by the fence move"
    );
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(temporary.path().join("install/companion"))
            .is_err(),
        "rejected readmission retains original file readers"
    );
    std::fs::remove_file(temporary.path().join("install/unexpected")).unwrap();
    drop(after);
    fence
        .lock()
        .rename_to(install.directory().clone(), name("cc-desk.exe").unwrap())
        .unwrap();
    prior.tree().verify().unwrap();
}
