use crate::cli::profiles::{EnvValue, Override, Profile};
use crate::cli::storage::{Patch, WorkspaceRepository, WriteStage};
use crate::cli::types::{CliKind, WireU64};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;

fn rev(value: &str) -> WireU64 {
    WireU64::parse(value).unwrap()
}

fn create(id: &str) -> Patch {
    Patch::Create {
        profile: Profile::new(id, CliKind::Codex),
    }
}

#[test]
fn D06_Legacy_CodexIgnoresCorruptFile_06() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("legacy.json");
    fs::write(&path, "{broken-synthetic-secret").unwrap();
    let codex = Profile::new("codex", CliKind::Codex);
    assert!(codex.read_legacy(&path).unwrap().is_none());
    let claude = Profile::new("legacyClaude", CliKind::Claude);
    let error = claude.read_legacy(&path).unwrap_err();
    assert_eq!(error.code, "LEGACY_INVALID");
    assert!(!error.to_string().contains("synthetic-secret"));
    assert_eq!(fs::read_to_string(path).unwrap(), "{broken-synthetic-secret");
}

#[test]
fn D06_Legacy_EnvNeverPersistedOrCopiedToCodex_07() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("workspace.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    let legacy = json!({"claudeEnvVars":{"TOKEN":"synthetic-old-secret"}});
    let mut profile = Profile::new("legacyClaude", CliKind::Claude);
    let host = BTreeMap::from([("EXTERNAL".into(), "synthetic-host-secret".into())]);
    assert_eq!(
        profile.resolve_env(Some(&legacy), &host).unwrap()["TOKEN"],
        Some("synthetic-old-secret".into())
    );
    profile.env.insert("TOKEN".into(), Override::Unset);
    profile.env.insert(
        "OTHER".into(),
        Override::Set(EnvValue::HostRef {
            name: "EXTERNAL".into(),
        }),
    );
    let resolved = profile.resolve_env(Some(&legacy), &host).unwrap();
    assert_eq!(resolved["TOKEN"], None);
    assert_eq!(resolved["OTHER"], Some("synthetic-host-secret".into()));
    repo.apply(rev("0"), Patch::Create { profile }).unwrap();
    let saved = fs::read_to_string(path).unwrap();
    assert!(!saved.contains("synthetic-old-secret"));
    assert!(!saved.contains("synthetic-host-secret"));
    assert!(Profile::new("codex", CliKind::Codex)
        .resolve_env(Some(&legacy), &host)
        .unwrap()
        .is_empty());
}

#[test]
fn D06_Storage_FaultBoundaryPreservesOrReportsCommit_07() {
    for stage in [
        WriteStage::BeforeSync,
        WriteStage::BeforeReplace,
        WriteStage::AfterReplace,
    ] {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("workspace.json");
        let repo = WorkspaceRepository::open(path.clone()).unwrap();
        repo.apply(rev("0"), create("old")).unwrap();
        let before = fs::read(&path).unwrap();
        let error = repo
            .apply_with_fault(rev("1"), create("new"), stage)
            .unwrap_err();
        if stage == WriteStage::AfterReplace {
            assert_eq!(error.code, "COMMIT_STATE_UNKNOWN");
            assert!(!error.retryable);
            assert_eq!(repo.read().unwrap().revision.get(), 2);
        } else {
            assert_eq!(error.code, "STORAGE_IO");
            assert_eq!(fs::read(&path).unwrap(), before);
        }
        let leftovers = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "tmp"))
            .count();
        assert_eq!(leftovers, 0, "own temporary files must be cleaned");
    }
}

#[test]
fn D06_Storage_RevisionExhaustionPreservesDocument_08() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("workspace.json");
    let contents = json!({"schemaVersion":1,"revision":"18446744073709551615","profiles":{}});
    fs::write(&path, serde_json::to_vec(&contents).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    assert_eq!(
        repo.apply(rev("18446744073709551615"), create("new"))
            .unwrap_err()
            .code,
        "REVISION_EXHAUSTED"
    );
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn D06_Storage_EnvPatchPreservesSiblingAndUnset_09() {
    let dir = TempDir::new().unwrap();
    let repo = WorkspaceRepository::open(dir.path().join("workspace.json")).unwrap();
    let mut profile = Profile::new("codex", CliKind::Codex);
    profile.env.insert("FIRST".into(), Override::Unset);
    profile.env.insert("SECOND".into(), Override::Inherit);
    repo.apply(rev("0"), Patch::Create { profile }).unwrap();
    let changes = json!({"env":{"SECOND":{"mode":"set","value":{"kind":"literal","value":"","nonSecret":true}}}});
    let updated = repo
        .apply(
            rev("1"),
            Patch::Update {
                id: "codex".into(),
                changes: changes.as_object().unwrap().clone(),
            },
        )
        .unwrap();
    assert_eq!(updated.profiles["codex"].env["FIRST"], Override::Unset);
    let env = updated.profiles["codex"]
        .resolve_env(None, &BTreeMap::new())
        .unwrap();
    assert_eq!(env["FIRST"], None);
    assert_eq!(env["SECOND"], Some(String::new()));
}

#[cfg(windows)]
#[test]
fn D06_Storage_WindowsBusyTargetPreservesBytes_10() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("workspace.json");
    let repo = WorkspaceRepository::open(path.clone()).unwrap();
    repo.apply(rev("0"), create("old")).unwrap();
    let before = fs::read(&path).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(&path)
        .unwrap();
    assert!(repo.apply(rev("1"), create("new")).is_err());
    drop(held);
    assert_eq!(fs::read(path).unwrap(), before);
}

struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn D06_Storage_TwoProcessesPreserveBothWriters_11() {
    use std::time::{Duration, Instant};
    let dir = TempDir::new().unwrap();
    let mut children: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|id| {
            ChildGuard(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "tests::native_cli_profile_edges::D06_Storage_ChildWorker_99",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("CC_DESK_D06_TEST_ROOT", dir.path())
                    .env("CC_DESK_D06_TEST_ID", id)
                    .spawn()
                    .unwrap(),
            )
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !(dir.path().join("first.ready").exists() && dir.path().join("second.ready").exists()) {
        assert!(Instant::now() < deadline, "child readiness timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(dir.path().join("go"), b"synthetic").unwrap();
    for child in &mut children {
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "child completion timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let document = WorkspaceRepository::open(dir.path().join("workspace.json"))
        .unwrap()
        .read()
        .unwrap();
    assert_eq!(document.revision.get(), 2);
    assert!(document.profiles.contains_key("first"));
    assert!(document.profiles.contains_key("second"));
}

#[test]
#[ignore = "subprocess worker invoked by D06_Storage_TwoProcessesPreserveBothWriters_11"]
fn D06_Storage_ChildWorker_99() {
    use std::time::{Duration, Instant};
    let root = std::path::PathBuf::from(std::env::var_os("CC_DESK_D06_TEST_ROOT").unwrap());
    let id = std::env::var("CC_DESK_D06_TEST_ID").unwrap();
    assert!(matches!(id.as_str(), "first" | "second"));
    let repo = WorkspaceRepository::open(root.join("workspace.json")).unwrap();
    fs::write(root.join(format!("{id}.ready")), b"synthetic").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("go").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let result = repo.apply(rev("0"), create(&id));
    if let Err(error) = result {
        assert_eq!(error.code, "REVISION_CONFLICT");
        repo.apply(repo.read().unwrap().revision, create(&id))
            .unwrap();
    }
}
