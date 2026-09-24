#[cfg(windows)]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;
use crate::cli::environment::EnvMap;
use crate::cli::launch_service::LaunchService;
use crate::cli::native_projection::service::ProjectionService;
use crate::cli::native_projection::wire::*;
use crate::cli::profiles::{EnvValue, Override, Profile};
use crate::cli::snapshot::CallerIdentity;
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, WireU64};
use std::{fs, path::Path, sync::Arc};
fn number(n: u64) -> WireU64 {
    WireU64::parse(&n.to_string()).unwrap()
}
struct Fixture {
    _temp: tempfile::TempDir,
    repo: WorkspaceRepository,
    launch: Arc<LaunchService>,
    service: ProjectionService,
    caller: CallerIdentity,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = WorkspaceRepository::open(temp.path().join("metadata/workspace.json")).unwrap();
        let launch = Arc::new(LaunchService::new(repo.clone(), Some(EnvMap::new()), None));
        let caller = launch.registry().activate_window("main").unwrap();
        Self {
            service: ProjectionService::new(launch.clone()),
            _temp: temp,
            repo,
            launch,
            caller,
        }
    }
    fn profile(&self, id: &str, root: &Path) -> ScopeTarget {
        fs::create_dir_all(root.join("projects/encoded")).unwrap();
        fs::write(
            root.join("projects/encoded/same.jsonl"),
            format!("{{\"type\":\"custom-title\",\"customTitle\":\"{id}\"}}\n"),
        )
        .unwrap();
        let mut p = Profile::new(id, CliKind::Claude);
        p.env.insert(
            "CLAUDE_CONFIG_DIR".into(),
            Override::Set(EnvValue::Literal {
                value: root.to_str().unwrap().into(),
                non_secret: true,
            }),
        );
        let d = self
            .repo
            .apply(
                self.repo.read().unwrap().revision,
                Patch::Create { profile: p },
            )
            .unwrap();
        ScopeTarget::Profile {
            profile_id: id.into(),
            expected_profile_revision: d.profiles[id].revision,
            project_id: None,
        }
    }
}
fn req(source: SourceRef) -> ReadRequest {
    ReadRequest {
        source,
        resource_kind: ResourceKind::History,
        request_epoch: number(1),
        query: None,
        session_id: None,
        limit: 100,
        offset: 0,
    }
}
#[test]
fn D12_Production_ProfilesActuallyReadIndependentRoots_001() {
    let f = Fixture::new();
    for id in ["left", "right"] {
        let t = f.profile(id, &f._temp.path().join(id));
        let r = f.service.scope(&f.caller, &t).unwrap();
        let data = f.service.read(&f.caller, &req(r)).unwrap();
        assert_eq!(data.state, ProjectionState::Ready);
        assert!(matches!(&data.items[0],ResourceItem::Session{title,..} if title==id));
    }
}
#[test]
fn D12_Production_ProfileRevisionRevokesScope_002() {
    let f = Fixture::new();
    let t = f.profile("p", &f._temp.path().join("root"));
    let source = f.service.scope(&f.caller, &t).unwrap();
    let doc = f.repo.read().unwrap();
    f.repo
        .apply(
            doc.revision,
            Patch::Update {
                id: "p".into(),
                changes: serde_json::json!({"name":"updated"})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        )
        .unwrap();
    assert_eq!(
        f.service.read(&f.caller, &req(source)).unwrap_err().code,
        "SCOPE_REVOKED"
    );
    assert_eq!(
        f.service.scope(&f.caller, &t).unwrap_err().code,
        "REVISION_CONFLICT"
    );
}
#[test]
fn D12_Production_RevokedDocumentFailsBeforeDamagedWorkspace_003() {
    let f = Fixture::new();
    let t = f.profile("p", &f._temp.path().join("root"));
    let source = f.service.scope(&f.caller, &t).unwrap();
    f.launch.registry().revoke_window(&f.caller).unwrap();
    fs::write(
        f._temp.path().join("metadata/workspace.json"),
        "SECRET corrupt",
    )
    .unwrap();
    assert_eq!(
        f.service.scope(&f.caller, &t).unwrap_err().code,
        "FORBIDDEN"
    );
    assert_eq!(
        f.service.read(&f.caller, &req(source)).unwrap_err().code,
        "FORBIDDEN"
    );
}
#[test]
fn D12_Production_ExplicitProjectNotTranscriptCwdControlsResourceAccess_004() {
    let f = Fixture::new();
    let mut t = f.profile("p", &f._temp.path().join("root"));
    let work = f._temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("CLAUDE.md"), "registered instruction").unwrap();
    let registered = crate::cli::workspace::register_project(&f.repo, &work).unwrap();
    if let ScopeTarget::Profile { project_id, .. } = &mut t {
        *project_id = Some(registered.project_id.clone());
    }
    let source = f.service.scope(&f.caller, &t).unwrap();
    let mut r = req(source);
    r.resource_kind = ResourceKind::Instructions;
    let data = f.service.read(&f.caller, &r).unwrap();
    assert!(
        matches!(&data.items[0],ResourceItem::Document{text,..} if text=="registered instruction")
    );
    crate::cli::workspace::remove_project(
        &f.repo,
        f.repo.read().unwrap().revision,
        &registered.project_id,
    )
    .unwrap();
    assert_eq!(
        f.service.read(&f.caller, &r).unwrap_err().code,
        "SCOPE_REVOKED"
    );
    assert_eq!(
        fs::read_to_string(work.join("CLAUDE.md")).unwrap(),
        "registered instruction"
    );
}
#[test]
fn D12_Production_UnknownRunAndUnsafeProfileNeverFallBack_005() {
    let f = Fixture::new();
    assert!(f
        .service
        .scope(
            &f.caller,
            &ScopeTarget::Run {
                run_id: "missing".into(),
                generation: 1
            }
        )
        .is_err());
    let t = f.profile("p", &f._temp.path().join("root"));
    let doc = f.repo.read().unwrap();
    let updated = f
        .repo
        .apply(
            doc.revision,
            Patch::Update {
                id: "p".into(),
                changes:
                    serde_json::json!({"defaultArgs":{"mode":"set","value":["--config","other"]}})
                        .as_object()
                        .unwrap()
                        .clone(),
            },
        )
        .unwrap();
    let ScopeTarget::Profile {
        profile_id,
        project_id,
        ..
    } = t
    else {
        unreachable!()
    };
    let t = ScopeTarget::Profile {
        profile_id,
        project_id,
        expected_profile_revision: updated.profiles["p"].revision,
    };
    assert_eq!(
        f.service.scope(&f.caller, &t).unwrap_err().code,
        "SCOPE_UNKNOWN"
    );
}

// Actual process-backed RunAccess proves that scopes use the frozen run, not today's profile.
#[test]
fn D12_Production_RunSnapshotSurvivesProfileDeleteButNotDocumentRevoke_006() {
    use crate::cli::launch_service::{NativeRun, RunSupervisor};
    use crate::cli::output_route::OutputRoutes;
    use crate::cli::run_registry::{RunKey, RunRegistry};
    use crate::cli::types::{LaunchAction, LaunchRequest, SafeError};
    use parking_lot::Mutex;
    #[cfg(windows)]
    bundled_runtime::initialize().unwrap();
    #[derive(Default)]
    struct Consumer(Mutex<Vec<Arc<NativeRun>>>);
    impl RunSupervisor for Consumer {
        fn adopt(\n            &self,\n            _registry: Arc<RunRegistry<NativeRun>>,\n            _run: &RunKey,\n            r: Arc<NativeRun>,\n        ) -> Result<(), SafeError> {
            self.0.lock().push(r);
            Ok(())
        }
    }
    impl Drop for Consumer {
        fn drop(&mut self) {
            for r in self.0.get_mut().iter() {
                let _ = r.process.pty.terminate_root();
                let _ = r.process.pty.wait();
            }
        }
    }
    let t = tempfile::tempdir().unwrap();
    let repo = WorkspaceRepository::open(t.path().join("meta/workspace.json")).unwrap();
    let native = t.path().join("native");
    fs::create_dir_all(&native).unwrap();
    fs::write(native.join("config.toml"), "model='frozen'\n").unwrap();
    let mut p = Profile::new("run-profile", CliKind::Codex);
    p.program_path = Override::Set(crate::platform::find_executable("node").unwrap());
    p.env.insert(
        "CODEX_HOME".into(),
        Override::Set(EnvValue::Literal {
            value: native.to_str().unwrap().into(),
            non_secret: true,
        }),
    );
    let d = repo.apply(number(0), Patch::Create { profile: p }).unwrap();
    let consumer = Arc::new(Consumer::default());
    let launch = Arc::new(LaunchService::new(
        repo.clone(),
        Some(std::env::vars_os().collect()),
        Some(consumer.clone()),
    ));
    let caller = launch.registry().activate_window("main").unwrap();
    let projection = ProjectionService::new(launch.clone());
    let routes = OutputRoutes::new(2);
    let request = LaunchRequest {
        request_id: "run-request".into(),
        tab_id: "tab".into(),
        run_id: "run".into(),
        generation: 1,
        profile_id: "run-profile".into(),
        expected_profile_revision: d.profiles["run-profile"].revision,
        cli: CliKind::Codex,
        launch_cwd: t.path().to_str().unwrap().into(),
        action: LaunchAction::New,
        extra_args: vec![],
        cols: 80,
        rows: 24,
    };
    let status = launch
        .start(&caller, &request, |_| {
            routes.bind(1, Box::new(|| Ok(())), || {
                Ok(tauri::ipc::Channel::new(|_| Ok(())))
            })
        })
        .unwrap();
    let target = ScopeTarget::Run {
        run_id: status.run.run_id,
        generation: status.run.generation,
    };
    let reference = projection.scope(&caller, &target).unwrap();
    assert_eq!(reference.basis, SourceBasis::LaunchEnvironment);
    repo.apply(
        repo.read().unwrap().revision,
        Patch::Delete {
            id: "run-profile".into(),
        },
    )
    .unwrap();
    let mut query = req(reference);
    query.resource_kind = ResourceKind::Config;
    let data = projection.read(&caller, &query).unwrap();
    assert!(matches!(&data.items[0],ResourceItem::Setting{value,..} if value=="frozen"));
    let stale = ScopeTarget::Run {
        run_id: "run".into(),
        generation: 2,
    };
    assert!(projection.scope(&caller, &stale).is_err());
    launch.registry().revoke_window(&caller).unwrap();
    assert_eq!(
        projection.read(&caller, &query).unwrap_err().code,
        "FORBIDDEN"
    );
    // The supervisor's explicit cleanup owns the process; the projection has no stop primitive.
    for r in consumer.0.lock().drain(..) {
        let _ = r.process.pty.terminate_root();
        r.process.pty.wait().unwrap();
    }
}
