use crate::cli::environment::EnvMap;
use crate::cli::launch::LaunchCoordinator;
use crate::cli::profiles::{error, Override, Profile};
use crate::cli::run_registry::LaunchPhase;
use crate::cli::snapshot::{freeze_launch, CallerIdentity, FreezeContext, LaunchSnapshot};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, SafeError};
use std::sync::{mpsc, Arc};
use std::time::Duration;

struct Inputs {
    _directory: tempfile::TempDir,
    profile: Profile,
    request: LaunchRequest,
}

impl Inputs {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = Profile::new("edges", CliKind::Codex);
        profile.program_path =
            Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
        let request = LaunchRequest {
            request_id: "edge-request".into(),
            tab_id: "edge-tab".into(),
            run_id: "edge-run".into(),
            generation: 1,
            profile_id: profile.id.clone(),
            expected_profile_revision: profile.revision,
            cli: CliKind::Codex,
            launch_cwd: directory.path().to_str().unwrap().into(),
            action: LaunchAction::New,
            extra_args: vec![],
            cols: 80,
            rows: 24,
        };
        Self {
            _directory: directory,
            profile,
            request,
        }
    }

    fn freeze(&self, caller: &CallerIdentity) -> Result<LaunchSnapshot, SafeError> {
        let empty = EnvMap::new();
        freeze_launch(
            &self.request,
            &self.profile,
            caller,
            &FreezeContext {
                inherited: &empty,
                terminal: &empty,
                legacy: None,
                observer: None,
            },
        )
    }
}

#[test]
fn D11_Edges_RoutingIdsBoundedBeforePrepare_01() {
    let driver = LaunchCoordinator::<usize>::new(8);
    let caller = driver.registry().activate_window("main").unwrap();
    for field in ["requestId", "runId", "tabId"] {
        for value in ["x".repeat(129), "control\nvalue".into()] {
            let mut inputs = Inputs::new();
            match field {
                "requestId" => inputs.request.request_id = value,
                "runId" => inputs.request.run_id = value,
                _ => inputs.request.tab_id = value,
            }
            let failure = driver
                .start(
                    &caller,
                    &inputs.request,
                    || panic!("oversized routing identity reached preparation"),
                    |_| Ok(()),
                    |_| Ok(1),
                )
                .unwrap_err();
            assert_eq!(failure.code, "INVALID_REQUEST");
            assert_eq!(failure.field.as_deref(), Some(field));
        }
    }
}

#[test]
fn D11_Edges_FingerprintBudgetRejectsBeforePrepare_02() {
    let driver = LaunchCoordinator::<usize>::new(8);
    let caller = driver.registry().activate_window("main").unwrap();
    let mut inputs = Inputs::new();
    inputs.request.action = LaunchAction::Raw {
        argv: vec!["x".repeat(8 * 1024 * 1024)],
    };
    let failure = driver
        .start(
            &caller,
            &inputs.request,
            || panic!("oversized request reached preparation"),
            |_| Ok(()),
            |_| Ok(1),
        )
        .unwrap_err();
    assert_eq!(failure.code, "REQUEST_TOO_LARGE");
    assert!(!format!("{failure:?}").contains("xxxxxxxx"));
}

#[test]
fn D11_Edges_ReceiptHasInstanceAndMonotoneRevision_03() {
    let driver = LaunchCoordinator::<usize>::new(8);
    let caller = driver.registry().activate_window("main").unwrap();
    let inputs = Inputs::new();
    let registry = driver.registry();
    let status = driver
        .start(
            &caller,
            &inputs.request,
            || inputs.freeze(&caller),
            |pending| {
                let value = serde_json::to_value(pending).unwrap();
                assert_eq!(value["instanceId"], caller.instance_id);
                assert_eq!(value["revision"], "0");
                Ok(())
            },
            |_| {
                let value =
                    serde_json::to_value(registry.status(&caller, "edge-request")?).unwrap();
                assert_eq!(value["revision"], "1");
                Ok(1)
            },
        )
        .unwrap();
    assert_eq!(serde_json::to_value(&status).unwrap()["revision"], "2");
    registry.mark_exited(&status.run).unwrap();
    let exited = registry.status(&caller, "edge-request").unwrap();
    assert_eq!(serde_json::to_value(&exited).unwrap()["revision"], "3");
    registry.mark_exited(&status.run).unwrap();
    registry.retire(&status.run).unwrap();
    assert_eq!(registry.status(&caller, "edge-request").unwrap(), exited);
}

#[test]
fn D11_Edges_SameBodyWrongSnapshotOwnerIsRejected_04() {
    let driver = LaunchCoordinator::<usize>::new(8);
    let caller = driver.registry().activate_window("main").unwrap();
    let mut other = caller.clone();
    other.instance_id = "different-backend".into();
    let inputs = Inputs::new();
    let failure = driver
        .start(
            &caller,
            &inputs.request,
            || inputs.freeze(&other),
            |_| panic!("wrong owner reached route"),
            |_| panic!("wrong owner reached spawn"),
        )
        .unwrap_err();
    assert_eq!(failure.code, "REQUEST_SNAPSHOT_MISMATCH");
}

struct ReentrantResource(Box<dyn Fn() + Send + Sync>);

impl Drop for ReentrantResource {
    fn drop(&mut self) {
        (self.0)();
    }
}

#[test]
fn D11_Edges_RetirementDestructorRunsOutsideRegistryLock_05() {
    let driver = LaunchCoordinator::<ReentrantResource>::new(8);
    let caller = driver.registry().activate_window("main").unwrap();
    let inputs = Inputs::new();
    let weak = Arc::downgrade(driver.registry());
    let on_drop_caller = caller.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let status = driver
        .start(
            &caller,
            &inputs.request,
            || inputs.freeze(&caller),
            |_| Ok(()),
            |_| {
                Ok(ReentrantResource(Box::new(move || {
                    let registry = weak.upgrade().unwrap();
                    let status = registry.status(&on_drop_caller, "edge-request").unwrap();
                    done_tx.send(status.phase).unwrap();
                })))
            },
        )
        .unwrap();
    driver.registry().mark_exited(&status.run).unwrap();
    let registry = driver.registry().clone();
    let worker = std::thread::spawn(move || registry.retire(&status.run));
    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        LaunchPhase::Exited
    );
    worker.join().unwrap().unwrap();
}

#[test]
fn D11_Edges_PrepareFailureDoesNotHideConcurrentWinner_06() {
    let driver = LaunchCoordinator::<usize>::new(8);
    let caller = driver.registry().activate_window("main").unwrap();
    let inputs = Inputs::new();
    let status = driver
        .start(
            &caller,
            &inputs.request,
            || {
                driver.start(
                    &caller,
                    &inputs.request,
                    || inputs.freeze(&caller),
                    |_| Ok(()),
                    |_| Ok(42),
                )?;
                Err(error("PROFILE_DELETED_AFTER_WINNER"))
            },
            |_| panic!("loser must not replace route"),
            |_| panic!("loser must not spawn"),
        )
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    assert_eq!(
        *driver.registry().resource(&caller, &status.run).unwrap(),
        42
    );
}

#[test]
fn D11_Edges_ExitedResourcesCannotRetireBeforeSpawnReturns_07() {
    let driver = LaunchCoordinator::<usize>::new(8);
    let caller = driver.registry().activate_window("main").unwrap();
    let inputs = Inputs::new();
    let status = driver
        .start(
            &caller,
            &inputs.request,
            || inputs.freeze(&caller),
            |_| Ok(()),
            |_| {
                let status = driver.registry().status(&caller, "edge-request")?;
                driver.registry().mark_exited(&status.run)?;
                assert_eq!(
                    driver.registry().retire(&status.run).unwrap_err().code,
                    "RUN_NOT_READY"
                );
                Ok(42)
            },
        )
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Exited);
    driver.registry().retire(&status.run).unwrap();
}
