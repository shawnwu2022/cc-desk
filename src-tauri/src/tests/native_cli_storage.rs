use crate::cli::profiles::Profile;
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, WireU64};
use serde_json::json;
use std::fs;
use tempfile::TempDir;

fn revision(value: &str) -> WireU64 {
    WireU64::parse(value).unwrap()
}

fn create(id: &str) -> Patch {
    Patch::Create {
        profile: Profile::new(id, CliKind::Codex),
    }
}

#[test]
fn D06_Storage_CreatePatchDeleteAndConflict_01() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    assert_eq!(repo.read().unwrap().revision.get(), 0);
    assert!(!path.exists());
    let first = repo.apply(revision("0"), create("one")).unwrap();
    assert_eq!(first.revision.get(), 1);
    let before = fs::read(&path).unwrap();
    assert_eq!(
        repo.apply(revision("0"), create("two")).unwrap_err().code,
        "REVISION_CONFLICT"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let second = repo
        .apply(
            revision("1"),
            Patch::Update {
                id: "one".into(),
                changes: json!({"name":"renamed","observer":{"mode":"set","value":false}})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        )
        .unwrap();
    assert_eq!(second.profiles["one"].name, "renamed");
    assert_eq!(second.profiles["one"].cli, CliKind::Codex);
    let snapshot = second.profiles["one"].clone();
    repo.apply(revision("2"), Patch::Delete { id: "one".into() })
        .unwrap();
    assert_eq!(snapshot.name, "renamed");
    assert!(repo.read().unwrap().profiles.is_empty());
    assert_eq!(
        repo.get_profile("one").unwrap_err().code,
        "PROFILE_NOT_FOUND"
    );
}

#[test]
fn D06_Storage_PreservesUnknownWorkspaceFields_02() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    fs::write(&path, serde_json::to_vec(&json!({"schemaVersion":1,"revision":"0","profiles":{},"registeredProjects":{"kept":true},"future":{"nested":[1,2,3]}})).unwrap()).unwrap();
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    repo.apply(revision("0"), create("one")).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["future"], json!({"nested":[1,2,3]}));
    assert_eq!(saved["registeredProjects"], json!({"kept":true}));
}

#[test]
fn D06_Storage_CorruptAndFutureSchemaNeverOverwritten_03() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    for contents in [
        "{broken",
        "{\"schemaVersion\":2,\"revision\":\"0\",\"profiles\":{}}",
        "{\"schemaVersion\":1,\"revision\":\"01\",\"profiles\":{}}",
    ] {
        fs::write(&path, contents).unwrap();
        assert!(repo.read().is_err());
        assert!(repo.apply(revision("0"), create("one")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
    }
}

#[test]
fn D06_Storage_RejectsUnknownPatchAndIdentityMutation_04() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    repo.apply(revision("0"), create("one")).unwrap();
    let before = fs::read(&path).unwrap();
    for changes in [
        json!({"cli":"claude"}),
        json!({"revision":"999"}),
        json!({"unexpected":"fixture-secret"}),
        json!({"name":null}),
    ] {
        let e = repo
            .apply(
                revision("1"),
                Patch::Update {
                    id: "one".into(),
                    changes: changes.as_object().unwrap().clone(),
                },
            )
            .unwrap_err();
        assert!(!e.to_string().contains("fixture-secret"));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[test]
fn D06_Storage_SeparateHandlesConflictWithoutLosingWinner_05() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|id| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let repo = WorkspaceRepository::open(path).unwrap();
                barrier.wait();
                let result = repo.apply(revision("0"), create(id));
                match result {
                    Ok(_) => (),
                    Err(e) if e.code == "REVISION_CONFLICT" => {
                        repo.apply(repo.read().unwrap().revision, create(id))
                            .unwrap();
                    }
                    Err(e) => panic!("unexpected: {e}"),
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let saved = WorkspaceRepository::open(path).unwrap().read().unwrap();
    assert_eq!(saved.revision.get(), 2);
    assert!(saved.profiles.contains_key("one"));
    assert!(saved.profiles.contains_key("two"));
}

#[test]
fn D06_Storage_DirectoryTargetIsNotRemoved_06() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    fs::create_dir(&path).unwrap();
    assert!(WorkspaceRepository::open(path.clone())
        .unwrap()
        .apply(revision("0"), create("one"))
        .is_err());
    assert!(path.is_dir());
}
