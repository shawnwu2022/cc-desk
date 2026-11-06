#![allow(non_snake_case)]
use super::*;
use crate::cli::profiles::{Override, Profile};
use crate::cli::storage::{Patch, WorkspaceRepository};
use std::path::PathBuf;
use std::sync::OnceLock;
fn executable() -> &'static PathBuf {
    static FIXTURE: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    &FIXTURE
        .get_or_init(|| {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("fixture.rs");
            let binary = root.path().join(if cfg!(windows) {
                "fixture.exe"
            } else {
                "fixture"
            });
            std::fs::write(
                &source,
                r#"
use std::io::Write;
fn main() {
 assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(),vec!["--version"]);
 let marker=std::env::var("FIXTURE_MARKER").unwrap();
 let mut f=std::fs::OpenOptions::new().create(true).append(true).open(&marker).unwrap();
 writeln!(f,"spawn").unwrap();
 match std::env::var("FIXTURE_MODE").unwrap().as_str() {
  "hang" => std::thread::sleep(std::time::Duration::from_secs(60)),
  "cap" => { print!("{}", "x".repeat(9000)); },
  "stderr-cap" => { eprint!("{}", "x".repeat(9000)); },
  "sensitive" => println!("codex-cli 1.2.3\nsecret-token"),
  "stderr" => { eprintln!("secret-token"); println!("codex-cli 1.2.3"); },
  "failure" => std::process::exit(2),
  _ => println!("codex-cli 1.2.3"),
 }
}
"#,
            )
            .unwrap();
            let status = std::process::Command::new("rustc")
                .args(["--edition", "2021"])
                .arg(&source)
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap();
            assert!(status.success());
            (root, binary)
        })
        .1
}
struct Fixture {
    root: tempfile::TempDir,
    service: Arc<LaunchService>,
    caller: CallerIdentity,
    query: ProbeRequest,
}
impl Fixture {
    fn new(mode: &str, launcher: Launcher) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = WorkspaceRepository::open(root.path().join("workspace.json")).unwrap();
        let mut profile = Profile::new("fixture", CliKind::Codex);
        profile.program_path = Override::Set(executable().to_string_lossy().into_owned());
        profile.launcher = launcher;
        profile.default_args =
            Override::Set(vec!["--eval".into(), "malicious script string".into()]);
        profile.observer = Override::Set(true);
        repo.apply(repo.read().unwrap().revision, Patch::Create { profile })
            .unwrap();
        let profile = repo.get_profile("fixture").unwrap();
        let mut env = EnvMap::from([
            ("FIXTURE_MODE".into(), mode.into()),
            (
                "FIXTURE_MARKER".into(),
                root.path().join("spawned").into_os_string(),
            ),
            ("HOME".into(), root.path().into()),
            ("USERPROFILE".into(), root.path().into()),
        ]);
        if let Some(value) = std::env::var_os("SystemRoot") {
            env.insert("SystemRoot".into(), value);
        }
        let service = Arc::new(LaunchService::new(repo, Some(env), None));
        let caller = service.registry().activate_window("main").unwrap();
        let query = ProbeRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            profile_id: "fixture".into(),
            expected_profile_revision: profile.revision,
        };
        Self {
            root,
            service,
            caller,
            query,
        }
    }
    fn probe(&self) -> ProbeResult {
        self.service
            .probe_cli_version(&self.caller, self.query.clone())
            .unwrap()
    }
    fn start(&self) -> std::thread::JoinHandle<Result<ProbeResult, SafeError>> {
        let (service, caller, query) = (
            self.service.clone(),
            self.caller.clone(),
            self.query.clone(),
        );
        std::thread::spawn(move || service.probe_cli_version(&caller, query))
    }
    fn wait_spawn(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.root.path().join("spawned").exists() {
            assert!(Instant::now() < deadline, "fixture failed to start");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn cancel(&self) -> CancelResult {
        self.service
            .cancel_cli_version(
                &self.caller,
                CancelRequest {
                    request_id: self.query.request_id.clone(),
                },
            )
            .unwrap()
    }
}
#[test]
fn CliVersion_ExactProfileRawArgsAndReplay_003() {
    let f = Fixture::new("known", Launcher::Native);
    let first = f.probe();
    assert_eq!(first.state, "known");
    assert_eq!(first.version.as_deref(), Some("1.2.3"));
    assert_eq!(first.reason, None);
    assert_eq!(f.probe(), first);
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("spawned")).unwrap(),
        "spawn\n"
    );
    let mut other = f.query.clone();
    other.profile_id = "other".into();
    assert_eq!(
        f.service
            .probe_cli_version(&f.caller, other)
            .unwrap_err()
            .code,
        "REQUEST_CONFLICT"
    );
    assert_eq!(f.cancel().status, "settled");
    assert_eq!(*f.service.command_owners.active.lock(), 0);
}
#[test]
fn CliVersion_CancelBeforeBeginFencesSpawn_004() {
    let f = Fixture::new("known", Launcher::Native);
    assert_eq!(f.cancel().status, "requested");
    assert_eq!(f.cancel().status, "requested");
    assert_eq!(f.probe().state, "cancelled");
    assert!(!f.root.path().join("spawned").exists());
    assert_eq!(f.cancel().status, "settled");
}
#[test]
fn CliVersion_CancelRunningRetainsOwnerUntilReap_005() {
    let f = Fixture::new("hang", Launcher::Native);
    let worker = f.start();
    f.wait_spawn();
    assert_eq!(f.cancel().status, "requested");
    let result = worker.join().unwrap().unwrap();
    assert_eq!(result.state, "cancelled");
    assert_eq!(result.version, None);
    assert_eq!(result.reason, None);
    assert_eq!(*f.service.command_owners.active.lock(), 0);
}
#[test]
fn CliVersion_ChangedRevisionStopsChild_006() {
    let f = Fixture::new("hang", Launcher::Native);
    let worker = f.start();
    f.wait_spawn();
    f.service
        .repository
        .apply(
            f.service.repository.read().unwrap().revision,
            Patch::Update {
                id: "fixture".into(),
                changes: serde_json::from_value(serde_json::json!({"name":"changed"})).unwrap(),
            },
        )
        .unwrap();
    let result = worker.join().unwrap().unwrap();
    assert_eq!(result.reason, Some("profile-changed"));
    assert_eq!(result.version, None);
    assert_eq!(*f.service.command_owners.active.lock(), 0);
}
#[test]
fn CliVersion_RevokedDocumentCannotPublish_007() {
    let f = Fixture::new("hang", Launcher::Native);
    let worker = f.start();
    f.wait_spawn();
    f.service.registry().activate_window("main").unwrap();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(*f.service.command_owners.active.lock(), 0);
}
#[test]
fn CliVersion_TimeoutIsBoundedAndReaped_008() {
    let f = Fixture::new("hang", Launcher::Native);
    let result = f.probe();
    assert_eq!(result.reason, Some("timeout"));
    assert_eq!(*f.service.command_owners.active.lock(), 0);
}
#[test]
fn CliVersion_OutputCapAndSensitiveTextNeverSerialized_009() {
    for (mode, reason) in [
        ("cap", "output-limit"),
        ("stderr-cap", "output-limit"),
        ("sensitive", "invalid-output"),
        ("stderr", "invalid-output"),
        ("failure", "command-failed"),
    ] {
        let f = Fixture::new(mode, Launcher::Native);
        let result = f.probe();
        assert_eq!(result.reason, Some(reason), "{mode}");
        assert_eq!(result.version, None);
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("secret-token"));
        assert!(!json.contains(&f.root.path().to_string_lossy().to_string()));
    }
}
#[test]
fn CliVersion_UnsupportedShellNeverSpawns_010() {
    let f = Fixture::new(
        "known",
        Launcher::Shell {
            program: executable().to_string_lossy().into_owned(),
            dialect: Dialect::Bash,
        },
    );
    assert_eq!(f.probe().reason, Some("unsupported-launcher"));
    assert!(!f.root.path().join("spawned").exists());
}
#[test]
fn CliVersion_ConcurrentDuplicatesShareOneChild_011() {
    let f = Fixture::new("hang", Launcher::Native);
    let first = f.start();
    f.wait_spawn();
    let second = f.start();
    f.cancel();
    assert_eq!(
        first.join().unwrap().unwrap(),
        second.join().unwrap().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("spawned")).unwrap(),
        "spawn\n"
    );
}

#[test]
fn CliVersion_StaleRevisionNeverSpawns_012() {
    let mut f = Fixture::new("known", Launcher::Native);
    f.query.expected_profile_revision = WireU64::parse("999").unwrap();
    assert_eq!(f.probe().reason, Some("profile-changed"));
    assert!(!f.root.path().join("spawned").exists());
}
#[test]
fn CliVersion_MaintenanceGatePreventsSpawn_013() {
    let f = Fixture::new("known", Launcher::Native);
    let frozen = f
        .service
        .repository
        .admission()
        .freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap();
    assert_eq!(f.probe().reason, Some("unavailable"));
    assert!(!f.root.path().join("spawned").exists());
    assert_eq!(*f.service.command_owners.active.lock(), 0);
    frozen.release_review().unwrap();
}
#[test]
fn CliVersion_BrokenExactSelectionNeverFallsBack_014() {
    let f = Fixture::new("known", Launcher::Native);
    f.service.repository.apply(f.service.repository.read().unwrap().revision,Patch::Update{id:"fixture".into(),changes:serde_json::from_value(serde_json::json!({"programPath":{"mode":"set","value":f.root.path().join("missing.exe").to_string_lossy()}})).unwrap()}).unwrap();
    let mut query = f.query.clone();
    query.expected_profile_revision = f
        .service
        .repository
        .get_profile("fixture")
        .unwrap()
        .revision;
    assert_eq!(
        f.service
            .probe_cli_version(&f.caller, query)
            .unwrap()
            .reason,
        Some("program-unavailable")
    );
    assert!(!f.root.path().join("spawned").exists());
}
#[test]
fn CliVersion_CancelFencesBoundedAndDocumentScoped_015() {
    let f = Fixture::new("known", Launcher::Native);
    for _ in 0..512 {
        f.service
            .cancel_cli_version(
                &f.caller,
                CancelRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                },
            )
            .unwrap();
    }
    assert_eq!(
        f.service
            .cancel_cli_version(
                &f.caller,
                CancelRequest {
                    request_id: uuid::Uuid::new_v4().to_string()
                }
            )
            .err()
            .unwrap()
            .code,
        "REGISTRY_CAPACITY"
    );
    let caller = f.service.registry().activate_window("main").unwrap();
    let result = f
        .service
        .probe_cli_version(&caller, f.query.clone())
        .unwrap();
    assert_eq!(result.state, "known");
}

#[test]
fn CliVersion_RequestBoundaryRejectsScriptsAndInvalidIds_016() {
    assert!(serde_json::from_value::<ProbeRequest>(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"profileId":"fixture","expectedProfileRevision":"0","argv":["--eval","secret"]})).is_err());
    let f = Fixture::new("known", Launcher::Native);
    let mut query = f.query.clone();
    query.request_id = "not-a-uuid".into();
    assert_eq!(
        f.service
            .probe_cli_version(&f.caller, query)
            .unwrap_err()
            .field
            .as_deref(),
        Some("requestId")
    );
    assert!(!f.root.path().join("spawned").exists());
}
#[test]
fn CliVersion_OnlySelectedProfileRuns_017() {
    let f = Fixture::new("known", Launcher::Native);
    let mut foreign = Profile::new("foreign", CliKind::Claude);
    foreign.program_path = Override::Set(
        f.root
            .path()
            .join("foreign-missing.exe")
            .to_string_lossy()
            .into_owned(),
    );
    f.service
        .repository
        .apply(
            f.service.repository.read().unwrap().revision,
            Patch::Create { profile: foreign },
        )
        .unwrap();
    let result = f.probe();
    assert_eq!(result.cli, CliKind::Codex);
    assert_eq!(result.version.as_deref(), Some("1.2.3"));
}

#[cfg(windows)]
#[test]
fn CliVersion_CmdShimUsesFixedSafeArgv_018() {
    let runner = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("cmd.exe");
    for (name, expected) in [("version.cmd", "known"), ("unsafe&version.cmd", "unknown")] {
        let f = Fixture::new(
            "known",
            Launcher::Shim {
                runner: runner.to_string_lossy().into_owned(),
                dialect: Dialect::Cmd,
            },
        );
        let shim = f.root.path().join(name);
        std::fs::write(&shim, format!("@\"{}\" %*\r\n", executable().display())).unwrap();
        f.service.repository.apply(f.service.repository.read().unwrap().revision,Patch::Update{id:"fixture".into(),changes:serde_json::from_value(serde_json::json!({"programPath":{"mode":"set","value":shim.to_string_lossy()}})).unwrap()}).unwrap();
        let mut query = f.query.clone();
        query.expected_profile_revision = f
            .service
            .repository
            .get_profile("fixture")
            .unwrap()
            .revision;
        let result = f.service.probe_cli_version(&f.caller, query).unwrap();
        assert_eq!(result.state, expected, "{result:?}");
        if expected == "known" {
            assert_eq!(result.version.as_deref(), Some("1.2.3"));
        } else {
            assert_eq!(result.reason, Some("unsupported-launcher"));
            assert!(!f.root.path().join("spawned").exists());
        }
    }
}

// A controlled owner capability stands in for an exact child awaiting reap.
// This injects the lifecycle boundary without claiming an OS termination failure.
fn pending_owner(f: &Fixture) -> (ProfileOwner, Arc<Entry>) {
    let owner = f
        .service
        .version_probes
        .claim_profile(&f.query.profile_id)
        .unwrap();
    let entry = f
        .service
        .version_probes
        .entry(&f.service, &f.caller, &f.query.request_id)
        .unwrap();
    let mut state = entry.state.lock();
    state.request = Some(f.query.clone());
    state.started = true;
    state.result = Some(Ok(ProbeResult {
        request_id: f.query.request_id.clone(),
        profile_id: f.query.profile_id.clone(),
        profile_revision: f.query.expected_profile_revision,
        cli: CliKind::Codex,
        state: "unknown",
        version: None,
        reason: Some("termination-unknown"),
    }));
    drop(state);
    (owner, entry)
}
fn fresh_query(f: &Fixture) -> ProbeRequest {
    let mut query = f.query.clone();
    query.request_id = uuid::Uuid::new_v4().to_string();
    query
}
#[test]
fn CliVersion_PendingReapBlocksNewRequestUntilSettled_019() {
    let f = Fixture::new("known", Launcher::Native);
    let (owner, entry) = pending_owner(&f);
    assert_eq!(f.probe().reason, Some("termination-unknown"));
    assert_eq!(f.cancel().status, "requested");
    let blocked = fresh_query(&f);
    let result = f
        .service
        .probe_cli_version(&f.caller, blocked.clone())
        .unwrap();
    assert_eq!(result.reason, Some("probe-in-progress"));
    assert_eq!(result.version, None);
    assert!(!f.root.path().join("spawned").exists());
    entry.state.lock().settled = true;
    drop(owner);
    assert_eq!(f.cancel().status, "settled");
    assert_eq!(
        f.service
            .probe_cli_version(&f.caller, blocked)
            .unwrap()
            .reason,
        Some("probe-in-progress")
    );
    assert_eq!(
        f.service
            .probe_cli_version(&f.caller, fresh_query(&f))
            .unwrap()
            .state,
        "known"
    );
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("spawned")).unwrap(),
        "spawn\n"
    );
}
#[test]
fn CliVersion_RevokedPendingOwnerBlocksSameProfileOnly_020() {
    let f = Fixture::new("known", Launcher::Native);
    let (owner, entry) = pending_owner(&f);
    let caller = f.service.registry().activate_window("main").unwrap();
    assert_eq!(
        f.service
            .probe_cli_version(&caller, fresh_query(&f))
            .unwrap()
            .reason,
        Some("probe-in-progress")
    );
    assert!(!f.root.path().join("spawned").exists());
    let mut other = Profile::new("other", CliKind::Codex);
    other.program_path = Override::Set(executable().to_string_lossy().into_owned());
    f.service
        .repository
        .apply(
            f.service.repository.read().unwrap().revision,
            Patch::Create { profile: other },
        )
        .unwrap();
    let mut query = fresh_query(&f);
    query.profile_id = "other".into();
    query.expected_profile_revision = f.service.repository.get_profile("other").unwrap().revision;
    assert_eq!(
        f.service.probe_cli_version(&caller, query).unwrap().state,
        "known"
    );
    entry.state.lock().settled = true;
    drop(owner);
    assert_eq!(
        f.service
            .probe_cli_version(&caller, fresh_query(&f))
            .unwrap()
            .state,
        "known"
    );
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("spawned")).unwrap(),
        "spawn\nspawn\n"
    );
}
#[test]
fn CliVersion_RealRunningChildBlocksFreshRequest_021() {
    let f = Fixture::new("hang", Launcher::Native);
    let worker = f.start();
    f.wait_spawn();
    let result = f
        .service
        .probe_cli_version(&f.caller, fresh_query(&f))
        .unwrap();
    f.cancel();
    assert_eq!(worker.join().unwrap().unwrap().state, "cancelled");
    assert_eq!(result.reason, Some("probe-in-progress"));
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("spawned")).unwrap(),
        "spawn\n"
    );
}
