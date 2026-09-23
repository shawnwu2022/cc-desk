use crate::cli::environment::EnvMap;
use crate::cli::invocation::build_invocation;
use crate::cli::profiles::{Dialect, Launcher, Override, Profile};
use crate::cli::snapshot::{freeze_launch, CallerIdentity, FreezeContext};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, SafeError, WireU64};
use crate::platform::launch::{resolve_process, ProcessLaunchSpec};

fn resolve(
    temp: &tempfile::TempDir,
    dialect: Option<Dialect>,
    argv: Vec<String>,
    environment: &EnvMap,
) -> Result<ProcessLaunchSpec, SafeError> {
    let mut profile = Profile::new("limits", CliKind::Codex);
    profile.program_path = Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
    if let Some(dialect) = dialect {
        let runner = match dialect {
            Dialect::Cmd => std::env::var("COMSPEC").unwrap(),
            Dialect::PowerShell => crate::platform::find_executable("pwsh").unwrap(),
            Dialect::Bash => unreachable!("this fixture only needs Windows parsers"),
        };
        profile.launcher = Launcher::Shell {
            program: runner,
            dialect,
        };
    }
    let request = LaunchRequest {
        request_id: "request".into(),
        tab_id: "tab".into(),
        run_id: "run".into(),
        generation: 1,
        profile_id: profile.id.clone(),
        expected_profile_revision: profile.revision,
        cli: profile.cli,
        launch_cwd: temp.path().to_str().unwrap().into(),
        action: LaunchAction::Raw { argv },
        extra_args: Vec::new(),
        cols: 80,
        rows: 24,
    };
    let owner = CallerIdentity {
        instance_id: "test-instance".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let empty = EnvMap::new();
    let snapshot = freeze_launch(
        &request,
        &profile,
        &owner,
        &FreezeContext {
            inherited: environment,
            terminal: &empty,
            legacy: None,
            observer: None,
        },
    )?;
    resolve_process(&build_invocation(&request, &snapshot)?)
}

#[test]
fn D10_Limits_CmdLongArgRejectedBeforeSpawn_01() {
    let temp = tempfile::tempdir().unwrap();
    let error = resolve(
        &temp,
        Some(Dialect::Cmd),
        vec!["x".repeat(8192)],
        &EnvMap::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "ARG_NOT_REPRESENTABLE");
}

#[test]
fn D10_Limits_CmdLongEnvironmentCannotDisappear_02() {
    let temp = tempfile::tempdir().unwrap();
    let environment = EnvMap::from([("PRIVATE_FIXTURE_NAME".into(), "s".repeat(8192).into())]);
    let error = resolve(&temp, Some(Dialect::Cmd), Vec::new(), &environment).unwrap_err();
    assert_eq!(error.code, "ENV_NOT_REPRESENTABLE");
    assert!(!format!("{error:?}").contains("PRIVATE_FIXTURE_NAME"));
}

#[test]
fn D10_Limits_NativeLongArgRejectedBeforeSpawn_03() {
    let temp = tempfile::tempdir().unwrap();
    let error = resolve(&temp, None, vec!["x".repeat(32768)], &EnvMap::new()).unwrap_err();
    assert_eq!(error.code, "ARG_NOT_REPRESENTABLE");
}

#[test]
fn D10_Limits_PowerShellEncodedSizeIsChecked_04() {
    let temp = tempfile::tempdir().unwrap();
    let error = resolve(
        &temp,
        Some(Dialect::PowerShell),
        vec!["x".repeat(25000)],
        &EnvMap::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "ARG_NOT_REPRESENTABLE");
}

#[test]
fn D10_Limits_CmdRestrictionDoesNotTruncateNativeEnvironment_05() {
    let temp = tempfile::tempdir().unwrap();
    let value = "x".repeat(9000);
    let environment = EnvMap::from([("LARGE_FIXTURE".into(), value.clone().into())]);
    let spec = resolve(&temp, None, Vec::new(), &environment).unwrap();
    let command = spec.command().unwrap();
    assert_eq!(command.get_env("LARGE_FIXTURE"), Some(std::ffi::OsStr::new(&value)));
}
