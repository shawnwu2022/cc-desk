use crate::cli::environment::EnvMap;
use crate::cli::launch_service::LaunchService;
use crate::cli::output_route::OutputRoutes;
use crate::cli::profiles::{Override, Profile};
use crate::cli::run_registry::{LaunchPhase, RunKey};
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, WireU64};
use crate::run_lifecycle::{OutputLifecycle, ProcessLifecycle};
use crate::run_supervisor::NativeRunSupervisor;
use crate::terminal_transport::{OutputAck, TerminalStream, TerminalTransports};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
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
    next_route_id: AtomicU32,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let transports = Arc::new(TerminalTransports::new());
        let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
        Self::with_runtime(mode, supervisor, transports)
    }

    fn with_runtime(
        mode: &str,
        supervisor: Arc<NativeRunSupervisor>,
        transports: Arc<TerminalTransports>,
    ) -> Self {
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
            next_route_id: AtomicU32::new(1),
        }
    }

    fn start(&self) -> crate::cli::run_registry::LaunchStatus {
        self.start_request(&self.request)
    }

    fn request_run(&self) -> RunKey {
        RunKey {
            run_id: self.request.run_id.clone(),
            generation: self.request.generation,
        }
    }

    fn start_request(&self, request: &LaunchRequest) -> crate::cli::run_registry::LaunchStatus {
        let events = self.events.clone();
        let route_id = self.next_route_id.fetch_add(1, Ordering::SeqCst);
        self.service
            .start(&self.caller, request, |_| {
                self.routes.bind(route_id, Box::new(|| Ok(())), || {
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

    fn attach_probe_stream(&self, run: &RunKey) -> Arc<TerminalStream> {
        let events = self.events.clone();
        let route_id = self.next_route_id.fetch_add(1, Ordering::SeqCst);
        let route = Arc::new(
            self.routes
                .bind(route_id, Box::new(|| Ok(())), || {
                    Ok(Channel::new(move |body| {
                        let InvokeResponseBody::Json(text) = body else {
                            panic!("json output frame expected");
                        };
                        events.lock().push(serde_json::from_str(&text).unwrap());
                        Ok(())
                    }))
                })
                .unwrap(),
        );
        self.transports
            .attach(self.caller.clone(), run.clone(), route)
            .unwrap()
    }

    fn wait_lifecycle(
        &self,
        predicate: impl Fn(&crate::run_lifecycle::LifecycleRecord) -> bool,
    ) -> crate::run_lifecycle::LifecycleRecord {
        let run = self.request_run();
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

    fn wait_registry_retired(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                Instant::now() < deadline,
                "D15 registry retirement timed out"
            );
            let resource = self
                .service
                .registry()
                .resource(&self.caller, &self.request_run());
            assert!(
                Instant::now() < deadline,
                "D15 registry read exceeded deadline"
            );
            match resource {
                Err(failure) => {
                    assert_eq!(failure.code, "RUN_NOT_READY");
                    break;
                }
                Ok(resource) => drop(resource),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            self.service
                .registry()
                .status(&self.caller, &self.request.request_id)
                .unwrap()
                .phase,
            LaunchPhase::Exited,
        );
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
    struct RegistryStepsPause(Arc<NativeRunSupervisor>);
    impl Drop for RegistryStepsPause {
        fn drop(&mut self) {
            self.0.release_registry_steps_for_test();
        }
    }
    // 固定 reap 已发生但 registry 尚未收口的窗口，失败路径也释放阻塞。
    fixture.supervisor.pause_registry_steps_for_test();
    let registry_pause = RegistryStepsPause(fixture.supervisor.clone());
    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert!(!exited.can_retire_as_complete());
    // Actual waiter evidence settles maintenance independently of renderer ACK
    // and retained output resources; no registry/map inference is involved.
    fixture
        .service
        .repository()
        .admission()
        .freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
    let resource = fixture
        .service
        .registry()
        .resource(&fixture.caller, &status.run);
    assert!(
        resource.is_ok(),
        "real reap can be observed before paused registry retirement"
    );
    drop(resource);
    drop(registry_pause);
    let retirement_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        assert!(
            Instant::now() < retirement_deadline,
            "registry retirement did not converge without renderer ACK"
        );
        let resource = fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run);
        assert!(
            Instant::now() < retirement_deadline,
            "registry retirement returned after its deadline"
        );
        match resource {
            Err(failure) => {
                assert_eq!(
                    failure.code, "RUN_NOT_READY",
                    "root ownership should be released after wait/reap, not held until renderer ACK"
                );
                break;
            }
            Ok(resource) => drop(resource),
        }
        std::thread::sleep(Duration::from_millis(10));
    }

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
fn D15_Supervisor_RootExitDrainsAttachedTailWithoutAssumingChildSurvival_003() {
    let fixture = Fixture::new("descendant");
    let status = fixture.start();
    assert_eq!(status.phase, LaunchPhase::Running);

    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert_eq!(exited.output(), OutputLifecycle::Draining);

    let terminal = fixture.wait_lifecycle(|state| {
        state.final_offset().is_some() || state.output() == OutputLifecycle::Incomplete
    });
    let descendant_start =
        fs::read_to_string(fixture.root.path().join("work/descendant-start.json"))
            .expect("descendant never completed its startup handshake");
    let marker = fixture
        .root
        .path()
        .join("work/descendant-after-root.marker");
    let marker_deadline = Instant::now() + Duration::from_secs(2);
    while !marker.exists() && Instant::now() < marker_deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let descendant_survived = marker.exists();
    #[cfg(not(windows))]
    assert!(
        descendant_survived,
        "attached descendant unexpectedly died with root; startup={descendant_start}"
    );

    let tail_deadline = Instant::now() + Duration::from_secs(1);
    let events = if descendant_survived {
        loop {
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
            if bytes
                .windows(b"DESCENDANT_TAIL".len())
                .any(|window| window == b"DESCENDANT_TAIL")
                || Instant::now() >= tail_deadline
            {
                break events;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        fixture.events.lock().clone()
    };
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

    if descendant_survived {
        assert!(
            bytes
                .windows(b"DESCENDANT_TAIL".len())
                .any(|window| window == b"DESCENDANT_TAIL"),
            "surviving attached descendant lost its PTY tail; startup={descendant_start}; observed={:?}",
            String::from_utf8_lossy(&bytes)
        );
    } else {
        #[cfg(windows)]
        {
            let startup: Value = serde_json::from_str(&descendant_start).unwrap();
            assert_eq!(
                startup["stdoutIsTTY"], true,
                "fixture child must have been attached before root exit"
            );
            assert!(
                !bytes
                    .windows(b"DESCENDANT_TAIL".len())
                    .any(|window| window == b"DESCENDANT_TAIL"),
                "tail cannot precede the descendant's after-root marker"
            );
        }
    }

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

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
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
    fixture.wait_registry_retired();
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

#[test]
fn D15_Supervisor_AttachFailureAfterSpawnStillStopsAndReaps_008() {
    let fixture = Fixture::new("hold");
    let run = RunKey {
        run_id: fixture.request.run_id.clone(),
        generation: fixture.request.generation,
    };

    let occupied_route = Arc::new(
        fixture
            .routes
            .bind(2, Box::new(|| Ok(())), || Ok(Channel::new(|_body| Ok(()))))
            .unwrap(),
    );
    let _occupied = fixture
        .transports
        .attach(fixture.caller.clone(), run.clone(), occupied_route)
        .unwrap();

    let events = fixture.events.clone();
    let failure = fixture
        .service
        .start(&fixture.caller, &fixture.request, |_| {
            fixture.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    let InvokeResponseBody::Json(text) = body else {
                        panic!("json output frame expected");
                    };
                    events.lock().push(serde_json::from_str(&text).unwrap());
                    Ok(())
                }))
            })
        })
        .unwrap_err();
    assert_eq!(failure.code, "RUN_HANDOFF_FAILED");

    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    fixture.wait_registry_retired();
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
    assert!(
        fixture.child_reports() <= 1,
        "handoff failure must not replay or orphan a second child"
    );
}

#[test]
fn D15_Supervisor_RouteLossThenShutdownKeepsDegraded_009() {
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

    fixture.supervisor.shutdown();
    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    assert_eq!(exited.output(), OutputLifecycle::Degraded);
    assert!(!exited.can_retire_as_complete());
    assert_eq!(
        fixture.child_reports(),
        1,
        "shutdown restarted degraded run"
    );
}

// 第一次 wait 失败后，显式 stop 必须继续控制并 reap 原 child，不能重启或丢失所有权。
#[test]
fn D15_WaitFail_StopReaps_010() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.fail_next_wait_for_test();
    let status = fixture.start();

    let failed = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Failed);
    assert_eq!(failed.output(), OutputLifecycle::Incomplete);
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(
            Instant::now() < ready_deadline,
            "wait-fault child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap()
            .phase,
        LaunchPhase::Running,
        "wait failure must not manufacture process exit"
    );

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    fixture.wait_registry_retired();
    assert_eq!(exited.output(), OutputLifecycle::Incomplete);
    assert_eq!(fixture.child_reports(), 1, "stop replayed the child");
    assert_eq!(
        fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap()
            .phase,
        LaunchPhase::Exited
    );
    assert_eq!(
        fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run)
            .unwrap_err()
            .code,
        "RUN_NOT_READY"
    );
}

// shutdown 从 wait 故障态重试同一 child；完成前仍保持真实 registry 状态。
#[test]
fn D15_WaitFail_ShutdownReaps_011() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.fail_next_wait_for_test();
    let status = fixture.start();

    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Failed);
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(
            Instant::now() < ready_deadline,
            "wait-fault child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fixture.supervisor.shutdown();

    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    fixture.wait_registry_retired();
    assert_eq!(exited.output(), OutputLifecycle::Incomplete);
    assert_eq!(fixture.child_reports(), 1, "shutdown replayed the child");
    assert_eq!(
        fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap()
            .phase,
        LaunchPhase::Exited
    );
    assert_eq!(
        fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run)
            .unwrap_err()
            .code,
        "RUN_NOT_READY"
    );
}

// 真实 reap 后 registry 两步失败都保留监督权，并由显式 stop 逐步重试收口。
#[test]
fn D15_RegistryFail_StopRetries_012() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.fail_next_registry_steps_for_test();
    let status = fixture.start();
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(
            Instant::now() < ready_deadline,
            "registry-fault child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    let retry_deadline = Instant::now() + Duration::from_secs(15);
    while !fixture.supervisor.retry_ready_for_test(&status.run)
        || fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap()
            .phase
            != LaunchPhase::Exited
    {
        assert!(
            Instant::now() < retry_deadline,
            "registry failures did not leave an explicitly retryable owner"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fixture
        .service
        .registry()
        .resource(&fixture.caller, &status.run)
        .expect("retire failure must retain exact-owned resource");

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
    let retired_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let result = fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run);
        if result
            .as_ref()
            .is_err_and(|failure| failure.code == "RUN_NOT_READY")
        {
            break;
        }
        assert!(
            Instant::now() < retired_deadline,
            "registry resource was not retired after explicit retry"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fixture.child_reports(),
        1,
        "registry retry replayed the child"
    );
}

// terminate 已接受后原 waiter 才失败，单次 stop 也必须有界重试并 reap 同一 child。
#[test]
fn D15_StopThenWaitFail_AutoRetries_013() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.fail_wait_after_stop_for_test();
    let status = fixture.start();
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(
            Instant::now() < ready_deadline,
            "delayed wait-fault child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    fixture.wait_registry_retired();
    assert_eq!(exited.output(), OutputLifecycle::Incomplete);
    assert_eq!(fixture.child_reports(), 1, "wait retry replayed the child");
    assert_eq!(
        fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap()
            .phase,
        LaunchPhase::Exited
    );
    assert_eq!(
        fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run)
            .unwrap_err()
            .code,
        "RUN_NOT_READY"
    );
}

// 已收口 old state 的迟到 retry 只能完成本地释放，不能再次调用 registry。
#[test]
fn D15_LateRetry_DoesNotTouchRegistry_014() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.pause_automatic_retry_for_test();
    fixture.supervisor.fail_next_mark_exited_for_test();
    let status = fixture.start();
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(
            Instant::now() < ready_deadline,
            "late-retry child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    let retry_deadline = Instant::now() + Duration::from_secs(15);
    while !fixture.supervisor.retry_ready_for_test(&status.run) {
        assert!(
            Instant::now() < retry_deadline,
            "mark-exited fault did not become retryable"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
    let retired_deadline = Instant::now() + Duration::from_secs(15);
    while fixture
        .service
        .registry()
        .resource(&fixture.caller, &status.run)
        .is_ok()
    {
        assert!(
            Instant::now() < retired_deadline,
            "explicit retry did not retire the old resource"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fixture.supervisor.registry_step_calls_for_test(), 3);

    fixture.supervisor.release_automatic_retry_for_test();
    let delayed_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.supervisor.automatic_retry_calls_for_test() == 0 {
        assert!(
            Instant::now() < delayed_deadline,
            "delayed retry did not execute"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        fixture.supervisor.registry_step_calls_for_test(),
        3,
        "late retry touched registry after old state retired"
    );
    assert_eq!(fixture.child_reports(), 1, "late retry replayed the child");
}

// 连续 wait 错误即使提前唤醒 shutdown，也必须按单调时钟节流后再控制。
#[test]
fn D15_ShutdownRetry_IsThrottled_015() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.fail_wait_count_for_test(3);
    let status = fixture.start();
    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Failed);
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(
            Instant::now() < ready_deadline,
            "throttled-retry child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let started = Instant::now();
    fixture.supervisor.shutdown();
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "failure notifications bypassed shutdown retry throttling"
    );
    let exited = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    fixture.wait_registry_retired();
    assert_eq!(exited.output(), OutputLifecycle::Incomplete);
    assert_eq!(
        fixture.child_reports(),
        1,
        "shutdown retry replayed the child"
    );
    assert_eq!(
        fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap()
            .phase,
        LaunchPhase::Exited
    );
    assert_eq!(
        fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run)
            .unwrap_err()
            .code,
        "RUN_NOT_READY"
    );
}

// reap 后 registry step 尚在运行时的一次 stop，也必须登记后续自动重试。
#[test]
fn D15_StopDuringRegistry_AutoRetries_016() {
    let fixture = Fixture::new("exit");
    fixture.supervisor.pause_registry_steps_for_test();
    fixture.supervisor.fail_next_mark_exited_for_test();
    let status = fixture.start();
    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
    fixture.supervisor.release_registry_steps_for_test();

    let retired_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let result = fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run);
        if result
            .as_ref()
            .is_err_and(|failure| failure.code == "RUN_NOT_READY")
        {
            break;
        }
        assert!(
            Instant::now() < retired_deadline,
            "single stop did not retry failed registry convergence"
        );
        std::thread::sleep(Duration::from_millis(10));
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
    assert_eq!(
        fixture.child_reports(),
        1,
        "registry retry replayed the child"
    );
}

// 旧 caller 已校验的同 run key 不得在新 owner 采用后被重定向到新 child。
#[test]
fn D15_StaleCaller_CannotStopReusedRun_017() {
    let old = Fixture::new("hold");
    let old_status = old.start();
    old.service
        .registry()
        .check_run(&old.caller, &old_status.run)
        .unwrap();
    old.supervisor.stop(&old.caller, &old_status).unwrap();
    old.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    let removed_deadline = Instant::now() + Duration::from_secs(15);
    while old.supervisor.active_for_test(&old_status.run) {
        assert!(
            Instant::now() < removed_deadline,
            "old supervisor state did not finish exact retirement"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    let current = Fixture::with_runtime("hold", old.supervisor.clone(), old.transports.clone());
    let current_status = current.start();
    assert_eq!(current_status.run, old_status.run);
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while current.child_reports() == 0 {
        assert!(
            Instant::now() < ready_deadline,
            "reused-run child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        current
            .supervisor
            .stop(&old.caller, &current_status)
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    let running = current.wait_lifecycle(|state| state.process() == ProcessLifecycle::Running);
    assert_eq!(running.process(), ProcessLifecycle::Running);
    assert_eq!(current.child_reports(), 1, "stale stop replaced the child");

    current
        .supervisor
        .stop(&current.caller, &current_status)
        .unwrap();
    current.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
}

// 同 caller 的旧 receipt 完全收口并被淘汰后，也不能停止复用同 RunKey 的新 child。
#[test]
fn D15_LateReceipt_RejectsReusedRun_018() {
    let fixture = Fixture::new("hold");
    let old_status = fixture.start();
    let first_ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(
            Instant::now() < first_ready_deadline,
            "old same-caller child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let old_stream = fixture
        .supervisor
        .stream_for_test(&old_status.run)
        .expect("old stream must stay strongly referenced by the regression");
    fixture
        .supervisor
        .stop(&fixture.caller, &old_status)
        .unwrap();
    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
    let removed_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.supervisor.active_for_test(&old_status.run) {
        assert!(
            Instant::now() < removed_deadline,
            "old same-caller state did not finish exact retirement"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    let mut evicted = false;
    for index in 0..5000 {
        let mut tombstone = fixture.request.clone();
        tombstone.request_id = format!("d15-evict-request-{index}");
        tombstone.tab_id = format!("d15-evict-tab-{index}");
        tombstone.run_id = format!("d15-evict-run-{index}");
        fixture
            .service
            .registry()
            .cancel(&fixture.caller, &tombstone)
            .unwrap();
        if fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .is_err_and(|failure| failure.code == "LAUNCH_NOT_FOUND")
        {
            evicted = true;
            break;
        }
    }
    assert!(evicted, "old completed receipt was not evicted");

    let mut replacement = fixture.request.clone();
    replacement.request_id = "d15-replacement-request".into();
    replacement.tab_id = "d15-replacement-tab".into();
    let current_status = fixture.start_request(&replacement);
    assert_eq!(current_status.run, old_status.run);
    assert_eq!(current_status.phase, LaunchPhase::Running);
    assert_eq!(
        old_stream.send(&[1]).unwrap_err().code,
        "OUTPUT_STREAM_DEGRADED"
    );
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() < 2 {
        assert!(
            Instant::now() < ready_deadline,
            "same-caller replacement child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        fixture
            .service
            .registry()
            .admit_run(&fixture.caller, &old_status.run, &old_status.request_id)
            .unwrap_err()
            .code,
        "REQUEST_CONFLICT"
    );
    assert_eq!(
        fixture
            .supervisor
            .stop(&fixture.caller, &old_status)
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    let running = fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Running);
    assert_eq!(running.process(), ProcessLifecycle::Running);
    assert_eq!(
        fixture.child_reports(),
        2,
        "late stop replaced the new child"
    );

    let admitted = fixture
        .service
        .registry()
        .admit_run(
            &fixture.caller,
            &current_status.run,
            &current_status.request_id,
        )
        .unwrap();
    fixture.supervisor.stop(&fixture.caller, &admitted).unwrap();
    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
}

// resource 已 retire 但本地标志尚未发布时，精确 receipt 的 stop 仍应成功。
#[test]
fn D15_StopAfterRetire_IsAuthorized_019() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.pause_retirement_publish_for_test();
    let status = fixture.start();
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.child_reports() == 0 {
        assert!(
            Instant::now() < ready_deadline,
            "retirement-race child never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    fixture.supervisor.stop(&fixture.caller, &status).unwrap();
    let retire_deadline = Instant::now() + Duration::from_secs(15);
    let retired = loop {
        if fixture
            .service
            .registry()
            .resource(&fixture.caller, &status.run)
            .is_err_and(|failure| failure.code == "RUN_NOT_READY")
        {
            break true;
        }
        if Instant::now() >= retire_deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stop_during_publish = retired.then(|| fixture.supervisor.stop(&fixture.caller, &status));
    fixture.supervisor.release_retirement_publish_for_test();

    assert!(
        retired,
        "registry resource did not retire before publish barrier"
    );
    stop_during_publish.unwrap().unwrap();
    fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
}

// stop 在 waiter 已安装、stream 尚未安装时只能结束 child，不能提前删除 active owner。
#[test]
fn D15_AdoptGate_StopPinsState_020() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.pause_adoption_setup_for_test();
    let (stop_result, exited_while_paused, pinned_while_paused) = std::thread::scope(|scope| {
        let starting = scope.spawn(|| fixture.start());
        let setup_deadline = Instant::now() + Duration::from_secs(15);
        while !fixture.supervisor.adoption_setup_waiting_for_test()
            && Instant::now() < setup_deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        let receipt = fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap();
        let stop_result = fixture.supervisor.stop(&fixture.caller, &receipt);
        let exit_deadline = Instant::now() + Duration::from_secs(15);
        let exited_while_paused = loop {
            if fixture
                .supervisor
                .snapshot(&receipt.run)
                .is_some_and(|state| state.process() == ProcessLifecycle::Exited)
            {
                break true;
            }
            if Instant::now() >= exit_deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let pinned_while_paused = fixture.supervisor.active_for_test(&receipt.run)
            && fixture
                .service
                .registry()
                .status(&fixture.caller, &receipt.request_id)
                .is_ok();
        fixture.supervisor.release_adoption_setup_for_test();
        starting.join().unwrap();
        (stop_result, exited_while_paused, pinned_while_paused)
    });

    stop_result.unwrap();
    assert!(exited_while_paused, "stopped setup child was not reaped");
    assert!(
        pinned_while_paused,
        "setup gate released active ownership before stream installation"
    );
    let removed_deadline = Instant::now() + Duration::from_secs(15);
    while fixture.supervisor.active_for_test(&fixture.request_run()) {
        assert!(
            Instant::now() < removed_deadline,
            "setup-stopped state did not retire after adoption completed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(fixture.attach_probe_stream(&fixture.request_run()));
}

// shutdown 同样等待 adoption gate 发布，避免 setup 中的 child 留下无 owner stream。
#[test]
fn D15_AdoptGate_ShutdownPinsState_021() {
    let fixture = Fixture::new("hold");
    fixture.supervisor.pause_adoption_setup_for_test();
    let (exited_while_paused, pinned_while_paused) = std::thread::scope(|scope| {
        let starting = scope.spawn(|| fixture.start());
        let setup_deadline = Instant::now() + Duration::from_secs(15);
        while !fixture.supervisor.adoption_setup_waiting_for_test()
            && Instant::now() < setup_deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        let receipt = fixture
            .service
            .registry()
            .status(&fixture.caller, &fixture.request.request_id)
            .unwrap();
        let shutting_down = scope.spawn(|| fixture.supervisor.shutdown());
        let exit_deadline = Instant::now() + Duration::from_secs(15);
        let exited_while_paused = loop {
            if fixture
                .supervisor
                .snapshot(&receipt.run)
                .is_some_and(|state| state.process() == ProcessLifecycle::Exited)
            {
                break true;
            }
            if Instant::now() >= exit_deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let pinned_while_paused = fixture.supervisor.active_for_test(&receipt.run)
            && fixture
                .service
                .registry()
                .status(&fixture.caller, &receipt.request_id)
                .is_ok();
        fixture.supervisor.release_adoption_setup_for_test();
        starting.join().unwrap();
        shutting_down.join().unwrap();
        (exited_while_paused, pinned_while_paused)
    });

    assert!(exited_while_paused, "shutdown setup child was not reaped");
    assert!(
        pinned_while_paused,
        "shutdown released active ownership before stream installation"
    );
    assert!(!fixture.supervisor.active_for_test(&fixture.request_run()));
    drop(fixture.attach_probe_stream(&fixture.request_run()));
}
