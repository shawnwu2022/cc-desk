use crate::cli::environment::{EnvMap, ObserverEnv};
use crate::cli::profiles::{EnvValue, Override, Profile};
use crate::cli::snapshot::{availability, discover_candidates, freeze_launch, CallerIdentity, FreezeContext, HostStatus};
use crate::cli::types::{CliKind, LaunchRequest, WireU64};
use serde_json::{json, Value};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;

fn revision(value: &str) -> WireU64 { WireU64::parse(value).unwrap() }

fn caller() -> CallerIdentity {
    CallerIdentity { instance_id: "backend-instance".into(), window_label: "main".into(), webview_epoch: revision("7") }
}

fn profile() -> Profile {
    let mut profile = Profile::new("one", CliKind::Codex);
    profile.revision = revision("1");
    profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
    profile
}

fn request(profile: &Profile, cwd: &Path, changes: Value) -> LaunchRequest {
    let mut value = json!({"requestId":"request","tabId":"tab","runId":"run","generation":2,"profileId":profile.id,"expectedProfileRevision":profile.revision,"cli":profile.cli,"launchCwd":cwd.to_str().unwrap(),"action":{"kind":"new"},"extraArgs":[],"cols":80,"rows":24});
    for (key, change) in changes.as_object().unwrap() { value[key] = change.clone(); }
    serde_json::from_value(value).unwrap()
}

#[test]
fn D08_Snapshot_FreezesProfileEnvironmentAndOwner_01() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = profile();
    let mut inherited = EnvMap::from([("PRIVATE_HOST_VALUE".into(), "original-secret".into())]);
    let terminal = EnvMap::new();
    let owner = caller();
    let snapshot = freeze_launch(&request(&profile, temp.path(), json!({})), &profile, &owner, &FreezeContext { inherited: &inherited, terminal: &terminal, legacy: None, observer: None }).unwrap();
    profile.revision = revision("2");
    profile.program_path = Override::Set("changed".into());
    inherited.insert("PRIVATE_HOST_VALUE".into(), "changed-secret".into());
    assert_eq!(snapshot.profile_revision(), revision("1"));
    assert_eq!(snapshot.environment().get(OsStr::new("PRIVATE_HOST_VALUE")).unwrap(), "original-secret");
    assert!(snapshot.owner() == &owner);
    assert!(snapshot.program().is_absolute());
    assert!(snapshot.default_args().is_empty());
    assert!(!format!("{snapshot:?}").contains("original-secret"));
    let wire = serde_json::to_string(&availability(&snapshot, HostStatus::NotChecked)).unwrap();
    assert!(!wire.contains("PRIVATE_HOST_VALUE"));
    assert!(!wire.contains("original-secret"));
}

#[test]
fn D08_Snapshot_RejectsWrongRevisionAndCliBeforeResolution_02() {
    let temp = tempfile::tempdir().unwrap();
    let profile = profile();
    let empty = EnvMap::new();
    let context = FreezeContext { inherited: &empty, terminal: &empty, legacy: None, observer: None };
    let old = request(&profile, temp.path(), json!({"expectedProfileRevision":"0"}));
    assert_eq!(freeze_launch(&old, &profile, &caller(), &context).unwrap_err().code, "REVISION_CONFLICT");
    let wrong = request(&profile, temp.path(), json!({"cli":"claude"}));
    assert_eq!(freeze_launch(&wrong, &profile, &caller(), &context).unwrap_err().code, "PROFILE_CLI_MISMATCH");
    let wrong = request(&profile, temp.path(), json!({"profileId":"other"}));
    assert_eq!(freeze_launch(&wrong, &profile, &caller(), &context).unwrap_err().code, "PROFILE_MISMATCH");
}

#[test]
fn D08_Snapshot_RejectsUntrustedCallerAndInvalidCwd_03() {
    let temp = tempfile::tempdir().unwrap();
    let profile = profile();
    let empty = EnvMap::new();
    let context = FreezeContext { inherited: &empty, terminal: &empty, legacy: None, observer: None };
    let mut owner = caller();
    owner.window_label = "untrusted".into();
    assert_eq!(freeze_launch(&request(&profile, temp.path(), json!({})), &profile, &owner, &context).unwrap_err().code, "FORBIDDEN");
    assert_eq!(freeze_launch(&request(&profile, temp.path(), json!({"launchCwd":"relative"})), &profile, &caller(), &context).unwrap_err().code, "INVALID_REQUEST");
}

#[test]
fn D08_Snapshot_LockedMissingProgramNeverSearchesAgain_04() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = profile();
    profile.program_path = Override::Set(temp.path().join("gone.exe").to_str().unwrap().into());
    let inherited = EnvMap::from([("PATH".into(), std::env::current_exe().unwrap().parent().unwrap().as_os_str().to_owned())]);
    let empty = EnvMap::new();
    let context = FreezeContext { inherited: &inherited, terminal: &empty, legacy: None, observer: None };
    assert_eq!(freeze_launch(&request(&profile, temp.path(), json!({})), &profile, &caller(), &context).unwrap_err().code, "PROGRAM_UNAVAILABLE");
    profile.program_path = Override::Inherit;
    assert_eq!(freeze_launch(&request(&profile, temp.path(), json!({})), &profile, &caller(), &context).unwrap_err().code, "PROGRAM_TRUST_REQUIRED");
}

#[test]
fn D08_Snapshot_RawPreservesArgumentsAndSuppressesDeskDefaults_05() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = profile();
    profile.cli = CliKind::Claude;
    profile.observer = Override::Set(true);
    profile.default_args = Override::Set(vec!["--model".into(), "desk-default".into()]);
    profile.env.insert("PROFILE_VALUE".into(), Override::Set(EnvValue::Literal { value: "retained".into(), non_secret: true }));
    let empty = EnvMap::new();
    let observer = ObserverEnv { values: EnvMap::from([("CC_BOX_SESSION_ID".into(), "observer-only".into())]) };
    let context = FreezeContext { inherited: &empty, terminal: &empty, legacy: None, observer: Some(&observer) };
    let args = vec!["--future", "a b", "", "|", "$(echo x)", "中文", "tail\\", "--"];
    let snapshot = freeze_launch(&request(&profile, temp.path(), json!({"action":{"kind":"raw","argv":args}})), &profile, &caller(), &context).unwrap();
    assert_eq!(snapshot.raw_args().unwrap(), args.iter().map(OsString::from).collect::<Vec<_>>());
    assert!(snapshot.default_args().is_empty());
    assert!(snapshot.extra_args().is_empty());
    assert_eq!(snapshot.environment().get(OsStr::new("PROFILE_VALUE")).unwrap(), "retained");
    assert!(!snapshot.environment().contains_key(OsStr::new("CC_BOX_SESSION_ID")));
}

#[test]
fn D08_Snapshot_UnknownExtraArgumentsAreNotFiltered_06() {
    let temp = tempfile::tempdir().unwrap();
    let profile = profile();
    let empty = EnvMap::new();
    let context = FreezeContext { inherited: &empty, terminal: &empty, legacy: Some(&json!({"claudeEnvVars":false})), observer: None };
    let snapshot = freeze_launch(&request(&profile, temp.path(), json!({"extraArgs":["--future-option","opaque secret",""]})), &profile, &caller(), &context).unwrap();
    assert_eq!(snapshot.extra_args(), [OsString::from("--future-option"), OsString::from("opaque secret"), OsString::from("")]);
    assert!(!format!("{snapshot:?}").contains("opaque secret"));
}

#[test]
fn D08_Snapshot_AvailabilityUsesFrozenPathAndSeparatesHost_07() {
    let temp = tempfile::tempdir().unwrap();
    let profile = profile();
    let empty = EnvMap::new();
    let snapshot = freeze_launch(&request(&profile, temp.path(), json!({})), &profile, &caller(), &FreezeContext { inherited: &empty, terminal: &empty, legacy: None, observer: None }).unwrap();
    let result = availability(&snapshot, HostStatus::Unavailable);
    assert_eq!(result.state, "available-unverified");
    assert_eq!(result.host_status, HostStatus::Unavailable);
    assert!(!result.certified);
}

#[test]
fn D08_Snapshot_DiscoverySkipsProjectCandidatesAndNeverExecutes_08() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let trusted_dir = temp.path().join("external");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&trusted_dir).unwrap();
    let name = if cfg!(windows) { "codex.exe" } else { "codex" };
    let script = b"#!/bin/sh\ntouch should-never-exist\n";
    fs::write(project.join(name), script).unwrap();
    fs::write(trusted_dir.join(name), script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(project.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(trusted_dir.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = std::env::join_paths([project.as_path(), Path::new("."), trusted_dir.as_path()]).unwrap();
    let candidates = discover_candidates("codex", &EnvMap::from([("PATH".into(), path)]), &[project.clone()]).unwrap();
    assert_eq!(candidates, vec![fs::canonicalize(trusted_dir.join(name)).unwrap()]);
    assert!(!project.join("should-never-exist").exists());
    assert!(!trusted_dir.join("should-never-exist").exists());
}
