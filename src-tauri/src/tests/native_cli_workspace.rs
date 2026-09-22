use crate::cli::profiles::{Override, Profile};
use crate::cli::source_scope::resolve_path_key;
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, WireU64};
use crate::cli::workspace::{
    list_registered_projects, patch_project, register_project, remove_project, LegacyMetadata,
};
use serde_json::json;
use std::fs;
use std::path::Path;

fn revision(value: &str) -> WireU64 {
    WireU64::parse(value).unwrap()
}

#[test]
fn D07_Workspace_NoClaudeDirectoryAndReopen_01() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    for name in ["codex-project", "another-project"] {
        let project = tmp.path().join(name);
        fs::create_dir(&project).unwrap();
        register_project(&repo, &project).unwrap();
    }
    drop(repo);
    let reopened = WorkspaceRepository::open(path).unwrap();
    assert_eq!(list_registered_projects(&reopened).unwrap().len(), 2);
    assert!(!tmp.path().join(".claude").exists());
}

#[test]
fn D07_Workspace_IdempotentAndPreservesProfiles_02() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    repo.apply(
        revision("0"),
        Patch::Create {
            profile: Profile::new("codex", CliKind::Codex),
        },
    )
    .unwrap();
    let first = register_project(&repo, tmp.path()).unwrap();
    let bytes = fs::read(&path).unwrap();
    let same = register_project(&repo, &tmp.path().join(".")).unwrap();
    assert_eq!(first.project_id, same.project_id);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(repo.get_profile("codex").unwrap().revision.get(), 1);
}

#[test]
fn D07_Workspace_MissingPathsRetainedAndNotCaseFolded_03() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = WorkspaceRepository::open(tmp.path().join("workspace.json")).unwrap();
    let upper = tmp.path().join("Missing-A");
    let lower = tmp.path().join("missing-a");
    let a = register_project(&repo, &upper).unwrap();
    let b = register_project(&repo, &lower).unwrap();
    assert_ne!(a.project_id, b.project_id);
    assert!(a.canonical_path.is_none());
    assert_eq!(list_registered_projects(&repo).unwrap().len(), 2);
    assert!(!upper.exists());
    assert!(!lower.exists());
}

#[test]
fn D07_Workspace_RemoveDoesNotDeleteDirectoriesOrTranscripts_04() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = WorkspaceRepository::open(tmp.path().join("workspace.json")).unwrap();
    let folder = tmp.path().join("project");
    fs::create_dir(&folder).unwrap();
    let transcript = folder.join("native.jsonl");
    fs::write(&transcript, "synthetic-original").unwrap();
    let project = register_project(&repo, &folder).unwrap();
    remove_project(&repo, revision("1"), &project.project_id).unwrap();
    assert!(list_registered_projects(&repo).unwrap().is_empty());
    assert_eq!(fs::read_to_string(transcript).unwrap(), "synthetic-original");
}

#[test]
fn D07_Workspace_MetadataConflictAndTriState_05() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    let project = register_project(&repo, tmp.path()).unwrap();
    let changes = json!({
        "alias": {"mode": "set", "value": ""},
        "pinned": {"mode": "set", "value": false},
        "hidden": {"mode": "unset"}
    });
    let saved = patch_project(&repo, revision("1"), &project.project_id, changes).unwrap();
    let legacy = LegacyMetadata {
        alias: Some("old".into()),
        pinned: Some(true),
        hidden: Some(true),
    };
    let resolved = saved.resolve_metadata(&legacy);
    assert_eq!(resolved.alias, Some(String::new()));
    assert_eq!(resolved.pinned, Some(false));
    assert_eq!(resolved.hidden, None);
    let before = fs::read(&path).unwrap();
    let error = remove_project(&repo, revision("1"), &project.project_id).unwrap_err();
    assert_eq!(error.code, "REVISION_CONFLICT");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn D07_Workspace_RejectsIdentityMutationAndUnsafeMetadata_06() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    let project = register_project(&repo, tmp.path()).unwrap();
    let before = fs::read(&path).unwrap();
    for changes in [
        json!({"selectedPath": "synthetic-secret"}),
        json!({"sourcePathKey": "spoofed"}),
        json!({"alias": null}),
        json!({"alias": {"mode": "set", "value": "bad\u0000value"}}),
        json!({"pinned": {"mode": "set", "value": "yes"}}),
        json!({"archivedSessions": ["same-id"]}),
    ] {
        let error = patch_project(&repo, revision("1"), &project.project_id, changes)
            .unwrap_err();
        assert!(!error.to_string().contains("synthetic-secret"));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[test]
fn D07_Workspace_NativeDamageOrMissingDirectoryDoesNotPrune_07() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = WorkspaceRepository::open(tmp.path().join("workspace.json")).unwrap();
    let folder = tmp.path().join("project");
    fs::create_dir(&folder).unwrap();
    let registered = register_project(&repo, &folder).unwrap();
    fs::remove_dir(&folder).unwrap();
    fs::write(tmp.path().join("corrupt-native-history.jsonl"), "{broken").unwrap();
    let saved = list_registered_projects(&repo).unwrap();
    assert_eq!(saved[0].project_id, registered.project_id);
}

#[test]
fn D07_Workspace_ConcurrentRegistrationsDoNotLoseUpdates_08() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace.json");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|index| {
            let path = path.clone();
            let folder = tmp.path().join(format!("project-{index}"));
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let repo = WorkspaceRepository::open(path).unwrap();
                barrier.wait();
                register_project(&repo, &folder).unwrap();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let repo = WorkspaceRepository::open(path).unwrap();
    assert_eq!(list_registered_projects(&repo).unwrap().len(), 8);
    assert_eq!(repo.read().unwrap().revision.get(), 8);
}

#[test]
fn D07_Path_RejectsRelativeNulAndFiles_09() {
    assert!(resolve_path_key(Path::new("relative/project")).is_err());
    assert!(resolve_path_key(Path::new("bad\0path")).is_err());
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("regular-file");
    fs::write(&file, "fixture").unwrap();
    assert!(resolve_path_key(&file).is_err());
    let unresolved = tmp.path().join("missing/../other");
    assert_ne!(
        resolve_path_key(&unresolved).unwrap().key,
        resolve_path_key(&tmp.path().join("other")).unwrap().key
    );
}

#[cfg(unix)]
#[test]
fn D07_Path_UnixSymlinkAndCaseIdentity_10() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("Actual");
    fs::create_dir(&target).unwrap();
    let alias = tmp.path().join("alias");
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    assert_eq!(
        resolve_path_key(&target).unwrap().key,
        resolve_path_key(&alias).unwrap().key
    );
    let other = tmp.path().join("actual");
    if !other.exists() {
        fs::create_dir(&other).unwrap();
        assert_ne!(
            resolve_path_key(&target).unwrap().key,
            resolve_path_key(&other).unwrap().key
        );
    }
}

#[cfg(windows)]
#[test]
fn D07_Path_WindowsCaseAndJunctionIdentity_11() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("Actual");
    fs::create_dir(&target).unwrap();
    let upper = std::path::PathBuf::from(target.to_str().unwrap().to_uppercase());
    assert_eq!(
        resolve_path_key(&target).unwrap().key,
        resolve_path_key(&upper).unwrap().key
    );
    let alias = tmp.path().join("junction");
    let status = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&alias)
        .arg(&target)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(
        resolve_path_key(&target).unwrap().key,
        resolve_path_key(&alias).unwrap().key
    );
    fs::remove_dir(alias).unwrap();
}

#[test]
fn D07_Workspace_LegacyOverrideDefaults_12() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = WorkspaceRepository::open(tmp.path().join("workspace.json")).unwrap();
    let project = register_project(&repo, tmp.path()).unwrap();
    assert_eq!(project.alias, Override::Inherit);
    assert_eq!(project.pinned, Override::Inherit);
    assert_eq!(project.hidden, Override::Inherit);
}
