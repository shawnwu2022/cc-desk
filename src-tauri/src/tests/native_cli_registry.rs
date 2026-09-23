use crate::cli::environment::EnvMap;
use crate::cli::launch::LaunchCoordinator;
use crate::cli::profiles::{error, Override, Profile};
use crate::cli::run_registry::{LaunchFailure, LaunchPhase, LaunchStatus, RunKey};
use crate::cli::snapshot::{freeze_launch, CallerIdentity, FreezeContext, LaunchSnapshot};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, SafeError, WireU64};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

struct Fixture {
    _directory: tempfile::TempDir,
    profile: Profile,
    request: LaunchRequest,
    driver: LaunchCoordinator<usize>,
    caller: CallerIdentity,
    prepares: AtomicUsize,
    connects: AtomicUsize,
    spawns: AtomicUsize,
}

impl Fixture {
    fn new(capacity: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = Profile::new("test-profile", CliKind::Codex);
        profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
        let request = LaunchRequest {
            request_id: "request-one".into(),
            tab_id: "tab-one".into(),
            run_id: "run-one".into(),
            generation: 1,
            profile_id: profile.id.clone(),
            expected_profile_revision: profile.revision,
            cli: CliKind::Codex,
            launch_cwd: directory.path().to_str().unwrap().into(),
            action: LaunchAction::Raw { argv: vec!["private-prompt".into()] },
            extra_args: vec![],
            cols: 80,
            rows: 24,
        };
        let driver = LaunchCoordinator::new(capacity);
        let caller = driver.registry().activate_window("main").unwrap();
        Self {
            _directory: directory,
            profile,
            request,
            driver,
            caller,
            prepares: AtomicUsize::new(0),
            connects: AtomicUsize::new(0),
            spawns: AtomicUsize::new(0),
        }
    }

    fn freeze(&self, request: &LaunchRequest) -> Result<LaunchSnapshot, SafeError> {
        self.prepares.fetch_add(1, Ordering::SeqCst);
        let empty = EnvMap::new();
        freeze_launch(request, &self.profile, &self.caller, &FreezeContext {
            inherited: &empty,
            terminal: &empty,
            legacy: None,
            observer: None,
        })
    }

    fn start(&self, request: &LaunchRequest) -> Result<LaunchStatus, SafeError> {
        self.driver.start(&self.caller, request, || self.freeze(request), |_| {
            self.connects.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }, |_| {
            self.spawns.fetch_add(1, Ordering::SeqCst);
            Ok(42)
        })
    }

    fn next_request(&self) -> LaunchRequest {
        let mut request = self.request.clone();
        request.request_id = "request-two".into();
        request.run_id = "run-two".into();
        request.generation = 2;
        request
    }
}

#[test]
fn D11_Registry_ConcurrentRequestSpawnsOnce_01() {
    let fixture = Fixture::new(128);
    let barrier = Barrier::new(100);
    std::thread::scope(|scope| {
        for _ in 0..100 {
            let fixture = &fixture;
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                let status = fixture.start(&fixture.request).unwrap();
                assert_eq!(status.run.run_id, "run-one");
            });
        }
    });
    assert_eq!(fixture.spawns.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.connects.load(Ordering::SeqCst), 1);
    let status = fixture.driver.registry().status(&fixture.caller, "request-one").unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
}

#[test]
fn D11_Registry_ReplayBeforeProfileReads_02() {
    let mut fixture = Fixture::new(8);
    let original = fixture.start(&fixture.request).unwrap();
    fixture.profile.program_path = Override::Set("no-longer-a-valid-profile".into());
    let replay = fixture.driver.start(&fixture.caller, &fixture.request,
        || panic!("must query before deleted/edited profile"),
        |_| panic!("must not replace route"), |_| panic!("must not respawn")).unwrap();
    assert_eq!(replay, original);
}

#[test]
fn D11_Registry_SameIdChangedBodyConflictsBeforePrepare_03() {
    let fixture = Fixture::new(8);
    fixture.start(&fixture.request).unwrap();
    let mut changes = Vec::new();
    let mut request = fixture.request.clone();
    request.cols += 1;
    changes.push(request);
    let mut request = fixture.request.clone();
    request.rows += 1;
    changes.push(request);
    let mut request = fixture.request.clone();
    request.generation += 1;
    changes.push(request);
    let mut request = fixture.request.clone();
    request.action = LaunchAction::Raw { argv: vec!["different-private-prompt".into()] };
    changes.push(request);
    let mut request = fixture.request.clone();
    request.cli = CliKind::Claude;
    changes.push(request);
    let mut request = fixture.request.clone();
    request.expected_profile_revision = WireU64::parse("99").unwrap();
    changes.push(request);
    let mut request = fixture.request.clone();
    request.launch_cwd = "different-private-directory".into();
    changes.push(request);
    let mut request = fixture.request.clone();
    request.tab_id = "different-tab".into();
    changes.push(request);
    let mut request = fixture.request.clone();
    request.run_id = "different-run".into();
    changes.push(request);
    for request in changes {
        let failure = fixture.driver.start(&fixture.caller, &request,
            || panic!("conflicts must not re-read profile"), |_| Ok(()), |_| Ok(1)).unwrap_err();
        assert_eq!(failure.code, "REQUEST_CONFLICT");
        assert!(!format!("{failure:?}").contains("private"));
    }
}

#[test]
fn D11_Registry_RoutePrecedesSpawnWithoutHoldingGlobalLock_04() {
    let fixture = Fixture::new(8);
    let registry = fixture.driver.registry();
    let status = fixture.driver.start(&fixture.caller, &fixture.request,
        || fixture.freeze(&fixture.request), |pending| {
            assert_eq!(pending.phase, LaunchPhase::Reserved);
            assert_eq!(registry.status(&fixture.caller, "request-one")?.phase, LaunchPhase::Reserved);
            Ok(())
        }, |_| {
            assert_eq!(registry.status(&fixture.caller, "request-one")?.phase, LaunchPhase::Starting);
            Ok(7)
        }).unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    assert_eq!(*registry.resource(&fixture.caller, &status.run).unwrap(), 7);
}

#[test]
fn D11_Registry_RouteFailureRetainedWithoutSpawning_05() {
    let fixture = Fixture::new(8);
    let status = fixture.driver.start(&fixture.caller, &fixture.request,
        || fixture.freeze(&fixture.request), |_| Err(error("private-route-error")),
        |_| panic!("route failure must not spawn")).unwrap();
    assert_eq!(status.phase, LaunchPhase::Failed);
    assert_eq!(status.failure, Some(LaunchFailure::RouteUnavailable));
    assert_eq!(fixture.start(&fixture.request).unwrap(), status);
    assert_eq!(fixture.spawns.load(Ordering::SeqCst), 0);
    assert!(!serde_json::to_string(&status).unwrap().contains("private"));
}

#[test]
fn D11_Registry_SpawnFailureRetainedWithoutRetry_06() {
    let fixture = Fixture::new(8);
    let status = fixture.driver.start(&fixture.caller, &fixture.request,
        || fixture.freeze(&fixture.request), |_| Ok(()), |_| Err(error("private-spawn-error"))).unwrap();
    assert_eq!(status.failure, Some(LaunchFailure::ProcessStartFailed));
    assert_eq!(status.phase, LaunchPhase::Failed);
    assert_eq!(fixture.start(&fixture.request).unwrap(), status);
    assert_eq!(fixture.spawns.load(Ordering::SeqCst), 0);
}

#[test]
fn D11_Registry_RejectsWrongCallerBeforeAnyWork_07() {
    let fixture = Fixture::new(8);
    let mut callers = Vec::new();
    let mut caller = fixture.caller.clone();
    caller.instance_id = "other-backend".into();
    callers.push(caller);
    let mut caller = fixture.caller.clone();
    caller.window_label = "other-window".into();
    callers.push(caller);
    let mut caller = fixture.caller.clone();
    caller.webview_epoch = WireU64::parse("99").unwrap();
    callers.push(caller);
    for caller in callers {
        let failure = fixture.driver.start(&caller, &fixture.request,
            || panic!("unauthorized prepare"), |_| panic!("unauthorized route"),
            |_| panic!("unauthorized spawn")).unwrap_err();
        assert_eq!(failure.code, "FORBIDDEN");
    }
    assert!(fixture.driver.registry().activate_window("untrusted").is_err());
}

#[test]
fn D11_Registry_NewWindowDoesNotInheritOldRun_08() {
    let fixture = Fixture::new(8);
    let status = fixture.start(&fixture.request).unwrap();
    let registry = fixture.driver.registry();
    let new_caller = registry.activate_window("main").unwrap();
    assert_ne!(new_caller.webview_epoch, fixture.caller.webview_epoch);
    assert_eq!(registry.status(&fixture.caller, "request-one").unwrap_err().code, "FORBIDDEN");
    assert_eq!(registry.resource(&new_caller, &status.run).unwrap_err().code, "FORBIDDEN");
    assert_eq!(registry.revoke_window(&fixture.caller).unwrap_err().code, "FORBIDDEN");
    assert_eq!(registry.status(&new_caller, "missing").unwrap_err().code, "LAUNCH_NOT_FOUND");
    registry.mark_exited(&status.run).unwrap();
    registry.retire(&status.run).unwrap();
}

#[test]
fn D11_Registry_RunAndGenerationNeverAlias_09() {
    let fixture = Fixture::new(8);
    let first = fixture.start(&fixture.request).unwrap();
    let mut second_request = fixture.next_request();
    assert_eq!(fixture.start(&second_request).unwrap_err().code, "TAB_BUSY");
    fixture.driver.registry().mark_exited(&first.run).unwrap();
    fixture.driver.registry().retire(&first.run).unwrap();
    second_request.generation = 1;
    assert_eq!(fixture.start(&second_request).unwrap_err().code, "STALE_GENERATION");
    second_request.generation = 2;
    second_request.run_id = first.run.run_id.clone();
    assert_eq!(fixture.start(&second_request).unwrap_err().code, "RUN_ID_CONFLICT");
    second_request.run_id = "run-two".into();
    let second = fixture.start(&second_request).unwrap();
    let stale = RunKey { run_id: second.run.run_id.clone(), generation: 1 };
    assert!(fixture.driver.registry().resource(&fixture.caller, &stale).is_err());
    assert_eq!(*fixture.driver.registry().resource(&fixture.caller, &second.run).unwrap(), 42);
}

#[test]
fn D11_Registry_BudgetNeverEvictsReplayTombstones_10() {
    let fixture = Fixture::new(1);
    let first = fixture.start(&fixture.request).unwrap();
    fixture.driver.registry().mark_exited(&first.run).unwrap();
    fixture.driver.registry().retire(&first.run).unwrap();
    assert_eq!(fixture.start(&fixture.next_request()).unwrap_err().code, "REGISTRY_CAPACITY");
    assert_eq!(fixture.start(&fixture.request).unwrap().phase, LaunchPhase::Exited);
    assert_eq!(fixture.spawns.load(Ordering::SeqCst), 1);
}

#[test]
fn D11_Registry_PanicDoesNotAllowReplay_11() {
    for at_spawn in [false, true] {
        let fixture = Fixture::new(8);
        let result = catch_unwind(AssertUnwindSafe(|| {
            fixture.driver.start(&fixture.caller, &fixture.request,
                || fixture.freeze(&fixture.request), |_| {
                    assert!(at_spawn, "synthetic route panic");
                    Ok(())
                }, |_| panic!("synthetic spawn panic"))
        }));
        assert!(result.is_err());
        let replay = fixture.start(&fixture.request).unwrap();
        assert_eq!(replay.phase, if at_spawn { LaunchPhase::Indeterminate } else { LaunchPhase::Failed });
        assert_eq!(fixture.spawns.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn D11_Registry_RevokeDuringRouteStopsBeforeSpawn_12() {
    let fixture = Fixture::new(8);
    let status = fixture.driver.start(&fixture.caller, &fixture.request,
        || fixture.freeze(&fixture.request), |_| {
            fixture.driver.registry().revoke_window(&fixture.caller)?;
            Ok(())
        }, |_| panic!("revoked before spawn")).unwrap();
    assert_eq!(status.phase, LaunchPhase::Cancelled);
}

#[test]
fn D11_Registry_ImmediateExitCannotRegressToRunning_13() {
    let fixture = Fixture::new(8);
    let registry = fixture.driver.registry();
    let key = RunKey { run_id: fixture.request.run_id.clone(), generation: fixture.request.generation };
    let status = fixture.driver.start(&fixture.caller, &fixture.request,
        || fixture.freeze(&fixture.request), |_| Ok(()), |_| {
            registry.mark_exited(&key)?;
            Ok(123)
        }).unwrap();
    assert_eq!(status.phase, LaunchPhase::Exited);
    assert_eq!(*registry.resource(&fixture.caller, &key).unwrap(), 123);
    registry.retire(&key).unwrap();
    assert!(registry.resource(&fixture.caller, &key).is_err());
    assert_eq!(fixture.start(&fixture.request).unwrap().phase, LaunchPhase::Exited);
}

#[test]
fn D11_Registry_SnapshotOwnerAndBodyCannotBeSubstituted_14() {
    let fixture = Fixture::new(8);
    let other = fixture.next_request();
    let failure = fixture.driver.start(&fixture.caller, &fixture.request,
        || fixture.freeze(&other), |_| panic!("mismatched route"), |_| panic!("mismatched spawn")).unwrap_err();
    assert_eq!(failure.code, "REQUEST_SNAPSHOT_MISMATCH");
    assert_eq!(fixture.driver.registry().status(&fixture.caller, "request-one").unwrap_err().code, "LAUNCH_NOT_FOUND");
}

#[test]
fn D11_Registry_RevokeAfterSpawnPointRetainsOwnedResource_15() {
    let fixture = Fixture::new(8);
    let registry = fixture.driver.registry();
    let status = fixture.driver.start(&fixture.caller, &fixture.request,
        || fixture.freeze(&fixture.request), |_| Ok(()), |_| {
            registry.revoke_window(&fixture.caller)?;
            Ok(7)
        }).unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    assert_eq!(registry.resource(&fixture.caller, &status.run).unwrap_err().code, "FORBIDDEN");
    registry.mark_exited(&status.run).unwrap();
    registry.retire(&status.run).unwrap();
}

#[test]
fn D11_Registry_SafeReceiptsContainNoSnapshotSecrets_16() {
    let fixture = Fixture::new(8);
    let status = fixture.start(&fixture.request).unwrap();
    let receipt = serde_json::to_string(&status).unwrap();
    assert!(!receipt.contains("private-prompt"));
    assert!(!receipt.contains(&fixture.request.launch_cwd));
    assert_eq!(format!("{:?}", fixture.driver.registry()), "RunRegistry(<redacted>)");
}

#[test]
fn D11_Registry_DifferentRunCanStartWhileFirstIsBlocked_17() {
    use std::sync::mpsc;
    use std::time::Duration;
    let fixture = Arc::new(Fixture::new(8));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = fixture.clone();
    let worker = std::thread::spawn(move || {
        first.driver.start(&first.caller, &first.request,
            || first.freeze(&first.request), |_| Ok(()), |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(1)
            })
    });
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut second = fixture.next_request();
    second.tab_id = "tab-two".into();
    let status = fixture.start(&second).unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap().is_ok());
}
