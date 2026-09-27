use crate::cli::profiles::{EnvValue, Override, Profile};
use crate::cli::storage::{Patch, WorkspaceRepository, WriteStage};
use crate::cli::workspace::register_project;
use crate::cli::types::{CliKind, WireU64};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;

fn rev(value: &str) -> WireU64 {
    WireU64::parse(value).unwrap()
}

#[test]
fn D25_MixedVersion_LegacyWritebackCannotReviveWorkspaceUnsetOrTouchCodex_001() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path().join("cli-workspace.v1.json");
    let legacy = dir.path().join("config.json");
    let repo = WorkspaceRepository::open(workspace.clone()).unwrap();

    let mut claude = Profile::new("legacyClaude", CliKind::Claude);
    claude.skip_permissions = Override::Unset;
    claude.env.insert("TOKEN".into(), Override::Unset);
    repo.apply(rev("0"), Patch::Create { profile: claude }).unwrap();

    let codex = Profile::new("codexDefault", CliKind::Codex);
    repo.apply(rev("1"), Patch::Create { profile: codex }).unwrap();

    let before = fs::read(&workspace).unwrap();
    fs::write(
        &legacy,
        serde_json::to_vec(&json!({
            "defaultSkipPermissions": true,
            "claudeEnvVars": {"TOKEN": "old-package-secret"},
            "futureLegacyField": {"kept": true}
        })).unwrap(),
    ).unwrap();

    let saved = repo.read().unwrap();
    let claude = saved.profiles["legacyClaude"].clone();
    let codex = saved.profiles["codexDefault"].clone();
    let old = claude.read_legacy(&legacy).unwrap().unwrap();

    assert_eq!(claude.resolve_skip_permissions(Some(&old)), None);
    assert_eq!(
        claude.resolve_env(Some(&old), &BTreeMap::new()).unwrap()["TOKEN"],
        None
    );
    assert_eq!(codex.resolve_skip_permissions(Some(&old)), None);
    assert!(codex.resolve_env(Some(&old), &BTreeMap::new()).unwrap().is_empty());
    assert_eq!(fs::read(&workspace).unwrap(), before);
}

#[test]
fn D25_CommitUnknown_ReReadWinsAndOldCasCannotReplay_002() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    let repo = WorkspaceRepository::open(path).unwrap();

    repo.apply(
        rev("0"),
        Patch::Create { profile: Profile::new("old", CliKind::Codex) },
    ).unwrap();

    let failure = repo.apply_with_fault(
        rev("1"),
        Patch::Create { profile: Profile::new("new", CliKind::Codex) },
        WriteStage::AfterReplace,
    ).unwrap_err();
    assert_eq!(failure.code, "COMMIT_STATE_UNKNOWN");
    assert!(!failure.retryable);

    let recovered = repo.read().unwrap();
    assert_eq!(recovered.revision.get(), 2);
    assert!(recovered.profiles.contains_key("new"));

    let before = fs::read(dir.path().join("cli-workspace.v1.json")).unwrap();
    let replay = repo.apply(
        rev("1"),
        Patch::Create { profile: Profile::new("new", CliKind::Codex) },
    ).unwrap_err();
    assert_eq!(replay.code, "REVISION_CONFLICT");
    assert_eq!(fs::read(dir.path().join("cli-workspace.v1.json")).unwrap(), before);
}

#[test]
fn D25_UnknownWorkspaceExtensionsSurviveProfileAndProjectEraWrites_003() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schemaVersion": 1,
            "revision": "0",
            "profiles": {},
            "registeredProjects": {},
            "future": {
                "nested": ["alpha", {"beta": true}],
                "flag": false
            }
        })).unwrap(),
    ).unwrap();
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    repo.apply(
        rev("0"),
        Patch::Create { profile: Profile::new("codex", CliKind::Codex) },
    ).unwrap();

    let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        saved["future"],
        json!({"nested":["alpha",{"beta":true}],"flag":false})
    );
}

#[test]
fn D25_LegacyLiteralSecretIsNeverCopiedIntoWorkspace_004() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli-workspace.v1.json");
    let legacy = json!({"claudeEnvVars":{"TOKEN":"fixture-secret"}});
    let mut profile = Profile::new("legacyClaude", CliKind::Claude);
    profile.env.insert(
        "TOKEN".into(),
        Override::Set(EnvValue::HostRef { name: "CC_TEST_TOKEN".into() }),
    );
    assert_eq!(
        profile.resolve_env(
            Some(&legacy),
            &BTreeMap::from([("CC_TEST_TOKEN".into(), "host-secret".into())]),
        ).unwrap()["TOKEN"],
        Some("host-secret".into())
    );
    WorkspaceRepository::open(path.clone()).unwrap()
        .apply(rev("0"), Patch::Create { profile }).unwrap();
    let bytes = fs::read_to_string(path).unwrap();
    assert!(!bytes.contains("fixture-secret"));
    assert!(!bytes.contains("host-secret"));
}


#[test]
fn D25_ConcurrentProfileAndProjectWritesPreserveBothDomains_005() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path().join("cli-workspace.v1.json");
    let project_path = dir.path().join("project");
    fs::create_dir(&project_path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

    let profile_worker = {
        let workspace = workspace.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let repo = WorkspaceRepository::open(workspace).unwrap();
            barrier.wait();
            loop {
                let revision = repo.read().unwrap().revision;
                match repo.apply(
                    revision,
                    Patch::Create {
                        profile: Profile::new("codex", CliKind::Codex),
                    },
                ) {
                    Ok(_) => break,
                    Err(error) if error.code == "REVISION_CONFLICT" => continue,
                    Err(error) => panic!("unexpected profile write failure: {error}"),
                }
            }
        })
    };

    let project_worker = {
        let workspace = workspace.clone();
        let barrier = barrier.clone();
        let project_path = project_path.clone();
        std::thread::spawn(move || {
            let repo = WorkspaceRepository::open(workspace).unwrap();
            barrier.wait();
            register_project(&repo, &project_path).unwrap();
        })
    };

    profile_worker.join().unwrap();
    project_worker.join().unwrap();

    let saved = WorkspaceRepository::open(workspace).unwrap().read().unwrap();
    assert_eq!(saved.revision.get(), 2);
    assert!(saved.profiles.contains_key("codex"));
    assert_eq!(saved.registered_projects.len(), 1);
    assert_eq!(
        saved.registered_projects.values().next().unwrap().selected_path,
        project_path
    );
}
