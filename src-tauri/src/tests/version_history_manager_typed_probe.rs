//! Test-only phased supervision of the real manager/typed-child wrappers.
use super::super::super::process::ProcessIdentity;
use super::*;
use std::io::{self, Write};

// One observed preparation took ~121 s with the real 55 MB libtest image.
// These are checked hang guards, not a performance SLO or preemptive wall limit.
pub(super) const PREPARATION: Duration = Duration::from_secs(180);
const BEHAVIOR_REPORT: Duration = Duration::from_secs(25); // original 20 + publication
const PROOF: Duration = Duration::from_secs(15); // fresh identity + original terminal
const TOTAL: Duration = Duration::from_secs(460);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    source_pid: u32,
    pid: u32,
    identity: ProcessIdentity,
}
pub(super) fn record(exact: &ExactProcess) -> Record {
    Record {
        source_pid: std::process::id(),
        pid: exact.pid(),
        identity: exact.identity().clone(),
    }
}
pub(super) fn check(deadline: Instant, phase: &str) {
    assert!(Instant::now() < deadline, "typed {phase} timed out");
}
pub(super) fn publish(path: &Path, value: &Record) {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path.with_extension("tmp"))
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::rename(path.with_extension("tmp"), path).unwrap();
}
pub(super) fn custody(directory: &Path, observed: &Record, deadline: Instant) {
    publish(&directory.join("typed-observed.json"), observed);
    loop {
        check(deadline, "preparation custody");
        assert!(
            !directory.join("cleanup").try_exists().unwrap(),
            "typed supervisor aborted"
        );
        let path = directory.join("typed-observed.ack");
        if path.try_exists().unwrap() {
            let acknowledged: Record =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            check(deadline, "preparation custody");
            assert_eq!(&acknowledged, observed, "typed custody identity changed");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn fallback(directory: &Path) {
    // Must be durable before release: a successful completion cannot be supplied
    // by worker fallback, even if it races the source-death observation.
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("typed-fallback-started"))
        .unwrap();
    file.write_all(b"fallback-before-child-release").unwrap();
    file.sync_all().unwrap();
}
fn milliseconds(deadline: Instant, maximum: u32) -> u32 {
    deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(u128::from(maximum)) as u32
}
fn full_window(deadline: Instant, phase: &str) {
    assert!(
        deadline.saturating_duration_since(Instant::now()) >= Duration::from_secs(5),
        "typed {phase}: budget cannot shorten terminal observation"
    );
}

struct Supervisor {
    source: Worker,
    children: Vec<ObservedChild>,
    total: Instant,
}
impl Supervisor {
    fn new(directory: &Path, mode: &str, fault: &str, total: Instant) -> Self {
        Self {
            source: Worker::spawn(directory, mode, fault),
            children: vec![],
            total,
        }
    }
    fn await_record(
        &mut self,
        name: &str,
        phase: &str,
        deadline: Instant,
        expected: Option<&Record>,
    ) -> Record {
        let path = self.source.directory.join(name);
        loop {
            check(deadline, phase);
            if path.try_exists().unwrap() {
                let value: Record = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                check(deadline, phase); // a late file/native return never rescues a timeout
                assert_eq!(
                    value.source_pid,
                    self.source.child.id(),
                    "typed source changed"
                );
                value.identity.validate().unwrap();
                if let Some(expected) = expected {
                    assert_eq!(&value, expected, "typed child changed");
                }
                return value;
            }
            if let Some(status) = self.source.child.try_wait().unwrap() {
                panic!("typed {phase}: worker exited {status}");
            }
            check(deadline, phase);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn retain(&mut self, record: &Record) {
        let child = ObservedChild::capture(record.pid);
        // Register custody before asserting equivalence, so mismatch also cleans
        // the held process object rather than reopening an untrusted PID later.
        self.children.push(child);
        assert_eq!(
            self.children.last().unwrap().exact.identity(),
            &record.identity
        );
    }
    fn run(&mut self, mode: &str, fault: &str, preparation: Instant) {
        let observed = self.await_record("typed-observed.json", "preparation", preparation, None);
        self.retain(&observed);
        check(preparation, "preparation identity");
        let ready_deadline = if fault == "typed-prepare-timeout" {
            // Start injected timeout only after costly real identity custody.
            // Withholding ACK keeps this real child suspended in the same path.
            (Instant::now() + Duration::from_secs(1)).min(preparation)
        } else {
            publish(&self.source.directory.join("typed-observed.ack"), &observed);
            preparation
        };
        self.await_record(
            "typed-ready.json",
            "preparation",
            ready_deadline,
            Some(&observed),
        );
        let report_deadline = (Instant::now() + BEHAVIOR_REPORT).min(self.total);
        if fault == "typed-marker-timeout" {
            let gate = self.source.directory.join("typed-child.gate-entered");
            while !gate.try_exists().unwrap() {
                check(report_deadline, "injected marker gate");
                assert!(
                    self.source.child.try_wait().unwrap().is_none(),
                    "worker exited before marker gate"
                );
                assert!(
                    self.children[0].exact.terminal(0).unwrap().is_none(),
                    "child exited before marker gate"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(std::fs::read(gate).unwrap(), b"waiting-before-marker");
            assert!(
                !self
                    .source
                    .directory
                    .join("typed-child")
                    .try_exists()
                    .unwrap(),
                "marker was published before injected gate"
            );
            check(report_deadline, "injected marker gate returned");
        }
        let report_budget = if fault == "typed-marker-timeout" {
            Duration::from_secs(1)
        } else {
            BEHAVIOR_REPORT
        };
        let report = self.await_record(
            "typed.json",
            "marker report",
            (Instant::now() + report_budget).min(report_deadline),
            Some(&observed),
        );
        let proof = (Instant::now() + PROOF).min(self.total);
        self.retain(&report); // retain the original fresh post-report verification
        check(proof, "fresh identity");
        if fault == "typed-fallback" {
            std::fs::write(
                self.source.directory.join("typed-fallback-go"),
                b"exact-custody-held",
            )
            .unwrap();
            self.await_record(
                "typed-fallback-released.json",
                "injected fallback release",
                proof,
                Some(&observed),
            );
        }
        if fault == "typed-self-timeout" {
            // The real controlled child returns 124 only after fresh custody.
            std::fs::write(
                self.source.directory.join("typed-child.release"),
                b"timeout-124",
            )
            .unwrap();
            full_window(proof, "injected exit 124");
            let exit = self
                .children
                .last()
                .unwrap()
                .exact
                .terminal(5000)
                .unwrap()
                .unwrap();
            assert_eq!(
                exit.exit_code(),
                124,
                "controlled exit-124 injection did not run"
            );
        }
        self.source
            .child
            .kill()
            .expect("hard-kill exact typed source");
        loop {
            check(proof, "source terminal");
            if let Some(status) = self.source.child.try_wait().unwrap() {
                assert!(!status.success(), "typed source exited before hard kill");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !self
                .source
                .directory
                .join("typed-fallback-started")
                .try_exists()
                .unwrap(),
            "typed worker fallback used"
        );
        check(proof, "source and fallback provenance");
        let child = self.children.last().unwrap();
        if mode == "typed-installer" {
            full_window(proof, "installer");
            let terminal = child.exact.terminal(5000).unwrap();
            check(proof, "installer terminal");
            let terminal = terminal.expect("installer outlived its last dedicated-job owner");
            assert_ne!(
                terminal.exit_code(),
                124,
                "controlled child self-timeout cannot prove Job close"
            );
            assert!(
                !self
                    .source
                    .directory
                    .join("typed-child.completed")
                    .try_exists()
                    .unwrap(),
                "installer completed rather than being stopped by job close"
            );
        } else {
            assert!(
                proof.saturating_duration_since(Instant::now()) >= Duration::from_millis(250),
                "budget cannot shorten historical survival"
            );
            assert!(
                child.exact.terminal(250).unwrap().is_none(),
                "historical app was tied to manager lifetime"
            );
            check(proof, "historical survival");
            std::fs::write(
                self.source.directory.join("typed-child.release"),
                b"user-finished",
            )
            .unwrap();
            full_window(proof, "historical");
            let terminal = child.exact.terminal(5000).unwrap().unwrap();
            check(proof, "historical terminal");
            assert_eq!(terminal.exit_code(), 0);
            assert!(
                self.source
                    .directory
                    .join("typed-child.completed")
                    .try_exists()
                    .unwrap(),
                "historical app did not finish its own work"
            );
        }
        check(proof, "lifetime assertions");
        check(self.total, "total");
        self.disarm_cleanup(); // every registered identity and source is terminal
    }
    fn disarm_cleanup(&mut self) {
        self.source.cleanup_armed = false;
        for child in &mut self.children {
            child.cleanup_armed = false;
        }
    }
    fn cleanup(&mut self) -> Vec<String> {
        let deadline = (Instant::now() + Duration::from_secs(10)).min(self.total);
        // One explicit attempt and one shared budget; Drop cannot add fresh waits.
        self.disarm_cleanup();
        let mut errors = vec![];
        for name in ["cleanup", "release", "typed-child.release"] {
            if let Err(error) = std::fs::write(self.source.directory.join(name), b"cleanup") {
                errors.push(error.to_string());
            }
        }
        let cooperative = (Instant::now() + Duration::from_secs(2)).min(deadline);
        while Instant::now() < cooperative {
            match self.source.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    errors.push(error.to_string());
                    break;
                }
            }
        }
        match self.source.child.try_wait() {
            Ok(Some(_)) => (),
            Ok(None) => {
                if let Err(error) = self.source.child.kill() {
                    errors.push(error.to_string());
                }
            }
            Err(error) => errors.push(error.to_string()),
        }
        let source_result = (|| -> io::Result<()> {
            loop {
                if self.source.child.try_wait()?.is_some() {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::other("typed source cleanup terminal unresolved"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        if let Err(error) = source_result {
            errors.push(error.to_string());
        }
        for child in &self.children {
            let result = (|| -> io::Result<()> {
                if child.exact.terminal(0)?.is_none() {
                    unsafe {
                        windows::Win32::System::Threading::TerminateProcess(
                            handle(&child.cleanup),
                            1,
                        )
                    }
                    .map_err(win_error)?;
                }
                if child
                    .exact
                    .terminal(milliseconds(deadline, 5000))?
                    .is_none()
                {
                    return Err(io::Error::other(
                        "typed exact child cleanup terminal unresolved",
                    ));
                }
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(error.to_string());
            }
        }
        if Instant::now() >= deadline {
            errors.push("typed cleanup exceeded its shared budget".into());
        }
        errors
    }
}

pub(super) fn normal() {
    require_job_free_source().expect("typed-child probe requires a job-free Windows host");
    let total = Instant::now() + TOTAL;
    for mode in ["typed-installer", "typed-historical"] {
        check(total, "mode start");
        let preparation = (Instant::now() + PREPARATION).min(total);
        let temp = tempfile::tempdir().unwrap();
        let mut supervisor = Supervisor::new(temp.path(), mode, "", total);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            supervisor.run(mode, "", preparation)
        }));
        if let Err(original) = outcome {
            let errors = supervisor.cleanup();
            if !errors.is_empty() {
                eprintln!("typed cleanup unresolved: {errors:?}");
            }
            std::panic::resume_unwind(original); // cleanup never replaces or greens the original failure
        }
        drop(supervisor);
        drop(temp);
        check(total, "mode teardown");
    }
    check(total, "complete");
}
fn failure(fault: &str, expected: &str, modes: &[&str]) {
    require_job_free_source().expect("typed cleanup regression requires a job-free Windows host");
    let total = Instant::now() + TOTAL;
    for mode in modes {
        let preparation = (Instant::now() + PREPARATION).min(total);
        let temp = tempfile::tempdir().unwrap();
        let mut supervisor = Supervisor::new(temp.path(), mode, fault, total);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            supervisor.run(mode, fault, preparation)
        }));
        let errors = supervisor.cleanup();
        assert!(
            errors.is_empty(),
            "{fault}: exact cleanup failed: {errors:?}"
        );
        let original =
            outcome.expect_err("injected failure was accepted as a positive lifetime result");
        let message = original
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| original.downcast_ref::<&str>().copied())
            .unwrap_or("non-string panic");
        assert!(
            message.contains(expected),
            "{fault}: expected {expected:?}, got {message:?}"
        );
        assert!(
            !supervisor.children.is_empty(),
            "fault never reached real child custody"
        );
        if fault.starts_with("typed-fail-") {
            assert_eq!(
                std::fs::read(supervisor.source.directory.join("typed-injected-failure")).unwrap(),
                fault.as_bytes(),
                "worker failed before the intended injection"
            );
            assert_eq!(
                supervisor.source.child.try_wait().unwrap().unwrap().code(),
                Some(101),
                "worker did not exit with its original libtest failure"
            );
        }
        drop(supervisor);
        drop(temp);
        check(total, "failure assertions and teardown");
    }
}

// 超时发生在真实 child 精确身份已登记但 ACK 尚未发出之后，清理不接受未知终态。
#[test]
fn HistoryManager_PrepareTimeout_015() {
    failure(
        "typed-prepare-timeout",
        "typed preparation timed out",
        &["typed-installer", "typed-historical"],
    );
}
// 真实受控 child 不发布 marker，必须由正常 report 等待超时并清理同一精确进程。
#[test]
fn HistoryManager_MarkerTimeout_016() {
    failure(
        "typed-marker-timeout",
        "typed marker report timed out",
        &["typed-installer", "typed-historical"],
    );
}
// worker 在已 ACK 的 suspended 阶段以原 libtest 失败退出，不能变成准备成功。
#[test]
fn HistoryManager_SuspendedFailure_017() {
    failure(
        "typed-fail-suspended",
        "typed preparation: worker exited",
        &["typed-installer", "typed-historical"],
    );
}
// worker 恢复真实 child 后失败，historical 清理仍只能针对已持有的精确身份。
#[test]
fn HistoryManager_RunningFailure_018() {
    failure(
        "typed-fail-running",
        "typed marker report: worker exited",
        &["typed-installer", "typed-historical"],
    );
}
// worker fallback 的真实 release 即使让 child 正常结束也不能满足正常寿命断言。
#[test]
fn HistoryManager_RejectFallback_019() {
    failure(
        "typed-fallback",
        "typed worker fallback used",
        &["typed-installer", "typed-historical"],
    );
}
// 已新鲜捕获的真实 installer 返回 124，不能冒充最后 Job owner 消失导致终止。
#[test]
fn HistoryManager_RejectExit124_020() {
    failure(
        "typed-self-timeout",
        "controlled child self-timeout cannot prove Job close",
        &["typed-installer"],
    );
}
