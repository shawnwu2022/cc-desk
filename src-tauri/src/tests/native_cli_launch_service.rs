use crate::cli::environment::EnvMap;
use crate::cli::launch_service::{LaunchService, NativeRun, RunSupervisor};
use crate::cli::output_route::OutputRoutes;
use crate::cli::profiles::{error, Override, Profile};
use crate::cli::run_registry::{LaunchPhase, RunKey};
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
    fn adopt(&self, _run: &RunKey, resource: Arc<NativeRun>) -> Result<(), SafeError> {
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
