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
            let result = service.scope(&caller, &target).and_then(|source| {
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
