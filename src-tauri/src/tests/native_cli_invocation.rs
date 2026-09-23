use crate::cli::environment::{EnvMap, ObserverEnv};
use crate::cli::invocation::build_invocation;
use crate::cli::profiles::{Dialect, EnvValue, Launcher, Override, Profile};
use crate::cli::snapshot::{freeze_launch, CallerIdentity, FreezeContext, LaunchSnapshot};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, ResumeScope, WireU64};
use serde_json::{json, Value};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;

const LOCATOR: &str = "11111111-2222-4333-8444-555555555555";

fn words(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

struct Fixture {
    root: tempfile::TempDir,
    profile: Profile,
    request: LaunchRequest,
    inherited: EnvMap,
    legacy: Option<Value>,
    observer: Option<ObserverEnv>,
}

impl Fixture {
    fn new(cli: CliKind, action: LaunchAction) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut profile = Profile::new("one", cli);
        profile.program_path =
            Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
        let request = LaunchRequest {
            request_id: "request-one".into(),
            tab_id: "tab-one".into(),
            run_id: "run-one".into(),
            generation: 7,
            profile_id: profile.id.clone(),
            expected_profile_revision: profile.revision,
            cli,
            launch_cwd: root.path().to_str().unwrap().into(),
            action,
            extra_args: Vec::new(),
            cols: 80,
            rows: 24,
        };
        Self {
            root,
            profile,
            request,
            inherited: EnvMap::new(),
            legacy: None,
            observer: None,
        }
    }

    fn freeze(&self) -> LaunchSnapshot {
        let owner = CallerIdentity {
            instance_id: "backend-one".into(),
            window_label: "main".into(),
            webview_epoch: WireU64::parse("3").unwrap(),
        };
        freeze_launch(
            &self.request,
            &self.profile,
            &owner,
            &FreezeContext {
                inherited: &self.inherited,
                terminal: &EnvMap::new(),
                legacy: self.legacy.as_ref(),
                observer: self.observer.as_ref(),
            },
        )
        .unwrap()
    }

    fn legacy(&mut self, args: &str) {
        self.profile.id = "legacyClaude".into();
        self.request.profile_id = "legacyClaude".into();
        self.legacy = Some(json!({"defaultCustomArgs":args}));
    }
}

fn picker(scope: ResumeScope) -> LaunchAction {
    LaunchAction::ResumePicker { scope }
}

fn resume(locator: &str) -> LaunchAction {
    LaunchAction::ResumeId {
        native_session_id: locator.into(),
    }
}

#[test]
fn D09_Golden_AllSupportedActions_001() {
    let cases = [
        (CliKind::Claude, LaunchAction::New, vec![]),
        (CliKind::Codex, LaunchAction::New, vec![]),
        (CliKind::Shell, LaunchAction::New, vec![]),
        (CliKind::Claude, picker(ResumeScope::CurrentProject), vec!["--resume"]),
        (CliKind::Codex, picker(ResumeScope::CurrentProject), vec!["resume"]),
        (CliKind::Codex, picker(ResumeScope::All), vec!["resume", "--all"]),
        (CliKind::Claude, resume(LOCATOR), vec!["--resume", LOCATOR]),
        (CliKind::Codex, resume(LOCATOR), vec!["resume", LOCATOR]),
    ];
    for (cli, action, expected) in cases {
        let fixture = Fixture::new(cli, action);
        let snapshot = fixture.freeze();
        let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
        assert_eq!(invocation.args(), words(&expected));
        assert_eq!(invocation.program(), snapshot.program());
        assert_eq!(invocation.cwd(), fixture.root.path());
        assert!(invocation.owner() == snapshot.owner());
    }
}

#[test]
fn D09_Unsupported_ShellResumeAndClaudeAll_002() {
    for (cli, action) in [
        (CliKind::Shell, resume(LOCATOR)),
        (CliKind::Shell, picker(ResumeScope::CurrentProject)),
        (CliKind::Shell, picker(ResumeScope::All)),
        (CliKind::Claude, picker(ResumeScope::All)),
    ] {
        let fixture = Fixture::new(cli, action);
        let snapshot = fixture.freeze();
        let error = build_invocation(&fixture.request, &snapshot).unwrap_err();
        assert_eq!(error.code, "UNSUPPORTED_ACTION");
    }
}

#[test]
fn D09_Raw_LosslessWithoutDeskAdditions_003() {
    let argv = [
        "--future", "a b", "", "中文", "\"", "'", "\n", "tail\\", "$HOME",
        "%TEMP%", "!x!", "^", "`", "$(echo x)", "&", "|", ">", "--", "-literal",
    ];
    for cli in [CliKind::Claude, CliKind::Codex, CliKind::Shell] {
        let mut fixture = Fixture::new(
            cli,
            LaunchAction::Raw { argv: argv.iter().map(|s| (*s).into()).collect() },
        );
        fixture.profile.default_args = Override::Set(vec!["--model".into(), "desk-default".into()]);
        fixture.profile.observer = Override::Set(true);
        if cli == CliKind::Claude {
            fixture.profile.skip_permissions = Override::Set(true);
        }
        fixture.observer = Some(ObserverEnv {
            values: EnvMap::from([("CC_BOX_SESSION_ID".into(), "observer-only".into())]),
        });
        fixture.inherited.insert("USER_OWNED".into(), "keep-me".into());
        let snapshot = fixture.freeze();
        let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
        assert_eq!(invocation.args(), words(&argv));
        assert_eq!(invocation.environment().get(OsStr::new("USER_OWNED")).unwrap(), "keep-me");
        assert!(!invocation.environment().contains_key(OsStr::new("CC_BOX_SESSION_ID")));
    }
}

#[test]
fn D09_Arguments_ExplicitDefaultsThenExtras_004() {
    for cli in [CliKind::Claude, CliKind::Codex, CliKind::Shell] {
        let mut fixture = Fixture::new(cli, LaunchAction::New);
        fixture.profile.default_args = Override::Set(vec!["--model".into(), "chosen".into()]);
        fixture.request.extra_args = vec!["--future".into(), "a b".into(), "".into(), "--".into()];
        let snapshot = fixture.freeze();
        let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
        assert_eq!(invocation.args(), words(&["--model", "chosen", "--future", "a b", "", "--"]));
    }
    let mut fixture = Fixture::new(CliKind::Codex, resume(LOCATOR));
    fixture.profile.default_args = Override::Set(vec!["--model".into(), "chosen".into()]);
    fixture.request.extra_args = vec!["--future".into()];
    let snapshot = fixture.freeze();
    assert_eq!(build_invocation(&fixture.request, &snapshot).unwrap().args(), words(&["resume", LOCATOR, "--model", "chosen", "--future"]));
}

#[test]
fn D09_Locator_RejectsOptionsAndControls_005() {
    for cli in [CliKind::Claude, CliKind::Codex] {
        for locator in ["--last", "--dangerously-skip-permissions", "-", "\nsecret", "\tsecret", "secret\r", "   "] {
            let fixture = Fixture::new(cli, resume(locator));
            let snapshot = fixture.freeze();
            let error = build_invocation(&fixture.request, &snapshot).unwrap_err();
            assert_eq!(error.code, "INVALID_REQUEST");
            assert_eq!(error.field.as_deref(), Some("action.nativeSessionId"));
            assert!(!error.to_string().contains("secret"));
        }
    }
}

#[test]
fn D09_Locator_UnicodeAndSpacesStayOneArgument_006() {
    for cli in [CliKind::Claude, CliKind::Codex] {
        let fixture = Fixture::new(cli, resume("重构 alpha & beta"));
        let snapshot = fixture.freeze();
        let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
        assert_eq!(invocation.args().len(), 2);
        assert_eq!(invocation.args()[1], "重构 alpha & beta");
        assert_eq!(invocation.requested_session_id(), Some("重构 alpha & beta"));
    }
}

#[test]
fn D09_Snapshot_RejectsChangedRequests_007() {
    let fixture = Fixture::new(CliKind::Codex, LaunchAction::New);
    let snapshot = fixture.freeze();
    let mut changes = Vec::new();
    let mut changed = fixture.request.clone();
    changed.extra_args.push("private-changed-argument".into());
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.action = resume(LOCATOR);
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.launch_cwd = "private-changed-directory".into();
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.profile_id = "other".into();
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.expected_profile_revision = WireU64::parse("1").unwrap();
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.cli = CliKind::Claude;
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.request_id = "other".into();
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.run_id = "other".into();
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.tab_id = "other".into();
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.generation += 1;
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.cols += 1;
    changes.push(changed);
    let mut changed = fixture.request.clone();
    changed.rows += 1;
    changes.push(changed);
    for changed in changes {
        let error = build_invocation(&changed, &snapshot).unwrap_err();
        assert_eq!(error.code, "REQUEST_SNAPSHOT_MISMATCH");
        assert!(!error.to_string().contains("private-changed"));
    }
}

#[test]
fn D09_Environment_UsesFrozenValuesAndDeletions_008() {
    let mut fixture = Fixture::new(CliKind::Codex, LaunchAction::New);
    fixture.inherited = EnvMap::from([
        ("KEEP".into(), "original".into()),
        ("DELETE".into(), "must-not-reappear".into()),
    ]);
    fixture.profile.env.insert("DELETE".into(), Override::Unset);
    let snapshot = fixture.freeze();
    fixture.inherited.insert("KEEP".into(), "changed".into());
    fixture.profile.env.insert("NEW".into(), Override::Set(EnvValue::Literal {
        value: "late-overlay".into(), non_secret: true,
    }));
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    assert_eq!(invocation.environment(), &EnvMap::from([("KEEP".into(), "original".into())]));
}

#[test]
fn D09_Paths_AreNotRediscoveredAfterFreeze_009() {
    let mut fixture = Fixture::new(CliKind::Codex, LaunchAction::New);
    let selected = fixture.root.path().join("chosen-agent");
    fs::write(&selected, b"fixture never executed").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fixture.profile.program_path = Override::Set(selected.to_str().unwrap().into());
    let snapshot = fixture.freeze();
    fs::remove_file(&selected).unwrap();
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    assert_eq!(invocation.program(), selected);
    assert_eq!(invocation.cwd(), fixture.root.path());
}

#[test]
fn D09_Legacy_OpaqueTextRequiresExplicitMigration_010() {
    let mut fixture = Fixture::new(CliKind::Claude, LaunchAction::New);
    fixture.legacy("--model 'private model' | private-command");
    let snapshot = fixture.freeze();
    let error = build_invocation(&fixture.request, &snapshot).unwrap_err();
    assert_eq!(error.code, "LEGACY_ARGUMENTS_REQUIRE_MIGRATION");
    assert!(!error.to_string().contains("private"));
    fixture.request.action = LaunchAction::Raw { argv: vec!["--help".into()] };
    let snapshot = fixture.freeze();
    assert_eq!(build_invocation(&fixture.request, &snapshot).unwrap().args(), words(&["--help"]));
}

#[test]
fn D09_Legacy_EmptyUnsetAndExplicitArray_011() {
    for override_value in [Override::Set(Vec::new()), Override::Unset] {
        let mut fixture = Fixture::new(CliKind::Claude, LaunchAction::New);
        fixture.legacy("private shell text");
        fixture.profile.default_args = override_value;
        let snapshot = fixture.freeze();
        assert!(build_invocation(&fixture.request, &snapshot).unwrap().args().is_empty());
    }
    let mut fixture = Fixture::new(CliKind::Claude, LaunchAction::New);
    fixture.legacy("");
    let snapshot = fixture.freeze();
    assert!(build_invocation(&fixture.request, &snapshot).unwrap().args().is_empty());
}

#[test]
fn D09_Permissions_OnlyExplicitClaudeSetting_012() {
    for setting in [Override::Inherit, Override::Unset, Override::Set(false), Override::Set(true)] {
        let mut fixture = Fixture::new(CliKind::Claude, LaunchAction::New);
        let expected = if setting == Override::Set(true) {
            words(&["--dangerously-skip-permissions"])
        } else {
            Vec::new()
        };
        fixture.profile.skip_permissions = setting;
        let snapshot = fixture.freeze();
        assert_eq!(build_invocation(&fixture.request, &snapshot).unwrap().args(), expected);
    }
    for cli in [CliKind::Codex, CliKind::Shell] {
        let mut fixture = Fixture::new(cli, LaunchAction::New);
        fixture.legacy = Some(json!({"defaultSkipPermissions":true,"defaultCustomArgs":"--model claude-only","claudeEnvVars":false}));
        let snapshot = fixture.freeze();
        assert!(build_invocation(&fixture.request, &snapshot).unwrap().args().is_empty());
    }
}

#[test]
fn D09_Identity_LocatorAndOverridesAreNotVerified_013() {
    let mut fixture = Fixture::new(CliKind::Codex, resume(LOCATOR));
    fixture.request.extra_args = vec!["--cd".into(), "opaque-other-directory".into(), "--future".into()];
    let snapshot = fixture.freeze();
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    let identity = serde_json::to_value(invocation.initial_identity()).unwrap();
    assert_eq!(identity["runId"], "run-one");
    assert_eq!(identity["generation"], 7);
    assert_eq!(identity["cli"], "codex");
    assert_eq!(identity["launchCwd"], fixture.request.launch_cwd);
    for key in ["effectiveCwd", "effectiveConfigRoot", "nativeSessionId"] {
        assert_eq!(identity[key]["state"], "unknown");
        assert!(identity[key].get("value").is_none());
    }
    assert_eq!(invocation.requested_session_id(), Some(LOCATOR));
}

#[test]
fn D09_Launcher_RunnerAndDialectStayFrozen_014() {
    let mut fixture = Fixture::new(CliKind::Codex, LaunchAction::New);
    let runner = std::env::current_exe().unwrap();
    fixture.profile.launcher = Launcher::Shim {
        runner: runner.to_str().unwrap().into(),
        dialect: Dialect::Cmd,
    };
    let expected = fixture.profile.launcher.clone();
    let snapshot = fixture.freeze();
    fixture.profile.launcher = Launcher::Native;
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    assert_eq!(invocation.launcher(), &expected);
    assert_eq!(invocation.runner(), Some(runner.as_path()));
}

#[test]
fn D09_Debug_RedactsAllLaunchValues_015() {
    let mut fixture = Fixture::new(CliKind::Codex, LaunchAction::Raw { argv: vec!["private-argument".into()] });
    fixture.inherited.insert("PRIVATE_TOKEN".into(), "private-token".into());
    let snapshot = fixture.freeze();
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    assert_eq!(format!("{invocation:?}"), "CliInvocation(<redacted>)");
}

#[test]
fn D09_Raw_EmptyDoesNotBecomeDefaultNewSession_016() {
    let mut fixture = Fixture::new(CliKind::Codex, LaunchAction::Raw { argv: Vec::new() });
    fixture.profile.default_args = Override::Set(vec!["--model".into(), "chosen".into()]);
    let snapshot = fixture.freeze();
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    assert!(invocation.args().is_empty());
    assert!(invocation.requested_session_id().is_none());
}

#[test]
fn D09_Raw_OptionLikeLocatorsRemainNativeEscapeHatch_017() {
    for cli in [CliKind::Claude, CliKind::Codex, CliKind::Shell] {
        let fixture = Fixture::new(cli, LaunchAction::Raw { argv: vec!["resume".into(), "--future-locator".into()] });
        let snapshot = fixture.freeze();
        assert_eq!(build_invocation(&fixture.request, &snapshot).unwrap().args(), words(&["resume", "--future-locator"]));
    }
}

#[cfg(unix)]
#[test]
fn D09_Environment_NonUnicodeOsValuesSurvive_018() {
    use std::os::unix::ffi::OsStringExt;
    let mut fixture = Fixture::new(CliKind::Codex, LaunchAction::New);
    fixture.inherited.insert("BYTES".into(), OsString::from_vec(vec![0xff, 0xfe]));
    let snapshot = fixture.freeze();
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    assert_eq!(invocation.environment(), &fixture.inherited);
    assert!(invocation.cwd() == Path::new(&fixture.request.launch_cwd));
}
