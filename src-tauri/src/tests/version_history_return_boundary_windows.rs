//! Actual package namespace and current-image probes. These never fabricate
//! terminal process/job custody or admit a production ReturnBoundary.
use super::*;

// 保留包位于事务的 package 子目录；父事务、其他事务和同级目录不能替代。
#[test]
fn HistoryReturnBoundary_ExactPackageChild_001() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let data =
        PrivateDirectory::create_new(parent.clone(), name("transaction").unwrap(), &user).unwrap();
    let package =
        PrivateDirectory::create_new(data.directory().clone(), name("package").unwrap(), &user)
            .unwrap();
    assert_ne!(package.directory().identity(), data.directory().identity());
    verify_package_child(&package, &data, &user).unwrap();
    assert!(verify_package_child(&data, &data, &user).is_err());

    let other =
        PrivateDirectory::create_new(data.directory().clone(), name("other").unwrap(), &user)
            .unwrap();
    assert!(verify_package_child(&other, &data, &user).is_err());
    let foreign =
        PrivateDirectory::create_new(parent, name("foreign-transaction").unwrap(), &user).unwrap();
    let foreign_package =
        PrivateDirectory::create_new(foreign.directory().clone(), name("package").unwrap(), &user)
            .unwrap();
    assert!(verify_package_child(&foreign_package, &data, &user).is_err());
    verify_package_child(&foreign_package, &foreign, &user).unwrap();
    verify_package_child(&package, &data, &user).unwrap();
}

// 部分安装留下的任意当前映像按实物加锁保留，不要求其等于成功目标载荷。
#[test]
fn HistoryReturnBoundary_PartialInstallerImageIsActuallyFenced_002() {
    let temporary = tempfile::tempdir().unwrap();
    let bytes = b"actual changed partial installer payload";
    std::fs::write(temporary.path().join("cc-desk.exe"), bytes).unwrap();
    let directory = Directory::open_absolute(temporary.path()).unwrap();
    let mut evidence = None;
    capture_current_image(
        &mut evidence,
        directory.clone(),
        name("cc-desk.exe").unwrap(),
        None,
    )
    .unwrap();
    let Some(CurrentImageEvidence::Fenced(fence)) = evidence else {
        panic!("existing partial payload must be fenced");
    };
    let mut fence = fence.lock();
    assert_eq!(fence.digest().unwrap(), sha256(bytes));
    assert!(directory
        .open_file(name("cc-desk.exe").unwrap(), FileAccess::Read)
        .is_err());
    let user = CurrentUser::capture().unwrap();
    let quarantine =
        PrivateDirectory::create_new(directory.clone(), name("quarantine").unwrap(), &user)
            .unwrap();
    fence
        .rename_to(
            quarantine.directory().clone(),
            name("current-image.exe").unwrap(),
        )
        .unwrap();
    assert_eq!(fence.digest().unwrap(), sha256(bytes));
    assert_eq!(
        fence.held_location().unwrap().0,
        *quarantine.directory().identity()
    );
    assert_eq!(
        directory
            .open_file(name("cc-desk.exe").unwrap(), FileAccess::Read)
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::NotFound,
    );
}

// 即使没有历史启动，实际共享冲突仍阻止取得当前映像的排他屏障。
#[test]
fn HistoryReturnBoundary_BusyCurrentImageCannotBecomeAbsence_003() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::write(temporary.path().join("cc-desk.exe"), b"partial payload").unwrap();
    let directory = Directory::open_absolute(temporary.path()).unwrap();
    let holder = directory
        .open_file(name("cc-desk.exe").unwrap(), FileAccess::Read)
        .unwrap();
    let mut evidence = None;
    assert!(capture_current_image(
        &mut evidence,
        directory.clone(),
        name("cc-desk.exe").unwrap(),
        None
    )
    .is_err());
    assert!(evidence.is_none());
    holder.verify().unwrap();
    drop(holder);
    capture_current_image(&mut evidence, directory, name("cc-desk.exe").unwrap(), None).unwrap();
    assert!(matches!(evidence, Some(CurrentImageEvidence::Fenced(_))));
}

// 只有真实文件缺失可作为缺失观察；之后出现新映像即撤销该观察。
#[test]
fn HistoryReturnBoundary_ActualAbsenceRejectsReplacement_004() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = Directory::open_absolute(temporary.path()).unwrap();
    let mut evidence = None;
    capture_current_image(&mut evidence, directory, name("cc-desk.exe").unwrap(), None).unwrap();
    let Some(CurrentImageEvidence::Absent(absence)) = evidence else {
        panic!("missing image must have an actual absence observation");
    };
    absence.verify().unwrap();
    std::fs::write(temporary.path().join("cc-desk.exe"), b"new unmanaged image").unwrap();
    assert!(absence.verify().is_err());
}

// 排他取得后检查失败仍由 preparation 持有实物屏障，不能重新取得或替换该 owner。
#[test]
fn HistoryReturnBoundary_PostAcquireFailureRetainsFence_005() {
    let temporary = tempfile::tempdir().unwrap();
    let bytes = b"partial installed payload retained after failed check";
    std::fs::write(temporary.path().join("cc-desk.exe"), bytes).unwrap();
    let directory = Directory::open_absolute(temporary.path()).unwrap();
    let mut preparation = ReturnBoundaryPreparation::new();
    IMAGE_CAPTURE_FAILURE.set(true);
    assert!(capture_current_image(
        &mut preparation.image,
        directory.clone(),
        name("cc-desk.exe").unwrap(),
        None,
    )
    .is_err());
    let Some(CurrentImageEvidence::Fenced(fence)) = preparation.image.as_ref() else {
        panic!("post-acquire failure must retain the exclusive image");
    };
    let identity = fence.lock().identity().clone();
    assert_eq!(fence.lock().digest().unwrap(), sha256(bytes));
    assert!(directory
        .open_file(name("cc-desk.exe").unwrap(), FileAccess::Read)
        .is_err());
    assert!(std::fs::write(temporary.path().join("cc-desk.exe"), b"replacement").is_err());
    assert!(capture_current_image(
        &mut preparation.image,
        directory.clone(),
        name("cc-desk.exe").unwrap(),
        None,
    )
    .is_err());
    let Some(CurrentImageEvidence::Fenced(fence)) = preparation.image.as_ref() else {
        panic!("rejected replay must not discard retained image");
    };
    assert_eq!(fence.lock().identity(), &identity);
    drop(preparation);
    assert_eq!(
        directory
            .open_file(name("cc-desk.exe").unwrap(), FileAccess::Read)
            .unwrap()
            .digest()
            .unwrap(),
        sha256(bytes)
    );
}
