//! Windows production-wrapper probes; positive cases require a job-free host.
//! From the repository root, first run `node scripts/prepare-conpty.mjs`, then:
//! `cargo test --manifest-path src-tauri/Cargo.toml --lib version_history::windows::manager_process::tests -- --test-threads=1`
//! The ignored worker is launched by exact libtest name with bounded supervision.
//! These probes do not certify actual WebView readiness or full manager handoff.

use super::super::files::{Directory, FileAccess};
use super::*;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};
use windows::Win32::System::{
    JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        QueryInformationJobObject, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
    },
    Threading::{
        GetProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE,
    },
};

const TRANSACTION: &str = "11111111-1111-4111-8111-111111111111";
const WORKER: &str =
    "version_history::windows::manager_process::tests::HistoryManager_AtomicWorker_090";
const CHECKPOINT_ENV: &str = "CC_DESK_MANAGER_TEST_CHECKPOINT";
const DIRECTORY_ENV: &str = "CC_DESK_MANAGER_TEST_DIRECTORY";
const MODE_ENV: &str = "CC_DESK_MANAGER_TEST_MODE";
#[path = "version_history_manager_typed_probe.rs"]
mod typed_probe;
static CHECKPOINT_PID: AtomicU32 = AtomicU32::new(0);
thread_local! {
    static CONCURRENT_CHILD: std::cell::RefCell<Option<ExactProcess>> = const { std::cell::RefCell::new(None) };
}

// 检查挂起管理者在创建时就属于私有准备 job，释放准备所有者后精确进程退出。
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
    require_job_free_source().unwrap_or_else(|error| {
        panic!(
            "positive manager probe requires a job-free source: {error:?}; source_in_job={source_in_job}; source_job_limits={source_job_limits:?}"
        )
    });
    let job = ManagerPreparationJob::create(TRANSACTION, &user).unwrap();
    let (process, thread, mut pending) =
        create_manager_process(image, TRANSACTION, &job)
            .unwrap_or_else(|error| {
                panic!(
                    "suspended manager creation failed: {error:?}; source_in_job={source_in_job}; source_job_limits={source_job_limits:?}"
                )
            });
    assert!(in_job(handle(pending.0.as_ref().unwrap())).unwrap());
    assert!(process.is_in_job(Some(handle(&job.handle))).unwrap());
    assert_eq!(job.phase, Some(ManagerJobPhase::ArmedPreparation));
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
    assert!(!temp.path().join("bundle").join(READY).exists());
    // Remove the explicit never-resumed cleanup so only last-job-handle close
    // can make this assertion pass. The observer holds process handles only.
    drop(pending.0.take());
    drop(job);
    assert!(process.terminal(5000).unwrap().is_some());
    assert!(reopened.terminal(0).unwrap().is_some());
    let terminal = process.retain_terminal().unwrap();
    assert_eq!(terminal.identity(), process.identity());
    drop(reopened);
    drop(process);
    terminal.verify().unwrap();
    drop(thread);
}
// 检查新版本就绪链必须绑定同一 job、解除准备记录和恢复记录。
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
    let job = ManagerPreparationJob::create(transaction, &user).unwrap();
    let launch = LaunchBinding {
        schema: 2,
        transaction: transaction.into(),
        data_root: root.directory().identity().clone(),
        bundle: bundle.reference().clone(),
        package_root: root.directory().identity().clone(),
        package_digest: "1".repeat(64),
        source: process.identity().clone(),
        command_digest: "2".repeat(64),
        job: job.identity.clone(),
    };
    let launch_record = ManagerRecord::create(root.clone(), LAUNCH, &launch, &user).unwrap();
    let process_record = ManagerRecord::create(
        root.clone(),
        PROCESS,
        &ProcessBinding {
            schema: 2,
            transaction: transaction.into(),
            launch: launch_record.reference().clone(),
            process: process.identity().clone(),
        },
        &user,
    )
    .unwrap();
    let disarm_record = ManagerRecord::create(
        root.clone(),
        DISARM,
        &DisarmBinding {
            schema: 1,
            transaction: transaction.into(),
            data_root: root.directory().identity().clone(),
            launch: launch_record.reference().clone(),
            process: process_record.reference().clone(),
            job: job.identity.clone(),
        },
        &user,
    )
    .unwrap();
    let resume_record = ManagerRecord::create(
        root.clone(),
        RESUME,
        &ResumeBinding {
            schema: 2,
            transaction: transaction.into(),
            data_root: root.directory().identity().clone(),
            launch: launch_record.reference().clone(),
            process: process_record.reference().clone(),
            disarm: disarm_record.reference().clone(),
            job: job.identity.clone(),
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
// 检查管理者命令只接受绑定路径和规范事务标识。
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

// The production wrapper calls this only in cfg(test) builds. Every worker is
// invoked by its exact libtest name with a private directory; normal tests have
// no hook configuration. These are actual syscall boundaries, not replacement
// implementations of job creation, process creation or disarming.
pub(super) fn manager_checkpoint(label: &str, process: Option<HANDLE>) -> io::Result<()> {
    if let Some(process) = process {
        CHECKPOINT_PID.store(unsafe { GetProcessId(process) }, Ordering::SeqCst);
        if label == "create-returned"
            && std::env::var(MODE_ENV).is_ok_and(|mode| mode == "assign-after-check")
        {
            let child = ExactProcess::capture_observed(unsafe { GetProcessId(process) })?;
            CONCURRENT_CHILD.with(|slot| *slot.borrow_mut() = Some(child));
        }
    }
    let Some(target) = std::env::var_os(CHECKPOINT_ENV) else {
        return Ok(());
    };
    if target != OsStr::new(label) {
        return Ok(());
    }
    let directory = PathBuf::from(
        std::env::var_os(DIRECTORY_ENV).ok_or_else(|| blocked("test directory missing"))?,
    );
    let bytes = serde_json::to_vec(&serde_json::json!({
        "checkpoint": label,
        "pid": CHECKPOINT_PID.load(Ordering::SeqCst),
    }))?;
    std::fs::write(directory.join("checkpoint.tmp"), bytes)?;
    std::fs::rename(
        directory.join("checkpoint.tmp"),
        directory.join("checkpoint.json"),
    )?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !directory.join("release").exists() {
        if Instant::now() >= deadline {
            return Err(blocked("bounded manager checkpoint was not released"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if std::env::var(MODE_ENV).is_ok_and(|mode| mode == "disarm-error") {
        return Err(blocked("injected disarm checkpoint failure"));
    }
    if label == "before-create"
        && std::env::var(MODE_ENV).is_ok_and(|mode| mode == "assign-after-check")
    {
        require_job_free_source()?;
        let foreign = unsafe { own(CreateJobObjectW(None, PCWSTR::null()).map_err(win_error)?) };
        unsafe {
            AssignProcessToJobObject(handle(&foreign), GetCurrentProcess()).map_err(win_error)?;
        }
        assert!(in_job(unsafe { GetCurrentProcess() })?);
        // A zero-limit job remains associated until this disposable source
        // exits. The observer never acquires a handle to either job.
    }
    Ok(())
}

// This supervisor owns only a process handle. In particular it never opens or
// duplicates the manager's armed job, which would invalidate last-handle tests.
struct Worker {
    child: Child,
    directory: PathBuf,
    cleanup_armed: bool,
}
impl Worker {
    fn spawn(directory: &Path, mode: &str, checkpoint: &str) -> Self {
        Self {
            child: Command::new(std::env::current_exe().unwrap())
                .args(["--exact", WORKER, "--ignored", "--nocapture"])
                .env(DIRECTORY_ENV, directory)
                .env(MODE_ENV, mode)
                .env(CHECKPOINT_ENV, checkpoint)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("spawn exact manager wrapper worker"),
            directory: directory.to_owned(),
            cleanup_armed: true,
        }
    }
    fn await_file(&mut self, path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !path.exists() {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "manager worker exited before {}",
                path.display()
            );
            assert!(
                Instant::now() < deadline,
                "timed out awaiting {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn finish(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "manager worker failed: {status}");
                return;
            }
            assert!(Instant::now() < deadline, "manager worker did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn hard_kill(&mut self) {
        self.child.kill().expect("hard-kill exact source worker");
        assert!(!self.child.wait().unwrap().success());
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if !self.cleanup_armed {
            return;
        }
        if self.child.try_wait().ok().flatten().is_none() {
            // On assertion failure, first let the bounded fixture clean up a
            // disarmed child before resorting to hard source termination.
            let _ = std::fs::write(self.directory.join("release"), b"release");
            let _ = std::fs::write(self.directory.join("cleanup"), b"cleanup");
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// A process-only cleanup guard prevents a failing assertion from leaving a
// disarmed suspended test executable behind. It never participates in a job.
struct ObservedChild {
    exact: ExactProcess,
    cleanup: OwnedHandle,
    cleanup_armed: bool,
}
impl ObservedChild {
    fn capture(pid: u32) -> Self {
        let exact = ExactProcess::capture_observed(pid).unwrap();
        let cleanup = unsafe {
            own(OpenProcess(
                PROCESS_TERMINATE | PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
            .unwrap())
        };
        // The first exact handle keeps the process object alive while the
        // termination handle is opened, so this cannot select a reused PID.
        assert!(exact.terminal(0).unwrap().is_none());
        Self {
            exact,
            cleanup,
            cleanup_armed: true,
        }
    }
}
impl Drop for ObservedChild {
    fn drop(&mut self) {
        if !self.cleanup_armed {
            return;
        }
        if self.exact.terminal(0).ok().flatten().is_none() {
            let _ = stop_suspended(&self.cleanup);
        }
    }
}

// 检查真实准备、创建、身份和解除阶段的 source 硬退出，不保留任何 job 句柄。
#[test]
fn HistoryManager_SourceCrash_004() {
    require_job_free_source().expect("positive crash matrix requires a job-free Windows host");
    for (checkpoint, has_child, survives) in [
        ("job-armed", false, false),
        ("before-create", false, false),
        ("create-returned", true, false),
        ("identity-captured", true, false),
        ("identity-persisted", true, false),
        ("intent-persisted", true, false),
        ("disarm-intent", true, false),
        ("disarm-set", true, true),
        ("disarm-verified", true, true),
        ("lifetime-persisted", true, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut source = Worker::spawn(temp.path(), "crash", checkpoint);
        let marker = temp.path().join("checkpoint.json");
        source.await_file(&marker);
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(marker).unwrap()).unwrap();
        assert_eq!(record["checkpoint"], checkpoint);
        let pid = u32::try_from(record["pid"].as_u64().unwrap()).unwrap();
        assert_eq!(pid != 0, has_child, "wrong child presence at {checkpoint}");
        let child = has_child.then(|| ObservedChild::capture(pid));
        source.hard_kill();
        if let Some(child) = child {
            let terminal = child
                .exact
                .terminal(if survives { 250 } else { 5000 })
                .unwrap();
            assert_eq!(
                terminal.is_none(),
                survives,
                "wrong exact manager lifetime after source death at {checkpoint}"
            );
        }
        assert!(
            !temp.path().join("bundle").join(READY).exists(),
            "suspended manager published ready at {checkpoint}"
        );
    }
}

// 检查已有 job 即使允许显式或静默 breakaway，也在任何管理者创建前拒绝。
#[test]
fn HistoryManager_ContainedSources_005() {
    require_job_free_source().expect("containment fixtures need a known job-free outer host");
    for mode in [
        "deny-breakaway",
        "allow-breakaway",
        "silent-breakaway",
        "nested-jobs",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut source = Worker::spawn(temp.path(), mode, "before-create");
        source.await_file(&temp.path().join("complete"));
        source.finish();
        assert!(
            !temp.path().join("checkpoint.json").exists(),
            "{mode} reached CreateProcess"
        );
        assert!(
            !temp.path().join("bundle").join(READY).exists(),
            "{mode} produced a readiness receipt"
        );
    }
}

// 检查 Set 已成功但 readback 失败时保持 unknown，不能重试或重新装备准备 job。
#[test]
fn HistoryManager_DisarmUnknown_006() {
    require_job_free_source().expect("disarm probe requires a job-free Windows host");
    for checkpoint in ["disarm-intent", "disarm-set"] {
        let temp = tempfile::tempdir().unwrap();
        let mut source = Worker::spawn(temp.path(), "disarm-error", checkpoint);
        source.await_file(&temp.path().join("checkpoint.json"));
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(temp.path().join("checkpoint.json")).unwrap())
                .unwrap();
        let child = ObservedChild::capture(u32::try_from(record["pid"].as_u64().unwrap()).unwrap());
        std::fs::write(temp.path().join("release"), b"release").unwrap();
        source.await_file(&temp.path().join("unknown-verified"));
        assert!(
            child.exact.terminal(0).unwrap().is_none(),
            "unknown disarm explicitly killed its manager"
        );
        source.hard_kill();
        let survives = checkpoint == "disarm-set";
        assert_eq!(
            child
                .exact
                .terminal(if survives { 250 } else { 5000 })
                .unwrap()
                .is_none(),
            survives,
            "unknown phase changed kernel lifetime after {checkpoint}"
        );
    }
}

// 检查命名 job 冲突不能重用现有句柄或修改其准备标志。
#[test]
fn HistoryManager_JobCollision_007() {
    require_job_free_source().expect("collision probe requires a job-free Windows host");
    let user = CurrentUser::capture().unwrap();
    let job = ManagerPreparationJob::create(TRANSACTION, &user).unwrap();
    assert!(ManagerPreparationJob::new_with_identity(job.identity.clone(), &user).is_err());
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(handle(&job.handle)),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            None,
        )
    }
    .unwrap();
    assert_eq!(
        limits.BasicLimitInformation.LimitFlags,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    );
    assert_eq!(job.phase, Some(ManagerJobPhase::ArmedPreparation));
}

// 检查同一事务每次创建不同且不可预测的私有 job 身份。
#[test]
fn HistoryManager_UniqueJobs_008() {
    require_job_free_source().expect("identity probe requires a job-free Windows host");
    let user = CurrentUser::capture().unwrap();
    let first = ManagerPreparationJob::create(TRANSACTION, &user).unwrap();
    let second = ManagerPreparationJob::create(TRANSACTION, &user).unwrap();
    assert_ne!(first.identity, second.identity);
    assert_ne!(first.identity.name, second.identity.name);
    assert_ne!(first.identity.nonce, second.identity.nonce);
    assert_eq!(first.identity.owner, user.sid_text());
    assert_eq!(first.identity.transaction, TRANSACTION);
}

// 检查真实生产 wrapper 的专用子进程，只允许父测试按精确名称显式调用。
#[test]
#[ignore = "supervised by manager production-wrapper tests; requires private fixture environment"]
fn HistoryManager_AtomicWorker_090() {
    require_job_free_source().expect("worker inherited containment; this is not a positive result");
    let preparation = Instant::now() + typed_probe::PREPARATION;
    let directory = PathBuf::from(std::env::var_os(DIRECTORY_ENV).expect("private test directory"));
    let mode = std::env::var(MODE_ENV).expect("explicit worker mode");
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(&directory).unwrap();
    let root = PrivateDirectory::create_new(parent, name("bundle").unwrap(), &user).unwrap();
    std::fs::copy(
        std::env::current_exe().unwrap(),
        directory.join("bundle").join(MANAGER_BASENAME),
    )
    .unwrap();
    let image = root
        .directory()
        .open_file(name(MANAGER_BASENAME).unwrap(), FileAccess::Read)
        .unwrap();
    let mut job = ManagerPreparationJob::create(TRANSACTION, &user).unwrap();

    if mode == "typed-installer" || mode == "typed-historical" {
        use super::super::{
            lease::LeaseFiles,
            process::{CommandLine, JobKind, PreparedProcess},
        };
        // Only this disposable worker is assigned. Real guard admission still
        // checks the protected ACL, lifetime phase and exact member process.
        let current = ExactProcess::capture_observed(std::process::id()).unwrap();
        unsafe {
            AssignProcessToJobObject(handle(&job.handle), GetCurrentProcess()).unwrap();
        }
        assert!(AdmittedManagerJob::open(&job.identity, &current, &user).is_err());
        job.disarm().unwrap();
        let admitted = AdmittedManagerJob::open(&job.identity, &current, &user).unwrap();
        admitted.verify_current().unwrap();
        typed_probe::check(preparation, "worker preparation admission");
        let root = Arc::new(root);
        let leases = LeaseFiles::open(root.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let mut exclusive = leases.acquire_exclusive(&control).unwrap();
        let executable = std::env::current_exe().unwrap();
        let image_directory = Directory::open_absolute(executable.parent().unwrap()).unwrap();
        let image_name =
            super::super::files::ComponentName::new(executable.file_name().unwrap()).unwrap();
        let ordinary_marker = directory.join("ordinary-child");
        let mut ordinary = PreparedProcess::create_suspended(
            image_directory
                .open_file(image_name.clone(), FileAccess::Read)
                .unwrap(),
            CommandLine::probe_controlled(&executable, &ordinary_marker).unwrap(),
            JobKind::Installer,
            root.clone(),
            &user,
            &mut exclusive,
        )
        .unwrap();
        let ordinary_exact = ordinary.probe_exact().unwrap();
        assert!(
            ordinary_exact.is_in_job(Some(handle(&job.handle))).unwrap(),
            "general creator silently gained breakaway"
        );
        assert!(admitted.verify_child_outside(&ordinary_exact).is_err());
        ordinary.cancel_before_resume().unwrap();
        drop(ordinary);
        assert!(
            !ordinary_marker.exists(),
            "default child ran before admission"
        );
        typed_probe::check(preparation, "worker ordinary child");

        let marker = directory.join("typed-child");
        let kind = if mode == "typed-installer" {
            JobKind::Installer
        } else {
            JobKind::HistoricalApplication
        };
        let mut child = PreparedProcess::create_suspended_from_manager(
            image_directory
                .open_file(image_name, FileAccess::Read)
                .unwrap(),
            CommandLine::probe_controlled(&executable, &marker).unwrap(),
            kind,
            root,
            &user,
            &mut exclusive,
            &admitted,
        )
        .unwrap();
        let exact = child.probe_exact().unwrap();
        typed_probe::check(preparation, "worker typed identity");
        let observed = typed_probe::record(&exact);
        typed_probe::custody(&directory, &observed, preparation);
        let fault = std::env::var(CHECKPOINT_ENV).unwrap();
        if fault == "typed-fail-suspended" {
            std::fs::write(directory.join("typed-injected-failure"), fault.as_bytes()).unwrap();
            panic!("injected typed suspended failure");
        }
        assert!(
            exact.is_in_job(None).unwrap(),
            "typed child escaped its dedicated job"
        );
        admitted.verify_child_outside(&exact).unwrap();
        let receipt = child.persist_identity(&user).unwrap();
        child.resume(&receipt).unwrap();
        assert!(child.resume(&receipt).is_err(), "typed child resumed twice");
        typed_probe::check(preparation, "worker resume");
        let deadline = Instant::now() + Duration::from_secs(20);
        typed_probe::publish(&directory.join("typed-ready.json"), &observed);
        if fault == "typed-fail-running" {
            std::fs::write(directory.join("typed-injected-failure"), fault.as_bytes()).unwrap();
            panic!("injected typed running failure");
        }
        loop {
            typed_probe::check(deadline, "worker marker");
            let exists = marker.try_exists().unwrap();
            typed_probe::check(deadline, "worker marker query");
            if exists {
                break;
            }
            assert!(
                exact.terminal(0).unwrap().is_none(),
                "typed child exited before executing"
            );
            typed_probe::check(deadline, "worker marker terminal");
            assert!(Instant::now() < deadline, "typed child did not execute");
            std::thread::sleep(Duration::from_millis(10));
        }
        typed_probe::publish(&directory.join("typed.json"), &observed);
        typed_probe::check(
            deadline + Duration::from_secs(5),
            "worker report publication",
        );
        if fault == "typed-fallback" {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !directory.join("typed-fallback-go").try_exists().unwrap() {
                typed_probe::check(deadline, "injected fallback custody");
                if directory.join("cleanup").try_exists().unwrap() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            typed_probe::fallback(&directory);
            std::fs::write(marker.with_extension("release"), b"fallback").unwrap();
            typed_probe::publish(&directory.join("typed-fallback-released.json"), &observed);
            // Keep this exact source available for the normal parent hard kill;
            // only the real fallback-provenance assertion may reject this case.
            while !directory.join("cleanup").try_exists().unwrap() {
                typed_probe::check(deadline, "injected fallback cleanup");
                std::thread::sleep(Duration::from_millis(10));
            }
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while !directory.join("release").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        // Bounded fallback if the supervisor fails; no disposable child remains.
        typed_probe::fallback(&directory);
        std::fs::write(marker.with_extension("release"), b"release").unwrap();
        assert!(exact.terminal(5000).unwrap().is_some());
        return;
    }

    if [
        "deny-breakaway",
        "allow-breakaway",
        "silent-breakaway",
        "nested-jobs",
    ]
    .contains(&mode.as_str())
    {
        let flags = match mode.as_str() {
            "allow-breakaway" => JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            "silent-breakaway" => JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
            _ => JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // All host jobs are made and held inside this disposable source. The
        // external observer cannot accidentally keep its manager job alive.
        let outer = unsafe { own(CreateJobObjectW(None, PCWSTR::null()).unwrap()) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = flags;
        unsafe {
            SetInformationJobObject(
                handle(&outer),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .unwrap();
            AssignProcessToJobObject(handle(&outer), GetCurrentProcess()).unwrap();
        }
        let inner = if mode == "nested-jobs" {
            let inner = unsafe { own(CreateJobObjectW(None, PCWSTR::null()).unwrap()) };
            unsafe {
                AssignProcessToJobObject(handle(&inner), GetCurrentProcess()).unwrap();
            }
            Some(inner)
        } else {
            None
        };
        assert!(in_job(unsafe { GetCurrentProcess() }).unwrap());
        assert!(
            require_job_free_source().is_err(),
            "contained source admission succeeded"
        );
        assert!(
            ManagerPreparationJob::create(TRANSACTION, &user).is_err(),
            "contained source prepared a job"
        );
        assert!(
            create_manager_process(image, TRANSACTION, &job).is_err(),
            "contained source created a manager"
        );
        assert!(
            !directory.join("checkpoint.json").exists(),
            "contained source attempted creation"
        );
        std::fs::write(directory.join("complete"), b"blocked-before-create").unwrap();
        // Closing a KILL_ON_JOB_CLOSE host here would kill this worker before
        // libtest reports success. OS process exit releases these test handles.
        std::mem::forget(inner);
        std::mem::forget(outer);
        return;
    }

    let root = Arc::new(root);
    let source = ExactProcess::capture_observed(std::process::id()).unwrap();
    let bundle =
        ManagerRecord::create(root.clone(), "test-bundle.json", &"reference-only", &user).unwrap();
    let launch = LaunchBinding {
        schema: 2,
        transaction: TRANSACTION.into(),
        data_root: root.directory().identity().clone(),
        bundle: bundle.reference().clone(),
        package_root: root.directory().identity().clone(),
        package_digest: "1".repeat(64),
        source: source.identity().clone(),
        command_digest: "2".repeat(64),
        job: job.identity.clone(),
    };
    let launch_record = ManagerRecord::create(root.clone(), LAUNCH, &launch, &user).unwrap();
    let created = create_manager_process(image, TRANSACTION, &job);
    if mode == "assign-after-check" {
        assert!(
            created.is_err(),
            "source containment after precheck was admitted"
        );
        let child = CONCURRENT_CHILD
            .with(|slot| slot.borrow_mut().take())
            .expect("fixture must reach successful native creation before postcheck rejection");
        assert!(
            child.terminal(5000).unwrap().is_some(),
            "postcheck failure leaked its exact suspended child"
        );
        assert_eq!(
            job_members(handle(&job.handle)).unwrap(),
            0,
            "preparation job retained a leaked child"
        );
        std::fs::write(directory.join("complete"), b"rejected-after-create").unwrap();
        return;
    }
    let (process, thread, mut pending) = created.unwrap();
    assert!(process.is_in_job(Some(handle(&job.handle))).unwrap());
    let process_record = ManagerRecord::create(
        root.clone(),
        PROCESS,
        &ProcessBinding {
            schema: 2,
            transaction: TRANSACTION.into(),
            launch: launch_record.reference().clone(),
            process: process.identity().clone(),
        },
        &user,
    )
    .unwrap();
    let cleanup = ObservedChild {
        exact: ExactProcess::reopen(process.identity()).unwrap(),
        cleanup_armed: true,
        cleanup: pending
            .0
            .as_ref()
            .expect("never-resumed cleanup handle")
            .try_clone()
            .unwrap(),
    };
    let collision = match mode.as_str() {
        "collision-disarm" => {
            Some(ManagerRecord::create(root.clone(), DISARM, &"preexisting intent", &user).unwrap())
        }
        "collision-resume" => Some(
            ManagerRecord::create(root.clone(), RESUME, &"preexisting lifetime", &user).unwrap(),
        ),
        _ => None,
    };
    let disarm = job.prepare_lifetime(
        &process,
        &mut pending,
        root.clone(),
        (&launch, &launch_record, &process_record),
        &user,
    );
    if let Some(collision) = collision {
        assert!(disarm.is_err(), "existing durable record was overwritten");
        collision.verify(&user).unwrap();
        let survives = mode == "collision-resume";
        assert_eq!(
            pending.0.is_none(),
            survives,
            "wrong explicit-cleanup state after collision"
        );
        assert_eq!(
            job.phase,
            Some(if survives {
                ManagerJobPhase::ManagerLifetime
            } else {
                ManagerJobPhase::ArmedPreparation
            })
        );
        assert!(
            job.prepare_lifetime(
                &process,
                &mut pending,
                root,
                (&launch, &launch_record, &process_record),
                &user
            )
            .is_err(),
            "lifetime preparation was replayed"
        );
        collision.verify(&user).unwrap();
        let bytes = serde_json::to_vec(&serde_json::json!({"pid": process.pid()})).unwrap();
        std::fs::write(directory.join("collision.tmp"), bytes).unwrap();
        std::fs::rename(
            directory.join("collision.tmp"),
            directory.join("collision.json"),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline && !directory.join("cleanup").exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
        stop_suspended(&cleanup.cleanup).unwrap();
        return;
    }
    if mode == "disarm-error" {
        assert!(disarm.is_err(), "injected readback failure was lost");
        assert_eq!(job.phase, None, "uncertain disarm must poison the owner");
        assert!(job.disarm().is_err(), "uncertain disarm was retried");
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(handle(&job.handle)),
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                None,
            )
            .unwrap();
        }
        let expected = if std::env::var(CHECKPOINT_ENV).unwrap() == "disarm-set" {
            JOB_OBJECT_LIMIT_BREAKAWAY_OK
        } else {
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        };
        assert_eq!(limits.BasicLimitInformation.LimitFlags, expected);
        assert!(
            pending.0.is_none(),
            "unknown disarm retained explicit termination authority"
        );

        assert!(process.terminal(0).unwrap().is_none());
        std::fs::write(directory.join("unknown-verified"), b"unknown-no-retry").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline && !directory.join("cleanup").exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        disarm.unwrap();
        assert_eq!(job.phase, Some(ManagerJobPhase::ManagerLifetime));
        assert!(job.disarm().is_err(), "confirmed disarm was replayed");
    }
    stop_suspended(&cleanup.cleanup).unwrap();
    drop(thread);
    std::fs::write(directory.join("complete"), b"complete").unwrap();
}

// 检查受保护记录的交叉引用、事务、job 和 schema 不能混用或降级。
#[test]
fn HistoryManager_RejectReceiptMix_009() {
    require_job_free_source().expect("receipt fixture requires a job-free Windows host");
    for corruption in [
        "launch-schema",
        "process-schema",
        "resume-schema",
        "disarm-schema",
        "launch-job",
        "resume-job",
        "disarm-job",
        "job-owner",
        "job-session",
        "job-nonce",
        "disarm-transaction",
        "disarm-root",
        "disarm-launch",
        "disarm-process",
        "resume-disarm",
        "missing-disarm",
        "unknown-disarm-field",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let root = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("records").unwrap(), &user).unwrap(),
        );
        let other_root =
            PrivateDirectory::create_new(parent, name("other").unwrap(), &user).unwrap();
        let process = ExactProcess::capture_observed(std::process::id()).unwrap();
        let job = ManagerPreparationJob::create(TRANSACTION, &user).unwrap();
        let other_job = ManagerPreparationJob::create(TRANSACTION, &user).unwrap();
        let bundle =
            ManagerRecord::create(root.clone(), "test-bundle.json", &"reference-only", &user)
                .unwrap();
        let mut launch = LaunchBinding {
            schema: 2,
            transaction: TRANSACTION.into(),
            data_root: root.directory().identity().clone(),
            bundle: bundle.reference().clone(),
            package_root: root.directory().identity().clone(),
            package_digest: "1".repeat(64),
            source: process.identity().clone(),
            command_digest: "2".repeat(64),
            job: job.identity.clone(),
        };
        match corruption {
            "launch-schema" => launch.schema = 1,
            "launch-job" => launch.job = other_job.identity.clone(),
            "job-owner" => launch.job.owner.push_str("-1"),
            "job-session" => launch.job.session = launch.job.session.wrapping_add(1),
            "job-nonce" => launch.job.nonce = "22222222-2222-4222-8222-222222222222".into(),
            _ => {}
        }
        let launch_record = ManagerRecord::create(root.clone(), LAUNCH, &launch, &user).unwrap();
        let process_record = ManagerRecord::create(
            root.clone(),
            PROCESS,
            &ProcessBinding {
                schema: if corruption == "process-schema" { 1 } else { 2 },
                transaction: TRANSACTION.into(),
                launch: launch_record.reference().clone(),
                process: process.identity().clone(),
            },
            &user,
        )
        .unwrap();
        let mut disarm = DisarmBinding {
            schema: 1,
            transaction: TRANSACTION.into(),
            data_root: root.directory().identity().clone(),
            launch: launch_record.reference().clone(),
            process: process_record.reference().clone(),
            job: job.identity.clone(),
        };
        match corruption {
            "disarm-schema" => disarm.schema = 0,
            "disarm-transaction" => {
                disarm.transaction = "22222222-2222-4222-8222-222222222222".into()
            }
            "disarm-root" => disarm.data_root = other_root.directory().identity().clone(),
            "disarm-launch" => disarm.launch = bundle.reference().clone(),
            "disarm-process" => disarm.process = bundle.reference().clone(),
            "disarm-job" => disarm.job = other_job.identity.clone(),
            _ => {}
        }
        let mut disarm_json = serde_json::to_value(disarm).unwrap();
        if corruption == "unknown-disarm-field" {
            disarm_json["callerSaysDisarmed"] = serde_json::json!(true);
        }
        let disarm_record = (corruption != "missing-disarm")
            .then(|| ManagerRecord::create(root.clone(), DISARM, &disarm_json, &user).unwrap());
        let resume_record = ManagerRecord::create(
            root.clone(),
            RESUME,
            &ResumeBinding {
                schema: if corruption == "resume-schema" { 1 } else { 2 },
                transaction: TRANSACTION.into(),
                data_root: root.directory().identity().clone(),
                launch: launch_record.reference().clone(),
                process: process_record.reference().clone(),
                job: if corruption == "resume-job" {
                    other_job.identity.clone()
                } else {
                    job.identity.clone()
                },
                disarm: if corruption == "resume-disarm" {
                    bundle.reference().clone()
                } else {
                    disarm_record
                        .as_ref()
                        .map_or(bundle.reference(), ManagerRecord::reference)
                        .clone()
                },
            },
            &user,
        )
        .unwrap();
        let ready = ManagerRecord::create(
            root.clone(),
            READY,
            &ReadyBinding {
                schema: 1,
                transaction: TRANSACTION.into(),
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
        assert!(
            ManagerReadyObservation::reopen(
                root,
                TRANSACTION,
                resume_record.reference(),
                ready.reference(),
                &user
            )
            .is_err(),
            "corrupt {corruption} was accepted as ready"
        );
    }
}

// 检查 admitted job 必须属于精确管理者，且重开句柄没有改变 job 配置的权限。
#[test]
fn HistoryManager_AdmittedJob_010() {
    require_job_free_source().expect("admission probe requires a job-free Windows host");
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = PrivateDirectory::create_new(parent, name("bundle").unwrap(), &user).unwrap();
    std::fs::copy(
        std::env::current_exe().unwrap(),
        temp.path().join("bundle").join(MANAGER_BASENAME),
    )
    .unwrap();
    let image = root
        .directory()
        .open_file(name(MANAGER_BASENAME).unwrap(), FileAccess::Read)
        .unwrap();
    let mut job = ManagerPreparationJob::create(TRANSACTION, &user).unwrap();
    let (manager, _thread, _pending) = create_manager_process(image, TRANSACTION, &job).unwrap();
    assert!(
        AdmittedManagerJob::open(&job.identity, &manager, &user).is_err(),
        "armed job was admitted"
    );
    job.disarm().unwrap();
    let admitted = AdmittedManagerJob::open(&job.identity, &manager, &user).unwrap();
    admitted.verify_manager(&manager, &user).unwrap();
    assert!(
        admitted.verify_current().is_err(),
        "an unrelated source borrowed manager authority"
    );
    let source = ExactProcess::capture_observed(std::process::id()).unwrap();
    assert!(
        AdmittedManagerJob::open(&job.identity, &source, &user).is_err(),
        "nonmember process was admitted"
    );
    for field in ["owner", "session", "nonce", "name"] {
        let mut forged = job.identity.clone();
        match field {
            "owner" => forged.owner.push_str("-1"),
            "session" => forged.session = forged.session.wrapping_add(1),
            "nonce" => forged.nonce = "22222222-2222-4222-8222-222222222222".into(),
            "name" => forged.name.push_str("-other"),
            _ => unreachable!(),
        }
        assert!(
            AdmittedManagerJob::open(&forged, &manager, &user).is_err(),
            "forged {field} was admitted"
        );
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    assert!(
        unsafe {
            SetInformationJobObject(
                handle(&admitted.handle),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .is_err(),
        "query-only admitted job could mutate limits"
    );
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
    unsafe {
        SetInformationJobObject(
            handle(&job.handle),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .unwrap();
    }
    assert!(
        admitted.verify_manager(&manager, &user).is_err(),
        "unexpected lifetime limits were accepted"
    );
    assert!(AdmittedManagerJob::open(&job.identity, &manager, &user).is_err());
}

// 检查专用 installer 仍受 kill-on-close 保护，历史应用离开管理者 job 后独立结束。
#[test]
fn HistoryManager_TypedChildren_011() {
    typed_probe::normal();
}

// 检查 durable create-new 冲突既不覆盖记录，也不把已解除的 job 重新装备。
#[test]
fn HistoryManager_LifetimeCollision_012() {
    require_job_free_source().expect("lifetime probe requires a job-free Windows host");
    for mode in ["collision-disarm", "collision-resume"] {
        let temp = tempfile::tempdir().unwrap();
        let mut source = Worker::spawn(temp.path(), mode, "");
        source.await_file(&temp.path().join("collision.json"));
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(temp.path().join("collision.json")).unwrap())
                .unwrap();
        let child = ObservedChild::capture(u32::try_from(record["pid"].as_u64().unwrap()).unwrap());
        source.hard_kill();
        let survives = mode == "collision-resume";
        assert_eq!(
            child
                .exact
                .terminal(if survives { 250 } else { 5000 })
                .unwrap()
                .is_none(),
            survives,
            "wrong lifetime after source death following {mode}"
        );
        assert!(!temp.path().join("bundle").join(READY).exists());
    }
}

// 检查通过 precheck 后才进入外部 job 的 source 在真实创建后仍被拒绝并清理。
#[test]
fn HistoryManager_ConcurrentHostJob_013() {
    require_job_free_source().expect("concurrent containment probe needs a job-free host");
    let temp = tempfile::tempdir().unwrap();
    let mut source = Worker::spawn(temp.path(), "assign-after-check", "before-create");
    source.await_file(&temp.path().join("checkpoint.json"));
    std::fs::write(temp.path().join("release"), b"assign-source-after-precheck").unwrap();
    source.await_file(&temp.path().join("complete"));
    source.finish();
    assert!(!temp.path().join("bundle").join(READY).exists());
}

// 检查把进程伪句柄交给 job 查询时，真实 API 错误不会变成默认有效状态。
#[test]
fn HistoryManager_RejectJobQueryError_014() {
    let process = unsafe { GetCurrentProcess() };
    assert!(
        verify_job_phase(process, ManagerJobPhase::ManagerLifetime).is_err(),
        "failed job-limits query defaulted to an admitted lifetime"
    );
    assert!(
        job_members(process).is_err(),
        "failed job-members query defaulted to an empty preparation job"
    );
}
