use crate::cli::environment::EnvMap;
use crate::cli::profiles::{Launcher, Override, Profile};
use crate::cli::program_discovery::{discover_programs, DiscoveryRequest};
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::CliKind;
use std::{fs, path::Path};

fn executable(path: &Path) {
    fs::write(path, "synthetic executable, must never run").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn ProgramDiscovery_FindsBothClisWithoutTrustingOrExecuting_001() {
    for cli in [CliKind::Claude, CliKind::Codex] {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let installed = root.path().join("installed tools");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&installed).unwrap();
        let name = format!(
            "{}{}",
            cli.as_str(),
            if cfg!(windows) { ".exe" } else { "" }
        );
        executable(&project.join(&name));
        executable(&installed.join(&name));
        let repo = WorkspaceRepository::open(root.path().join("desk/workspace.json")).unwrap();
        let project = crate::cli::workspace::register_project(&repo, &project).unwrap();
        let profile = Profile::new("fixture", cli);
        let before = repo
            .apply(repo.read().unwrap().revision, Patch::Create { profile })
            .unwrap();
        let request = DiscoveryRequest {
            profile_id: "fixture".into(),
            expected_revision: before.profiles["fixture"].revision,
            project_id: project.project_id,
        };
        let mut env = EnvMap::new();
        env.insert(
            "PATH".into(),
            std::env::join_paths([
                project.selected_path.as_path(),
                Path::new("."),
                installed.as_path(),
            ])
            .unwrap(),
        );
        let result = discover_programs(&repo, "main", &request, &env).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            fs::canonicalize(&result.candidates[0].program_path).unwrap(),
            fs::canonicalize(installed.join(name)).unwrap()
        );
        assert_eq!(result.candidates[0].launcher, Launcher::Native);
        assert_eq!(repo.read().unwrap().revision, before.revision);
        assert_eq!(
            repo.get_profile("fixture").unwrap().program_path,
            Override::Inherit
        );
        assert_eq!(
            discover_programs(&repo, "peer", &request, &env)
                .unwrap_err()
                .code,
            "FORBIDDEN"
        );
        let unavailable = repo.apply(before.revision, Patch::Update { id: "fixture".into(), changes:
            serde_json::json!({"programPath":{"mode":"set","value":root.path().join("missing-cli").to_str().unwrap()}}).as_object().unwrap().clone()
        }).unwrap();
        let selected = DiscoveryRequest {
            profile_id: request.profile_id.clone(),
            expected_revision: unavailable.profiles["fixture"].revision,
            project_id: request.project_id.clone(),
        };
        assert_eq!(
            discover_programs(&repo, "main", &selected, &env)
                .unwrap_err()
                .code,
            "PROGRAM_UNAVAILABLE"
        );
        let stale = DiscoveryRequest {
            expected_revision: crate::cli::types::WireU64::parse("0").unwrap(),
            ..request
        };
        assert_eq!(
            discover_programs(&repo, "main", &stale, &env)
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
    }
}

#[cfg(windows)]
#[test]
fn ProgramDiscovery_WindowsNpmShimCarriesExplicitExternalRunner_003() {
    use crate::cli::profiles::Dialect;
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let installed = root.path().join("npm");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&installed).unwrap();
    executable(&installed.join("codex.cmd"));
    executable(&installed.join("cmd.exe"));
    executable(&project.join("cmd.exe"));
    let repo = WorkspaceRepository::open(root.path().join("desk/workspace.json")).unwrap();
    let registered = crate::cli::workspace::register_project(&repo, &project).unwrap();
    let document = repo
        .apply(
            repo.read().unwrap().revision,
            Patch::Create {
                profile: Profile::new("codex", CliKind::Codex),
            },
        )
        .unwrap();
    let request = DiscoveryRequest {
        profile_id: "codex".into(),
        expected_revision: document.profiles["codex"].revision,
        project_id: registered.project_id,
    };
    let mut env = EnvMap::new();
    env.insert("PATH".into(), installed.as_os_str().into());
    env.insert("ComSpec".into(), installed.join("cmd.exe").into());
    let result = discover_programs(&repo, "main", &request, &env).unwrap();
    assert_eq!(result.candidates.len(), 1);
    let Launcher::Shim { runner, dialect } = &result.candidates[0].launcher else {
        panic!("npm shim requires its selected runner")
    };
    assert_eq!(*dialect, Dialect::Cmd);
    assert_eq!(
        fs::canonicalize(runner).unwrap(),
        fs::canonicalize(installed.join("cmd.exe")).unwrap()
    );
    assert!(!runner.starts_with(r"\\?\"));
    env.insert("ComSpec".into(), project.join("cmd.exe").into());
    assert!(discover_programs(&repo, "main", &request, &env)
        .unwrap()
        .candidates
        .is_empty());
}

#[test]
fn ProgramDiscovery_RejectsCallerSuppliedPathsAndNumericRevision_002() {
    for value in [
        serde_json::json!({"profileId":"p","expectedRevision":1,"projectId":"p"}),
        serde_json::json!({"profileId":"p","expectedRevision":"1","projectId":"p","path":"C:\\untrusted"}),
    ] {
        assert!(serde_json::from_value::<DiscoveryRequest>(value).is_err());
    }
}
