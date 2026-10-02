use super::super::files::{Directory, FileAccess};
use super::*;
#[test]
fn HistoryManagerProcess_SuspendedIdentity_001() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = PrivateDirectory::create_new(parent, name("bundle").unwrap(), &user).unwrap();
    let path = temp.path().join("bundle").join(MANAGER_BASENAME);
    std::fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
    let image = root
        .directory()
        .open_file(name(MANAGER_BASENAME).unwrap(), FileAccess::Read)
        .unwrap();
    let expected_image = image.identity().clone();
    let source_in_job = in_job(unsafe { GetCurrentProcess() }).unwrap();
    let source_job_limits = if source_in_job {
        use windows::Win32::System::JobObjects::{
            JobObjectExtendedLimitInformation, QueryInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                None,
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                None,
            )
        }
        .map(|()| limits.BasicLimitInformation.LimitFlags.0)
    } else {
        Ok(0)
    };
    let (process, thread, pending) =
        create_manager_process(image, "11111111-1111-4111-8111-111111111111")
            .unwrap_or_else(|error| {
                panic!(
                    "suspended manager creation failed: {error:?}; source_in_job={source_in_job}; source_job_limits={source_job_limits:?}"
                )
            });
    assert!(!in_job(handle(pending.0.as_ref().unwrap())).unwrap());
    assert!(process.terminal(0).unwrap().is_none());
    let same_image = root
        .directory()
        .open_file(name(MANAGER_BASENAME).unwrap(), FileAccess::Read)
        .unwrap();
    assert_eq!(same_image.identity(), &expected_image);
    process.verify_held_image(&same_image).unwrap();
    let reopened = ExactProcess::reopen(process.identity()).unwrap();
    assert_eq!(reopened.identity(), process.identity());
    // The test runner is never resumed. A suspended process is not ready.
    assert!(!temp.path().join(READY).exists());
    drop(pending);
    assert!(process.terminal(5000).unwrap().is_some());
    assert!(reopened.terminal(0).unwrap().is_some());
    drop(thread);
}
#[test]
fn HistoryManagerProcess_ReadyChain_002() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root =
        Arc::new(PrivateDirectory::create_new(parent, name("records").unwrap(), &user).unwrap());
    let transaction = "11111111-1111-4111-8111-111111111111";
    let process = ExactProcess::capture_observed(std::process::id()).unwrap();
    let bundle =
        ManagerRecord::create(root.clone(), "test-bundle.json", &"reference-only", &user).unwrap();
    let launch = LaunchBinding {
        schema: 1,
        transaction: transaction.into(),
        data_root: root.directory().identity().clone(),
        bundle: bundle.reference().clone(),
        package_root: root.directory().identity().clone(),
        package_digest: "1".repeat(64),
        source: process.identity().clone(),
        command_digest: "2".repeat(64),
    };
    let launch_record = ManagerRecord::create(root.clone(), LAUNCH, &launch, &user).unwrap();
    let process_record = ManagerRecord::create(
        root.clone(),
        PROCESS,
        &ProcessBinding {
            schema: 1,
            transaction: transaction.into(),
            launch: launch_record.reference().clone(),
            process: process.identity().clone(),
        },
        &user,
    )
    .unwrap();
    let resume_record = ManagerRecord::create(
        root.clone(),
        RESUME,
        &ResumeBinding {
            schema: 1,
            transaction: transaction.into(),
            data_root: root.directory().identity().clone(),
            launch: launch_record.reference().clone(),
            process: process_record.reference().clone(),
        },
        &user,
    )
    .unwrap();
    let ready = ManagerRecord::create(
        root.clone(),
        READY,
        &ReadyBinding {
            schema: 1,
            transaction: transaction.into(),
            data_root: root.directory().identity().clone(),
            admission: resume_record.reference().clone(),
            manager: process.identity().clone(),
            source: process.identity().clone(),
            bundle: bundle.reference().clone(),
            package_digest: "1".repeat(64),
        },
        &user,
    )
    .unwrap();
    // This is only reopening a recorded observation; production readiness
    // additionally requires the actual live child in observe_ready.
    let observation = ManagerReadyObservation::reopen(
        root.clone(),
        transaction,
        resume_record.reference(),
        ready.reference(),
        &user,
    )
    .unwrap();
    assert_eq!(observation.reference(), ready.reference());
    assert!(ManagerReadyObservation::reopen(
        root.clone(),
        "22222222-2222-4222-8222-222222222222",
        resume_record.reference(),
        ready.reference(),
        &user
    )
    .is_err());
    assert!(ManagerReadyObservation::reopen(
        root,
        transaction,
        launch_record.reference(),
        ready.reference(),
        &user
    )
    .is_err());
}
#[test]
fn HistoryManagerProcess_CommandGrammar_003() {
    let image = OsStr::new(r"C:\private bundle\cc-desk-version-manager.exe");
    let id = "11111111-1111-4111-8111-111111111111";
    assert_eq!(
        manager_command(image, id).unwrap(),
        format!("\"C:\\private bundle\\cc-desk-version-manager.exe\" --version-manager {id}")
    );
    for bad in [
        "",
        "11111111111141118111111111111111",
        "../transaction",
        "11111111-1111-4111-8111-111111111111 --install",
    ] {
        assert!(manager_command(image, bad).is_err());
    }
    assert!(manager_command(OsStr::new(r"C:\private\cc-desk.exe"), id).is_err());
    assert!(manager_command(OsStr::new("C:\\bad\"path\\cc-desk-version-manager.exe"), id).is_err());
}
