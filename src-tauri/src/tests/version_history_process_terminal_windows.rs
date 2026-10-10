//! Real Windows terminal custody probes, compiled as a child of `process`.
//! Every process is the existing explicit test worker in a private NTFS fixture.
use super::*;
use crate::version_history::windows::{fence::ImageFence, lease::LeaseFiles};
use std::time::{Duration, Instant};
use windows::Win32::System::{JobObjects::AssignProcessToJobObject, Threading::PROCESS_SET_QUOTA};

fn name(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
}

fn await_marker(marker: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "controlled worker did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn await_empty(process: &PreparedProcess<'_>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process.active_processes().unwrap() != 0 {
        assert!(
            Instant::now() < deadline,
            "private job did not drain after its exact worker exited"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// 检查进程工作目录使用同一持有目录的盘符路径，避免卷 GUID 路径触发启动延迟。
#[test]
fn TerminalGuard_CurrentDirectoryIdentity_009() {
    let temporary = tempfile::tempdir().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let (guard, current_directory) = launch_directory(&parent).unwrap();
    let units: Vec<_> = current_directory.encode_wide().collect();

    validate_absolute(&units).unwrap();
    assert!(current_directory
        .to_string_lossy()
        .as_bytes()
        .get(1..3)
        .is_some_and(|prefix| prefix == b":\\"));
    assert_eq!(guard.identity(), parent.identity());
    assert_eq!(
        Directory::open_absolute(Path::new(&current_directory))
            .unwrap()
            .identity(),
        parent.identity()
    );
    guard.recheck().unwrap();
    parent.recheck().unwrap();
}

// 检查两个角色的存活进程返回待定，且未创建终态托管回执。
#[test]
fn TerminalGuard_LivePending_001() {
    for kind in [JobKind::Installer, JobKind::HistoricalApplication] {
        let temporary = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temporary.path()).unwrap();
        let root = Arc::new(PrivateDirectory::create_new(parent, name("private"), &user).unwrap());
        let files = LeaseFiles::open(root.clone(), &user).unwrap();
        let control = files.acquire_control().unwrap();
        let mut lease = files.acquire_exclusive(&control).unwrap();
        let executable = std::env::current_exe().unwrap();
        let image_parent = Directory::open_absolute(executable.parent().unwrap()).unwrap();
        let image = image_parent
            .open_file(
                ComponentName::new(executable.file_name().unwrap()).unwrap(),
                FileAccess::Read,
            )
            .unwrap();
        let marker = temporary.path().join("started");
        let mut process = PreparedProcess::create_suspended(
            image,
            CommandLine::probe_controlled(&executable, &marker).unwrap(),
            kind,
            root,
            &user,
            &mut lease,
        )
        .unwrap();
        let receipt = process.persist_identity(&user).unwrap();
        let (receipt_name, receipt_identity, receipt_digest) = receipt.record_reference().unwrap();
        assert_eq!(&receipt_name, receipt.record.name());
        assert_eq!(&receipt_identity, receipt.record.file_identity());
        assert_eq!(receipt_digest, receipt.record.digest());
        assert_eq!(receipt.record_bytes().unwrap(), receipt.record.bytes());
        process.resume(&receipt).unwrap();
        await_marker(&marker);

        assert!(process.try_terminal().unwrap().is_none(), "{kind:?}");
        assert!(
            process
                .observe_terminal_guard(&receipt, &user)
                .unwrap()
                .is_none(),
            "{kind:?}"
        );
        assert!(!temporary
            .path()
            .join("private")
            .join(format!("terminal-custody-{}.json", process.launch))
            .exists());
        assert!(
            process.resume(&receipt).is_err(),
            "pending observation cannot authorize replay"
        );

        std::fs::write(marker.with_extension("release"), b"release").unwrap();
        assert_eq!(
            process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
            0
        );
        await_empty(&process);
    }
}

// 检查终态句柄与空 job 独立留存后可以封锁源映像并复用同一排他租约。
#[test]
fn TerminalGuard_ReleaseImageLease_002() {
    for kind in [JobKind::Installer, JobKind::HistoricalApplication] {
        let temporary = tempfile::tempdir().unwrap();
        let executable = temporary.path().join("worker.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temporary.path()).unwrap();
        let root =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("private"), &user).unwrap());
        let files = LeaseFiles::open(root.clone(), &user).unwrap();
        let control = files.acquire_control().unwrap();
        let mut lease = files.acquire_exclusive(&control).unwrap();
        let image = parent
            .open_file(name("worker.exe"), FileAccess::Read)
            .unwrap();
        let image_identity = image.identity().clone();
        let image_digest = image.digest().unwrap();
        let marker = temporary.path().join("executed");
        let mut process = PreparedProcess::create_suspended(
            image,
            CommandLine::probe(&executable, &marker).unwrap(),
            kind,
            root.clone(),
            &user,
            &mut lease,
        )
        .unwrap();
        let receipt = process.persist_identity(&user).unwrap();
        process.resume(&receipt).unwrap();
        assert_eq!(
            process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
            0
        );
        await_empty(&process);
        let expected_identity = process.process.identity().clone();
        let expected_launch = process.launch.clone();
        let expected_lifetime = process
            .historical_lifetime
            .as_ref()
            .map(|record| record.digest().to_owned());

        // The original receipt still owns a writable no-share-write handle.
        // Custody must validate that typed receipt, not reopen its filename.
        assert!(DurableRecord::open(
            root.clone(),
            receipt.record.name().clone(),
            receipt.record.digest(),
            &user
        )
        .is_err());
        let guard = process
            .observe_terminal_guard(&receipt, &user)
            .unwrap()
            .unwrap();
        guard.verify().unwrap();
        assert!(!guard.was_cancelled_before_resume());
        assert_eq!(guard.job_kind(), kind);
        assert_eq!(guard.root_identity(), root.directory().identity());
        assert_eq!(guard.process_identity(), &expected_identity);
        let terminal: serde_json::Value = serde_json::from_slice(guard.terminal_bytes()).unwrap();
        assert_eq!(terminal["schema"], 1);
        assert_eq!(terminal["launch"], expected_launch);
        assert_eq!(
            terminal["root"],
            serde_json::to_value(root.directory().identity()).unwrap()
        );
        assert_eq!(
            terminal["process"],
            serde_json::to_value(&expected_identity).unwrap()
        );
        assert_eq!(terminal["processReceipt"], receipt.record.digest());
        assert_eq!(terminal["launchIntent"], process.intent.digest());
        assert_eq!(terminal["exitCode"], 0);
        assert_eq!(terminal["activeProcesses"], 0);
        assert_eq!(
            terminal["historicalLifetime"],
            serde_json::to_value(expected_lifetime).unwrap()
        );
        assert_eq!(
            terminal["jobPhase"],
            match kind {
                JobKind::Installer => "armedPreparation",
                JobKind::HistoricalApplication => "historicalLifetime",
                JobKind::OrdinaryInstaller => "ordinaryInstallerLifetime",
            }
        );
        let (terminal_name, terminal_digest) = guard.terminal_reference();
        assert_eq!(
            terminal_name,
            format!("terminal-custody-{expected_launch}.json")
        );
        assert_eq!(
            terminal_digest,
            crate::version_history::verified_package::sha256(guard.terminal_bytes())
        );
        assert!(
            process.observe_terminal_guard(&receipt, &user).is_err(),
            "a second observation cannot recreate terminal custody"
        );

        drop(process);
        drop(receipt);
        let fence = ImageFence::acquire(parent, name("worker.exe"), &image_identity, &image_digest)
            .unwrap();
        fence.verify().unwrap();
        guard.verify().unwrap();

        // The guard's type must release the previous mutable lease borrow.
        let next_executable = std::env::current_exe().unwrap();
        let next_parent = Directory::open_absolute(next_executable.parent().unwrap()).unwrap();
        let next_image = next_parent
            .open_file(
                ComponentName::new(next_executable.file_name().unwrap()).unwrap(),
                FileAccess::Read,
            )
            .unwrap();
        let mut next = PreparedProcess::create_suspended(
            next_image,
            CommandLine::probe(&next_executable, &temporary.path().join("must-not-run")).unwrap(),
            JobKind::Installer,
            root,
            &user,
            &mut lease,
        )
        .unwrap();
        next.cancel_before_resume().unwrap();
        drop(next);
        lease.verify().unwrap();
        guard.verify().unwrap();
        assert!(!temporary.path().join("must-not-run").exists());
    }
}

// 检查真实私有 job 仍有第二个进程时，首进程退出不能产生终态托管。
#[test]
fn TerminalGuard_WaitForJobDrain_003() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let root = Arc::new(PrivateDirectory::create_new(parent, name("private"), &user).unwrap());
    let files = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = files.acquire_control().unwrap();
    let mut lease = files.acquire_exclusive(&control).unwrap();
    let executable = std::env::current_exe().unwrap();
    let image_parent = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = image_parent
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    let mut process = PreparedProcess::create_suspended(
        image,
        CommandLine::probe(&executable, &temporary.path().join("primary")).unwrap(),
        JobKind::Installer,
        root,
        &user,
        &mut lease,
    )
    .unwrap();
    let receipt = process.persist_identity(&user).unwrap();
    let member_marker = temporary.path().join("member");
    let mut member = std::process::Command::new(&executable)
        .args([
            "--exact",
            "tests::version_history_windows::HistoryWindows_ProcessWorker_013",
            "--ignored",
            "--nocapture",
        ])
        .env("CC_DESK_HISTORY_PROBE_MARKER", &member_marker)
        .env(
            "CC_DESK_HISTORY_PROBE_RELEASE",
            member_marker.with_extension("release"),
        )
        .env_remove("CC_DESK_HISTORY_PROBE_WAIT")
        .spawn()
        .unwrap();
    let exact_member =
        ExactProcess::capture_with_access(member.id(), PROCESS_SET_QUOTA | PROCESS_TERMINATE)
            .unwrap();
    unsafe { AssignProcessToJobObject(handle(&process.job.handle), handle(&exact_member.process)) }
        .unwrap();
    await_marker(&member_marker);
    process.resume(&receipt).unwrap();
    assert_eq!(
        process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
        0
    );

    assert!(exact_member.terminal(0).unwrap().is_none());
    assert!(process.active_processes().unwrap() >= 1);
    assert!(process
        .observe_terminal_guard(&receipt, &user)
        .unwrap()
        .is_none());
    assert!(!temporary
        .path()
        .join("private")
        .join(format!("terminal-custody-{}.json", process.launch))
        .exists());

    std::fs::write(member_marker.with_extension("release"), b"release").unwrap();
    assert_eq!(
        exact_member.terminal(30_000).unwrap().unwrap().exit_code(),
        0
    );
    assert!(member.wait().unwrap().success());
    await_empty(&process);
    process
        .observe_terminal_guard(&receipt, &user)
        .unwrap()
        .unwrap()
        .verify()
        .unwrap();
}

// 检查不同目录的同字节回执及被改写的类型绑定均不能授权终态托管。
#[test]
fn TerminalGuard_RejectReceipt_004() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let root =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("private"), &user).unwrap());
    let foreign_root =
        Arc::new(PrivateDirectory::create_new(parent, name("foreign"), &user).unwrap());
    let files = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = files.acquire_control().unwrap();
    let mut lease = files.acquire_exclusive(&control).unwrap();
    let executable = std::env::current_exe().unwrap();
    let image_parent = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = image_parent
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    let mut process = PreparedProcess::create_suspended(
        image,
        CommandLine::probe(&executable, &temporary.path().join("executed")).unwrap(),
        JobKind::Installer,
        root,
        &user,
        &mut lease,
    )
    .unwrap();
    let mut receipt = process.persist_identity(&user).unwrap();
    process.resume(&receipt).unwrap();
    assert_eq!(
        process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
        0
    );
    await_empty(&process);

    let foreign_record = DurableRecord::create(
        foreign_root,
        receipt.record.name().clone(),
        receipt.record.bytes(),
        &user,
    )
    .unwrap();
    let foreign_receipt = DurableProcessIdentity::open(foreign_record, &user).unwrap();
    assert_eq!(foreign_receipt.record.digest(), receipt.record.digest());
    assert!(
        process
            .observe_terminal_guard(&foreign_receipt, &user)
            .is_err(),
        "identical bytes in a different secured root must not authorize custody"
    );

    let original = receipt.binding.clone();
    for mismatch in [
        "launch", "process", "intent", "command", "job", "phase", "lease",
    ] {
        receipt.binding = original.clone();
        match mismatch {
            "launch" => receipt.binding.launch = "f".repeat(32),
            "process" => receipt.binding.process.created += 1,
            "intent" => receipt.binding.intent = "0".repeat(64),
            "command" => receipt.binding.command_digest = "0".repeat(64),
            "job" => receipt.binding.job.kind = JobKind::HistoricalApplication,
            "phase" => receipt.binding.job_phase = JobPhase::HistoricalLifetime,
            "lease" => receipt.binding.lease = receipt.binding.process.image.clone(),
            _ => unreachable!(),
        }
        assert!(
            process.observe_terminal_guard(&receipt, &user).is_err(),
            "mismatched {mismatch} binding must reject custody"
        );
        assert!(!temporary
            .path()
            .join("private")
            .join(format!("terminal-custody-{}.json", process.launch))
            .exists());
    }
    receipt.binding = original;
    process
        .observe_terminal_guard(&receipt, &user)
        .unwrap()
        .unwrap()
        .verify()
        .unwrap();
}

// 检查内存 job 阶段与真实限制不一致时拒绝托管，已取得的托管也重新检查限制。
#[test]
fn TerminalGuard_RejectJobPhase_005() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let root = Arc::new(PrivateDirectory::create_new(parent, name("private"), &user).unwrap());
    let files = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = files.acquire_control().unwrap();
    let mut lease = files.acquire_exclusive(&control).unwrap();
    let executable = std::env::current_exe().unwrap();
    let image_parent = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = image_parent
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    let mut process = PreparedProcess::create_suspended(
        image,
        CommandLine::probe(&executable, &temporary.path().join("executed")).unwrap(),
        JobKind::Installer,
        root,
        &user,
        &mut lease,
    )
    .unwrap();
    let receipt = process.persist_identity(&user).unwrap();
    process.resume(&receipt).unwrap();
    assert_eq!(
        process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
        0
    );
    await_empty(&process);

    process.job.phase = None;
    assert!(process.observe_terminal_guard(&receipt, &user).is_err());
    process.job.phase = Some(JobPhase::HistoricalLifetime);
    assert!(process.observe_terminal_guard(&receipt, &user).is_err());
    process.job.phase = Some(JobPhase::ArmedPreparation);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    unsafe {
        SetInformationJobObject(
            handle(&process.job.handle),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    }
    .unwrap();
    assert!(
        process.observe_terminal_guard(&receipt, &user).is_err(),
        "cached armed phase cannot override actual disarmed limits"
    );
    assert!(!temporary
        .path()
        .join("private")
        .join(format!("terminal-custody-{}.json", process.launch))
        .exists());

    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            handle(&process.job.handle),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    }
    .unwrap();
    let guard = process
        .observe_terminal_guard(&receipt, &user)
        .unwrap()
        .unwrap();
    guard.verify().unwrap();
    limits.BasicLimitInformation.LimitFlags = Default::default();
    unsafe {
        SetInformationJobObject(
            handle(&process.job.handle),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    }
    .unwrap();
    assert!(
        guard.verify().is_err(),
        "retained custody must still check the actual private job limits"
    );
}

// 检查历史应用必须保留本次启动的真实 lifetime 回执，缺失或外来回执不能补造终态。
#[test]
fn TerminalGuard_RequireLifetime_006() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let root =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("private"), &user).unwrap());
    let foreign_root =
        Arc::new(PrivateDirectory::create_new(parent, name("foreign"), &user).unwrap());
    let files = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = files.acquire_control().unwrap();
    let mut lease = files.acquire_exclusive(&control).unwrap();
    let executable = std::env::current_exe().unwrap();
    let image_parent = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = image_parent
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    let mut process = PreparedProcess::create_suspended(
        image,
        CommandLine::probe(&executable, &temporary.path().join("executed")).unwrap(),
        JobKind::HistoricalApplication,
        root.clone(),
        &user,
        &mut lease,
    )
    .unwrap();
    let receipt = process.persist_identity(&user).unwrap();
    process.resume(&receipt).unwrap();
    assert_eq!(
        process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
        0
    );
    await_empty(&process);
    assert_eq!(process.job.phase, Some(JobPhase::HistoricalLifetime));
    let lifetime = process.historical_lifetime.take().unwrap();
    assert!(
        process.observe_terminal_guard(&receipt, &user).is_err(),
        "actual disarm alone is insufficient without its durable receipt"
    );

    let foreign = DurableRecord::create(
        foreign_root,
        lifetime.name().clone(),
        lifetime.bytes(),
        &user,
    )
    .unwrap();
    assert_eq!(foreign.digest(), lifetime.digest());
    process.historical_lifetime = Some(foreign);
    assert!(
        process.observe_terminal_guard(&receipt, &user).is_err(),
        "same bytes in a foreign root are not the actual lifetime receipt"
    );
    drop(process.historical_lifetime.take());
    let mut wrong: serde_json::Value = serde_json::from_slice(lifetime.bytes()).unwrap();
    wrong["processReceipt"] = serde_json::Value::String("0".repeat(64));
    process.historical_lifetime = Some(
        DurableRecord::create(
            root,
            name("wrong-lifetime.json"),
            &serde_json::to_vec(&wrong).unwrap(),
            &user,
        )
        .unwrap(),
    );
    assert!(
        process.observe_terminal_guard(&receipt, &user).is_err(),
        "a persisted record for another identity cannot authorize the historical phase"
    );
    assert!(!temporary
        .path()
        .join("private")
        .join(format!("terminal-custody-{}.json", process.launch))
        .exists());

    process.historical_lifetime = Some(lifetime);
    process
        .observe_terminal_guard(&receipt, &user)
        .unwrap()
        .unwrap()
        .verify()
        .unwrap();
}

// 检查已有终态托管文件保持原字节和身份，冲突与再次检查不能触发重新执行。
#[test]
fn TerminalGuard_PreserveCollision_007() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let root = Arc::new(PrivateDirectory::create_new(parent, name("private"), &user).unwrap());
    let files = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = files.acquire_control().unwrap();
    let mut lease = files.acquire_exclusive(&control).unwrap();
    let executable = std::env::current_exe().unwrap();
    let image_parent = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = image_parent
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    let marker = temporary.path().join("executed");
    let mut process = PreparedProcess::create_suspended(
        image,
        CommandLine::probe(&executable, &marker).unwrap(),
        JobKind::Installer,
        root.clone(),
        &user,
        &mut lease,
    )
    .unwrap();
    let receipt = process.persist_identity(&user).unwrap();
    process.resume(&receipt).unwrap();
    assert_eq!(
        process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
        0
    );
    await_empty(&process);
    let collision = DurableRecord::create(
        root,
        name(&format!("terminal-custody-{}.json", process.launch)),
        b"partial earlier evidence",
        &user,
    )
    .unwrap();
    let identity = collision.file_identity().clone();
    let digest = collision.digest().to_owned();

    for _ in 0..2 {
        assert!(process.observe_terminal_guard(&receipt, &user).is_err());
        collision.verify().unwrap();
        assert_eq!(collision.file_identity(), &identity);
        assert_eq!(collision.digest(), digest);
        assert_eq!(collision.bytes(), b"partial earlier evidence");
        assert!(process.resume(&receipt).is_err());
        assert_eq!(process.try_terminal().unwrap().unwrap().exit_code(), 0);
    }
    assert_eq!(std::fs::read(marker).unwrap(), b"executed");
    assert_eq!(
        std::fs::read(temporary.path().join("executed.completed")).unwrap(),
        b"completed"
    );
}

// 检查空的其他私有 job 或缺失 job 名称不能替代本次启动的 job 证据。
#[test]
fn TerminalGuard_RejectForeignJob_008() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let root = Arc::new(PrivateDirectory::create_new(parent, name("private"), &user).unwrap());
    let files = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = files.acquire_control().unwrap();
    let mut lease = files.acquire_exclusive(&control).unwrap();
    let executable = std::env::current_exe().unwrap();
    let image_parent = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = image_parent
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    let mut process = PreparedProcess::create_suspended(
        image,
        CommandLine::probe(&executable, &temporary.path().join("executed")).unwrap(),
        JobKind::Installer,
        root,
        &user,
        &mut lease,
    )
    .unwrap();
    let receipt = process.persist_identity(&user).unwrap();
    process.resume(&receipt).unwrap();
    assert_eq!(
        process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
        0
    );
    await_empty(&process);
    let original_identity = process.job.identity.clone();
    let mut unrelated_identity = original_identity.clone();
    unrelated_identity.name.push_str("-unrelated");
    let unrelated = PrivateJob::new(unrelated_identity, &user).unwrap();
    assert_eq!(unrelated.active_processes().unwrap(), 0);
    process.job.identity = unrelated.identity.clone();
    assert!(
        process.observe_terminal_guard(&receipt, &user).is_err(),
        "an empty authenticated foreign job is insufficient"
    );
    process.job.identity = original_identity.clone();
    process.job.identity.name.push_str("-missing");
    assert!(
        PrivateJob::open_recorded(&process.job.identity, JobPhase::ArmedPreparation, &user)
            .is_err()
    );
    assert!(
        process.observe_terminal_guard(&receipt, &user).is_err(),
        "absence cannot establish a terminal job"
    );
    assert!(!temporary
        .path()
        .join("private")
        .join(format!("terminal-custody-{}.json", process.launch))
        .exists());

    process.job.identity = original_identity;
    process
        .observe_terminal_guard(&receipt, &user)
        .unwrap()
        .unwrap()
        .verify()
        .unwrap();
}
