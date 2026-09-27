use crate::cli::environment::{build_environment, EnvMap};
use crate::cli::profiles::{Override, Profile};
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, WireU64};
use crate::cli::workspace::{patch_project, register_project, LegacyMetadata};
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
    // registeredProjects is a validated D07 field, no longer an unknown extension.
    fs::write(&path, serde_json::to_vec(&json!({"schemaVersion":1,"revision":"0","profiles":{},"futureProjectExtensions":{"kept":true},"future":{"nested":[1,2,3]}})).unwrap()).unwrap();
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    repo.apply(revision("0"), create("one")).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["future"], json!({"nested":[1,2,3]}));
    assert_eq!(saved["futureProjectExtensions"], json!({"kept":true}));
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


#[test]
fn D25_Rollback_LegacyWritebackCannotReviveUnsetOrLeakCodex_01() {
    let dir = TempDir::new().unwrap();
    let workspace_path = dir.path().join("cli-workspace.v1.json");
    let repo = WorkspaceRepository::open(workspace_path.clone()).unwrap();

    let mut legacy_claude = Profile::new("legacyClaude", CliKind::Claude);
    legacy_claude.skip_permissions = Override::Unset;
    legacy_claude
        .env
        .insert("ROLLBACK_SECRET".into(), Override::Unset);
    repo.apply(
        revision("0"),
        Patch::Create {
            profile: legacy_claude,
        },
    )
    .unwrap();
    repo.apply(revision("1"), create("codex")).unwrap();

    let project_dir = dir.path().join("project");
    fs::create_dir(&project_dir).unwrap();
    let project = register_project(&repo, &project_dir).unwrap();
    let project = patch_project(
        &repo,
        revision("3"),
        &project.project_id,
        json!({
            "alias": {"mode":"unset"},
            "pinned": {"mode":"set","value":false},
            "hidden": {"mode":"unset"}
        }),
    )
    .unwrap();

    // Simulate an older package writing only the legacy files it knows about.
    let legacy = json!({
        "defaultSkipPermissions": true,
        "claudeEnvVars": {
            "ROLLBACK_SECRET": "fixture-secret",
            "LEGACY_ONLY": "legacy-value"
        },
        "unknownOldField": {"keep": true}
    });
    let legacy_projects = json!({
        "displayNames": { project_dir.to_str().unwrap(): "Old alias" },
        "pinnedProjects": [project_dir.to_str().unwrap()],
        "hiddenProjects": [project_dir.to_str().unwrap()],
        "futureOldField": ["keep"]
    });
    fs::write(
        dir.path().join("config.json"),
        serde_json::to_vec_pretty(&legacy).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.path().join("projects.json"),
        serde_json::to_vec_pretty(&legacy_projects).unwrap(),
    )
    .unwrap();

    let reopened = WorkspaceRepository::open(workspace_path).unwrap();
    let document = reopened.read().unwrap();
    let claude = &document.profiles["legacyClaude"];
    let codex = &document.profiles["codex"];

    assert_eq!(claude.resolve_skip_permissions(Some(&legacy)), None);
    let claude_env =
        build_environment(&EnvMap::new(), &EnvMap::new(), claude, Some(&legacy), None).unwrap();
    assert!(!claude_env.contains_key(std::ffi::OsStr::new("ROLLBACK_SECRET")));
    assert_eq!(
        claude_env
            .get(std::ffi::OsStr::new("LEGACY_ONLY"))
            .unwrap(),
        "legacy-value"
    );

    let codex_env =
        build_environment(&EnvMap::new(), &EnvMap::new(), codex, Some(&legacy), None).unwrap();
    assert!(!codex_env.contains_key(std::ffi::OsStr::new("ROLLBACK_SECRET")));
    assert!(!codex_env.contains_key(std::ffi::OsStr::new("LEGACY_ONLY")));

    let resolved = project.resolve_metadata(&LegacyMetadata {
        alias: Some("Old alias".into()),
        pinned: Some(true),
        hidden: Some(true),
    });
    assert_eq!(resolved.alias, None);
    assert_eq!(resolved.pinned, Some(false));
    assert_eq!(resolved.hidden, None);
}

#[test]
fn D25_Rollback_NewWorkspaceWritesNeverTouchLegacyFiles_02() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.json");
    let projects_path = dir.path().join("projects.json");
    let config_bytes = br#"{"legacySecret":"fixture-secret","future":{"x":1}}"#;
    let projects_bytes = br#"{"displayNames":{},"future":["preserve"]}"#;
    fs::write(&config_path, config_bytes).unwrap();
    fs::write(&projects_path, projects_bytes).unwrap();

    let workspace_path = dir.path().join("cli-workspace.v1.json");
    let repo = WorkspaceRepository::open(workspace_path).unwrap();
    repo.apply(revision("0"), create("codex")).unwrap();
    let folder = dir.path().join("project");
    fs::create_dir(&folder).unwrap();
    register_project(&repo, &folder).unwrap();

    assert_eq!(fs::read(config_path).unwrap(), config_bytes);
    assert_eq!(fs::read(projects_path).unwrap(), projects_bytes);
}

#[test]
fn D25_Rollback_UnknownWorkspaceExtensionsSurviveMixedVersionWrites_03() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 1,
            "revision": "0",
            "profiles": {},
            "registeredProjects": {},
            "futureWorkspace": {
                "writer": "future-version",
                "opaque": ["a", {"b": 2}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    repo.apply(revision("0"), create("codex")).unwrap();

    // An older package may rewrite its own legacy files between new-version
    // workspace writes. The next new-version write must still retain extensions.
    fs::write(
        dir.path().join("config.json"),
        br#"{"defaultSkipPermissions":true,"unknown":"old-writer"}"#,
    )
    .unwrap();

    repo.apply(
        revision("1"),
        Patch::Update {
            id: "codex".into(),
            changes: json!({"name":"after-rollback"})
                .as_object()
                .unwrap()
                .clone(),
        },
    )
    .unwrap();

    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        saved["futureWorkspace"],
        json!({
            "writer": "future-version",
            "opaque": ["a", {"b": 2}]
        })
    );
}
