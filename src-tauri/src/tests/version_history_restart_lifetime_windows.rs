//! Evidence-only probe for named historical jobs after their creator drops.
//! A successful test means the controlled observations completed, not that
//! restart Return is admitted. No production recovery owner is constructed.
use super::*;
use crate::{
    tests::version_history_payload::token::{run_restart_lifetime, verify_restart_worker},
    version_history::windows::lease::LeaseFiles,
};
use std::{path::PathBuf, time::Duration};

fn name(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
}

fn evidence(case: &str, stage: &str, observation: serde_json::Value) {
    println!(
        "HISTORY_RESTART_LIFETIME {}",
        serde_json::json!({
            "schema": 1,
            "case": case,
            "stage": stage,
            "observation": observation,
            "recoveryAuthority": false,
        })
    );
}

fn io_evidence(case: &str, stage: &str, error: &io::Error) {
    // No paths, SIDs, PIDs, job names, record bytes or raw error text leave the
    // disposable fixture. The stage distinguishes API failure from rejection.
    evidence(
        case,
        stage,
        serde_json::json!({
            "status": "failed",
            "kind": format!("{:?}", error.kind()),
            "osCode": error.raw_os_error(),
        }),
    );
}

/// Mirror the existing lookup checks, but report the exact boundary that
/// failed. In particular, a successful OpenJobObjectW followed by a phase
/// mismatch must not be reported as an absent named object.
fn observe_open(case: &str, identity: &JobIdentity, user: &CurrentUser) -> Option<PrivateJob> {
    assert_eq!(identity.owner, user.sid_text());
    assert_eq!(
        identity.session,
        session_id(unsafe { GetCurrentProcessId() }).unwrap()
    );
    let name: Vec<_> = identity.name.encode_utf16().chain(Some(0)).collect();
    let raw = match unsafe {
        // JOB_OBJECT_QUERY | READ_CONTROL, matching PrivateJob::open_recorded.
        OpenJobObjectW(0x0004 | 0x0002_0000, false, PCWSTR(name.as_ptr()))
    } {
        Ok(raw) => raw,
        Err(error) => {
            evidence(
                case,
                "openJobObject",
                serde_json::json!({ "status": "failed", "hresult": error.code().0 }),
            );
            return None;
        }
    };
    evidence(
        case,
        "openJobObject",
        serde_json::json!({ "status": "opened" }),
    );
    let job = PrivateJob {
        handle: unsafe { own(raw) },
        identity: identity.clone(),
        phase: Some(JobPhase::HistoricalLifetime),
    };
    if let Err(error) = user.verify_private_job(handle(&job.handle)) {
        io_evidence(case, "privateJobSecurity", &error);
        return None;
    }
    if let Err(error) = job.verify_limits() {
        io_evidence(case, "historicalLifetimeLimits", &error);
        return None;
    }
    evidence(
        case,
        "historicalLifetimeLimits",
        serde_json::json!({ "status": "verified" }),
    );
    Some(job)
}

fn observe_count(case: &str, stage: &str, job: &PrivateJob) -> Option<u32> {
    match job.active_processes() {
        Ok(count) => {
            evidence(case, stage, serde_json::json!({ "activeProcesses": count }));
            Some(count)
        }
        Err(error) => {
            io_evidence(case, stage, &error);
            None
        }
    }
}

struct ReleaseOnDrop(PathBuf);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        // Release only this test worker through its existing cooperative
        // protocol. Its own 60-second deadline also bounds panic cleanup.
        let _ = std::fs::write(&self.0, b"release");
    }
}

fn await_marker(marker: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "controlled worker did not start"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn observe_owner_loss(reopen_while_live: bool) {
    let case = if reopen_while_live {
        "liveRoot"
    } else {
        "terminalRoot"
    };
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    user.require_unelevated().unwrap();
    evidence(
        case,
        "fixtureAdmission",
        serde_json::json!({
            "sameUserMediumChild": true, "outerJobContainmentUnchanged": true,
            "installedProductAcceptance": false,
        }),
    );
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let root = Arc::new(PrivateDirectory::create_new(parent, name("private"), &user).unwrap());
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let mut lease = leases.acquire_exclusive(&control).unwrap();
    let executable = std::env::current_exe().unwrap();
    let image_parent = Directory::open_absolute(executable.parent().unwrap()).unwrap();
    let image = image_parent
        .open_file(
            ComponentName::new(executable.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    let marker = temporary.path().join("started");
    let release = ReleaseOnDrop(marker.with_extension("release"));
    let mut owner = PreparedProcess::create_suspended(
        image,
        CommandLine::probe_controlled(&executable, &marker).unwrap(),
        JobKind::HistoricalApplication,
        root.clone(),
        &user,
        &mut lease,
    )
    .unwrap();
    let receipt = owner.persist_identity(&user).unwrap();
    owner.resume(&receipt).unwrap();
    await_marker(&marker);

    // Bind the observed post-disarm phase to the exact original durable
    // lifetime receipt, not the preparation phase in the process receipt.
    let lifetime = owner.historical_lifetime.as_ref().unwrap();
    lifetime.verify().unwrap();
    assert_eq!(lifetime.root_identity(), root.directory().identity());
    let expected = serde_json::to_vec(&serde_json::json!({
        "schema": 1, "launch": owner.launch, "processReceipt": receipt.record.digest(),
        "job": owner.job.identity, "jobPhase": JobPhase::HistoricalLifetime,
        "operation": "historical-job-disarmed",
    }))
    .unwrap();
    assert_eq!(lifetime.bytes(), expected);
    owner.job.verify_limits().unwrap();
    let job_identity = owner.job.identity.clone();
    let exact = receipt.reopen_process().unwrap();
    exact.verify_current_user(&user).unwrap();
    assert!(exact.terminal(0).unwrap().is_none());
    owner.job.contains(&exact).unwrap();
    assert_eq!(owner.active_processes().unwrap(), 1);

    // This lookup is a fixture prerequisite while the original job is still
    // held. Drop it before dropping the creator: no hidden job keeper may make
    // the later restart-like lookup succeed accidentally.
    let baseline = observe_open("originalOwnerHeld", &job_identity, &user)
        .expect("correct historical-phase fixture lookup failed");
    baseline.contains(&exact).unwrap();
    drop(baseline);
    drop(owner);
    assert!(exact.terminal(0).unwrap().is_none());
    evidence(
        case,
        "originalOwnerDropped",
        serde_json::json!({
            "exactRootLive": true, "probeRetainsProcessHandle": true, "probeRetainsJobHandle": false,
        }),
    );

    let observed_job = if reopen_while_live {
        let job = observe_open(case, &job_identity, &user);
        if let Some(job) = &job {
            match job.contains(&exact) {
                Ok(()) => evidence(
                    case,
                    "liveMembership",
                    serde_json::json!({ "status": "verified" }),
                ),
                Err(error) => io_evidence(case, "liveMembership", &error),
            }
            let _ = observe_count(case, "liveAccounting", job);
        }
        job
    } else {
        None
    };

    std::fs::write(&release.0, b"release").unwrap();
    let terminal = exact
        .terminal(30_000)
        .unwrap()
        .expect("controlled worker did not exit");
    assert_eq!(terminal.exit_code(), 0);
    assert!(marker.with_extension("completed").exists());
    evidence(
        case,
        "exactRootTerminal",
        serde_json::json!({
            "exitCode": terminal.exit_code(), "terminalMembershipQueried": false,
        }),
    );

    if let Some(job) = &observed_job {
        // Accounting can lag the process wait. Bound the observation without
        // assuming that its outcome is already established on this platform.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match job.active_processes() {
                Ok(0) => {
                    evidence(
                        case,
                        "retainedJobAfterTerminal",
                        serde_json::json!({ "activeProcesses": 0 }),
                    );
                    break;
                }
                Ok(count) if std::time::Instant::now() >= deadline => {
                    evidence(
                        case,
                        "retainedJobAfterTerminal",
                        serde_json::json!({ "activeProcesses": count, "deadlineReached": true }),
                    );
                    break;
                }
                Ok(_) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    io_evidence(case, "retainedJobAfterTerminal", &error);
                    break;
                }
            }
        }
    }
    drop(observed_job);
    // In terminalRoot this is the first lookup after original owner loss.
    // In liveRoot it tests lookup after the observer's last job handle closes.
    // A result here is diagnostic only: zero cannot prove original-job identity.
    if let Some(job) = observe_open(case, &job_identity, &user) {
        let _ = observe_count(case, "reopenedAfterTerminal", &job);
    }
    evidence(
        case,
        "completed",
        serde_json::json!({ "status": "observed" }),
    );
}

// 检查真实历史进程仍存活时，正确解除清理的 job 是否能重开并持续观察至退出。
#[test]
fn RestartLifetime_LiveRootObservation_001() {
    run_restart_lifetime(true).expect("confined Medium diagnostic worker must complete");
}

// 检查原 owner 已释放且进程先退出后，首次重开 job 的实际结果，不推定成功或不存在。
#[test]
fn RestartLifetime_TerminalBeforeReopen_002() {
    run_restart_lifetime(false).expect("confined Medium diagnostic worker must complete");
}

// 检查指定的未提升子进程通过精确回执后，才采集存活根进程的 job 生命周期证据。
#[test]
#[ignore = "private restricted-token diagnostic child; requires exact controller receipt"]
fn RestartWorker_Live_003() {
    verify_restart_worker(true).unwrap();
    observe_owner_loss(true);
}

// 检查指定的未提升子进程通过精确回执后，才采集根进程退出后的 job 重开证据。
#[test]
#[ignore = "private restricted-token diagnostic child; requires exact controller receipt"]
fn RestartWorker_Terminal_004() {
    verify_restart_worker(false).unwrap();
    observe_owner_loss(false);
}
