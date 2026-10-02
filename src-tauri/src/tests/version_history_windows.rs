//! Real Windows adapter probes. These require a local NTFS temporary directory;
//! no installed CC Desk state, registry registration, or user sessions are changed.
use crate::version_history::windows::{
    durability::DurableRecord,
    fence::ImageFence,
    files::{ComponentName, Directory, FileAccess, PrivateDirectory},
    lease::{ExclusiveLease, LeaseFiles},
    process::{
        recover_never_resumed, CommandLine, DurableLaunchIntent, ExactProcess, JobKind,
        PreparedProcess, PrivateJob,
    },
    security::CurrentUser,
};
use std::ffi::OsStr;
use std::io::Write;
use std::sync::Arc;

fn private_fixture() -> (tempfile::TempDir, CurrentUser, Arc<PrivateDirectory>) {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let private = PrivateDirectory::create_new(parent, name("private"), &user).unwrap();
    (temporary, user, Arc::new(private))
}
fn name(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
}

// 检查父目录、流名称、设备名和尾点不能作为相对文件名。
#[test]
fn HistoryWindows_RejectNames_001() {
    for invalid in [
        "", ".", "..", "a/b", "a\\b", "C:", "a:stream", "tail.", "tail ", "NUL", "COM1.exe",
    ] {
        assert!(
            ComponentName::new(OsStr::new(invalid)).is_err(),
            "{invalid}"
        );
    }
    assert!(ComponentName::new(OsStr::new("中国 文档.json")).is_ok());
}

// 检查真实私有目录只有当前 SID 的受保护 DACL，且拒绝继承权限的现有目录。
#[test]
fn HistoryWindows_PrivateAcl_002() {
    let (temporary, user, private) = private_fixture();
    private.verify(&user).unwrap();
    std::fs::create_dir(temporary.path().join("ambient")).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    assert!(PrivateDirectory::open_existing(parent, name("ambient"), &user).is_err());
}

// 检查持有父目录句柄时不能把祖先改名后置换成另一个目录。
#[test]
fn HistoryWindows_PinAncestors_003() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temporary.path().join("parent").join("leaf")).unwrap();
    let leaf = Directory::open_absolute(&temporary.path().join("parent").join("leaf")).unwrap();
    assert!(std::fs::rename(
        temporary.path().join("parent"),
        temporary.path().join("moved")
    )
    .is_err());
    leaf.recheck().unwrap();
}

// 检查硬链接文件不会取得读取或写入能力。
#[test]
fn HistoryWindows_RejectLinks_004() {
    let (temporary, _, private) = private_fixture();
    let path = temporary.path().join("private/source");
    std::fs::write(&path, b"source").unwrap();
    std::fs::hard_link(&path, temporary.path().join("alias")).unwrap();
    assert!(private
        .directory()
        .open_file(name("source"), FileAccess::Read)
        .is_err());
}

// 检查双锁分别提供短期排他控制和进程生命周期共享锁。
#[test]
fn HistoryWindows_LeaseOrder_005() {
    let (_temporary, user, private) = private_fixture();
    let leases = LeaseFiles::open(private.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let first = leases.acquire_shared(&control).unwrap();
    let second = leases.acquire_shared(&control).unwrap();
    assert!(leases.acquire_control().is_err());
    assert!(leases.acquire_exclusive(&control).is_err());
    drop(first);
    assert!(leases.acquire_exclusive(&control).is_err());
    drop(second);
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    assert!(leases.acquire_shared(&control).is_err());
    drop(exclusive);
    drop(control);
    leases.acquire_control().unwrap();
}

// 检查两个进程适配器打开同一锁文件得到相同身份，且不能删除稳定锁文件。
#[test]
fn HistoryWindows_StableLocks_006() {
    let (temporary, user, private) = private_fixture();
    let first = LeaseFiles::open(private.clone(), &user).unwrap();
    let second = LeaseFiles::open(private, &user).unwrap();
    assert_eq!(first.identities(), second.identities());
    assert!(std::fs::remove_file(temporary.path().join("private/lifetime.lock")).is_err());
    let control = first.acquire_control().unwrap();
    assert!(second.acquire_control().is_err());
    drop(control);
    second.acquire_control().unwrap();
}

// 检查写穿加 FlushFileBuffers 后从同一句柄读回，并拒绝覆盖已存在回执。
#[test]
fn HistoryWindows_DurableRecord_007() {
    let (temporary, user, private) = private_fixture();
    let record = DurableRecord::create(
        private.clone(),
        name("receipt.json"),
        b"{\"state\":1}",
        &user,
    )
    .unwrap();
    record.verify().unwrap();
    assert_eq!(record.bytes(), b"{\"state\":1}");
    assert!(DurableRecord::create(private, name("receipt.json"), b"different", &user).is_err());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.path().join("private/receipt.json"))
        .is_err());
    assert!(std::fs::remove_file(temporary.path().join("private/receipt.json")).is_err());
}

// 检查同一句柄无覆盖改名保持文件身份，并继续拒绝写入和替换。
#[test]
fn HistoryWindows_ImageRename_008() {
    let (temporary, _, private) = private_fixture();
    let source = temporary.path().join("source.exe");
    std::fs::write(&source, b"held image").unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let identity = parent
        .open_file(name("source.exe"), FileAccess::Read)
        .unwrap()
        .identity()
        .clone();
    let digest = crate::version_history::verified_package::sha256(b"held image");
    let mut fence = ImageFence::acquire(parent, name("source.exe"), &identity, &digest).unwrap();
    let before = fence.identity().clone();
    let renamed = fence
        .rename_to(private.directory().clone(), name("quarantined.exe"))
        .unwrap();
    assert_eq!(renamed.identity(), &before);
    assert!(!source.exists());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.path().join("private/quarantined.exe"))
        .is_err());
    fence.verify().unwrap();
}

// 检查已有目标保持字节不变，改名失败后源排他句柄仍然有效。
#[test]
fn HistoryWindows_NoReplace_009() {
    let (temporary, _, private) = private_fixture();
    std::fs::write(temporary.path().join("source.exe"), b"old").unwrap();
    std::fs::write(temporary.path().join("private/existing.exe"), b"keep").unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let identity = parent
        .open_file(name("source.exe"), FileAccess::Read)
        .unwrap()
        .identity()
        .clone();
    let mut fence = ImageFence::acquire(
        parent,
        name("source.exe"),
        &identity,
        &crate::version_history::verified_package::sha256(b"old"),
    )
    .unwrap();
    assert!(fence
        .rename_to(private.directory().clone(), name("existing.exe"))
        .is_err());
    assert_eq!(
        std::fs::read(temporary.path().join("private/existing.exe")).unwrap(),
        b"keep"
    );
    fence.verify().unwrap();
}

// 检查真实 NTFS 在保留子文件保护句柄时成功改名父目录，失败不得释放后重开。
#[test]
fn HistoryWindows_TreeRename_010() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let private = Arc::new(
        PrivateDirectory::create_renameable_new(parent.clone(), name("private"), &user).unwrap(),
    );
    let child =
        DurableRecord::create(private.clone(), name("child.json"), b"complete", &user).unwrap();
    let original = private.directory().identity().clone();
    let receipt = private.directory().rename_to(parent, name("rotated"))
        .expect("positive NTFS parent rename with retained no-delete child guard is required; do not release/reopen on failure");
    assert_eq!(receipt.identity(), &original);
    assert!(!temporary.path().join("private").exists());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.path().join("rotated/child.json"))
        .is_err());
    child.verify_after_parent_rename().unwrap();
}

// 检查 NSIS 的 /D 是不带引号的末尾参数，且拒绝换行与引号路径。
#[test]
fn HistoryWindows_NsisArguments_011() {
    let command = CommandLine::nsis(
        OsStr::new("C:\\下载\\setup.exe"),
        OsStr::new("C:\\Users\\Name\\CC Desk"),
    )
    .unwrap();
    assert_eq!(
        command.text(),
        "\"C:\\下载\\setup.exe\" /S /UPDATE /NS /D=C:\\Users\\Name\\CC Desk"
    );
    assert!(CommandLine::nsis(OsStr::new("C:\\setup.exe"), OsStr::new("C:\\bad\"path")).is_err());
    assert!(
        CommandLine::nsis(OsStr::new("C:\\setup.exe"), OsStr::new("\\\\server\\share")).is_err()
    );
}

// 检查显式应用进程在回执落盘前保持挂起，恢复后取得真实句柄退出码与空 job。
#[test]
fn HistoryWindows_SuspendedJob_012() {
    let (temporary, user, private) = private_fixture();
    let executable = std::env::current_exe().unwrap();
    let directory = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = directory
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    let marker = temporary.path().join("executed");
    let command = CommandLine::probe(&executable, &marker).unwrap();
    let mut lease = exclusive(private.clone(), &user);
    let mut process = PreparedProcess::create_suspended(
        image,
        command,
        JobKind::Installer,
        private,
        &user,
        &mut lease,
    )
    .unwrap();
    assert!(!marker.exists());
    assert!(process.try_terminal().unwrap().is_none());
    let receipt = process.persist_identity(&user).unwrap();
    assert!(!marker.exists());
    process.resume(&receipt).unwrap();
    let terminal = process.wait_terminal(30_000).unwrap().unwrap();
    assert_eq!(terminal.exit_code(), 0);
    assert!(marker.exists());
    await_empty(|| process.active_processes());
    assert!(process.resume(&receipt).is_err());
}

// 检查被测试适配器通过显式 CreateProcess 创建的精确测试进程。
#[test]
#[ignore = "explicitly invoked by concrete process and fence probes"]
fn HistoryWindows_ProcessWorker_013() {
    let path = std::env::var_os("CC_DESK_HISTORY_PROBE_MARKER")
        .expect("supervised test child requires its marker");
    {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        file.write_all(b"executed").unwrap();
        if let Some(release) = std::env::var_os("CC_DESK_HISTORY_PROBE_RELEASE") {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while !std::path::Path::new(&release).exists() {
                if std::time::Instant::now() >= deadline {
                    std::process::exit(124);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        if std::env::var_os("CC_DESK_HISTORY_PROBE_WAIT").is_some() {
            // Only this disposable test child waits. It always exits by itself;
            // historical-job tests never terminate it to obtain a green result.
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    }
}

// 检查重开私有目录得到同一身份，不靠提前持有的 Rust 对象跳过 ACL 检查。
#[test]
fn HistoryWindows_ReopenRoot_014() {
    let (temporary, user, private) = private_fixture();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let reopened = PrivateDirectory::open_existing(parent, name("private"), &user).unwrap();
    assert_eq!(
        reopened.directory().identity(),
        private.directory().identity()
    );
}

// 检查真实目录符号链接在根路径和父句柄相对打开两条路径都被拒绝。
#[test]
fn HistoryWindows_RejectReparse_015() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir(temporary.path().join("target")).unwrap();
    std::os::windows::fs::symlink_dir(
        temporary.path().join("target"),
        temporary.path().join("link"),
    )
    .expect("this reparse probe requires symlink creation support; failure is not acceptance");
    assert!(Directory::open_absolute(&temporary.path().join("link")).is_err());
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    assert!(parent.open_directory(name("link")).is_err());
}

// 检查回执重开只接受日志绑定的精确摘要，且重新持有外部写入保护。
#[test]
fn HistoryWindows_ReopenReceipt_016() {
    let (temporary, user, private) = private_fixture();
    let record =
        DurableRecord::create(private.clone(), name("receipt"), b"durable", &user).unwrap();
    let digest = record.digest().to_owned();
    drop(record);
    assert!(DurableRecord::open(private.clone(), name("receipt"), &"0".repeat(64), &user).is_err());
    let reopened = DurableRecord::open(private, name("receipt"), &digest, &user).unwrap();
    reopened.verify().unwrap();
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.path().join("private/receipt"))
        .is_err());
}

fn exclusive(root: Arc<PrivateDirectory>, user: &CurrentUser) -> ExclusiveLease {
    let files = LeaseFiles::open(root, user).unwrap();
    let control = files.acquire_control().unwrap();
    files.acquire_exclusive(&control).unwrap()
}
fn waiting_process<'lease>(
    kind: JobKind,
    private: Arc<PrivateDirectory>,
    user: &CurrentUser,
    marker: &std::path::Path,
    lease: &'lease mut ExclusiveLease,
) -> PreparedProcess<'lease> {
    let executable = std::env::current_exe().unwrap();
    let directory = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = directory
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    PreparedProcess::create_suspended(
        image,
        CommandLine::probe_controlled(&executable, marker).unwrap(),
        kind,
        private,
        user,
        lease,
    )
    .unwrap()
}
fn await_marker(marker: &std::path::Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !marker.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "test child did not execute"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn await_empty(read: impl Fn() -> std::io::Result<u32>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while read().unwrap() != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "owned job did not drain after exact child exit"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

// 检查历史应用 job 在管理者释放后仍可重开且不会终止用户进程。
#[test]
fn HistoryWindows_HistoricalJob_017() {
    let (temporary, user, private) = private_fixture();
    let marker = temporary.path().join("historical-started");
    let mut lease = exclusive(private.clone(), &user);
    let mut process = waiting_process(
        JobKind::HistoricalApplication,
        private,
        &user,
        &marker,
        &mut lease,
    );
    let receipt = process.persist_identity(&user).unwrap();
    process.resume(&receipt).unwrap();
    await_marker(&marker);
    let exact = receipt.reopen_process().unwrap();
    drop(process);
    assert!(exact.terminal(0).unwrap().is_none());
    let job = PrivateJob::reopen(&receipt, &user).unwrap();
    assert_eq!(job.active_processes().unwrap(), 1);
    std::fs::write(marker.with_extension("release"), b"release").unwrap();
    assert_eq!(exact.terminal(10_000).unwrap().unwrap().exit_code(), 0);
    await_empty(|| job.active_processes());
}

// 检查仅 installer job 在最后一个 job 所有者关闭时终止其创建的测试进程。
#[test]
fn HistoryWindows_InstallerJob_018() {
    let (temporary, user, private) = private_fixture();
    let marker = temporary.path().join("installer-started");
    let mut lease = exclusive(private.clone(), &user);
    let mut process = waiting_process(JobKind::Installer, private, &user, &marker, &mut lease);
    let receipt = process.persist_identity(&user).unwrap();
    process.resume(&receipt).unwrap();
    await_marker(&marker);
    let exact = receipt.reopen_process().unwrap();
    // A second real job handle prevents kill-on-close until IT is closed.
    let last_job = PrivateJob::reopen(&receipt, &user).unwrap();
    drop(process);
    assert!(exact.terminal(200).unwrap().is_none());
    assert!(!marker.with_extension("release").exists());
    drop(last_job);
    let killed = exact.terminal(2000).unwrap();
    // Bounded cleanup cannot turn a missing kill-on-close into test success.
    if killed.is_none() {
        std::fs::write(marker.with_extension("release"), b"cleanup").unwrap();
        assert!(
            exact.terminal(5000).unwrap().is_some(),
            "controlled test child did not clean up"
        );
    }
    assert!(
        killed.is_some(),
        "final job-handle closure did not terminate the blocked child"
    );
    assert_ne!(killed.unwrap().exit_code(), 0);
}

// 检查真实可执行文件被排他句柄封锁，重命名后仍封锁该对象而不封锁新建目标。
#[test]
fn HistoryWindows_LaunchFence_019() {
    let (temporary, _, private) = private_fixture();
    let source = temporary.path().join("source.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &source).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let image = parent
        .open_file(name("source.exe"), FileAccess::Read)
        .unwrap();
    let identity = image.identity().clone();
    let digest = image.digest().unwrap();
    drop(image);
    let worker = "tests::version_history_windows::HistoryWindows_ProcessWorker_013";
    let marker = temporary.path().join("ran");
    let launch = |path: &std::path::Path| {
        std::process::Command::new(path)
            .args(["--exact", worker, "--ignored"])
            .env("CC_DESK_HISTORY_PROBE_MARKER", &marker)
            .spawn()
    };
    let mut before = launch(&source).unwrap();
    assert!(before.wait().unwrap().success());
    std::fs::remove_file(&marker).unwrap();
    let mut fence = ImageFence::acquire(parent, name("source.exe"), &identity, &digest).unwrap();
    for _ in 0..16 {
        assert!(launch(&source).is_err());
    }
    fence
        .rename_to(private.directory().clone(), name("sealed.exe"))
        .unwrap();
    assert!(launch(&source).is_err());
    assert!(launch(&temporary.path().join("private/sealed.exe")).is_err());
    std::fs::copy(std::env::current_exe().unwrap(), &source).unwrap();
    let mut recreated = launch(&source).unwrap();
    assert!(recreated.wait().unwrap().success());
    assert!(marker.exists());
    fence.verify().unwrap();
}

// 检查摘要相同但文件对象已经被替换时不能取得 image fence。
#[test]
fn HistoryWindows_ChangedImage_020() {
    let (temporary, _, _) = private_fixture();
    let source = temporary.path().join("image.exe");
    std::fs::write(&source, b"same").unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let original = parent
        .open_file(name("image.exe"), FileAccess::Read)
        .unwrap();
    let identity = original.identity().clone();
    drop(original);
    std::fs::rename(&source, temporary.path().join("old.exe")).unwrap();
    std::fs::write(&source, b"same").unwrap();
    assert!(ImageFence::acquire(
        parent,
        name("image.exe"),
        &identity,
        &crate::version_history::verified_package::sha256(b"same")
    )
    .is_err());
}

// 检查硬链接别名阻止 fence，且已持有的排他 fence 不能产生新的硬链接别名。
#[test]
fn HistoryWindows_FenceAliases_021() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("image.exe");
    let alias = temporary.path().join("alias.exe");
    std::fs::write(&source, b"image").unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let original = parent
        .open_file(name("image.exe"), FileAccess::Read)
        .unwrap();
    let identity = original.identity().clone();
    drop(original);
    let digest = crate::version_history::verified_package::sha256(b"image");
    std::fs::hard_link(&source, &alias).unwrap();
    assert!(ImageFence::acquire(parent.clone(), name("image.exe"), &identity, &digest).is_err());
    std::fs::remove_file(&alias).unwrap();
    let fence = ImageFence::acquire(parent, name("image.exe"), &identity, &digest).unwrap();
    assert!(
        std::fs::hard_link(&source, &alias).is_err(),
        "a concurrent alias must not bypass the exclusive image fence"
    );
    fence.verify().unwrap();
}

// 检查同步起跑的 CreateProcess 与 fence 获取不能同时留下可执行的源进程。
#[test]
fn HistoryWindows_InFlightLaunch_022() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("race.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &source).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let original = parent
        .open_file(name("race.exe"), FileAccess::Read)
        .unwrap();
    let identity = original.identity().clone();
    let digest = original.digest().unwrap();
    drop(original);
    for attempt in 0..8 {
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let gate = barrier.clone();
        let path = source.clone();
        let marker = temporary.path().join(format!("race-{attempt}"));
        let child_marker = marker.clone();
        let worker = std::thread::spawn(move || {
            gate.wait();
            std::process::Command::new(path)
                .args([
                    "--exact",
                    "tests::version_history_windows::HistoryWindows_ProcessWorker_013",
                    "--ignored",
                ])
                .env("CC_DESK_HISTORY_PROBE_MARKER", child_marker)
                .env("CC_DESK_HISTORY_PROBE_WAIT", "1")
                .spawn()
        });
        barrier.wait();
        let fence = ImageFence::acquire(parent.clone(), name("race.exe"), &identity, &digest);
        let launch = worker.join().unwrap();
        match (fence, launch) {
            (Ok(fence), Ok(mut child)) => {
                let status = child.try_wait().unwrap();
                let running_source = status.is_none();
                if running_source {
                    child.wait().unwrap();
                }
                assert!(
                    !running_source && !marker.exists(),
                    "source image ran concurrently with an acquired exclusive fence"
                );
                fence.verify().unwrap();
            }
            (Ok(fence), Err(_)) => fence.verify().unwrap(),
            (Err(_), Ok(mut child)) => {
                assert!(child.wait().unwrap().success());
            }
            (Err(_), Err(_)) => {
                panic!("neither contending operation succeeded on disposable NTFS fixture")
            }
        }
    }
    ImageFence::acquire(parent, name("race.exe"), &identity, &digest).unwrap();
}

// 检查不同恢复目录的 control lease 不能授权另一个 lifetime 文件。
#[test]
fn HistoryWindows_ForeignControl_023() {
    let (_first_temporary, user, first) = private_fixture();
    let (_second_temporary, other_user, second) = private_fixture();
    let one = LeaseFiles::open(first, &user).unwrap();
    let two = LeaseFiles::open(second, &other_user).unwrap();
    let control = one.acquire_control().unwrap();
    assert!(two.acquire_shared(&control).is_err());
    assert!(two.acquire_exclusive(&control).is_err());
}

// 检查基于目录句柄的完整枚举包含未知文件、providers 和禁用资源目录。
#[test]
fn HistoryWindows_Enumerate_024() {
    let (temporary, _, private) = private_fixture();
    std::fs::write(temporary.path().join("private/providers.json"), b"{}").unwrap();
    std::fs::write(temporary.path().join("private/未知.txt"), b"saved").unwrap();
    std::fs::create_dir(temporary.path().join("private/disabled")).unwrap();
    let mut names: Vec<_> = private
        .directory()
        .read_children(10)
        .unwrap()
        .into_iter()
        .map(|name| name.os_string())
        .collect();
    names.sort();
    let mut expected = vec![
        std::ffi::OsString::from("providers.json"),
        std::ffi::OsString::from("未知.txt"),
        std::ffi::OsString::from("disabled"),
    ];
    expected.sort();
    assert_eq!(names, expected);
    assert!(private.directory().read_children(2).is_err());
}

// 检查未恢复的历史子进程在调用者丢弃、身份回执写失败、恢复意图写失败后被精确清理。
#[test]
fn HistoryWindows_UnresumedDrop_025() {
    for boundary in ["caller-drop", "identity-write", "resume-write"] {
        let (temporary, user, private) = private_fixture();
        let marker = temporary.path().join("not-started");
        let mut lease = exclusive(private.clone(), &user);
        let mut process = waiting_process(
            JobKind::HistoricalApplication,
            private,
            &user,
            &marker,
            &mut lease,
        );
        let exact = process.probe_exact().unwrap();
        if boundary == "identity-write" {
            std::fs::write(
                temporary
                    .path()
                    .join("private")
                    .join(process.probe_identity_name()),
                b"collision",
            )
            .unwrap();
            assert!(process.persist_identity(&user).is_err());
        }
        if boundary == "resume-write" {
            let receipt = process.persist_identity(&user).unwrap();
            std::fs::write(
                temporary
                    .path()
                    .join("private")
                    .join(process.probe_resume_name()),
                b"collision",
            )
            .unwrap();
            assert!(process.resume(&receipt).is_err());
        }
        drop(process);
        let cleaned = exact.terminal(2000).unwrap();
        // Cleanup is confined to the test child and cannot establish success.
        if cleaned.is_none() {
            stop_probe(&exact);
        }
        assert!(
            cleaned.is_some(),
            "never-resumed owner leaked at {boundary}"
        );
        assert!(
            !marker.exists(),
            "never-resumed child executed at {boundary}"
        );
    }
}

fn stop_probe(exact: &ExactProcess) {
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE,
        },
    };
    if exact.terminal(0).unwrap().is_some() {
        return;
    }
    // The exact retained process object prevents PID reuse while this handle is
    // opened. This is test-only cleanup of the child launched by this harness.
    unsafe {
        let process =
            OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, exact.pid()).unwrap();
        TerminateProcess(process, 0xccde0002).unwrap();
        let waited = WaitForSingleObject(process, 5000);
        CloseHandle(process).unwrap();
        assert_eq!(
            waited, WAIT_OBJECT_0,
            "disposable probe child cleanup timed out"
        );
    }
}

// 检查创建者进程崩溃后从已落盘 launch/job 身份清理未恢复子进程；恢复意图存在则拒绝终止。
#[test]
fn HistoryWindows_CrashRecovery_026() {
    for boundary in [
        "before-identity",
        "after-identity",
        "resume-intent",
        "torn-resume",
        "resumed",
    ] {
        let (temporary, user, private) = private_fixture();
        let mut owner = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::version_history_windows::HistoryWindows_CrashWorker_099",
                "--ignored",
                "--nocapture",
            ])
            .env("CC_DESK_HISTORY_CRASH_ROOT", temporary.path())
            .env("CC_DESK_HISTORY_CRASH_MODE", boundary)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let status = loop {
            if let Some(status) = owner.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                owner.kill().unwrap();
                owner.wait().unwrap();
                panic!("disposable crash owner timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(
            status.code(),
            Some(23),
            "owner worker did not reach crash boundary {boundary}"
        );
        let ready: serde_json::Value = serde_json::from_slice(
            &std::fs::read(temporary.path().join("owner-ready.json")).unwrap(),
        )
        .unwrap();
        let exact = ExactProcess::capture_observed(ready["pid"].as_u64().unwrap() as u32).unwrap();
        let mut lease = exclusive(private.clone(), &user);
        let record = DurableRecord::open(
            private.clone(),
            name(ready["name"].as_str().unwrap()),
            ready["digest"].as_str().unwrap(),
            &user,
        )
        .unwrap();
        let intent = DurableLaunchIntent::open(record, &user).unwrap();
        let recovered = recover_never_resumed(intent, private, &user, &mut lease);
        if matches!(boundary, "before-identity" | "after-identity") {
            if recovered.is_err() {
                stop_probe(&exact);
            }
            recovered
                .expect("definitely never-resumed crash must be recoverable")
                .verify()
                .unwrap();
            assert!(exact.terminal(0).unwrap().is_some());
            assert!(!temporary.path().join("nested-started").exists());
        } else {
            let still_live = exact.terminal(0).unwrap().is_none();
            if boundary == "resumed" {
                std::fs::write(
                    temporary
                        .path()
                        .join("nested-started")
                        .with_extension("release"),
                    b"release",
                )
                .unwrap();
                assert_eq!(exact.terminal(5000).unwrap().unwrap().exit_code(), 0);
            } else {
                // This controlled worker deliberately never called ResumeThread.
                // Production recovery must nevertheless preserve the uncertainty.
                stop_probe(&exact);
            }
            assert!(
                recovered.is_err(),
                "a durable resume intent must prohibit automatic cleanup"
            );
            assert!(
                still_live,
                "uncertain/resumed historical child was terminated"
            );
        }
    }
}

// 仅被监督测试执行；process::exit 模拟不执行 Rust Drop 的管理者崩溃。
#[test]
#[ignore = "explicitly invoked by HistoryWindows_CrashRecovery_026"]
fn HistoryWindows_CrashWorker_099() {
    let root = std::path::PathBuf::from(std::env::var_os("CC_DESK_HISTORY_CRASH_ROOT").unwrap());
    let boundary = std::env::var("CC_DESK_HISTORY_CRASH_MODE").unwrap();
    assert!(matches!(
        boundary.as_str(),
        "before-identity" | "after-identity" | "resume-intent" | "torn-resume" | "resumed"
    ));
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(&root).unwrap();
    let private =
        Arc::new(PrivateDirectory::open_existing(parent, name("private"), &user).unwrap());
    let mut lease = exclusive(private.clone(), &user);
    let marker = root.join("nested-started");
    let mut process = waiting_process(
        JobKind::HistoricalApplication,
        private,
        &user,
        &marker,
        &mut lease,
    );
    if boundary != "before-identity" {
        let receipt = process.persist_identity(&user).unwrap();
        if boundary == "resume-intent" {
            process.probe_commit_resume_intent(&receipt).unwrap();
        }
        if boundary == "torn-resume" {
            std::fs::write(
                root.join("private").join(process.probe_resume_name()),
                b"{partial",
            )
            .unwrap();
        }
        if boundary == "resumed" {
            process.resume(&receipt).unwrap();
            await_marker(&marker);
        }
    }
    let exact = process.probe_exact().unwrap();
    let (name, digest) = process.launch_record();
    std::fs::write(
        root.join("owner-ready.json"),
        serde_json::to_vec(
            &serde_json::json!({ "pid": exact.pid(), "name": name, "digest": digest }),
        )
        .unwrap(),
    )
    .unwrap();
    std::process::exit(23);
}

// 检查 ResumeThread 返回先前挂起计数 2 后不自动终止可能已执行的历史进程。
#[test]
fn HistoryWindows_UncertainResume_027() {
    let (temporary, user, private) = private_fixture();
    let marker = temporary.path().join("uncertain");
    let mut lease = exclusive(private.clone(), &user);
    let mut process = waiting_process(
        JobKind::HistoricalApplication,
        private,
        &user,
        &marker,
        &mut lease,
    );
    let receipt = process.persist_identity(&user).unwrap();
    process.probe_suspend_primary().unwrap();
    let resumed = process.resume(&receipt);
    let exact = process.probe_exact().unwrap();
    drop(process);
    let live = exact.terminal(0).unwrap().is_none();
    stop_probe(&exact);
    assert!(
        resumed.is_err(),
        "unexpected suspension count must remain uncertain"
    );
    assert!(
        live,
        "uncertain resume must not trigger historical child cleanup"
    );
    assert!(!marker.exists());
}
