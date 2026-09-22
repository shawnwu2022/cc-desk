use crate::cli::availability::{get_availability, parse_request, probe_host, AvailabilityRequest};
use crate::cli::environment::EnvMap;
use crate::cli::profiles::{Dialect, EnvValue, Launcher, Override, Profile};
use crate::cli::snapshot::HostStatus;
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, WireU64};
use serde_json::json;
use std::cell::Cell;
use std::fs;
use std::path::Path;

#[cfg(windows)]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

fn revision(value: &str) -> WireU64 {
    WireU64::parse(value).unwrap()
}

fn repository(root: &Path) -> WorkspaceRepository {
    WorkspaceRepository::open(root.join("desk").join("cli-workspace.v1.json")).unwrap()
}

fn put(repository: &WorkspaceRepository, id: &str, cli: CliKind) -> Profile {
    let mut profile = Profile::new(id, cli);
    profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
    let document = repository
        .apply(repository.read().unwrap().revision, Patch::Create { profile })
        .unwrap();
    document.profiles[id].clone()
}

fn request(profile: &Profile) -> AvailabilityRequest {
    AvailabilityRequest {
        profile_id: profile.id.clone(),
        expected_revision: profile.revision,
    }
}

#[test]
fn D08_Availability_StrictSafeRequest_01() {
    let parsed = parse_request(json!({"profileId":"codex","expectedRevision":"1"})).unwrap();
    assert_eq!(parsed.profile_id, "codex");
    for value in [
        json!({"profileId":"codex","expectedRevision":1}),
        json!({"profileId":"codex","expectedRevision":"01"}),
        json!({"profileId":"codex","expectedRevision":"18446744073709551616"}),
        json!({"profileId":"../private-secret","expectedRevision":"0"}),
        json!({"profileId":"codex","expectedRevision":"1","path":"private-secret"}),
    ] {
        let error = parse_request(value).err().expect("must reject invalid input");
        assert_eq!(error.code, "INVALID_REQUEST");
        assert!(!format!("{error:?}").contains("private-secret"));
    }
}

#[test]
fn D08_Availability_UnauthorizedBeforeStorageAndHost_02() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let calls = Cell::new(0);
    let error = get_availability(
        &repo,
        "untrusted",
        &request(&Profile::new("one", CliKind::Codex)),
        &EnvMap::new(),
        || {
            calls.set(calls.get() + 1);
            HostStatus::Available
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "FORBIDDEN");
    assert_eq!(calls.get(), 0);
    assert!(!temp.path().join("desk").exists());
}

#[test]
fn D08_Availability_ProfileRevisionNotWorkspaceRevision_03() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let codex = put(&repo, "codex", CliKind::Codex);
    put(&repo, "claude", CliKind::Claude);
    assert_ne!(repo.read().unwrap().revision, codex.revision);
    let before = fs::read(temp.path().join("desk/cli-workspace.v1.json")).unwrap();
    let result = get_availability(&repo, "main", &request(&codex), &EnvMap::new(), || {
        HostStatus::Available
    })
    .unwrap();
    assert_eq!(result.profile_id, "codex");
    assert_eq!(result.availability.profile_revision, codex.revision);
    assert_eq!(result.availability.cli, CliKind::Codex);
    assert_eq!(result.availability.state, "available-unverified");
    assert!(!result.availability.certified);
    assert!(result.issue.is_none());
    assert_eq!(fs::read(temp.path().join("desk/cli-workspace.v1.json")).unwrap(), before);
}

#[test]
fn D08_Availability_StaleRevisionStopsBeforeHost_04() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let profile = put(&repo, "codex", CliKind::Codex);
    let mut old = request(&profile);
    old.expected_revision = revision("0");
    let error = get_availability(&repo, "main", &old, &EnvMap::new(), || {
        panic!("a stale request must not probe the host")
    })
    .unwrap_err();
    assert_eq!(error.code, "REVISION_CONFLICT");
}

#[test]
fn D08_Availability_CorruptLegacyDoesNotBlockCodex_05() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let codex = put(&repo, "codex", CliKind::Codex);
    let claude = put(&repo, "legacyClaude", CliKind::Claude);
    let legacy_path = temp.path().join("desk/config.json");
    fs::write(&legacy_path, "{broken-private-secret").unwrap();
    let bad = get_availability(&repo, "main", &request(&claude), &EnvMap::new(), || {
        HostStatus::Available
    })
    .unwrap();
    assert_eq!(bad.availability.state, "unavailable");
    assert_eq!(bad.issue.unwrap().code, "LEGACY_INVALID");
    let good = get_availability(&repo, "main", &request(&codex), &EnvMap::new(), || {
        HostStatus::Available
    })
    .unwrap();
    assert_eq!(good.availability.state, "available-unverified");
    assert_eq!(fs::read_to_string(legacy_path).unwrap(), "{broken-private-secret");
}

#[test]
fn D08_Availability_MissingProgramAndNoSilentPathFallback_06() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let profile = put(&repo, "codex", CliKind::Codex);
    let gone = temp.path().join("gone.exe");
    let changes = json!({"programPath":{"mode":"set","value":gone}});
    let updated = repo
        .apply(
            repo.read().unwrap().revision,
            Patch::Update {
                id: profile.id.clone(),
                changes: changes.as_object().unwrap().clone(),
            },
        )
        .unwrap();
    let host = EnvMap::from([("PATH".into(), std::env::var_os("PATH").unwrap_or_default())]);
    let result = get_availability(&repo, "main", &request(&updated.profiles["codex"]), &host, || {
        HostStatus::Available
    })
    .unwrap();
    assert_eq!(result.availability.state, "unavailable");
    assert_eq!(result.issue.unwrap().code, "PROGRAM_UNAVAILABLE");
}

#[test]
fn D08_Availability_HostFailureIsNotCliFailure_07() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let profile = put(&repo, "codex", CliKind::Codex);
    let result = get_availability(&repo, "main", &request(&profile), &EnvMap::new(), || {
        HostStatus::Unavailable
    })
    .unwrap();
    assert_eq!(result.availability.state, "available-unverified");
    assert_eq!(result.availability.host_status, HostStatus::Unavailable);
    assert!(result.issue.is_none());
    assert!(!result.availability.certified);
}

#[test]
fn D08_Availability_ResolvedSecretsNeverEnterReport_08() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let mut profile = Profile::new("secret-test", CliKind::Codex);
    profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
    profile.env.insert(
        "TOKEN".into(),
        Override::Set(EnvValue::HostRef { name: "SOURCE".into() }),
    );
    profile.default_args = Override::Set(vec!["opaque-secret-argument".into()]);
    let document = repo.apply(revision("0"), Patch::Create { profile }).unwrap();
    let inherited = EnvMap::from([("SOURCE".into(), "private-host-token".into())]);
    let result = get_availability(
        &repo,
        "main",
        &request(&document.profiles["secret-test"]),
        &inherited,
        || HostStatus::Available,
    )
    .unwrap();
    let wire = serde_json::to_string(&result).unwrap();
    for secret in ["private-host-token", "opaque-secret-argument", "SOURCE", "TOKEN"] {
        assert!(!wire.contains(secret));
    }
    let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
    assert!(value.get("env").is_none());
    assert!(value.get("programPath").is_none());
    assert!(value.get("argv").is_none());
    assert_eq!(value["profileRevision"], "1");
    assert_eq!(inherited.get(std::ffi::OsStr::new("SOURCE")).unwrap(), "private-host-token");
}

#[test]
fn D08_Availability_MissingHostReferenceIsProfileFailure_09() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let mut profile = Profile::new("missing-ref", CliKind::Codex);
    profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
    profile.env.insert(
        "TOKEN".into(),
        Override::Set(EnvValue::HostRef { name: "missing-private-source".into() }),
    );
    let document = repo.apply(revision("0"), Patch::Create { profile }).unwrap();
    let result = get_availability(
        &repo,
        "main",
        &request(&document.profiles["missing-ref"]),
        &EnvMap::new(),
        || HostStatus::Available,
    )
    .unwrap();
    assert_eq!(result.availability.state, "unavailable");
    assert_eq!(result.issue.as_ref().unwrap().code, "ENV_SOURCE_MISSING");
    assert!(!serde_json::to_string(&result).unwrap().contains("missing-private-source"));
}

#[test]
fn D08_Availability_RunnerFailureAndUnselectedProgram_10() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let mut profile = Profile::new("runner", CliKind::Codex);
    profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
    profile.launcher = Launcher::Shim {
        runner: temp.path().join("missing-runner.exe").to_str().unwrap().into(),
        dialect: Dialect::Cmd,
    };
    let document = repo.apply(revision("0"), Patch::Create { profile }).unwrap();
    let result = get_availability(
        &repo,
        "main",
        &request(&document.profiles["runner"]),
        &EnvMap::new(),
        || HostStatus::Available,
    )
    .unwrap();
    assert_eq!(result.issue.unwrap().code, "RUNNER_UNAVAILABLE");
    let document = repo
        .apply(document.revision, Patch::Create { profile: Profile::new("unselected", CliKind::Codex) })
        .unwrap();
    let result = get_availability(
        &repo,
        "main",
        &request(&document.profiles["unselected"]),
        &EnvMap::new(),
        || HostStatus::Available,
    )
    .unwrap();
    assert_eq!(result.availability.state, "configuration-required");
    assert_eq!(result.issue.unwrap().code, "PROGRAM_TRUST_REQUIRED");
}

#[test]
fn D08_Availability_RealHostProbeWithoutAgent_11() {
    #[cfg(windows)]
    bundled_runtime::initialize().unwrap();
    assert_eq!(probe_host(), HostStatus::Available);
}
