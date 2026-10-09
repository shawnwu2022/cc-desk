use crate::cli::storage::{WorkspaceDocument, WorkspaceRepository};
use serde_json::json;

#[path = "native_cli_workspace.rs"]
mod behavior;

#[test]
fn D07_Workspace_DefaultHasIndependentProjectRegistry_01() {
    let value = serde_json::to_value(WorkspaceDocument::default()).unwrap();
    assert_eq!(value.get("registeredProjects"), Some(&json!({})));
}

#[test]
fn D07_Workspace_RejectsMalformedRegistryWithoutOverwriting_02() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    for projects in [json!(null), json!([]), json!({"bad": {"selectedPath": 1}})] {
        let raw = json!({
            "schemaVersion": 1,
            "revision": "0",
            "profiles": {},
            "registeredProjects": projects
        });
        let bytes = serde_json::to_vec(&raw).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            repo.read().is_err(),
            "invalid project registry must not be accepted"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
