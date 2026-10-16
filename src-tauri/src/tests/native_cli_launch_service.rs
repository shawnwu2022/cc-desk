use crate::cli::environment::EnvMap;
use crate::cli::launch_service::{LaunchService, NativeRun, RunSupervisor};
use crate::cli::output_route::OutputRoutes;
use crate::cli::profiles::{error, Override, Profile};
use crate::cli::run_registry::{LaunchPhase, RunKey, RunRegistry};
use crate::cli::snapshot::CallerIdentity;
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, SafeError, WireU64};
use parking_lot::Mutex;
use portable_pty::PtySize;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

#[cfg(windows)]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

#[derive(Default)]
struct Consumer {
    calls: AtomicUsize,
    runs: Mutex<Vec<Arc<NativeRun>>>,
    fail: bool,
}
impl RunSupervisor for Consumer {
    fn adopt(
        &self,
        _registry: Arc<RunRegistry<NativeRun>>,
        _run: &RunKey,
        resource: Arc<NativeRun>,
    ) -> Result<(), SafeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.runs.lock().push(resource);
        if self.fail {
            Err(error("FIXTURE_ADOPTION_FAILED"))
        } else {
            Ok(())
        }
    }
}
struct Fixture {
    root: tempfile::TempDir,
    repository: WorkspaceRepository,
    service: Arc<LaunchService>,
    consumer: Arc<Consumer>,
    caller: CallerIdentity,
    request: LaunchRequest,
    routes: OutputRoutes,
}

fn metadata_fixture(
    hang: bool,
) -> (
    Fixture,
    Arc<LaunchService>,
    CallerIdentity,
    crate::cli::native_projection::registry::RenameAuthority,
    LaunchRequest,
) {
    use crate::cli::native_projection::{
        registry::{AdmittedProject, Grant, Owner, ScopeRegistry},
        scoped_fs::Root,
        wire::{ScopeTarget, SourceBasis},
    };
    let f = Fixture::new(false);
    let root = f.root.path().join("native-root");
    fs::create_dir_all(root.join("sessions")).unwrap();
    let path = root.join("sessions/rollout.jsonl");
    let id = "00000000-0000-4000-8000-000000000009";
    fs::write(&path,format!("{}\n{}\n",serde_json::json!({"type":"session_meta","payload":{"id":id,"cwd":f.request.launch_cwd,"source":"cli"}}),serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"fixture"}}))).unwrap();
    let mut env = EnvMap::new();
    env.insert("CODEX_HOME".into(), root.as_os_str().into());
    if let Some(value) = std::env::var_os("SystemRoot") {
        env.insert("SystemRoot".into(), value);
    }
    let service = Arc::new(LaunchService::new(f.repository.clone(), Some(env), None));
    let caller = service.registry().activate_window("main").unwrap();
    let owner = Owner {
        instance: caller.instance_id.clone(),
        window: caller.window_label.clone(),
        epoch: caller.webview_epoch,
    };
    let registry = ScopeRegistry::new(4);
    let source = registry
        .register(Grant {
            owner: owner.clone(),
            cli: CliKind::Codex,
            profile_id: f.request.profile_id.clone(),
            profile_revision: f.request.expected_profile_revision,
            target: ScopeTarget::Profile {
                profile_id: f.request.profile_id.clone(),
                expected_profile_revision: f.request.expected_profile_revision,
                project_ids: vec!["project".into()],
            },
            basis: SourceBasis::ConfiguredProfile,
            root: Root::open(&root).unwrap(),
            projects: vec![AdmittedProject {
                id: "project".into(),
                root: Root::open(Path::new(&f.request.launch_cwd)).unwrap(),
                paths: vec![f.request.launch_cwd.clone().into()],
            }],
            user_config: None,
            check: Arc::new(|| Ok(())),
        })
        .unwrap();
    let key = serde_json::to_string(&serde_json::json!([
        "local",
        "codex",
        source.source_root_key,
        id
    ]))
    .unwrap();
    let authority = registry.authorize_rename(&owner, &source, &key).unwrap();
    let mut request = f.request.clone();
    let thread = serde_json::json!({"id":id,"cwd":request.launch_cwd,"path":path});
    let script = format!(
        r#"const fs=require('fs');const thread={thread};let name=null;fs.writeFileSync('metadata-ready','ready');require('readline').createInterface({{input:process.stdin}}).on('line',line=>{{let q=JSON.parse(line);if(q.method==='initialized')return;if({hang})return;let result={{}};if(q.method==='thread/read')result={{thread:{{...thread,name}}}};else if(q.method==='thread/name/set')name=q.params.name;else if(q.method!=='initialize')process.exit(7);process.stdout.write(JSON.stringify({{id:q.id,result}})+'\n')}});"#
    );
    request.action = LaunchAction::Raw {
        argv: vec!["-e".into(), script],
    };
    (f, service, caller, authority, request)
}

#[test]
fn NativeRename_RealStdioFixtureReapsAndKeepsRollout_009() {
    let (f, service, caller, authority, request) = metadata_fixture(false);
    let rollout = f.root.path().join("native-root/sessions/rollout.jsonl");
    let before = fs::read(&rollout).unwrap();
    service
        .rename_metadata(&caller, &request, authority, "Native fixture title".into())
        .unwrap();
    service.wait_for_commands();
    assert_eq!(fs::read(&rollout).unwrap(), before);
    f.repository
        .admission()
        .freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

#[test]
fn NativeRename_ShutdownInterruptsReadAndRetainsReap_010() {
    let (f, service, caller, authority, request) = metadata_fixture(true);
    let worker_service = service.clone();
    let worker = std::thread::spawn(move || {
        worker_service.rename_metadata(&caller, &request, authority, "new".into())
    });
    let started = Instant::now();
    while !Path::new(&f.request.launch_cwd)
        .join("metadata-ready")
        .exists()
    {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    service.begin_shutdown();
    assert_eq!(
        worker.join().unwrap().unwrap_err().code,
        "RUN_SUPERVISOR_STOPPING"
    );
    service.wait_for_commands();
    assert!(started.elapsed() < Duration::from_secs(6));
    f.repository
        .admission()
        .freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

#[test]
fn NativeRename_OfficialClaudeSdkUsesOwnedHelperAndReaps_014() {
    use crate::cli::native_projection::{
        registry::{AdmittedProject, Grant, Owner, ScopeRegistry},
        scoped_fs::Root,
        wire::{ScopeTarget, SourceBasis},
    };
    let f = Fixture::new(false);
    let root = f.root.path().join("claude-native-root");
    fs::create_dir_all(root.join("projects/fixture")).unwrap();
    let id = "00000000-0000-4000-8000-000000000014";
    let transcript = root.join(format!("projects/fixture/{id}.jsonl"));
    let original = format!(
        "{}\n",
        serde_json::json!({"type":"user","sessionId":id,"cwd":f.request.launch_cwd,"message":{"content":"private fixture"}})
    );
    fs::write(&transcript, &original).unwrap();
    let mut profile = Profile::new("claude-metadata", CliKind::Claude);
    profile.program_path = Override::Set(crate::platform::find_executable("node").unwrap());
    let document = f.repository.read().unwrap();
    let document = f
        .repository
        .apply(
            document.revision,
            Patch::Create {
                profile: profile.clone(),
            },
        )
        .unwrap();
    let mut env: EnvMap = std::env::vars_os().collect();
    env.insert("CLAUDE_CONFIG_DIR".into(), root.as_os_str().into());
    let service = Arc::new(LaunchService::new(f.repository.clone(), Some(env), None));
    let caller = service.registry().activate_window("main").unwrap();
    let owner = Owner {
        instance: caller.instance_id.clone(),
        window: caller.window_label.clone(),
        epoch: caller.webview_epoch,
    };
    let scopes = ScopeRegistry::new(4);
    let source = scopes
        .register(Grant {
            owner: owner.clone(),
            cli: CliKind::Claude,
            profile_id: profile.id.clone(),
            profile_revision: document.profiles[&profile.id].revision,
            target: ScopeTarget::Profile {
                profile_id: profile.id.clone(),
                expected_profile_revision: document.profiles[&profile.id].revision,
                project_ids: vec!["project".into()],
            },
            basis: SourceBasis::ConfiguredProfile,
            root: Root::open(&root).unwrap(),
            projects: vec![AdmittedProject {
                id: "project".into(),
                root: Root::open(Path::new(&f.request.launch_cwd)).unwrap(),
                paths: vec![f.request.launch_cwd.clone().into()],
            }],
            user_config: None,
            check: Arc::new(|| Ok(())),
        })
        .unwrap();
    let key = serde_json::json!(["local", "claude", source.source_root_key, id]).to_string();
    let authority = scopes.authorize_rename(&owner, &source, &key).unwrap();
    let mut request = f.request.clone();
    request.profile_id = profile.id.clone();
    request.expected_profile_revision = document.profiles[&profile.id].revision;
    request.cli = CliKind::Claude;
    request.action = LaunchAction::Raw { argv: vec![] };
    service
        .rename_metadata(&caller, &request, authority, "Owned SDK title".into())
        .unwrap();
    service.wait_for_commands();
    let data = fs::read_to_string(&transcript).unwrap();
    assert!(data.starts_with(&original));
    let records: Vec<serde_json::Value> = data
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1]["customTitle"], "Owned SDK title");
    assert!(!fs::read_dir(&root).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".ccdesk-rename-")));
    f.repository
        .admission()
        .freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

impl Fixture {
    fn new(fail: bool) -> Self {
        #[cfg(windows)]
        bundled_runtime::initialize().unwrap();
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("work");
        fs::create_dir(&cwd).unwrap();
        let repository =
            WorkspaceRepository::open(root.path().join("metadata/workspace.json")).unwrap();
        let mut profile = Profile::new("native-service", CliKind::Codex);
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
            request_id: "service-request".into(),
            tab_id: "service-tab".into(),
            run_id: "service-run".into(),
            generation: 1,
            profile_id: profile.id.clone(),
            expected_profile_revision: document.profiles[&profile.id].revision,
            cli: CliKind::Codex,
            launch_cwd: cwd.to_str().unwrap().into(),
            action: LaunchAction::Raw {
                argv: vec![
                    script.to_str().unwrap().into(),
                    "hold".into(),
                    "中文".into(),
                    "".into(),
                ],
            },
            extra_args: vec![],
            cols: 80,
            rows: 24,
        };
        let mut inherited: EnvMap = std::env::vars_os().collect();
        inherited.insert("CC_DESK_TEST_ROOT".into(), cwd.as_os_str().to_owned());
        let consumer = Arc::new(Consumer {
            fail,
            ..Default::default()
        });
        let service = Arc::new(LaunchService::new(
            repository.clone(),
            Some(inherited),
            Some(consumer.clone()),
        ));
        let caller = service.registry().activate_window("main").unwrap();
        Self {
            root,
            repository,
            service,
            consumer,
            caller,
            request,
            routes: OutputRoutes::new(2),
        }
    }
    fn start(&self) -> Result<crate::cli::run_registry::LaunchStatus, SafeError> {
        self.service.start(&self.caller, &self.request, |_| {
            self.routes
                .bind(1, Box::new(|| Ok(())), || Ok(Channel::new(|_| Ok(()))))
        })
    }
    fn children(&self) -> usize {
        fs::read_dir(self.root.path().join("work"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|v| v == "json")
            })
            .count()
    }
    fn ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while self.children() == 0 {
            assert!(Instant::now() < deadline, "real child not ready");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn delete_profile(&self) {
        let document = self.repository.read().unwrap();
        self.repository
            .apply(
                document.revision,
                Patch::Delete {
                    id: self.request.profile_id.clone(),
                },
            )
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for run in self.consumer.runs.lock().iter() {
            let _ = run.process.pty.terminate_root();
            let _ = run.process.pty.wait();
        }
    }
}
fn size() -> PtySize {
    PtySize {
        rows: 30,
        cols: 100,
        pixel_width: 0,
        pixel_height: 0,
    }
}

// 正式服务并发一百次仍只创建一个实际 Node/PTY，不以计数 mock 替代。
#[test]
fn D11_Service_ConcurrentRealChildAndLostReceipt_001() {
    let f = Fixture::new(false);
    let barrier = Arc::new(Barrier::new(100));
    std::thread::scope(|scope| {
        for _ in 0..100 {
            let barrier = barrier.clone();
            let f = &f;
            scope.spawn(move || {
                barrier.wait();
                f.start().unwrap();
            });
        }
    });
    f.ready();
    assert_eq!(f.children(), 1);
    assert_eq!(f.consumer.calls.load(Ordering::SeqCst), 1);
    let status = f
        .service
        .registry()
        .status(&f.caller, &f.request.request_id)
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    let retained = f
        .service
        .start(&f.caller, &f.request, |_| panic!("replayed route"))
        .unwrap();
    assert_eq!(status, retained);
    assert_eq!(f.children(), 1);
}

// 原配置删除后查询与重放仍使用原回执；运行快照保持不变且仅后端可读。
#[test]
fn D11_Service_ProfileDeletionDoesNotMutateRun_002() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    let before = access.snapshot().unwrap();
    f.delete_profile();
    assert_eq!(
        f.service
            .start(&f.caller, &f.request, |_| panic!("replay"))
            .unwrap(),
        status
    );
    assert_eq!(access.snapshot().unwrap().request(), before.request());
    assert_eq!(
        access.snapshot().unwrap().environment(),
        before.environment()
    );
    assert!(!format!("{access:?}{before:?}").contains("中文"));
}

// 已取得的访问对象不是永久能力：撤权后 input/resize/stop/read 均再次拒绝。
#[test]
fn D11_Service_AccessRevalidatesEveryOperation_003() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    access.resize(size()).unwrap();
    f.service.registry().revoke_window(&f.caller).unwrap();
    assert_eq!(
        access
            .with_writer::<()>(|_| panic!("revoked writer"))
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    assert_eq!(access.resize(size()).unwrap_err().code, "FORBIDDEN");
    assert_eq!(access.terminate_root().unwrap_err().code, "FORBIDDEN");
    assert_eq!(access.snapshot().unwrap_err().code, "FORBIDDEN");
    assert!(f.consumer.runs.lock()[0]
        .process
        .pty
        .try_wait()
        .unwrap()
        .is_none());
}

// 错 instance、窗口或 generation 不能因持有 run UUID 而取得句柄。
#[test]
fn D11_Service_ForeignAndStaleRunRejected_004() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    for field in ["instance", "window", "epoch"] {
        let mut other = f.caller.clone();
        match field {
            "instance" => other.instance_id = "foreign".into(),
            "window" => other.window_label = "peer".into(),
            _ => other.webview_epoch = WireU64::parse("0").unwrap(),
        }
        assert_eq!(
            f.service.access(&other, &status.run).unwrap_err().code,
            "FORBIDDEN"
        );
    }
    let mut stale = status.run;
    stale.generation += 1;
    assert_eq!(
        f.service.access(&f.caller, &stale).unwrap_err().code,
        "STALE_GENERATION"
    );
}

// 未安装后续阶段的后端消费方时，正式服务必须在任何配置 I/O/启动前拒绝。
#[test]
fn D11_Service_MissingConsumerFailsBeforeIo_005() {
    let f = Fixture::new(false);
    let path = f.root.path().join("not-created/workspace.json");
    let service = LaunchService::new(WorkspaceRepository::open(path.clone()).unwrap(), None, None);
    let caller = service.registry().activate_window("main").unwrap();
    assert_eq!(
        service
            .start(&caller, &f.request, |_| panic!("unready route"))
            .unwrap_err()
            .code,
        "NATIVE_RUNTIME_NOT_READY"
    );
    assert!(!path.parent().unwrap().exists());
    assert_eq!(f.children(), 0);
}

// 输出路由失败不启动；即使随后配置被删除，失败结果也不能被重放覆盖。
#[test]
fn D11_Service_RouteFailureIsRetained_006() {
    let f = Fixture::new(false);
    let status = f
        .service
        .start(&f.caller, &f.request, |_| {
            Err(error("FIXTURE_ROUTE_FAILED"))
        })
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Failed);
    f.delete_profile();
    assert_eq!(
        f.service
            .start(&f.caller, &f.request, |_| panic!("replay"))
            .unwrap(),
        status
    );
    assert_eq!(f.children(), 0);
}

// 后端消费方接管失败也不得丢失已创建进程或再次 spawn。
#[test]
fn D11_Service_HandoffFailureKeepsOwnedProcess_007() {
    let f = Fixture::new(true);
    assert_eq!(f.start().unwrap_err().code, "RUN_HANDOFF_FAILED");
    f.ready();
    let status = f
        .service
        .registry()
        .status(&f.caller, &f.request.request_id)
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    assert!(f.service.access(&f.caller, &status.run).is_ok());
    assert_eq!(
        f.service
            .start(&f.caller, &f.request, |_| panic!("replay"))
            .unwrap(),
        status
    );
    assert_eq!(f.consumer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.children(), 1);
}

#[path = "native_cli_launch_edges.rs"]
mod edges;

#[path = "native_cli_observer_service.rs"]
mod observer;

#[test]
fn D15_Service_ShutdownGateRejectsNewLaunchButKeepsReceipt_012() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();

    f.service.begin_shutdown();

    let replay = f
        .service
        .start(&f.caller, &f.request, |_| {
            panic!("replay must not rebuild route")
        })
        .unwrap();
    assert_eq!(replay, status);

    let mut next = f.request.clone();
    next.request_id = "service-after-shutdown".into();
    next.tab_id = "service-after-shutdown-tab".into();
    next.run_id = "service-after-shutdown-run".into();
    assert_eq!(
        f.service
            .start(&f.caller, &next, |_| panic!(
                "shutdown launch must not bind route"
            ))
            .unwrap_err()
            .code,
        "RUN_SUPERVISOR_STOPPING"
    );
    assert_eq!(f.children(), 1, "shutdown gate created another child");
    assert_eq!(f.consumer.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn HistoryRuntime_NativePendingAndDuplicateAdmissions_01() {
    let f = Fixture::new(false);
    let gate = f.repository.admission().clone();
    let pending = f.service.admit_start().unwrap();
    assert!(gate.freeze(&uuid::Uuid::new_v4().to_string()).is_err());
    drop(pending);
    let frozen = gate.freeze(&uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(f.start().unwrap_err().code, "HISTORY_MAINTENANCE_ACTIVE");
    assert_eq!(f.children(), 0);
    frozen.release_review().unwrap();
    let status = f.start().unwrap();
    f.ready();
    assert_eq!(f.start().unwrap(), status);
    assert_eq!(f.children(), 1);
    let resource = f.consumer.runs.lock()[0].clone();
    resource.process.pty.terminate_root().unwrap();
    resource.process.pty.wait().unwrap();
    let frozen = gate.freeze(&uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(f.start().unwrap(), status); // Existing receipt wins even under freeze.
    frozen.release_review().unwrap();
}

#[test]
fn HistoryRuntime_NativeAdoptionFailureKeepsExactChildTicket_02() {
    let f = Fixture::new(true);
    assert_eq!(f.start().unwrap_err().code, "RUN_HANDOFF_FAILED");
    f.ready();
    let gate = f.repository.admission();
    assert!(gate.freeze(&uuid::Uuid::new_v4().to_string()).is_err());
    let resource = f.consumer.runs.lock()[0].clone();
    resource.process.pty.terminate_root().unwrap();
    resource.process.pty.wait().unwrap();
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

#[test]
fn HistoryRuntime_NativeBlockingLaunchSurvivesLostReceiver_03() {
    let f = Fixture::new(false);
    let gate = f.repository.admission().clone();
    let admission = f.service.admit_start().unwrap();
    let service = f.service.clone();
    let caller = f.caller.clone();
    let request = f.request.clone();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let ready = entered.clone();
    let resume = release.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let routes = OutputRoutes::new(2);
        service.start_admitted(admission, &caller, &request, |_| {
            ready.wait();
            resume.wait();
            routes.bind(1, Box::new(|| Ok(())), || Ok(Channel::new(|_| Ok(()))))
        })
    });
    entered.wait();
    assert!(gate.freeze(&uuid::Uuid::new_v4().to_string()).is_err());
    drop(worker);
    release.wait();
    f.ready();
    assert!(gate.freeze(&uuid::Uuid::new_v4().to_string()).is_err());
    let deadline = Instant::now() + Duration::from_secs(5);
    let resource = loop {
        if let Some(run) = f.consumer.runs.lock().first().cloned() {
            break run;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    resource.process.pty.terminate_root().unwrap();
    resource.process.pty.wait().unwrap();
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

// 已撤销的 document 不能借一次性命令绕过 launch 的 epoch 授权。
#[test]
fn NativeCommand_RevokedDocumentCreatesNoChild_001() {
    let mut f = Fixture::new(false);
    if let LaunchAction::Raw { argv } = &mut f.request.action {
        argv[1] = "exit".into();
    }
    f.service.registry().revoke_window(&f.caller).unwrap();
    let failure = f.service.run_command(&f.caller, &f.request).unwrap_err();
    assert_eq!(failure.code, "FORBIDDEN");
    assert_eq!(f.children(), 0, "revoked command created a real child");
    f.repository
        .admission()
        .freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}
// 正在运行的一次性 child 也必须响应 shutdown，回执不能等待整个命令截止。
#[test]
fn NativeCommand_ShutdownReapsExactChild_002() {
    let f = Fixture::new(false);
    let service = f.service.clone();
    let caller = f.caller.clone();
    let request = f.request.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = sender.send(service.run_command(&caller, &request));
    });
    f.ready();
    assert!(f
        .repository
        .admission()
        .freeze(&uuid::Uuid::new_v4().to_string())
        .is_err());
    f.service.begin_shutdown();
    let response = receiver.recv_timeout(Duration::from_secs(3));
    // RED 时旧实现会在 30 秒才返回；仍先回收测试线程，避免把失败留成后台 child。
    worker.join().unwrap();
    let failure = response
        .expect("shutdown command reply must be bounded")
        .unwrap_err();
    assert_eq!(failure.code, "RUN_SUPERVISOR_STOPPING");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(frozen) = f
            .repository
            .admission()
            .freeze(&uuid::Uuid::new_v4().to_string())
        {
            frozen.release_review().unwrap();
            break;
        }
        assert!(
            Instant::now() < deadline,
            "command retained an unreaped child ticket"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(f.children(), 1, "cleanup created a replacement child");
}

// 使用真实 hold child 验证命令截止返回与真实 reap，而非把错误回执当作退出证明。
#[test]
fn NativeCommand_TimeoutReapsExactChild_003() {
    let f = Fixture::new(false);
    let started = Instant::now();
    let failure = f.service.run_command(&f.caller, &f.request).unwrap_err();
    assert_eq!(failure.code, "COMMAND_TIMEOUT");
    assert!(started.elapsed() < Duration::from_secs(40));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(frozen) = f
            .repository
            .admission()
            .freeze(&uuid::Uuid::new_v4().to_string())
        {
            frozen.release_review().unwrap();
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timeout lost the exact child reaper"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(f.children(), 1, "timeout retried process creation");
}

// 实际 NativeRuntime 退出入口必须等待一次性 command 的真实 reap，不能仅发 flag。
#[test]
fn NativeCommand_RuntimeShutdownWaitsForReap_004() {
    let f = Fixture::new(false);
    let runtime = crate::cli::native_runtime::NativeRuntime::new(f.service.clone());
    let service = f.service.clone();
    let caller = f.caller.clone();
    let request = f.request.clone();
    let worker = std::thread::spawn(move || service.run_command(&caller, &request));
    f.ready();
    let started = Instant::now();
    runtime.shutdown();
    let settled = f
        .repository
        .admission()
        .freeze(&uuid::Uuid::new_v4().to_string());
    // RED 时仍等待后台线程自行收口，以免测试失败遗留活进程。
    let failure = worker.join().unwrap().unwrap_err();
    assert_eq!(failure.code, "RUN_SUPERVISOR_STOPPING");
    assert!(started.elapsed() < Duration::from_secs(6));
    settled
        .expect("runtime shutdown returned before command reap")
        .release_review()
        .unwrap();
    assert_eq!(f.children(), 1);
}
