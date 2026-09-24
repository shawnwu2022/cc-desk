use crate::cli::environment::EnvMap;
use crate::cli::launch_service::LaunchService;
use crate::cli::output_route::OutputRoutes;
use crate::cli::profiles::{Override, Profile};
use crate::cli::run_registry::{LaunchPhase, RunKey};
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, WireU64};
use crate::run_lifecycle::{OutputLifecycle, ProcessLifecycle};
use crate::run_supervisor::NativeRunSupervisor;
use crate::terminal_transport::{OutputAck, TerminalTransports};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};

#[cfg(windows)]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

struct Fixture {
    root: tempfile::TempDir,
    service: Arc<LaunchService>,
    supervisor: Arc<NativeRunSupervisor>,
    transports: Arc<TerminalTransports>,
    caller: crate::cli::snapshot::CallerIdentity,
    request: LaunchRequest,
    routes: Arc<OutputRoutes>,
    events: Arc<Mutex<Vec<Value>>>,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        #[cfg(windows)]
        bundled_runtime::initialize().unwrap();

        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("work");
        fs::create_dir(&cwd).unwrap();
        let repository =
            WorkspaceRepository::open(root.path().join("metadata/workspace.json")).unwrap();
        let mut profile = Profile::new("d15-supervisor", CliKind::Codex);
        profile.program_path =
            Override::Set(crate::platform::find_executable("node").expect("Node.js required"));
        let document = repository
            .apply(
                WireU64::parse("0").unwrap(),
                Patch::Create {
                    profile: profile.clone(),
                },
            )
            .unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/native-cli/owned-probe.mjs");
        let request = LaunchRequest {
            request_id: format!("d15-{mode}-request"),
            tab_id: format!("d15-{mode}-tab"),
            run_id: format!("d15-{mode}-run"),
            generation: 1,
            profile_id: profile.id.clone(),
            expected_profile_revision: document.profiles[&profile.id].revision,
            cli: CliKind::Codex,
            launch_cwd: cwd.to_str().unwrap().into(),
            action: LaunchAction::Raw {
                argv: vec![script.to_str().unwrap().into(), mode.into()],
            },
            extra_args: vec![],
            cols: 80,
            rows: 24,
        };
        let mut inherited: EnvMap = std::env::vars_os().collect();
        inherited.insert("CC_DESK_TEST_ROOT".into(), cwd.as_os_str().to_owned());

        let transports = Arc::new(TerminalTransports::new());
        let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
        let service = Arc::new(LaunchService::new(
            repository,
            Some(inherited),
            Some(supervisor.clone()),
        ));
        let caller = service.registry().activate_window("main").unwrap();
        Self {
            root,
            service,
            supervisor,
            transports,
            caller,
            request,
            routes: Arc::new(OutputRoutes::new(4)),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn start(&self) -> crate::cli::run_registry::LaunchStatus {
        let events = self.events.clone();
        self.service
            .start(&self.caller, &self.request, |_| {
                self.routes.bind(1, Box::new(|| Ok(())), || {
                    Ok(Channel::new(move |body| {
                        let InvokeResponseBody::Json(text) = body else {
                            panic!("json output frame expected");
                        };
                        events.lock().push(serde_json::from_str(&text).unwrap());
                        Ok(())
                    }))
                })
            })
            .unwrap()
    }

    fn wait_lifecycle(
        &self,
        predicate: impl Fn(&crate::run_lifecycle::LifecycleRecord) -> bool,
    ) -> crate::run_lifecycle::LifecycleRecord {
        let run = RunKey {
            run_id: self.request.run_id.clone(),
            generation: self.request.generation,
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(state) = self.supervisor.snapshot(&run) {
                if predicate(&state) {
                    return state;
                }
            }
            assert!(Instant::now() < deadline, "D15 lifecycle did not converge");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn child_reports(&self) -> usize {
        fs::read_dir(self.root.path().join("work"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .count()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let run = RunKey {
            run_id: self.request.run_id.clone(),
            generation: self.request.generation,
        };
        if let Ok(access) = self.service.access(&self.caller, &run) {
            let _ = access.terminate_root();
        }
    }
}

fn ack(run: &RunKey, epoch: &str, through: &str) -> OutputAck {
    serde_json::from_value(json!({
        "runId": run.run_id,
        "generation": run.generation,
        "streamEpoch": epoch,
        "throughOffset": through,
    }))
    .unwrap()
}

#[test]
fn D15_Supervisor_ProcessExitEofAndParsedAckAreIndependent_001() {
    let fixture = Fixture::new("exit");
    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert!(!exited.can_retire_as_complete());
    assert_eq!(
        fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run)
            .unwrap_err()
            .code,
        "RUN_NOT_READY",
        "root ownership should be released after wait/reap, not held until renderer ACK"
    );

    let terminal = fixture.wait_lifecycle(|state| {
        state.final_offset().is_some() || state.output() == OutputLifecycle::Incomplete
    });
    let events = fixture.events.lock().clone();
    let bytes: Vec<u8> = events
        .iter()
        .flat_map(|event| {
            event["bytes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_u64().unwrap() as u8)
        })
        .collect();
    assert!(
        bytes
            .windows(b"OWNED_TAIL".len())
            .any(|window| window == b"OWNED_TAIL"),
        "tail output was lost before bounded drain convergence"
    );

    if terminal.output() == OutputLifecycle::Draining {
        let final_offset = terminal
            .final_offset()
            .expect("draining terminal with observed EOF must retain final offset");
        assert!(
            final_offset.get() > 0,
            "real probe must emit tail bytes before EOF"
        );
        let epoch = events[0]["streamEpoch"].as_str().unwrap().to_string();
        fixture
            .transports
            .ack(
                &fixture.caller,
                &ack(&status.run, &epoch, &final_offset.to_string()),
            )
            .unwrap();
        let drained = fixture.wait_lifecycle(|state| state.output() == OutputLifecycle::Drained);
        assert!(drained.can_retire_as_complete());
        assert_eq!(drained.parsed_offset(), drained.final_offset().unwrap());
    } else {
        assert_eq!(terminal.output(), OutputLifecycle::Incomplete);
        assert!(
            !terminal.can_retire_as_complete(),
            "a PTY drain timeout must never be reported as complete"
        );
    }

    assert_eq!(
        fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap()
            .phase,
        LaunchPhase::Exited
    );
}

#[test]
fn D15_Supervisor_RouteLossIsDegradedAndDoesNotRestartRun_002() {
    let fixture = Fixture::new("hold");
    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(Instant::now() < deadline, "hold probe never became ready");
        std::thread::sleep(Duration::from_millis(10));
    }
    fixture.routes.revoke();
    let degraded = fixture.wait_lifecycle(|state| state.output() == OutputLifecycle::Degraded);
    assert_eq!(degraded.process(), ProcessLifecycle::Running);
    assert_eq!(fixture.child_reports(), 1, "route loss restarted the child");

    fixture
        .service
        .access(&fixture.caller, &status.run)
        .unwrap()
        .terminate_root()
        .unwrap();
    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert_eq!(exited.output(), OutputLifecycle::Degraded);
    assert_eq!(fixture.child_reports(), 1, "degraded run was replayed");
}

#[test]
fn D15_Supervisor_RootExitDoesNotCloseDescendantPtyBeforeEof_003() {
    let fixture = Fixture::new("descendant");
    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert_eq!(exited.output(), OutputLifecycle::Draining);

    let terminal = fixture.wait_lifecycle(|state| {
        state.final_offset().is_some() || state.output() == OutputLifecycle::Incomplete
    });
    let events = fixture.events.lock().clone();
    let bytes: Vec<u8> = events
        .iter()
        .flat_map(|event| {
            event["bytes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_u64().unwrap() as u8)
        })
        .collect();
    let descendant_start = fs::read_to_string(
        fixture.root.path().join("work/descendant-start.json"),
    )
    .expect("descendant never completed its startup handshake");
    assert!(
        fixture
            .root
            .path()
            .join("work/descendant-after-root.marker")
            .exists(),
        "descendant did not survive root exit; startup={descendant_start}"
    );
    assert!(
        bytes
            .windows(b"DESCENDANT_TAIL".len())
            .any(|window| window == b"DESCENDANT_TAIL"),
        "descendant survived root exit but its PTY tail was lost; startup={descendant_start}; observed={:?}",
        String::from_utf8_lossy(&bytes)
    );

    if terminal.output() == OutputLifecycle::Draining {
        let final_offset = terminal
            .final_offset()
            .expect("draining descendant stream must retain final offset");
        let epoch = events[0]["streamEpoch"].as_str().unwrap().to_string();
        fixture
            .transports
            .ack(
                &fixture.caller,
                &ack(&status.run, &epoch, &final_offset.to_string()),
            )
            .unwrap();
        let drained = fixture.wait_lifecycle(|state| state.output() == OutputLifecycle::Drained);
        assert!(drained.can_retire_as_complete());
    } else {
        assert_eq!(terminal.output(), OutputLifecycle::Incomplete);
        assert!(!terminal.can_retire_as_complete());
    }
}

#[test]
fn D15_Supervisor_ExplicitStopIsIncompleteAndDoesNotRestart_004() {
    let fixture = Fixture::new("hold");
    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(Instant::now() < deadline, "hold probe never became ready");
        std::thread::sleep(Duration::from_millis(10));
    }

    fixture.supervisor.stop(&status.run).unwrap();
    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert_eq!(exited.output(), OutputLifecycle::Incomplete);
    assert!(!exited.can_retire_as_complete());
    assert_eq!(
        fixture.child_reports(),
        1,
        "explicit stop restarted the child"
    );
}

#[test]
fn D15_Supervisor_ApplicationShutdownIsIncomplete_005() {
    let fixture = Fixture::new("hold");
    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(Instant::now() < deadline, "hold probe never became ready");
        std::thread::sleep(Duration::from_millis(10));
    }

    fixture.supervisor.shutdown();
    fixture.supervisor.shutdown();
    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert_eq!(exited.output(), OutputLifecycle::Incomplete);
    assert!(!exited.can_retire_as_complete());
}

#[test]
fn D15_Supervisor_ShutdownDuringExitedDrainStaysIncomplete_006() {
    let fixture = Fixture::new("descendant");
    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert_eq!(exited.output(), OutputLifecycle::Draining);

    fixture.supervisor.shutdown();
    let incomplete = fixture.wait_lifecycle(|state| state.output() == OutputLifecycle::Incomplete);
    assert_eq!(incomplete.process(), ProcessLifecycle::Exited);
    assert!(!incomplete.can_retire_as_complete());

    std::thread::sleep(Duration::from_millis(500));
    let stable = fixture.wait_lifecycle(|state| state.output() == OutputLifecycle::Incomplete);
    assert_eq!(stable.output(), OutputLifecycle::Incomplete);
    assert!(!stable.can_retire_as_complete());
}

#[test]
fn D15_Supervisor_ShutdownBeforeAdoptStillOwnsAndReaps_007() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.shutdown();

    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert_eq!(exited.output(), OutputLifecycle::Incomplete);
    assert!(exited.can_retire());
    assert!(!exited.can_retire_as_complete());
    assert_eq!(
        fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap()
            .phase,
        LaunchPhase::Exited
    );
}
