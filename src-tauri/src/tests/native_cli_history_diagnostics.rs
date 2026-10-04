//! Synthetic-only reproduction matrix for the two providers' shared history path.
use crate::cli::environment::EnvMap;
use crate::cli::launch_service::LaunchService;
use crate::cli::native_projection::service::ProjectionService;
use crate::cli::native_projection::wire::{ProjectionState, ReadRequest, ScopeTarget};
use crate::cli::profiles::Profile;
use crate::cli::storage::{decode_workspace, Patch, WorkspaceRepository};
use serde_json::json;
use std::{fs, sync::Arc};

fn history_with_environment(extra: EnvMap) -> Vec<Result<(), crate::cli::types::SafeError>> {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".claude/projects")).unwrap();
    fs::create_dir_all(temp.path().join(".codex/sessions")).unwrap();
    let repo = WorkspaceRepository::open(temp.path().join("desk/workspace.json")).unwrap();
    let project_path = temp.path().join("Synthetic project");
    fs::create_dir(&project_path).unwrap();
    let project = crate::cli::workspace::register_project(&repo, &project_path).unwrap();
    let mut inherited = EnvMap::new();
    inherited.insert("USERPROFILE".into(), temp.path().as_os_str().into());
    inherited.insert("HOME".into(), temp.path().as_os_str().into());
    inherited.extend(extra);
    let launch = Arc::new(LaunchService::new(repo.clone(), Some(inherited), None));
    let caller = launch.registry().activate_window("main").unwrap();
    let service = ProjectionService::new(launch);
    let mut results = vec![];
    for (cli, id) in [("claude", "legacyClaude"), ("codex", "desk-safe-codex")] {
        // Old/minimal profiles omit every optional default; revisions remain wire strings.
        let profile: Profile = serde_json::from_value(json!({
            "id": id, "revision": "0", "cli": cli, "name": "Synthetic profile"
        }))
        .unwrap();
        let document = repo
            .apply(repo.read().unwrap().revision, Patch::Create { profile })
            .unwrap();
        for project in [
            None,
            Some(serde_json::Value::Null),
            Some(json!(project.project_id)),
        ] {
            let mut target = json!({"kind":"profile", "profileId":id,
                "expectedProfileRevision":document.profiles[id].revision});
            if let Some(project) = project {
                target["projectId"] = project;
            }
            let target: ScopeTarget =
                serde_json::from_slice(&serde_json::to_vec(&target).unwrap()).unwrap();
            let scoped = service.scope_diagnosed(&caller, &target);
            if let Err(failure) = &scoped {
                let diagnostic = serde_json::to_value(failure).unwrap();
                assert_eq!(
                    diagnostic,
                    json!({"code":"INVALID_REQUEST", "stage":"scope-environment", "retryable":false})
                );
            }
            let result = scoped.map_err(|failure| failure.cause).and_then(|source| {
                let request: ReadRequest = serde_json::from_slice(
                    &serde_json::to_vec(&json!({
                        "source": source, "resourceKind": "history", "requestEpoch": "1",
                        "query": null, "sessionId": null, "limit": 200, "offset": 0
                    }))
                    .unwrap(),
                )
                .unwrap();
                request.validate()?;
                let data = service.read(&caller, &request)?;
                assert_eq!(data.state, ProjectionState::Ready);
                assert!(data.items.is_empty());
                Ok(())
            });
            results.push(result);
        }
    }
    results
}

#[test]
fn HistoryDiagnostics_MinimalProfilesAndWireDefaultsLoadBothProviders_001() {
    for result in history_with_environment(EnvMap::new()) {
        result.unwrap();
    }
}

// Windows 继承别名规范化后，两种 CLI 的默认历史请求均可读取隔离的测试目录。
#[cfg(windows)]
#[test]
fn HistoryDiagnostics_HostAliases_005() {
    let captured = crate::cli::environment::capture_windows_environment(
        vec![
            ("Path".into(), "first".into()),
            ("PATH".into(), "effective".into()),
        ],
        |_| Some("effective".into()),
    )
    .unwrap();
    for result in history_with_environment(captured) {
        result.unwrap();
    }
}

// Explorer 内部变量通过同一生产历史 scope 链路，两种 CLI 都应可读取测试目录。
#[cfg(windows)]
#[test]
fn HistoryDiagnostics_ExplorerReserved_006() {
    let inherited = [("=::".into(), "fixture".into())].into_iter().collect();
    for result in history_with_environment(inherited) {
        result.unwrap();
    }
}

#[test]
fn HistoryDiagnostics_SharedInvalidEnvironmentFailsBothProviders_002() {
    let mut inherited = EnvMap::new();
    inherited.insert("SYNTHETIC=INVALID".into(), "fixture".into());
    for result in history_with_environment(inherited) {
        let error = result.unwrap_err();
        assert_eq!(error.code, "INVALID_REQUEST");
        assert_eq!(error.field.as_deref(), Some("environment.name"));
    }
}

#[test]
fn HistoryDiagnostics_OldProfileSchemaFailureIsWorkspaceInvalid_003() {
    for cli in ["claude", "codex"] {
        let bytes = serde_json::to_vec(&json!({"schemaVersion":1,"revision":"1","profiles":{
            "fixture":{"id":"fixture","revision":"1","cli":cli,"name":"Fixture", "programPath":"C:\\Synthetic\\tool.exe"}
        }})).unwrap();
        assert_eq!(
            decode_workspace(&bytes).unwrap_err().code,
            "WORKSPACE_INVALID"
        );
    }
}

#[test]
fn HistoryDiagnostics_ProfileProjectAndWireStagesRemainDistinct_004() {
    let temp = tempfile::tempdir().unwrap();
    let repo = WorkspaceRepository::open(temp.path().join("desk/workspace.json")).unwrap();
    let profile = Profile::new("fixture", crate::cli::types::CliKind::Codex);
    let document = repo
        .apply(repo.read().unwrap().revision, Patch::Create { profile })
        .unwrap();
    let mut env = EnvMap::new();
    env.insert("HOME".into(), temp.path().as_os_str().into());
    env.insert("USERPROFILE".into(), temp.path().as_os_str().into());
    let launch = Arc::new(LaunchService::new(repo, Some(env), None));
    let caller = launch.registry().activate_window("main").unwrap();
    let service = ProjectionService::new(launch);
    for (profile_id, project_id, code, stage) in [
        (
            "bad/id",
            None,
            "INVALID_REQUEST",
            "scope-request-validation",
        ),
        (
            "missing",
            None,
            "PROFILE_NOT_FOUND",
            "scope-profile-validation",
        ),
        (
            "fixture",
            Some("missing-project"),
            "PROJECT_NOT_FOUND",
            "scope-project-registration",
        ),
    ] {
        let target = ScopeTarget::Profile {
            profile_id: profile_id.into(),
            expected_profile_revision: document.profiles["fixture"].revision,
            project_id: project_id.map(String::from),
        };
        let failure = service.scope_diagnosed(&caller, &target).unwrap_err();
        assert_eq!(
            serde_json::to_value(failure).unwrap(),
            json!({"code":code, "stage":stage, "retryable":false})
        );
    }
}
