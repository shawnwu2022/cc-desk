//! Actual private suspended-worker probes. No fabricated PID, job absence or
//! serialized record can replace the original PreparedProcess in these tests.
use super::*;
use crate::version_history::windows::lease::LeaseFiles;
use std::time::{Duration, Instant};
use windows::Win32::System::{JobObjects::AssignProcessToJobObject, Threading::PROCESS_SET_QUOTA};

fn name(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
}
fn with_suspended(
    kind: JobKind,
    test: impl FnOnce(&mut PreparedProcess<'_>, &CurrentUser, &Arc<PrivateDirectory>, &Path),
) {
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
    let marker = temporary.path().join("must-not-run");
    let mut process = PreparedProcess::create_suspended(
        image,
        CommandLine::probe_controlled(&executable, &marker).unwrap(),
        kind,
        root.clone(),
        &user,
        &mut lease,
    )
    .unwrap();
    test(&mut process, &user, &root, &marker);
}
fn await_empty(process: &PreparedProcess<'_>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process.active_processes().unwrap() != 0 {
        assert!(Instant::now() < deadline, "owned job failed to drain");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// 两种进程均须由原始创建句柄取消后取得真实终态、空 job 和专用回执。
#[test]
fn CancelBeforeResume_OriginalReceiptAndActualCustody_001() {
    for kind in [JobKind::Installer, JobKind::HistoricalApplication] {
        with_suspended(kind, |process, user, root, marker| {
            let receipt = process.persist_identity(user).unwrap();
            assert!(process.can_cancel_before_resume().unwrap());
            assert!(process
                .observe_cancelled_before_resume(Some(&receipt), user)
                .is_err());
            assert_eq!(
                process.cancel_before_resume().unwrap().exit_code(),
                0xccde0001
            );
            await_empty(process);
            assert!(!process.can_cancel_before_resume().unwrap());
            assert!(process.cancel_before_resume().is_err());
            let cancelled = process
                .observe_cancelled_before_resume(Some(&receipt), user)
                .unwrap()
                .unwrap();
            cancelled.verify().unwrap();
            assert_eq!(cancelled.terminal().job_kind(), kind);
            assert_eq!(
                cancelled.terminal().root_identity(),
                root.directory().identity()
            );
            let creation: IdentityBinding =
                serde_json::from_slice(cancelled.creation_bytes().unwrap()).unwrap();
            assert!(creation == receipt.binding);
            let terminal: serde_json::Value =
                serde_json::from_slice(cancelled.terminal_bytes()).unwrap();
            assert_eq!(terminal["processReceipt"], receipt.record.digest());
            assert_eq!(terminal["jobPhase"], "armedPreparation");
            assert_eq!(terminal["activeProcesses"], 0);
            assert!(terminal["cancellationIntent"].is_string());
            assert!(terminal["historicalLifetime"].is_null());
            assert!(process
                .observe_cancelled_before_resume(Some(&receipt), user)
                .is_err());
            assert!(process.resume(&receipt).is_err());
            cancelled.into_terminal().verify().unwrap();
            assert!(!marker.exists());
        });
    }
}

// 原进程回执写入失败时用原始实物创建独立取消回执，绝不改写不确定文件。
#[test]
fn CancelBeforeResume_UncertainIdentityRecordIsPreserved_002() {
    with_suspended(JobKind::Installer, |process, user, root, marker| {
        let collision = DurableRecord::create(
            root.clone(),
            name(&process.probe_identity_name()),
            b"partial earlier process receipt",
            user,
        )
        .unwrap();
        let saved_identity = collision.file_identity().clone();
        assert!(process.persist_identity(user).is_err());
        assert!(!process.identity_persisted);
        assert!(process.can_cancel_before_resume().unwrap());
        process.cancel_before_resume().unwrap();
        await_empty(process);
        let cancelled = process
            .observe_cancelled_before_resume(None, user)
            .unwrap()
            .unwrap();
        cancelled.verify().unwrap();
        let creation: IdentityBinding =
            serde_json::from_slice(cancelled.creation_bytes().unwrap()).unwrap();
        assert_eq!(creation.process, process.process.identity);
        assert_eq!(creation.intent, process.intent.digest());
        assert_eq!(collision.file_identity(), &saved_identity);
        assert_eq!(collision.bytes(), b"partial earlier process receipt");
        collision.verify().unwrap();
        assert!(!marker.exists());
    });
}

// 任一早期 resume 调用失败仍消耗启动权限，不能变成“未尝试”取消分支。
#[test]
fn CancelBeforeResume_EarlyResumeFailurePermanentlyBlocksCleanup_003() {
    for failure in ["foreign-receipt", "resume-record"] {
        with_suspended(
            JobKind::HistoricalApplication,
            |process, user, root, marker| {
                let mut receipt = process.persist_identity(user).unwrap();
                let original = receipt.binding.clone();
                let collision = if failure == "resume-record" {
                    Some(
                        DurableRecord::create(
                            root.clone(),
                            name(&process.probe_resume_name()),
                            b"partial resume record",
                            user,
                        )
                        .unwrap(),
                    )
                } else {
                    receipt.binding.launch = "f".repeat(32);
                    None
                };
                assert!(process.resume(&receipt).is_err());
                receipt.binding = original;
                assert!(process.resume_attempted);
                assert!(!process.can_cancel_before_resume().unwrap());
                assert!(process.cancel_before_resume().is_err());
                assert!(process
                    .observe_cancelled_before_resume(Some(&receipt), user)
                    .is_err());
                assert!(process.try_terminal().unwrap().is_none());
                if let Some(collision) = collision {
                    collision.verify().unwrap();
                    assert_eq!(collision.bytes(), b"partial resume record");
                }
                assert!(!marker.exists());
            },
        );
    }
}

// ResumeThread 的结果未知或成功都禁止清理；测试只放行自有探针正常退出。
#[test]
fn CancelBeforeResume_UnknownAndSuccessfulResumeAreExcluded_004() {
    for unknown in [false, true] {
        with_suspended(
            JobKind::HistoricalApplication,
            |process, user, _, marker| {
                let receipt = process.persist_identity(user).unwrap();
                if unknown {
                    process.probe_suspend_primary().unwrap();
                }
                std::fs::write(
                    marker.with_extension("release"),
                    b"release controlled worker",
                )
                .unwrap();
                let resumed = process.resume(&receipt);
                assert_eq!(resumed.is_err(), unknown);
                assert!(!process.can_cancel_before_resume().unwrap());
                assert!(process.cancel_before_resume().is_err());
                assert!(process
                    .observe_cancelled_before_resume(Some(&receipt), user)
                    .is_err());
                if unknown {
                    // Only this test's extra suspension is released; production
                    // cancellation must never perform this operation.
                    assert_eq!(unsafe { ResumeThread(handle(&process.thread)) }, 1);
                }
                assert_eq!(
                    process.wait_terminal(30_000).unwrap().unwrap().exit_code(),
                    0
                );
                await_empty(process);
                assert!(marker.exists());
            },
        );
    }
}

// 取消回执任一写入失败即终止本次清理权限，并保持原始文件与未运行子进程。
#[test]
fn CancelBeforeResume_RecordFailureCannotRetryOrResume_005() {
    for prefix in ["cancel-identity", "cancel-intent"] {
        with_suspended(JobKind::Installer, |process, user, root, marker| {
            let receipt = process.persist_identity(user).unwrap();
            let collision = DurableRecord::create(
                root.clone(),
                name(&format!("{prefix}-{}.json", process.launch)),
                b"uncertain cleanup record",
                user,
            )
            .unwrap();
            assert!(process.cancel_before_resume().is_err());
            assert!(!process.can_cancel_before_resume().unwrap());
            assert!(process.cancel_before_resume().is_err());
            assert!(process.resume(&receipt).is_err());
            assert!(process
                .observe_cancelled_before_resume(Some(&receipt), user)
                .is_err());
            assert!(process.try_terminal().unwrap().is_none());
            collision.verify().unwrap();
            assert_eq!(collision.bytes(), b"uncertain cleanup record");
            assert!(!marker.exists());
        });
    }
}

// 终态托管写入冲突保留所有原始实物，不能重试生成或拿无关空 job 替代。
#[test]
fn CancelBeforeResume_CustodyCollisionIsOneAttempt_006() {
    with_suspended(JobKind::Installer, |process, user, root, marker| {
        let receipt = process.persist_identity(user).unwrap();
        process.cancel_before_resume().unwrap();
        await_empty(process);
        let collision = DurableRecord::create(
            root.clone(),
            name(&format!("terminal-custody-{}.json", process.launch)),
            b"uncertain terminal custody",
            user,
        )
        .unwrap();
        for _ in 0..2 {
            assert!(process
                .observe_cancelled_before_resume(Some(&receipt), user)
                .is_err());
            collision.verify().unwrap();
            assert_eq!(collision.bytes(), b"uncertain terminal custody");
        }
        assert!(process.cancel_custody_attempted.load(Ordering::SeqCst));
        assert_eq!(
            process.try_terminal().unwrap().unwrap().exit_code(),
            0xccde0001
        );
        assert_eq!(process.active_processes().unwrap(), 0);
        assert!(!marker.exists());
    });
}

// 被取消的主进程终态不能代替实际空 job；存在另一个自有探针时保持待定。
#[test]
fn CancelBeforeResume_ActualJobMustBeEmpty_007() {
    with_suspended(JobKind::Installer, |process, user, _, marker| {
        let receipt = process.persist_identity(user).unwrap();
        process.cancel_before_resume().unwrap();
        await_empty(process);
        let executable = std::env::current_exe().unwrap();
        let member_marker = marker.with_extension("member");
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
        let exact =
            ExactProcess::capture_with_access(member.id(), PROCESS_SET_QUOTA | PROCESS_TERMINATE)
                .unwrap();
        unsafe { AssignProcessToJobObject(handle(&process.job.handle), handle(&exact.process)) }
            .unwrap();
        assert!(process.active_processes().unwrap() > 0);
        assert!(process
            .observe_cancelled_before_resume(Some(&receipt), user)
            .unwrap()
            .is_none());
        assert!(!process.cancel_custody_attempted.load(Ordering::SeqCst));
        std::fs::write(
            member_marker.with_extension("release"),
            b"release owned member",
        )
        .unwrap();
        assert_eq!(exact.terminal(30_000).unwrap().unwrap().exit_code(), 0);
        assert!(member.wait().unwrap().success());
        await_empty(process);
        process
            .observe_cancelled_before_resume(Some(&receipt), user)
            .unwrap()
            .unwrap()
            .verify()
            .unwrap();
        assert!(!marker.exists());
    });
}
