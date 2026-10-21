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
    repo: WorkspaceRepository,
    launch: Arc<LaunchService>,
    service: ProjectionService,
    caller: CallerIdentity,
    // Windows 目录句柄先随 service 释放，最后再清理临时树。
    _temp: tempfile::TempDir,
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
            project_ids: vec![],
        }
    }
}

#[test]
#[allow(non_snake_case)]
fn Opencode_ProfileScopeBindsExactDatabaseFilename_025() {
    let f = Fixture::new();
    let project_path = f._temp.path().join("project");
    fs::create_dir(&project_path).unwrap();
    let project = crate::cli::workspace::register_project(&f.repo, &project_path).unwrap();
    let data = f._temp.path().join("data");
    fs::create_dir(&data).unwrap();
    let mut keys = Vec::new();
    for (id, filename) in [("one", "one.db"), ("two", "two.db")] {
        let mut profile = Profile::new(id, CliKind::Opencode);
        profile.env.insert(
            "XDG_DATA_HOME".into(),
            Override::Set(EnvValue::Literal {
                value: data.to_string_lossy().into(),
                non_secret: true,
            }),
        );
        profile.env.insert(
            "OPENCODE_DB".into(),
            Override::Set(EnvValue::Literal {
                value: data.join(filename).to_string_lossy().into(),
                non_secret: true,
            }),
        );
        let document = f
            .repo
            .apply(f.repo.read().unwrap().revision, Patch::Create { profile })
            .unwrap();
        let source = f
            .service
            .scope(
                &f.caller,
                &ScopeTarget::Profile {
                    profile_id: id.into(),
                    expected_profile_revision: document.profiles[id].revision,
                    project_ids: vec![project.project_id.clone()],
                },
            )
            .unwrap();
        keys.push(source.source_root_key);
    }
    assert_ne!(keys[0], keys[1]);
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
#[allow(non_snake_case)]
fn NativeRename_RevokedDocumentReclaimsCompletedBudget_017() {
    use crate::cli::native_rename::RenameRequest;
    let f = Fixture::new();
    let target = f.profile("rename-budget", &f._temp.path().join("root"));
    let mut source = f.service.scope(&f.caller, &target).unwrap();
    source.scope_id = "nonexistent-fixture-scope".into();
    let mut request = RenameRequest {
        operation_id: String::new(),
        source,
        session_key: "fixture-key".into(),
        title: "fixture-title".into(),
    };
    // 仅制造已确定的准入失败，不启动 CLI 或触及真实会话。
    for index in 0..4096 {
        request.operation_id = format!("attempt-{index}");
        assert_eq!(
            f.service
                .rename(&f.caller, request.clone())
                .err()
                .unwrap()
                .code,
            "SCOPE_UNKNOWN"
        );
    }
    request.operation_id = "next-attempt".into();
    assert_eq!(
        f.service
            .rename(&f.caller, request.clone())
            .err()
            .unwrap()
            .code,
        "SOURCE_BUSY"
    );
    f.launch.registry().revoke_window(&f.caller).unwrap();
    let replacement = f.launch.registry().activate_window("main").unwrap();
    assert_eq!(
        f.service
            .rename(&f.caller, request.clone())
            .err()
            .unwrap()
            .code,
        "FORBIDDEN"
    );
    assert_eq!(
        f.service.rename(&replacement, request).err().unwrap().code,
        "SCOPE_UNKNOWN"
    );
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

// 64 项批量历史观察不会按项目数重复读取工作区，且末项移除仍撤销整组授权。
#[test]
#[allow(non_snake_case)]
fn ProjectionBatch_WorkspaceReadsRemainBounded_008() {
    let f = Fixture::new();
    let mut target = f.profile("batch", &f._temp.path().join("root"));
    let mut projects = Vec::new();
    for index in 0..64 {
        let path = f._temp.path().join(format!("project-{index}"));
        fs::create_dir(&path).unwrap();
        projects.push(crate::cli::workspace::register_project(&f.repo, &path).unwrap());
    }
    let ScopeTarget::Profile { project_ids, .. } = &mut target else {
        unreachable!()
    };
    project_ids.push(projects[0].project_id.clone());
    let before = f.repo.read_count_for_test();
    let single = f.service.scope(&f.caller, &target).unwrap();
    let single_scope_reads = f.repo.read_count_for_test() - before;
    let before = f.repo.read_count_for_test();
    let single_data = f.service.read(&f.caller, &req(single)).unwrap();
    let single_read_count = f.repo.read_count_for_test() - before;
    assert_eq!(single_data.state, ProjectionState::Ready);
    assert!(single_read_count > 0);

    let ScopeTarget::Profile { project_ids, .. } = &mut target else {
        unreachable!()
    };
    *project_ids = projects.iter().map(|p| p.project_id.clone()).collect();
    let before = f.repo.read_count_for_test();
    let batch = f.service.scope(&f.caller, &target).unwrap();
    let batch_scope_reads = f.repo.read_count_for_test() - before;
    assert!(
        batch_scope_reads <= single_scope_reads + 1,
        "scope workspace reads grew from {single_scope_reads} to {batch_scope_reads}"
    );
    let request = req(batch);
    let before = f.repo.read_count_for_test();
    let batch_data = f.service.read(&f.caller, &request).unwrap();
    let batch_read_count = f.repo.read_count_for_test() - before;
    assert_eq!(batch_data.state, ProjectionState::Ready);
    assert_eq!(batch_data.items.len(), single_data.items.len());
    assert_eq!(
        batch_read_count, single_read_count,
        "history workspace reads must not grow with the project count"
    );

    crate::cli::workspace::remove_project(
        &f.repo,
        f.repo.read().unwrap().revision,
        &projects[63].project_id,
    )
    .unwrap();
    assert_eq!(
        f.service.read(&f.caller, &request).unwrap_err().code,
        "SCOPE_REVOKED"
    );
}

// Cmd npm launcher 的只读 Codex 来源使用精确 CODEX_HOME，不启动 CLI。
#[test]
#[allow(non_snake_case)]
fn ScopeShim_ReadCodexRoot_007() {
    use crate::cli::profiles::{Dialect, Launcher};
    let f = Fixture::new();
    let root = f._temp.path().join("codex");
    fs::create_dir_all(root.join("sessions/2026/10/10")).unwrap();
    fs::write(
        root.join("sessions/2026/10/10/rollout.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"fixture-codex\"}}\n",
    )
    .unwrap();
    let mut p = Profile::new("codex-shim", CliKind::Codex);
    p.launcher = Launcher::Shim {
        runner: "cmd.exe".into(),
        dialect: Dialect::Cmd,
    };
    p.default_args = Override::Set(vec!["--no-alt-screen".into()]);
    p.env.insert(
        "CODEX_HOME".into(),
        Override::Set(EnvValue::Literal {
            value: root.to_str().unwrap().into(),
            non_secret: true,
        }),
    );
    let d = f
        .repo
        .apply(number(0), Patch::Create { profile: p })
        .unwrap();
    let target = ScopeTarget::Profile {
        profile_id: "codex-shim".into(),
        expected_profile_revision: d.profiles["codex-shim"].revision,
        project_ids: vec![],
    };
    let source = f.service.scope(&f.caller, &target).unwrap();
    let data = f.service.read(&f.caller, &req(source)).unwrap();
    assert!(
        matches!(&data.items[0], ResourceItem::Session { native_session_id, .. } if native_session_id == "fixture-codex")
    );
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
    if let ScopeTarget::Profile { project_ids, .. } = &mut t {
        *project_ids = vec![registered.project_id.clone()];
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
        project_ids,
        ..
    } = t
    else {
        unreachable!()
    };
    let t = ScopeTarget::Profile {
        profile_id,
        project_ids,
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
        fn adopt(
            &self,
            _registry: Arc<RunRegistry<NativeRun>>,
            _run: &RunKey,
            r: Arc<NativeRun>,
        ) -> Result<(), SafeError> {
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
    // 先证明真实 child 已 reap，再退役 PTY；只读来源证明不能依赖资源仍被 registry 持有。
    let run = RunKey {
        run_id: request.run_id.clone(),
        generation: request.generation,
    };
    let former_access = launch.access(&caller, &run).unwrap();
    for resource in consumer.0.lock().drain(..) {
        resource.process.pty.terminate_root().unwrap();
        resource.process.pty.wait().unwrap();
    }
    launch.registry().mark_exited(&run).unwrap();
    launch.registry().retire(&run).unwrap();
    assert_eq!(former_access.snapshot().unwrap_err().code, "RUN_NOT_READY");
    assert_eq!(
        launch.registry().resource(&caller, &run).unwrap_err().code,
        "RUN_NOT_READY"
    );
    drop(former_access);
    let exited = projection.scope(&caller, &target).unwrap();
    assert_eq!(exited, query.source);
    let after_retirement = projection.read(&caller, &query).unwrap();
    assert!(
        matches!(&after_retirement.items[0],ResourceItem::Setting{value,..} if value=="frozen")
    );
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
    assert_eq!(
        projection.scope(&caller, &target).unwrap_err().code,
        "FORBIDDEN"
    );
    let replacement = launch.registry().activate_window("main").unwrap();
    assert_eq!(
        projection.scope(&caller, &target).unwrap_err().code,
        "FORBIDDEN"
    );
    assert_eq!(
        projection.scope(&replacement, &target).unwrap_err().code,
        "FORBIDDEN"
    );
    // The supervisor's explicit cleanup owns the process; the projection has no stop primitive.
    for r in consumer.0.lock().drain(..) {
        let _ = r.process.pty.terminate_root();
        r.process.pty.wait().unwrap();
    }
}

// opencode 无 rename 动词：必须在 scope 准入与任何 spawn 之前拒绝（spec §2）。
// scope_id 故意编造：拿到 NATIVE_RENAME_UNSUPPORTED 而非 SCOPE_UNKNOWN，
// 证明拒绝发生在 authorize_rename 之前，而非准入失败的附带结果。
#[test]
#[allow(non_snake_case)]
fn Opencode_RenameRejectedBeforeScopeAdmission_01() {
    use crate::cli::native_projection::wire::{ScopeTarget, SourceBasis, SourceRef};
    use crate::cli::types::{CliKind, WireU64};
    let f = Fixture::new();
    let source = SourceRef {
        scope_id: "fixture-scope".into(),
        instance_id: "fixture-instance".into(),
        cli: CliKind::Opencode,
        source_root_key: "fixture-root-key".into(),
        identity_epoch: WireU64::parse("1").unwrap(),
        profile_id: "fixture-profile".into(),
        profile_revision: WireU64::parse("1").unwrap(),
        target: ScopeTarget::Profile {
            profile_id: "fixture-profile".into(),
            expected_profile_revision: WireU64::parse("1").unwrap(),
            project_ids: vec![],
        },
        basis: SourceBasis::ConfiguredProfile,
    };
    let request = crate::cli::native_rename::RenameRequest {
        operation_id: "opencode-rename".into(),
        source,
        session_key: "fixture-key".into(),
        title: "fixture-title".into(),
    };
    assert_eq!(
        f.service.rename(&f.caller, request).err().unwrap().code,
        "NATIVE_RENAME_UNSUPPORTED"
    );
}
