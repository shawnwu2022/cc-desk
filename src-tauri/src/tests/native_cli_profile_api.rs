use crate::cli::profile_service::{list_profiles, parse_patch, patch_profile};
use crate::cli::profiles::Profile;
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, WireU64};
use serde_json::json;
use tempfile::TempDir;

#[test]
fn D06_ProfileApi_RejectsOtherWindowBeforeIo_01() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("not-created");
    let repo = WorkspaceRepository::open(root.join("workspace.json")).unwrap();
    assert_eq!(list_profiles(&repo, "other").unwrap_err().code, "FORBIDDEN");
    assert!(!root.exists());
    let patch = Patch::Create {
        profile: Profile::new("codex", CliKind::Codex),
    };
    assert_eq!(
        patch_profile(&repo, "other", WireU64::parse("0").unwrap(), patch)
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    assert!(!root.exists());
}

#[test]
fn D06_ProfileApi_DoesNotReturnWorkspaceExtras_02() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("workspace.json");
    let raw = json!({"schemaVersion":1,"revision":"0","profiles":{},"futureSecret":"synthetic-only"});
    std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    let list = list_profiles(&repo, "main").unwrap();
    assert_eq!(serde_json::to_value(list).unwrap(), json!({"revision":"0","profiles":[]}));
    let next = patch_profile(
        &repo,
        "main",
        WireU64::parse("0").unwrap(),
        Patch::Create {
            profile: Profile::new("codex", CliKind::Codex),
        },
    )
    .unwrap();
    assert_eq!(next.revision.get(), 1);
    assert!(!serde_json::to_string(&next).unwrap().contains("synthetic-only"));
    let disk = std::fs::read_to_string(path).unwrap();
    assert!(disk.contains("synthetic-only"));
}

#[test]
fn D06_ProfileApi_SafeParseRejectsArbitraryPathAndSecret_03() {
    for patch in [
        json!({"op":"synthetic-secret-operation"}),
        json!({"op":"delete","id":"codex","path":"synthetic-secret-path"}),
    ] {
        let error = parse_patch(&json!("0"), patch).unwrap_err();
        assert_eq!(error.code, "INVALID_REQUEST");
        assert!(!error.to_string().contains("synthetic-secret"));
    }
    assert!(parse_patch(&json!("01"), json!({"op":"delete","id":"codex"})).is_err());
    assert!(parse_patch(&json!(0), json!({"op":"delete","id":"codex"})).is_err());
}
