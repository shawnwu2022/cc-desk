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
    let (temporary, user, private) = private_fixture();
    let path = temporary.path().join("private/source");
    std::fs::write(&path, b"source").unwrap();
    // Prepare the hostile fixture before admission pins its ancestor chain.
    drop(private);
    std::fs::hard_link(&path, temporary.path().join("alias")).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let private = PrivateDirectory::open_existing(parent, name("private"), &user).unwrap();
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

// 检查真实 NTFS 明确拒绝保留 no-delete 子文件句柄的父目录改名，并保持原对象与字节。
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
    let location = private.directory().observe_location().unwrap();
    let result = private.directory().rename_to(parent, name("rotated"));
    let error = match result {
        Ok(_) => panic!("retained-child NTFS contract unexpectedly changed"),
        Err(error) => error,
    };
    assert_eq!(
        error.to_string(),
        "relative NTFS rename failed: 0xc0000022, completion 0xc0000022"
    );
    assert_eq!(private.directory().identity(), &original);
    assert_eq!(private.directory().observe_location().unwrap(), location);
    assert!(temporary.path().join("private").is_dir());
    assert!(!temporary.path().join("rotated").exists());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.path().join("private/child.json"))
        .is_err());
    child.verify().unwrap();
    assert_eq!(child.bytes(), b"complete");
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
    let typed_fault = std::env::var("CC_DESK_MANAGER_TEST_CHECKPOINT").unwrap_or_default();
    // Only the explicit manager failure fixture withholds its actual marker.
    // Cleanup uses the existing controlled-child release, never a real CLI.
    if typed_fault == "typed-marker-timeout" {
        let mut gate = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(std::path::Path::new(&path).with_extension("gate-entered"))
            .unwrap();
        gate.write_all(b"waiting-before-marker").unwrap();
        gate.sync_all().unwrap();
        drop(gate);
        let release = std::env::var_os("CC_DESK_HISTORY_PROBE_RELEASE").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !std::path::Path::new(&release).try_exists().unwrap() {
            if std::time::Instant::now() >= deadline {
                std::process::exit(124);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
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
            if typed_fault == "typed-self-timeout"
                && std::fs::read(&release).unwrap() == b"timeout-124"
            {
                std::process::exit(124);
            }
        }
        if std::env::var_os("CC_DESK_HISTORY_PROBE_WAIT").is_some() {
            // Only this disposable test child waits. It always exits by itself;
            // historical-job tests never terminate it to obtain a green result.
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    }
    std::fs::write(
        std::path::Path::new(&path).with_extension("completed"),
        b"completed",
    )
    .unwrap();
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

// 检查历史应用在管理者释放后继续运行并由用户完成，消失的 job 名称不表示已退出。
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
    let prepared_job = PrivateJob::reopen(&receipt, &user).unwrap();
    assert_eq!(prepared_job.active_processes().unwrap(), 1);
    drop(prepared_job);
    process.resume(&receipt).unwrap();
    await_marker(&marker);
    assert!(
        PrivateJob::reopen(&receipt, &user).is_err(),
        "preparation receipt cannot claim current disarmed limits"
    );
    let exact = receipt.reopen_process().unwrap();
    drop(process);
    assert!(exact.terminal(0).unwrap().is_none());
    assert!(PrivateJob::reopen(&receipt, &user).is_err());
    std::fs::write(marker.with_extension("release"), b"release").unwrap();
    assert_eq!(exact.terminal(10_000).unwrap().unwrap().exit_code(), 0);
    assert!(marker.with_extension("completed").exists());
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
    // Windows chooses the kill-on-close exit code; it may be zero. The causal
    // evidence is a live, blocked child before final close and no normal return.
    assert!(!marker.with_extension("completed").exists());
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
    drop(parent);
    std::fs::rename(&source, temporary.path().join("old.exe")).unwrap();
    std::fs::write(&source, b"same").unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    assert!(ImageFence::acquire(
        parent,
        name("image.exe"),
        &identity,
        &crate::version_history::verified_package::sha256(b"same")
    )
    .is_err());
}

// 检查两个名称均不能绕过执行/数据保护；分别观察删除请求并拒绝失效的原名称绑定。
#[test]
fn HistoryWindows_FenceAliases_021() {
    for standard_unlink in [false, true] {
        for remove_source in [true, false] {
            probe_fenced_unlink(standard_unlink, remove_source);
        }
    }
}

fn probe_fenced_unlink(standard_unlink: bool, remove_source: bool) {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let operation = if standard_unlink { "std" } else { "win32" };
    let target = if remove_source { "source" } else { "alias" };
    let case = format!("{operation}/{target}");
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("image.exe");
    let alias = temporary.path().join("alias.exe");
    let quarantine = temporary.path().join("quarantined.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &source).unwrap();
    let marker = temporary.path().join("alias-probe");
    let launch = |path: &std::path::Path| {
        std::process::Command::new(path)
            .args([
                "--exact",
                "tests::version_history_windows::HistoryWindows_ProcessWorker_013",
                "--ignored",
            ])
            .env("CC_DESK_HISTORY_PROBE_MARKER", &marker)
            .env_remove("CC_DESK_HISTORY_PROBE_WAIT")
            .env_remove("CC_DESK_HISTORY_PROBE_RELEASE")
            .spawn()
    };
    assert!(launch(&source).unwrap().wait().unwrap().success());
    std::fs::remove_file(&marker).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let original = parent
        .open_file(name("image.exe"), FileAccess::Read)
        .unwrap();
    let identity = original.identity().clone();
    let digest = original.digest().unwrap();
    let observed_identity = probe_file_identity(&source);
    drop(original);
    drop(parent);
    std::fs::hard_link(&source, &alias).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    assert!(ImageFence::acquire(parent.clone(), name("image.exe"), &identity, &digest).is_err());
    drop(parent);
    std::fs::remove_file(&alias).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let mut fence =
        ImageFence::acquire(parent.clone(), name("image.exe"), &identity, &digest).unwrap();
    fence
        .verify()
        .expect("unchanged single-link fence must verify");
    let initial = probe_held_file(fence.probe_file());
    assert_eq!(initial.identity, observed_identity);
    assert_eq!(initial.links, 1);
    assert!(!initial.pending && initial.final_name.is_some());
    std::fs::hard_link(&source, &alias)
        .expect("real NTFS fixture must exercise an alias created during the fence");
    assert_eq!(probe_file_identity(&source), observed_identity);
    assert_eq!(probe_file_identity(&alias), observed_identity);
    assert_eq!(probe_held_file(fence.probe_file()).links, 2);

    // Complete both names' execution/data probes before any namespace mutation.
    for (label, path) in [("source", &source), ("alias", &alias)] {
        let launched = launch(path);
        if let Ok(mut escaped) = launched {
            let _ = escaped.kill();
            let _ = escaped.wait();
            panic!("{operation}/{target}: {label} bypassed the image execution fence");
        }
        assert!(
            std::fs::File::open(path).is_err(),
            "{label}: data read escaped"
        );
        assert!(
            std::fs::OpenOptions::new().write(true).open(path).is_err(),
            "{label}: data write escaped"
        );
        // A DELETE access request and a pathname deletion are separate probes.
        let deletion = std::fs::OpenOptions::new()
            .access_mode(DELETE.0)
            .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path);
        probe_namespace_log(format_args!("image namespace probe: operation={operation}, target={target}, name={label}, deleteOpen={}, error={:?}",
            deletion.is_ok(), deletion.as_ref().err().and_then(std::io::Error::raw_os_error)));
        drop(deletion);
    }
    assert!(!marker.exists());
    assert!(
        fence.verify().is_err(),
        "two links must invalidate verification"
    );
    assert!(
        fence
            .rename_to(parent.clone(), name("quarantined.exe"))
            .is_err(),
        "two links must block rename"
    );
    assert!(!quarantine.exists());

    let path = if remove_source { &source } else { &alias };
    let removal = if standard_unlink {
        std::fs::remove_file(path)
    } else {
        probe_delete_file(path)
    };
    probe_namespace_log(format_args!(
        "image namespace probe: operation={operation}, target={target}, removal={}, error={:?}",
        removal.is_ok(),
        removal
            .as_ref()
            .err()
            .and_then(std::io::Error::raw_os_error)
    ));
    // Success can mean deletion requested, pending, or a name actually removed.
    // Judge the held object and exact original binding, not the API's bool.
    probe_fence_binding(
        &mut fence, &parent, &source, &alias, &initial, &digest, &case,
    );
    assert!(!quarantine.exists());

    let replacement = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path);
    let replaced = replacement.is_ok();
    probe_namespace_log(format_args!("image namespace probe: operation={operation}, target={target}, createNew={replaced}, error={:?}",
        replacement.as_ref().err().and_then(std::io::Error::raw_os_error)));
    if let Ok(mut file) = replacement {
        file.write_all(b"independent replacement").unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert_ne!(probe_file_identity(path), observed_identity);
    }
    probe_fence_binding(
        &mut fence, &parent, &source, &alias, &initial, &digest, &case,
    );
    if replaced {
        assert_eq!(std::fs::read(path).unwrap(), b"independent replacement");
        if remove_source {
            assert!(
                fence.verify().is_err(),
                "substituted original name must refuse"
            );
        }
    }
    assert!(!quarantine.exists());
    assert_eq!(fence.identity(), &identity);
    assert!(!marker.exists());
}

fn probe_fence_binding(
    fence: &mut ImageFence,
    parent: &Arc<Directory>,
    source: &std::path::Path,
    alias: &std::path::Path,
    initial: &ProbeHeldFile,
    digest: &str,
    case: &str,
) {
    let held = probe_held_file(fence.probe_file());
    assert_eq!(held.identity, initial.identity, "original handle changed");
    assert_eq!(
        probe_held_digest(fence.probe_file()),
        digest,
        "held data changed"
    );
    let original_name = probe_metadata_file(source).map(|file| probe_held_file(&file));
    let alias_name = probe_metadata_file(alias).map(|file| probe_held_file(&file));
    let original_exact = original_name
        .as_ref()
        .is_ok_and(|state| state.identity == initial.identity);
    let names: Vec<_> = std::fs::read_dir(source.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    probe_namespace_log(format_args!("image namespace probe: case={case}, sourceListed={}, aliasListed={}, links={}, pending={}, heldNameUnchanged={}, sourceExact={original_exact}, sourceError={:?}, aliasExact={}, aliasError={:?}",
        names.iter().any(|name| name == source.file_name().unwrap()),
        names.iter().any(|name| name == alias.file_name().unwrap()),
        held.links, held.pending, held.final_name == initial.final_name,
        original_name.as_ref().err().and_then(std::io::Error::raw_os_error),
        alias_name.as_ref().is_ok_and(|state| state.identity == initial.identity),
        alias_name.as_ref().err().and_then(std::io::Error::raw_os_error)));
    let intact = original_exact
        && held.links == 1
        && !held.pending
        && held.final_name == initial.final_name
        && original_name
            .as_ref()
            .is_ok_and(|state| state.links == 1 && !state.pending);
    if intact {
        fence
            .verify()
            .expect("exact original single-link binding must remain usable");
        assert!(std::fs::File::open(source).is_err());
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(source)
            .is_err());
    } else {
        assert!(
            fence.verify().is_err(),
            "missing/pending/substituted/multiply-linked original must refuse"
        );
        assert!(
            fence
                .rename_to(parent.clone(), name("quarantined.exe"))
                .is_err(),
            "invalid original binding must not produce a rename receipt"
        );
    }
}

fn probe_namespace_log(message: std::fmt::Arguments<'_>) {
    // Bypass libtest capture for these bounded diagnostics, including passes.
    writeln!(std::io::stderr().lock(), "{message}").unwrap();
}

struct ProbeHeldFile {
    identity: (u64, [u8; 16]),
    links: u32,
    pending: bool,
    final_name: Option<Vec<u16>>,
}
fn probe_held_file(file: &std::fs::File) -> ProbeHeldFile {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
            FILE_ID_INFO, FILE_STANDARD_INFO, VOLUME_NAME_GUID,
        },
    };
    let raw = HANDLE(file.as_raw_handle());
    let mut identity = FILE_ID_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    let mut final_name = vec![0u16; 32768];
    unsafe {
        GetFileInformationByHandleEx(
            raw,
            FileIdInfo,
            (&mut identity as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
        .unwrap();
        GetFileInformationByHandleEx(
            raw,
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
        )
        .unwrap();
        let length = GetFinalPathNameByHandleW(raw, &mut final_name, VOLUME_NAME_GUID) as usize;
        let final_name = if length > 0 && length < final_name.len() {
            final_name.truncate(length);
            Some(final_name)
        } else {
            None
        };
        ProbeHeldFile {
            identity: (identity.VolumeSerialNumber, identity.FileId.Identifier),
            links: standard.NumberOfLinks,
            pending: standard.DeletePending,
            final_name,
        }
    }
}
fn probe_held_digest(mut file: &std::fs::File) -> String {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    format!("{:x}", hash.finalize())
}
fn probe_metadata_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };
    std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
}
fn probe_file_identity(path: &std::path::Path) -> (u64, [u8; 16]) {
    probe_held_file(&probe_metadata_file(path).unwrap()).identity
}
fn probe_delete_file(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::DeleteFileW;
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe { DeleteFileW(windows_core::PCWSTR(path.as_ptr())) }
        .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))
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
    try_stop_probe(exact).unwrap();
}
fn try_stop_probe(exact: &ExactProcess) -> std::io::Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows::Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0},
        System::Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE,
        },
    };
    if exact.terminal(0)?.is_some() {
        return Ok(());
    }
    // The exact retained process object prevents PID reuse while this handle is
    // opened. This is test-only cleanup of the child launched by this harness.
    unsafe {
        let raw = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, exact.pid())
            .map_err(std::io::Error::other)?;
        let owned = OwnedHandle::from_raw_handle(raw.0);
        let process = HANDLE(owned.as_raw_handle());
        if let Err(error) = TerminateProcess(process, 0xccde0002) {
            if WaitForSingleObject(process, 0) != WAIT_OBJECT_0 {
                return Err(std::io::Error::other(error));
            }
        }
        if WaitForSingleObject(process, 5000) != WAIT_OBJECT_0 {
            return Err(std::io::Error::other(
                "disposable probe child cleanup timed out",
            ));
        }
    }
    Ok(())
}
struct ProbeCleanup<'a>(&'a ExactProcess);
impl Drop for ProbeCleanup<'_> {
    fn drop(&mut self) {
        if try_stop_probe(self.0).is_err() {
            eprintln!("disposable exact-child cleanup failed");
        }
    }
}

// 检查准备阶段崩溃会由 job 清理真实子进程，解除自动清理后的不确定状态则保留子进程。
#[test]
fn HistoryWindows_CrashRecovery_026() {
    for boundary in [
        "before-identity",
        "after-identity",
        "resume-intent",
        "torn-resume",
        "disarm-readback",
        "disarmed",
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
        while !temporary.path().join("owner-ready").exists() {
            if owner.try_wait().unwrap().is_some() || std::time::Instant::now() >= deadline {
                let _ = owner.kill();
                let _ = owner.wait();
                panic!("disposable owner did not publish its exact child");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let ready: serde_json::Value = serde_json::from_slice(
            &std::fs::read(temporary.path().join("owner-ready.json")).unwrap(),
        )
        .unwrap();
        // Retain the actual process before the creator exits. Neither a missing
        // named job nor inability to reopen a dead PID is terminal evidence.
        let exact = ExactProcess::capture_observed(ready["pid"].as_u64().unwrap() as u32).unwrap();
        let _cleanup = ProbeCleanup(&exact);
        assert!(exact.terminal(0).unwrap().is_none());
        std::fs::write(temporary.path().join("owner-observed"), b"held exact child").unwrap();
        let status = loop {
            if let Some(status) = owner.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                owner.kill().unwrap();
                owner.wait().unwrap();
                stop_probe(&exact);
                panic!("disposable crash owner timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(
            status.code(),
            Some(23),
            "owner worker did not reach crash boundary {boundary}"
        );
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
        assert!(
            recovered.is_err(),
            "missing job or resume uncertainty must not claim recovery success"
        );
        if matches!(
            boundary,
            "before-identity" | "after-identity" | "resume-intent" | "torn-resume"
        ) {
            let cleaned = exact.terminal(2000).unwrap();
            if cleaned.is_none() {
                stop_probe(&exact);
            }
            assert!(
                cleaned.is_some(),
                "armed preparation crash leaked its never-started child at {boundary}"
            );
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
        "before-identity"
            | "after-identity"
            | "resume-intent"
            | "torn-resume"
            | "disarm-readback"
            | "disarmed"
            | "resumed"
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
    std::fs::write(root.join("owner-ready"), b"ready").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !root.join("owner-observed").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "observer did not retain child"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if boundary != "before-identity" {
        let receipt = process.persist_identity(&user).unwrap();
        if boundary == "resume-intent" {
            process.probe_commit_resume_intent(&receipt).unwrap();
        }
        if boundary == "disarm-readback" {
            process.probe_disarm_before_readback(&receipt).unwrap();
        }
        if boundary == "disarmed" {
            process.probe_disarm_before_resume(&receipt).unwrap();
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

// 检查链接诊断只发布固定类别，不能泄露任意目标、SID 或组件名称，也不授予跟随链接权限。
#[test]
fn HistoryWindows_LinkDiagnosis_028() {
    use crate::version_history::windows::registry::{link_diagnostic, RegistryView};
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    let encoded = |value: &str| {
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    let known = link_diagnostic(
        HKEY_LOCAL_MACHINE,
        RegistryView::View32,
        "Policies",
        &encoded(r"\REGISTRY\MACHINE\SOFTWARE\Policies"),
    );
    assert_eq!(known, "registry links are unsupported: hive=localMachine, view=View32, component=policies, target=machinePolicies");
    for value in [
        r"\REGISTRY\USER\S-1-private\Secrets",
        r"\REGISTRY\MACHINE\SOFTWARE\Policies\Redirect",
        "private\0target",
    ] {
        let unknown = link_diagnostic(
            HKEY_LOCAL_MACHINE,
            RegistryView::View64,
            "private-component",
            &encoded(value),
        );
        assert_eq!(unknown, "registry links are unsupported: hive=localMachine, view=View64, component=unknown, target=unknown");
    }
}

// 检查解除历史 job 自动清理后，回执失败不会恢复主线程，也不会重新武装或自动杀死子进程。
#[test]
fn HistoryWindows_LifetimeReceipt_029() {
    let (temporary, user, private) = private_fixture();
    let marker = temporary.path().join("not-resumed");
    let mut lease = exclusive(private.clone(), &user);
    let mut process = waiting_process(
        JobKind::HistoricalApplication,
        private,
        &user,
        &marker,
        &mut lease,
    );
    let receipt = process.persist_identity(&user).unwrap();
    let collision = temporary
        .path()
        .join("private")
        .join(process.probe_lifetime_name());
    std::fs::write(&collision, b"collision").unwrap();
    let outcome = process.resume(&receipt);
    let exact = receipt.reopen_process().unwrap();
    drop(process);
    let live = exact.terminal(0).unwrap().is_none();
    stop_probe(&exact);
    assert!(outcome.is_err());
    assert!(
        live,
        "post-disarm record failure must not re-arm or kill the child"
    );
    assert!(
        !marker.exists(),
        "record failure must not reach ResumeThread"
    );
    assert_eq!(std::fs::read(collision).unwrap(), b"collision");
}

// 检查目录允许 rename 所需的目标写访问，但仍拒绝祖先删除/替换，并持续保护子文件内容。
#[test]
fn HistoryWindows_DirectoryShare_030() {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_DATA, OPEN_EXISTING,
        SYNCHRONIZE,
    };
    use windows_core::PCWSTR;
    let (temporary, user, private) = private_fixture();
    let record = DurableRecord::create(private.clone(), name("held"), b"retained", &user).unwrap();
    let path: Vec<_> = temporary
        .path()
        .join("private")
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let open = |rights| unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            rights,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
        .map(|raw| std::os::windows::io::OwnedHandle::from_raw_handle(raw.0))
    };
    let writer = open((FILE_WRITE_DATA | SYNCHRONIZE).0)
        .expect("held destination directory must admit native rename helper write access");
    assert!(
        open(DELETE.0).is_err(),
        "directory must still deny DELETE access"
    );
    assert!(std::fs::rename(
        temporary.path().join("private"),
        temporary.path().join("replaced")
    )
    .is_err());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(temporary.path().join("private/held"))
        .is_err());
    assert!(std::fs::remove_file(temporary.path().join("private/held")).is_err());
    private.verify(&user).unwrap();
    record.verify().unwrap();
    drop(writer);
}

// 检查仅指定的系统策略别名可被识别，任意目标、位置、写用途和已配置策略仍拒绝。
#[test]
fn HistoryWindows_PolicyAlias_031() {
    use crate::version_history::windows::registry::{
        reject_policy_contents, shared_policy_alias, RegistryValue, RegistryView,
    };
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_LINK, REG_SZ,
    };
    let encode = |value: &str| {
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    let value = RegistryValue {
        kind: REG_LINK.0,
        bytes: encode(r"\REGISTRY\MACHINE\SOFTWARE\Policies"),
    };
    let path = "Software\\Policies\\Microsoft\\Edge\\WebView2\\UserDataFolder";
    assert!(shared_policy_alias(
        HKEY_LOCAL_MACHINE,
        RegistryView::View32,
        path,
        1,
        false,
        &value
    ));
    let terminated = RegistryValue {
        kind: REG_LINK.0,
        bytes: encode("\\registry\\machine\\software\\policies\0"),
    };
    assert!(shared_policy_alias(
        HKEY_LOCAL_MACHINE,
        RegistryView::View32,
        path,
        1,
        false,
        &terminated
    ));
    assert!(!shared_policy_alias(
        HKEY_CURRENT_USER,
        RegistryView::View32,
        path,
        1,
        false,
        &value
    ));
    assert!(!shared_policy_alias(
        HKEY_LOCAL_MACHINE,
        RegistryView::View64,
        path,
        1,
        false,
        &value
    ));
    assert!(!shared_policy_alias(
        HKEY_LOCAL_MACHINE,
        RegistryView::View32,
        path,
        0,
        false,
        &value
    ));
    assert!(!shared_policy_alias(
        HKEY_LOCAL_MACHINE,
        RegistryView::View32,
        path,
        1,
        true,
        &value
    ));
    for path in [
        "Software\\Other\\Microsoft\\Edge\\WebView2\\UserDataFolder",
        "Software\\Policies\\Other",
        "Software\\Policies",
    ] {
        assert!(!shared_policy_alias(
            HKEY_LOCAL_MACHINE,
            RegistryView::View32,
            path,
            1,
            false,
            &value
        ));
    }
    for bytes in [
        encode(r"\REGISTRY\USER\elsewhere"),
        encode(r"\REGISTRY\MACHINE\SOFTWARE\Policies\Redirect"),
        encode("\\REGISTRY\\MACHINE\\SOFTWARE\\Policies\0\0"),
        vec![0x00, 0xd8],
        vec![0x41],
    ] {
        let changed = RegistryValue {
            kind: REG_LINK.0,
            bytes,
        };
        assert!(!shared_policy_alias(
            HKEY_LOCAL_MACHINE,
            RegistryView::View32,
            path,
            1,
            false,
            &changed
        ));
    }
    let wrong_kind = RegistryValue {
        kind: REG_SZ.0,
        bytes: value.bytes,
    };
    assert!(!shared_policy_alias(
        HKEY_LOCAL_MACHINE,
        RegistryView::View32,
        path,
        1,
        false,
        &wrong_kind
    ));
    reject_policy_contents(0, 0).unwrap();
    for (values, subkeys) in [(1, 0), (0, 1), (u32::MAX, u32::MAX)] {
        assert!(reject_policy_contents(values, subkeys).is_err());
    }
}

fn make_probe_junction(directory: &std::path::Path, target: &std::path::Path) {
    use std::os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    };
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        },
        System::{
            Ioctl::FSCTL_SET_REPARSE_POINT, SystemServices::IO_REPARSE_TAG_MOUNT_POINT,
            IO::DeviceIoControl,
        },
    };
    use windows_core::PCWSTR;
    let path: Vec<_> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<_> = r"\??\"
        .encode_utf16()
        .chain(target.as_os_str().encode_wide())
        .collect();
    let bytes = target.len() * 2;
    assert!(bytes < 16000);
    // REPARSE_DATA_BUFFER mount-point header, substitute name + NUL, empty
    // print name + NUL. Storage is aligned and all offsets are byte offsets.
    let length = 16 + bytes + 4;
    let mut storage = vec![0u32; length.div_ceil(4)];
    let buffer =
        unsafe { std::slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u8>(), length) };
    buffer[..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer[4..6].copy_from_slice(&((length - 8) as u16).to_le_bytes());
    buffer[10..12].copy_from_slice(&(bytes as u16).to_le_bytes());
    buffer[12..14].copy_from_slice(&((bytes + 2) as u16).to_le_bytes());
    for (output, unit) in buffer[16..16 + bytes]
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(target)
    {
        *output = unit.to_le_bytes();
    }
    unsafe {
        let raw = CreateFileW(
            PCWSTR(path.as_ptr()),
            FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
        .unwrap();
        let owned = OwnedHandle::from_raw_handle(raw.0);
        let mut returned = 0;
        DeviceIoControl(
            HANDLE(owned.as_raw_handle()),
            FSCTL_SET_REPARSE_POINT,
            Some(storage.as_ptr().cast()),
            length as u32,
            None,
            0,
            Some(&mut returned),
            None,
        )
        .expect("disposable empty destination must actually become a junction");
    }
}

// 检查空目标目录被原位改为 junction 时拒绝操作，包括通过逐线程钩子命中预检与改名之间的窗口。
#[test]
fn HistoryWindows_RenameReparse_032() {
    use crate::version_history::windows::files::probe_before_rename;
    for after_precheck in [false, true] {
        let (temporary, user, private) = private_fixture();
        let foreign = temporary.path().join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        std::fs::write(foreign.join("untouched"), b"foreign fixture").unwrap();
        let source = temporary.path().join("source.exe");
        std::fs::write(&source, b"held image").unwrap();
        let parent = Directory::open_absolute(temporary.path()).unwrap();
        let image = parent
            .open_file(name("source.exe"), FileAccess::Read)
            .unwrap();
        let identity = image.identity().clone();
        let digest = image.digest().unwrap();
        drop(image);
        let mut fence =
            ImageFence::acquire(parent, name("source.exe"), &identity, &digest).unwrap();
        let destination = temporary.path().join("private");
        let changed = destination.clone();
        let target = foreign.clone();
        let exercised = std::rc::Rc::new(std::cell::Cell::new(false));
        let exercised_in_hook = exercised.clone();
        let hook = if after_precheck {
            Some(probe_before_rename(move || {
                make_probe_junction(&changed, &target);
                assert_eq!(
                    std::fs::read(changed.join("untouched")).unwrap(),
                    b"foreign fixture"
                );
                exercised_in_hook.set(true);
            }))
        } else {
            make_probe_junction(&destination, &foreign);
            assert_eq!(
                std::fs::read(destination.join("untouched")).unwrap(),
                b"foreign fixture"
            );
            assert!(private.verify(&user).is_err());
            assert!(private
                .directory()
                .open_file(name("untouched"), FileAccess::Read)
                .is_err());
            None
        };
        let result = fence.rename_to(private.directory().clone(), name("moved.exe"));
        drop(hook);
        assert!(
            !after_precheck || exercised.get(),
            "native-call race hook must actually execute"
        );
        assert!(
            result.is_err(),
            "changed destination must not produce a success receipt"
        );
        assert_eq!(
            fence.observe_location().unwrap().0,
            identity,
            "original source handle must survive refusal"
        );
        let contents: Vec<_> = std::fs::read_dir(&foreign)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            contents,
            [std::ffi::OsString::from("untouched")],
            "native rename followed the changed destination"
        );
        assert_eq!(
            std::fs::read(foreign.join("untouched")).unwrap(),
            b"foreign fixture"
        );
    }
}

// 检查同一构造方式的空目录能成功改名，单独隔离非空目录内保留子文件句柄的限制。
#[test]
fn HistoryWindows_EmptyTreeRename_033() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let private =
        PrivateDirectory::create_renameable_new(parent.clone(), name("private"), &user).unwrap();
    let original = private.directory().identity().clone();
    let receipt = private
        .directory()
        .rename_to(parent, name("rotated"))
        .expect("empty directory positive control must succeed with the same constructor");
    assert_eq!(receipt.identity(), &original);
    assert!(!temporary.path().join("private").exists());
    assert!(temporary.path().join("rotated").is_dir());
    private.verify(&user).unwrap();
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct ProbeTreeEntry {
    directory: bool,
    identity: crate::version_history::windows::files::FileIdentity,
    digest: Option<String>,
    size: u64,
    attributes: u32,
}
struct ProbeTree {
    entries: std::collections::BTreeMap<std::path::PathBuf, ProbeTreeEntry>,
    directories: Vec<Arc<Directory>>,
    files: Vec<crate::version_history::windows::files::PinnedFile>,
}
impl ProbeTree {
    fn verify(&self) {
        for directory in &self.directories {
            directory.recheck().unwrap();
        }
        for file in &self.files {
            file.verify().unwrap();
        }
    }
}
fn admit_probe_tree(root: Arc<Directory>) -> ProbeTree {
    use std::os::windows::fs::MetadataExt;
    fn walk(directory: Arc<Directory>, relative: &std::path::Path, tree: &mut ProbeTree) {
        let path = std::path::PathBuf::from(directory.path().unwrap());
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        tree.entries.insert(
            relative.to_owned(),
            ProbeTreeEntry {
                directory: true,
                identity: directory.identity().clone(),
                digest: None,
                size: 0,
                attributes: metadata.file_attributes(),
            },
        );
        for child in directory.read_children(32).unwrap() {
            let next = relative.join(child.os_string());
            let actual = path.join(child.os_string());
            let metadata = std::fs::symlink_metadata(&actual).unwrap();
            if metadata.is_dir() {
                let held = directory.open_directory(child).unwrap();
                walk(held, &next, tree);
            } else {
                assert!(metadata.is_file());
                let held = directory.open_file(child, FileAccess::Read).unwrap();
                tree.entries.insert(
                    next,
                    ProbeTreeEntry {
                        directory: false,
                        identity: held.identity().clone(),
                        digest: Some(held.digest().unwrap()),
                        size: metadata.len(),
                        attributes: metadata.file_attributes(),
                    },
                );
                tree.files.push(held);
            }
        }
        directory.recheck().unwrap();
        tree.directories.push(directory);
    }
    let mut tree = ProbeTree {
        entries: Default::default(),
        directories: vec![],
        files: vec![],
    };
    walk(root, std::path::Path::new(""), &mut tree);
    tree.verify();
    tree
}
fn copy_probe_tree(source: &std::path::Path, destination: &std::path::Path, tree: &ProbeTree) {
    tree.verify();
    for (relative, entry) in &tree.entries {
        if relative.as_os_str().is_empty() {
            continue;
        }
        let output = destination.join(relative);
        if entry.directory {
            std::fs::create_dir(&output).unwrap();
        } else {
            let bytes = std::fs::read(source.join(relative)).unwrap();
            assert_eq!(
                Some(crate::version_history::verified_package::sha256(&bytes)),
                entry.digest
            );
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)
                .unwrap();
            file.write_all(&bytes).unwrap();
            file.sync_all().unwrap(); // Regular file FlushFileBuffers, never directory fsync.
        }
    }
    tree.verify();
}
fn assert_probe_copy(original: &ProbeTree, copied: &ProbeTree) {
    assert_eq!(
        original.entries.keys().collect::<Vec<_>>(),
        copied.entries.keys().collect::<Vec<_>>()
    );
    for (path, entry) in &original.entries {
        let copy = &copied.entries[path];
        assert_ne!(
            entry.identity, copy.identity,
            "copy must be independent for {path:?}"
        );
        assert_eq!(
            (entry.directory, &entry.digest, entry.size, entry.attributes),
            (copy.directory, &copy.digest, copy.size, copy.attributes)
        );
    }
    original.verify();
    copied.verify();
}

// 检查先验证独立副本，再显式丢弃子级证明并重新接纳；间隙写入/新增和 fresh 路径冲突均保留两份数据并阻止推进。
#[test]
fn HistoryWindows_TreeHandoff_034() {
    // OS-only fixture: full production snapshot/permissions comparison and
    // SourceContext/SourceSealed journal binding remain Task4b responsibilities.
    for mutation in ["unchanged", "write", "added", "collision"] {
        let (temporary, user, control_root) = private_fixture();
        let parent = Directory::open_absolute(temporary.path()).unwrap();
        let source =
            PrivateDirectory::create_renameable_new(parent.clone(), name("source"), &user).unwrap();
        let source_path = temporary.path().join("source");
        std::fs::create_dir(source_path.join("nested")).unwrap();
        std::fs::create_dir(source_path.join("empty")).unwrap();
        std::fs::write(source_path.join("known.json"), b"{\"saved\":true}").unwrap();
        std::fs::write(
            source_path.join("nested").join("未知.data"),
            b"unknown content preserved",
        )
        .unwrap();
        let copy = PrivateDirectory::create_new(parent.clone(), name("copy"), &user).unwrap();
        source
            .directory()
            .require_disjoint(&[control_root.directory().clone(), copy.directory().clone()])
            .unwrap();
        let leases = LeaseFiles::open(control_root.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let lease = leases.acquire_exclusive(&control).unwrap();
        drop(control);
        let image_path = temporary.path().join("manager.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &image_path).unwrap();
        let image = parent
            .open_file(name("manager.exe"), FileAccess::Read)
            .unwrap();
        let image_identity = image.identity().clone();
        let image_digest = image.digest().unwrap();
        drop(image);
        let fence = ImageFence::acquire(
            parent.clone(),
            name("manager.exe"),
            &image_identity,
            &image_digest,
        )
        .unwrap();
        let original = admit_probe_tree(source.directory().clone());
        assert_eq!(original.entries.len(), 5);
        let m0 = original.entries.clone();
        let copy_path = temporary.path().join("copy");
        copy_probe_tree(&source_path, &copy_path, &original);
        let copied = admit_probe_tree(copy.directory().clone());
        assert_probe_copy(&original, &copied);
        let c0 = copied.entries.clone();
        let source_manifest = DurableRecord::create(
            control_root.clone(),
            name("source-observed.json"),
            &serde_json::to_vec(&m0).unwrap(),
            &user,
        )
        .unwrap();
        let copy_manifest = DurableRecord::create(
            control_root.clone(),
            name("copy-observed.json"),
            &serde_json::to_vec(&c0).unwrap(),
            &user,
        )
        .unwrap();
        let mapping: Vec<_> = m0
            .iter()
            .map(|(path, entry)| (path, &entry.identity, &c0[path].identity))
            .collect();
        let ready = DurableRecord::create(control_root.clone(), name("copy-ready.json"), &serde_json::to_vec(&serde_json::json!({
            "sourceManifest": source_manifest.digest(), "copyManifest": copy_manifest.digest(), "mapping": mapping,
        })).unwrap(), &user).unwrap();
        let rotation = DurableRecord::create(control_root.clone(), name("rotation-intent.json"), &serde_json::to_vec(&serde_json::json!({
            "copyReady": ready.digest(), "rootIdentity": source.directory().identity(), "from": "source", "to": "rotated",
        })).unwrap(), &user).unwrap();
        // Consume ONLY descendant guards. This deliberately invalidates the old
        // child proof; root DELETE handle, ancestors, fence and lease stay held.
        drop(original);
        match mutation {
            "write" => std::fs::write(source_path.join("known.json"), b"changed during guard loss")
                .unwrap(),
            "added" => {
                std::fs::write(source_path.join("extra-entry"), b"new during guard loss").unwrap()
            }
            _ => (),
        }
        let control = leases.acquire_control().unwrap();
        assert!(leases.acquire_shared(&control).is_err());
        drop(control);
        fence.verify().unwrap();
        let moved = source
            .directory()
            .rename_to(parent.clone(), name("rotated"))
            .unwrap();
        assert_eq!(moved.identity(), source.directory().identity());
        assert!(!source_path.exists());
        let rotated = temporary.path().join("rotated");
        assert!(rotated.is_dir());
        rotation.verify().unwrap();
        let admitted = admit_probe_tree(source.directory().clone());
        let observed_after = admitted.entries.clone();
        let unchanged = m0 == observed_after;
        if mutation == "collision" {
            std::fs::create_dir(&source_path).unwrap();
            std::fs::write(source_path.join("keep"), b"pre-existing fresh-path owner").unwrap();
        }
        let occupied = parent.read_children(32).unwrap().contains(&name("source"));
        if unchanged && !occupied {
            assert_eq!(mutation, "unchanged");
            let admission = DurableRecord::create(control_root.clone(), name("fresh-admission.json"), &serde_json::to_vec(&serde_json::json!({
                "rotationIntent": rotation.digest(), "sourceManifest": source_manifest.digest(), "observed": observed_after, "rootLocation": "rotated",
            })).unwrap(), &user).unwrap();
            let fresh =
                PrivateDirectory::create_new(parent.clone(), name("source"), &user).unwrap();
            assert_ne!(fresh.directory().identity(), source.directory().identity());
            assert!(fresh.directory().read_children(8).unwrap().is_empty());
            admission.verify().unwrap();
        } else {
            assert_ne!(mutation, "unchanged");
            assert!(!temporary
                .path()
                .join("private")
                .join("fresh-admission.json")
                .exists());
            if mutation == "collision" {
                assert!(
                    PrivateDirectory::create_new(parent.clone(), name("source"), &user).is_err()
                );
                assert_eq!(
                    std::fs::read(source_path.join("keep")).unwrap(),
                    b"pre-existing fresh-path owner"
                );
                assert!(unchanged);
            } else {
                assert!(!source_path.exists());
                assert!(!unchanged);
                if mutation == "write" {
                    assert_eq!(
                        std::fs::read(rotated.join("known.json")).unwrap(),
                        b"changed during guard loss"
                    );
                } else {
                    assert_eq!(
                        std::fs::read(rotated.join("extra-entry")).unwrap(),
                        b"new during guard loss"
                    );
                }
            }
        }
        admitted.verify();
        copied.verify();
        assert_eq!(admit_probe_tree(copy.directory().clone()).entries, c0);
        source_manifest.verify().unwrap();
        copy_manifest.verify().unwrap();
        ready.verify().unwrap();
        fence.verify().unwrap();
        let launch = std::process::Command::new(&image_path)
            .args([
                "--exact",
                "tests::version_history_windows::HistoryWindows_ProcessWorker_013",
                "--ignored",
            ])
            .env(
                "CC_DESK_HISTORY_PROBE_MARKER",
                temporary.path().join("escaped-manager"),
            )
            .env_remove("CC_DESK_HISTORY_PROBE_WAIT")
            .env_remove("CC_DESK_HISTORY_PROBE_RELEASE")
            .spawn();
        if let Ok(mut escaped) = launch {
            let _ = escaped.kill();
            let _ = escaped.wait();
            panic!("retained executable fence permitted a launch during handoff");
        }
        let control = leases.acquire_control().unwrap();
        assert!(leases.acquire_shared(&control).is_err());
        drop(control);
        drop(lease);
    }
}

// 检查确定的读取句柄只阻塞首次排他打开，释放后取得同一对象且不把后续失败归为等待。
#[test]
fn HistoryWindows_FenceBusy_035() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("image.exe");
    std::fs::write(&source, b"unchanged image fixture").unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let reader = parent
        .open_file(name("image.exe"), FileAccess::Read)
        .unwrap();
    let identity = reader.identity().clone();
    let digest = reader.digest().unwrap();
    for _ in 0..2 {
        let error = ImageFence::acquire(parent.clone(), name("image.exe"), &identity, &digest)
            .err()
            .expect("known reader must exclude an exclusive open");
        assert!(ImageFence::is_acquisition_busy(&error));
        assert_eq!(reader.identity(), &identity);
        assert_eq!(reader.digest().unwrap(), digest);
        assert!(!temporary.path().join("sealed.exe").exists());
    }
    assert!(!ImageFence::is_acquisition_busy(&std::io::Error::new(
        std::io::ErrorKind::WouldBlock,
        "initial exclusive NTFS open is busy: 0xc0000043"
    )));
    drop(reader);
    let mut fence =
        ImageFence::acquire(parent.clone(), name("image.exe"), &identity, &digest).unwrap();
    assert_eq!(fence.identity(), &identity);
    fence.verify().unwrap();
    let other_open = parent
        .open_file(name("image.exe"), FileAccess::Read)
        .err()
        .expect("fenced image must exclude a data reader");
    assert!(!ImageFence::is_acquisition_busy(&other_open));
    std::fs::write(temporary.path().join("sealed.exe"), b"collision sentinel").unwrap();
    let rename = fence
        .rename_to(parent, name("sealed.exe"))
        .err()
        .expect("rename collision must fail");
    assert!(!ImageFence::is_acquisition_busy(&rename));
    fence.verify().unwrap();
    assert_eq!(
        std::fs::read(temporary.path().join("sealed.exe")).unwrap(),
        b"collision sentinel"
    );
    std::fs::hard_link(&source, temporary.path().join("extra-link.exe")).unwrap();
    let validation = fence
        .verify()
        .expect_err("later link-count validation must remain a hard failure");
    assert!(!ImageFence::is_acquisition_busy(&validation));
}

// 检查等待期间身份置换或原对象字节变化都在取得排他句柄后立即拒绝，不再当作忙碌重试。
#[test]
fn HistoryWindows_FenceDrift_036() {
    for replace_identity in [true, false] {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("image.exe");
        std::fs::write(&source, b"original image fixture").unwrap();
        let parent = Directory::open_absolute(temporary.path()).unwrap();
        let reader = parent
            .open_file(name("image.exe"), FileAccess::Read)
            .unwrap();
        let identity = reader.identity().clone();
        let digest = reader.digest().unwrap();
        let busy = ImageFence::acquire(parent.clone(), name("image.exe"), &identity, &digest)
            .err()
            .unwrap();
        assert!(ImageFence::is_acquisition_busy(&busy));
        drop(reader);
        if replace_identity {
            std::fs::rename(&source, temporary.path().join("retained-original.exe")).unwrap();
            std::fs::write(&source, b"original image fixture").unwrap();
        } else {
            std::fs::write(&source, b"changed image fixture").unwrap();
        }
        let current = parent
            .open_file(name("image.exe"), FileAccess::Read)
            .unwrap();
        assert_eq!(current.identity() != &identity, replace_identity);
        drop(current);
        let failure = ImageFence::acquire(parent, name("image.exe"), &identity, &digest)
            .err()
            .expect("changed original binding must fail admission");
        assert!(!ImageFence::is_acquisition_busy(&failure));
        assert!(source.exists());
        assert!(!temporary.path().join("sealed.exe").exists());
        if replace_identity {
            assert_eq!(
                std::fs::read(temporary.path().join("retained-original.exe")).unwrap(),
                b"original image fixture"
            );
        } else {
            assert_eq!(std::fs::read(&source).unwrap(), b"changed image fixture");
        }
    }
}
