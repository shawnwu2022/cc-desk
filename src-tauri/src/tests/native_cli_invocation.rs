use crate::cli::environment::{EnvMap, ObserverEnv};
use crate::cli::invocation::build_invocation;
use crate::cli::profiles::{Dialect, EnvValue, Launcher, Override, Profile};
use crate::cli::snapshot::{freeze_launch, CallerIdentity, FreezeContext, LaunchSnapshot};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, ResumeScope, WireU64};
use serde_json::{json, Value};
use std::ffi::{OsStr, OsString};
use std::fs;

const LOCATOR: &str = "11111111-2222-4333-8444-555555555555";

// 已证明的 npm 文件入口独占 CLI tail；真实自有 Cmd/Node 输出不得把 action/hooks 交给解释器。
#[cfg(windows)]
#[test]
fn Recipe_NpmEntryOwnsActionTail_127() {
    for (action, action_words) in [
        (LaunchAction::New, vec![]),
        (resume(LOCATOR), vec!["resume", LOCATOR]),
        (picker(ResumeScope::CurrentProject), vec!["resume"]),
        (picker(ResumeScope::All), vec!["resume", "--all"]),
    ] {
        let mut f = Fixture::new(CliKind::Codex, action);
        let npm = f.root.path().join("npm");
        let package = npm.join("node_modules/@openai/codex");
        fs::create_dir_all(package.join("bin")).unwrap();
        let program = npm.join("codex.cmd");
        fs::write(
            &program,
            include_str!("../../../tests/fixtures/native-cli/codex-npm-0.163.0/codex.cmd"),
        )
        .unwrap();
        fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","version":"future-layout","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        fs::write(
            package.join("bin/codex.js"),
            "process.stdout.write(JSON.stringify(process.argv.slice(2)))",
        )
        .unwrap();
        f.profile.program_path = Override::Set(program.to_str().unwrap().into());
        f.profile.launcher = Launcher::Shim {
            runner: std::env::var("COMSPEC").unwrap(),
            dialect: Dialect::Cmd,
        };
        f.profile.default_args = Override::Set(vec!["--model".into(), "user-model".into()]);
        f.request.extra_args = vec!["two words".into(), "中文".into()];
        for key in ["PATH", "SystemRoot", "ComSpec", "PATHEXT", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                f.inherited.insert(key.into(), value);
            }
        }
        let observer = ObserverEnv {
            values: EnvMap::from([
                ("CC_DESK_CODEX_PORT".into(), "1234".into()),
                ("CC_DESK_CODEX_RUN".into(), f.request.run_id.clone().into()),
                (
                    "CC_DESK_CODEX_GENERATION".into(),
                    f.request.generation.to_string().into(),
                ),
                ("CC_DESK_CODEX_CAPABILITY".into(), "a".repeat(32).into()),
            ]),
        };
        let hooks = crate::codex_observer::hook_args(&std::env::current_exe().unwrap()).unwrap();
        let frozen = f.freeze();
        let prepared = frozen.with_codex_identity(&observer, &hooks).unwrap();
        let invocation = build_invocation(&f.request, &prepared).unwrap();
        let recipe = invocation.recipe();
        let entry = recipe
            .npm_entry()
            .expect("held forwarding proof must supply the file entry");
        assert_eq!(entry.entry(), package.join("bin/codex.js"));
        assert_eq!(entry.adjacent_interpreter(), npm.join("node.exe"));
        assert_eq!(entry.fallback_interpreter(), "node");
        assert!(entry.interpreter_prefix().is_empty());
        let mut tail = words(&action_words);
        tail.extend(words(&["--model", "user-model", "two words", "中文"]));
        assert_eq!(recipe.arguments(&f.request).unwrap(), tail);
        let expected = recipe.planned_entry_argv(&f.request).unwrap().unwrap();
        assert_eq!(
            std::path::Path::new(&expected[0]),
            package.join("bin/codex.js")
        );
        assert_eq!(&expected[expected.len() - tail.len()..], tail);
        assert_eq!(expected[1], "-c");
        let spec = crate::platform::launch::resolve_process(&invocation).unwrap();
        let output = spec.metadata_command().unwrap().output().unwrap();
        assert!(output.status.success(), "owned Cmd/Node fixture failed");
        let actual: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            actual.iter().map(OsString::from).collect::<Vec<_>>(),
            expected[1..]
        );
        let mut stale = f.request.clone();
        stale.generation += 1;
        assert_eq!(
            recipe.arguments(&stale).unwrap_err().code,
            "REQUEST_SNAPSHOT_MISMATCH"
        );
        // 未知参数 arity 与 hooks 冲突不得被配方迁移或猜测消费。
        f.profile.default_args = Override::Set(vec![
            "--future-option".into(),
            "-chooks.SessionStart=[]".into(),
        ]);
        let conflict = f.freeze();
        assert!(conflict.with_codex_identity(&observer, &hooks).is_err());
        let original = build_invocation(&f.request, &conflict).unwrap();
        assert!(original
            .recipe()
            .planned_entry_argv(&f.request)
            .unwrap()
            .is_none());
        let mut expected = words(&action_words);
        expected.extend(words(&[
            "--future-option",
            "-chooks.SessionStart=[]",
            "two words",
            "中文",
        ]));
        assert_eq!(original.args(), expected);
        // 终止符的原生语义保留；不强行投递 hooks。
        f.request.extra_args.push("--".into());
        let terminated = f.freeze();
        assert!(!terminated.codex_identity_requested());
        assert!(terminated.with_codex_identity(&observer, &hooks).is_err());
        expected.push("--".into());
        assert_eq!(
            build_invocation(&f.request, &terminated).unwrap().args(),
            expected
        );
    }
}

// 旧平面解释器和 raw 保持原 argv，并明确不提供已证明入口；冻结绑定不能跨 revision/run/action。
#[test]
fn Recipe_OpaqueAndRawKeepExactArgs_128() {
    for action in [
        resume(LOCATOR),
        LaunchAction::Raw {
            argv: vec!["--".into(), "".into(), "\n".into()],
        },
    ] {
        let mut f = Fixture::new(CliKind::Codex, action);
        f.profile.default_args = Override::Set(vec!["wrapper.py".into()]);
        f.request.extra_args = if matches!(f.request.action, LaunchAction::Raw { .. }) {
            vec![]
        } else {
            vec!["--".into(), "two words".into()]
        };
        let frozen = f.freeze();
        let invocation = build_invocation(&f.request, &frozen).unwrap();
        let recipe = invocation.recipe();
        assert!(recipe.planned_entry_argv(&f.request).unwrap().is_none());
        let expected = if matches!(f.request.action, LaunchAction::Raw { .. }) {
            words(&["--", "", "\n"])
        } else {
            words(&["resume", LOCATOR, "wrapper.py", "--", "two words"])
        };
        assert_eq!(recipe.arguments(&f.request).unwrap(), expected);
        for field in ["revision", "run", "action"] {
            let mut changed = f.request.clone();
            match field {
                "revision" => changed.expected_profile_revision = WireU64::parse("99").unwrap(),
                "run" => changed.run_id = "other-run".into(),
                _ => changed.action = LaunchAction::New,
            }
            assert_eq!(
                recipe.arguments(&changed).unwrap_err().code,
                "REQUEST_SNAPSHOT_MISMATCH"
            );
        }
        assert_eq!(invocation.program(), frozen.program());
    }
}

#[test]
fn CodexAdapter_StatusReportsPreparedIdentityWithoutVersionClaim_108() {
    let snapshot = Fixture::new(CliKind::Codex, LaunchAction::New).freeze();
    assert_eq!(
        crate::cli::observer_status::installation_reason(&snapshot, true, None),
        "OBSERVER_CONNECTING"
    );
    assert_eq!(
        crate::cli::observer_status::installation_reason(
            &snapshot,
            false,
            Some("OBSERVER_PREPARE_FAILED")
        ),
        "OBSERVER_PREPARE_FAILED"
    );
}

// 身份通道只叠加本次启动数据，用户 flags 顺序、关闭选择与 raw 完整字节保持原义。
#[test]
fn CodexIdentity_LaunchFlagsAndEnvironmentIsolation_034() {
    let mut fixture = Fixture::new(CliKind::Codex, resume(LOCATOR));
    fixture.profile.observer = Override::Set(false);
    fixture.profile.default_args = Override::Set(vec!["-c".into(), "hooks.SessionStart=[]".into()]);
    fixture.request.extra_args = vec!["--model".into(), "user-model".into()];
    fixture
        .inherited
        .insert("CC_DESK_CODEX_CAPABILITY".into(), "parent".into());
    fixture
        .inherited
        .insert("OPENAI_API_KEY".into(), "user-owned".into());
    let snapshot = fixture.freeze();
    assert!(snapshot.codex_identity_requested());
    let observer = ObserverEnv {
        values: EnvMap::from([
            ("CC_DESK_CODEX_PORT".into(), "1234".into()),
            ("CC_DESK_CODEX_RUN".into(), "run-one".into()),
            ("CC_DESK_CODEX_GENERATION".into(), "7".into()),
            ("CC_DESK_CODEX_CAPABILITY".into(), "fresh".into()),
        ]),
    };
    let prepared = snapshot
        .with_codex_identity(
            &observer,
            &words(&["-c", "hooks.SessionStart=[{hooks=[]}]"]),
        )
        .unwrap();
    let invocation = build_invocation(&fixture.request, &prepared).unwrap();
    assert_eq!(
        invocation.args(),
        words(&[
            "-c",
            "hooks.SessionStart=[{hooks=[]}]",
            "resume",
            LOCATOR,
            "-c",
            "hooks.SessionStart=[]",
            "--model",
            "user-model"
        ])
    );
    assert!(!snapshot
        .environment()
        .contains_key(OsStr::new("CC_DESK_CODEX_CAPABILITY")));
    assert_eq!(
        invocation.environment()[OsStr::new("CC_DESK_CODEX_CAPABILITY")],
        "fresh"
    );
    assert_eq!(
        invocation.environment()[OsStr::new("OPENAI_API_KEY")],
        "user-owned"
    );
    let forbidden = ObserverEnv {
        values: EnvMap::from([("CC_BOX_HOOK_PORT".into(), "1234".into())]),
    };
    assert!(snapshot.with_codex_identity(&forbidden, &[]).is_err());
    fixture.request.extra_args.push("--".into());
    assert!(!fixture.freeze().codex_identity_requested());
    for action in [
        LaunchAction::Raw {
            argv: vec!["--future".into()],
        },
        unarchive(LOCATOR),
    ] {
        let raw = Fixture::new(CliKind::Codex, action).freeze();
        assert!(!raw.codex_identity_requested());
        assert!(raw.with_codex_identity(&observer, &[]).is_err());
    }
}

fn words(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

// Node 自身不理解 Codex -c；可选身份增强不能把脚本路径前的参数交给解释器。
#[test]
fn CodexCompatibility_NodePrefixNeverReceivesHookFlags_073() {
    let mut fixture = Fixture::new(CliKind::Codex, LaunchAction::New);
    let program = fixture
        .root
        .path()
        .join(if cfg!(windows) { "node.exe" } else { "node" });
    fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    fixture.profile.program_path = Override::Set(program.to_str().unwrap().into());
    fixture.profile.default_args = Override::Set(vec!["official/codex.js".into()]);
    let snapshot = fixture.freeze();
    assert!(!snapshot.codex_identity_requested());
    assert_eq!(
        build_invocation(&fixture.request, &snapshot)
            .unwrap()
            .args(),
        words(&["official/codex.js"])
    );
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

fn unarchive(locator: &str) -> LaunchAction {
    LaunchAction::Unarchive {
        native_session_id: locator.into(),
    }
}

#[test]
fn D09_Golden_AllSupportedActions_001() {
    let cases = [
        (CliKind::Claude, LaunchAction::New, vec![]),
        (CliKind::Codex, LaunchAction::New, vec![]),
        (CliKind::Shell, LaunchAction::New, vec![]),
        (
            CliKind::Claude,
            picker(ResumeScope::CurrentProject),
            vec!["--resume"],
        ),
        (
            CliKind::Codex,
            picker(ResumeScope::CurrentProject),
            vec!["resume"],
        ),
        (
            CliKind::Codex,
            picker(ResumeScope::All),
            vec!["resume", "--all"],
        ),
        (CliKind::Claude, resume(LOCATOR), vec!["--resume", LOCATOR]),
        (CliKind::Codex, resume(LOCATOR), vec!["resume", LOCATOR]),
        // CLI 自身归档的会话必须由 CLI 取消归档：codex 拒绝 resume 归档会话。
        (
            CliKind::Codex,
            unarchive(LOCATOR),
            vec!["unarchive", LOCATOR],
        ),
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
        // 只有 codex 有会话归档语义；claude 桌面端不存在对应子命令。
        (CliKind::Claude, unarchive(LOCATOR)),
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
        "--future",
        "a b",
        "",
        "中文",
        "\"",
        "'",
        "\n",
        "tail\\",
        "$HOME",
        "%TEMP%",
        "!x!",
        "^",
        "`",
        "$(echo x)",
        "&",
        "|",
        ">",
        "--",
        "-literal",
    ];
    for cli in [CliKind::Claude, CliKind::Codex, CliKind::Shell] {
        let mut fixture = Fixture::new(
            cli,
            LaunchAction::Raw {
                argv: argv.iter().map(|s| (*s).into()).collect(),
            },
        );
        fixture.profile.default_args = Override::Set(vec!["--model".into(), "desk-default".into()]);
        fixture.profile.observer = Override::Set(true);
        if cli == CliKind::Claude {
            fixture.profile.skip_permissions = Override::Set(true);
        }
        fixture.observer = Some(ObserverEnv {
            values: EnvMap::from([("CC_BOX_SESSION_ID".into(), "observer-only".into())]),
        });
        fixture
            .inherited
            .insert("USER_OWNED".into(), "keep-me".into());
        let snapshot = fixture.freeze();
        let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
        assert_eq!(invocation.args(), words(&argv));
        assert_eq!(
            invocation
                .environment()
                .get(OsStr::new("USER_OWNED"))
                .unwrap(),
            "keep-me"
        );
        assert!(!invocation
            .environment()
            .contains_key(OsStr::new("CC_BOX_SESSION_ID")));
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
        assert_eq!(
            invocation.args(),
            words(&["--model", "chosen", "--future", "a b", "", "--"])
        );
    }
    let mut fixture = Fixture::new(CliKind::Codex, resume(LOCATOR));
    fixture.profile.default_args = Override::Set(vec!["--model".into(), "chosen".into()]);
    fixture.request.extra_args = vec!["--future".into()];
    let snapshot = fixture.freeze();
    assert_eq!(
        build_invocation(&fixture.request, &snapshot)
            .unwrap()
            .args(),
        words(&["resume", LOCATOR, "--model", "chosen", "--future"])
    );
}

#[test]
fn D09_Locator_RejectsOptionsAndControls_005() {
    for cli in [CliKind::Claude, CliKind::Codex] {
        for locator in [
            "--last",
            "--dangerously-skip-permissions",
            "-",
            "\nsecret",
            "\tsecret",
            "secret\r",
            "   ",
        ] {
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
    fixture.profile.env.insert(
        "NEW".into(),
        Override::Set(EnvValue::Literal {
            value: "late-overlay".into(),
            non_secret: true,
        }),
    );
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    assert_eq!(
        invocation.environment(),
        &EnvMap::from([("KEEP".into(), "original".into())])
    );
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
    fixture.request.action = LaunchAction::Raw {
        argv: vec!["--help".into()],
    };
    let snapshot = fixture.freeze();
    assert_eq!(
        build_invocation(&fixture.request, &snapshot)
            .unwrap()
            .args(),
        words(&["--help"])
    );
}

#[test]
fn D09_Legacy_EmptyUnsetAndExplicitArray_011() {
    for override_value in [Override::Set(Vec::new()), Override::Unset] {
        let mut fixture = Fixture::new(CliKind::Claude, LaunchAction::New);
        fixture.legacy("private shell text");
        fixture.profile.default_args = override_value;
        let snapshot = fixture.freeze();
        assert!(build_invocation(&fixture.request, &snapshot)
            .unwrap()
            .args()
            .is_empty());
    }
    let mut fixture = Fixture::new(CliKind::Claude, LaunchAction::New);
    fixture.legacy("");
    let snapshot = fixture.freeze();
    assert!(build_invocation(&fixture.request, &snapshot)
        .unwrap()
        .args()
        .is_empty());
}

#[test]
fn D09_Permissions_OnlyExplicitClaudeSetting_012() {
    for setting in [
        Override::Inherit,
        Override::Unset,
        Override::Set(false),
        Override::Set(true),
    ] {
        let mut fixture = Fixture::new(CliKind::Claude, LaunchAction::New);
        let expected = if setting == Override::Set(true) {
            words(&["--dangerously-skip-permissions"])
        } else {
            Vec::new()
        };
        fixture.profile.skip_permissions = setting;
        let snapshot = fixture.freeze();
        assert_eq!(
            build_invocation(&fixture.request, &snapshot)
                .unwrap()
                .args(),
            expected
        );
    }
    for cli in [CliKind::Codex, CliKind::Shell] {
        let mut fixture = Fixture::new(cli, LaunchAction::New);
        fixture.legacy = Some(
            json!({"defaultSkipPermissions":true,"defaultCustomArgs":"--model claude-only","claudeEnvVars":false}),
        );
        let snapshot = fixture.freeze();
        assert!(build_invocation(&fixture.request, &snapshot)
            .unwrap()
            .args()
            .is_empty());
    }
}

#[test]
fn D09_Identity_LocatorAndOverridesAreNotVerified_013() {
    let mut fixture = Fixture::new(CliKind::Codex, resume(LOCATOR));
    fixture.request.extra_args = vec![
        "--cd".into(),
        "opaque-other-directory".into(),
        "--future".into(),
    ];
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
    let mut fixture = Fixture::new(
        CliKind::Codex,
        LaunchAction::Raw {
            argv: vec!["private-argument".into()],
        },
    );
    fixture
        .inherited
        .insert("PRIVATE_TOKEN".into(), "private-token".into());
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
        let fixture = Fixture::new(
            cli,
            LaunchAction::Raw {
                argv: vec!["resume".into(), "--future-locator".into()],
            },
        );
        let snapshot = fixture.freeze();
        assert_eq!(
            build_invocation(&fixture.request, &snapshot)
                .unwrap()
                .args(),
            words(&["resume", "--future-locator"])
        );
    }
}

#[cfg(unix)]
#[test]
fn D09_Environment_NonUnicodeOsValuesSurvive_018() {
    use std::os::unix::ffi::OsStringExt;
    let mut fixture = Fixture::new(CliKind::Codex, LaunchAction::New);
    fixture
        .inherited
        .insert("BYTES".into(), OsString::from_vec(vec![0xff, 0xfe]));
    let snapshot = fixture.freeze();
    let invocation = build_invocation(&fixture.request, &snapshot).unwrap();
    assert_eq!(invocation.environment(), &fixture.inherited);
    assert!(invocation.cwd() == std::path::Path::new(&fixture.request.launch_cwd));
}

// 恢复 CLI 归档会话的请求只带身份与项目上下文，不接受 claude，也不接受被
// 当作选项注入的 locator；转换出的 LaunchRequest 必须与冻结路径完全一致。
#[test]
fn D09_UnarchiveRequest_ValidatesAndConverts_019() {
    use crate::cli::types::UnarchiveRequest;
    let request = UnarchiveRequest {
        request_id: "request-one".into(),
        cli: CliKind::Codex,
        profile_id: "profile-one".into(),
        expected_profile_revision: WireU64::parse("9").unwrap(),
        project_path: "C:\\repo".into(),
        native_session_id: LOCATOR.into(),
    };
    request.validate().unwrap();
    let launch = request.to_launch_request();
    assert_eq!(launch.cli, CliKind::Codex);
    assert_eq!(launch.launch_cwd, "C:\\repo");
    assert_eq!(
        launch.expected_profile_revision,
        request.expected_profile_revision
    );
    assert_eq!(launch.action, unarchive(LOCATOR));
    assert!(launch.extra_args.is_empty());
    launch.validate().unwrap();
    // 一次性命令没有 tab/run 归属，但形状必须与启动请求等价。
    assert_eq!(launch.request_id, request.request_id);
    assert_ne!(launch.tab_id, "");
    assert_ne!(launch.run_id, "");

    let claude = UnarchiveRequest {
        cli: CliKind::Claude,
        ..request.clone()
    };
    assert_eq!(claude.validate().unwrap_err().code, "INVALID_REQUEST");
    assert_eq!(claude.validate().unwrap_err().field.as_deref(), Some("cli"));

    for (field, value) in [
        ("requestId", ""),
        ("profileId", ""),
        ("projectPath", ""),
        ("nativeSessionId", ""),
    ] {
        let mut broken = request.clone();
        match field {
            "requestId" => broken.request_id = value.into(),
            "profileId" => broken.profile_id = value.into(),
            "projectPath" => broken.project_path = value.into(),
            _ => broken.native_session_id = value.into(),
        }
        let failure = broken.validate().unwrap_err();
        assert_eq!(failure.code, "INVALID_REQUEST");
        assert_eq!(failure.field.as_deref(), Some(field));
    }

    // 选项形式或含控制字符的 locator 仍被拒绝，且错误不回声原文。
    for locator in ["--last", "\nsecret"] {
        let mut broken = request.clone();
        broken.native_session_id = locator.into();
        broken.validate().unwrap();
        let fixture = Fixture::new(CliKind::Codex, unarchive(locator));
        let snapshot = fixture.freeze();
        let failure = build_invocation(&fixture.request, &snapshot).unwrap_err();
        assert_eq!(failure.code, "INVALID_REQUEST");
        assert_eq!(failure.field.as_deref(), Some("action.nativeSessionId"));
    }
}

// opencode：结构化 New 必须先由官方 API 预建；resume-id 是 -s <id>；picker/unarchive 拒绝。
// Raw 不进断言矩阵：raw_args() 在客户端分发前整体接管（实测不可达），
// opencode.rs 的 Raw arm 是与 claude.rs 同款的纵深防御，行为见 RawPassesThrough。
#[test]
fn Opencode_ArgvMappingFollowsSpecMatrix_01() {
    let f = Fixture::new(CliKind::Opencode, LaunchAction::New);
    let snapshot = f.freeze();
    let invocation = build_invocation(&f.request, &snapshot).unwrap();
    assert_eq!(invocation.args(), words(&[]).as_slice());

    let f = Fixture::new(
        CliKind::Opencode,
        LaunchAction::ResumeId {
            native_session_id: LOCATOR.into(),
        },
    );
    let snapshot = f.freeze();
    let invocation = build_invocation(&f.request, &snapshot).unwrap();
    assert_eq!(invocation.args(), words(&["-s", LOCATOR]).as_slice());

    for action in [
        LaunchAction::ResumePicker {
            scope: ResumeScope::CurrentProject,
        },
        LaunchAction::ResumePicker {
            scope: ResumeScope::All,
        },
        LaunchAction::Unarchive {
            native_session_id: LOCATOR.into(),
        },
    ] {
        let f = Fixture::new(CliKind::Opencode, action);
        let snapshot = f.freeze();
        let err = build_invocation(&f.request, &snapshot).unwrap_err();
        assert_eq!(err.code, "UNSUPPORTED_ACTION");
    }
}

#[test]
fn Opencode_PreparationFallbackPreservesNativeArguments_092() {
    let mut f = Fixture::new(CliKind::Opencode, LaunchAction::New);
    f.profile.default_args = Override::Set(vec![
        "--model".into(),
        "provider/model".into(),
        "--future".into(),
    ]);
    f.request.extra_args = vec!["exact value".into()];
    let snapshot = f.freeze();
    let invocation = build_invocation(&f.request, &snapshot).unwrap();
    assert_eq!(
        invocation.args(),
        words(&["--model", "provider/model", "--future", "exact value"]).as_slice()
    );
    assert!(invocation.requested_session_id().is_none());
}

// Raw 走公共 builder 透传（raw_args 提前返回），这是真实契约而非拒绝。
#[test]
fn Opencode_RawPassesThroughCommonBuilder_02() {
    let f = Fixture::new(
        CliKind::Opencode,
        LaunchAction::Raw {
            argv: vec!["--mini".into(), "--pure".into()],
        },
    );
    let snapshot = f.freeze();
    let invocation = build_invocation(&f.request, &snapshot).unwrap();
    assert_eq!(invocation.args(), words(&["--mini", "--pure"]).as_slice());
}

// locator 校验沿用公共 validate_locator：flag 形状、空白、控制字符都拒绝。
#[test]
fn Opencode_ResumeIdRejectsFlagShapedLocator_03() {
    let f = Fixture::new(
        CliKind::Opencode,
        LaunchAction::ResumeId {
            native_session_id: "--fork".into(),
        },
    );
    let snapshot = f.freeze();
    let err = build_invocation(&f.request, &snapshot).unwrap_err();
    assert_eq!(err.code, "INVALID_REQUEST");
}

// spec §6.2：opencode resume 的 effective cwd 必须保持 Unknown；Launch 来源不存在任何 Known。
// RunPublicIdentity 字段私有，经 serde 断言（derive 已实现 Serialize）。
#[test]
fn Opencode_InitialIdentityKeepsEffectiveCwdUnknown_04() {
    let f = Fixture::new(
        CliKind::Opencode,
        LaunchAction::ResumeId {
            native_session_id: LOCATOR.into(),
        },
    );
    let snapshot = f.freeze();
    let invocation = build_invocation(&f.request, &snapshot).unwrap();
    assert_eq!(invocation.requested_session_id(), Some(LOCATOR));
    let identity = serde_json::to_value(invocation.initial_identity()).unwrap();
    assert_eq!(identity["effectiveCwd"]["state"], "unknown");
    assert_eq!(identity["effectiveConfigRoot"]["state"], "unknown");
    assert_eq!(identity["nativeSessionId"]["state"], "unknown");
}

#[cfg(windows)]
#[test]
fn CodexIdentity_CmdNpmForwardsHooksOutsideCall_047() {
    codex_cmd_npm_forwards_hooks("0.163.0", false);
}

#[cfg(windows)]
#[test]
fn CodexIdentity_CmdNpm164ForwardsHooksOutsideCall_082() {
    codex_cmd_npm_forwards_hooks("0.164.0", false);
}

#[cfg(windows)]
#[test]
fn CodexAdapter_UserSessionHooksSurviveSameRunTransport_104() {
    codex_cmd_npm_forwards_hooks("future-protocol-compatible", true);
}

#[cfg(windows)]
fn codex_cmd_npm_forwards_hooks(version: &str, user_hooks: bool) {
    // CALL仍只接收用户安全参数；TOML由已识别的Node入口转发给原生CLI。
    let mut f = Fixture::new(CliKind::Codex, resume(LOCATOR));
    // 宿主 TEMP 可能带短名或 junction；成功场景显式选直接路径，别名拒绝仍在下方验证。
    let canonical_root = fs::canonicalize(f.root.path()).unwrap();
    let direct_root = canonical_root
        .to_str()
        .unwrap()
        .strip_prefix(r"\\?\")
        .unwrap();
    let npm = std::path::Path::new(direct_root).join("中文 空格");
    let package = npm.join("node_modules/@openai/codex");
    fs::create_dir_all(package.join("bin")).unwrap();
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/native-cli/codex-npm-0.163.0");
    fs::copy(fixtures.join("codex.cmd"), npm.join("codex.cmd")).unwrap();
    // Owned normal-file Node fixture; never executes an installed CLI/core.
    let script = "process.stdout.write(JSON.stringify({argv:process.argv.slice(2),options:process.env.NODE_OPTIONS??null,transport:process.env.CC_DESK_CODEX_ARGV??null,capability:process.env.CC_DESK_CODEX_CAPABILITY??null}))";
    fs::write(package.join("bin/codex.js"), script).unwrap();
    fs::write(npm.join("wrong.js"), script).unwrap();
    fs::write(
        package.join("package.json"),
        serde_json::to_vec(
            &json!({"name":"@openai/codex","version":version,"bin":{"codex":"bin/codex.js"}}),
        )
        .unwrap(),
    )
    .unwrap();
    f.profile.program_path = Override::Set(npm.join("codex.cmd").to_str().unwrap().into());
    f.profile.launcher = Launcher::Shim {
        runner: std::env::var("COMSPEC").unwrap(),
        dialect: Dialect::Cmd,
    };
    f.profile.default_args = Override::Set(vec!["--model".into(), "user-model".into()]);
    if user_hooks {
        f.profile.default_args = Override::Set(vec![
            "--model".into(),
            "user-model".into(),
            "-c".into(),
            r#"hooks.SessionStart=[{hooks=[{type="command",command="user-owned",timeout=1}]}]"#
                .into(),
            "-c".into(),
            r#"hooks.state={user-key={enabled=false,trusted_hash="user-trust"}}"#.into(),
        ]);
    }
    f.inherited
        .insert("cc_desk_codex_argv".into(), "ancestor-transport".into());
    // Windows Node 的系统随机源初始化需要 SystemRoot；不是待测参数层的一部分。
    f.inherited
        .insert("SystemRoot".into(), std::env::var_os("SystemRoot").unwrap());
    let original = f.freeze();
    let observer = ObserverEnv {
        values: EnvMap::from([
            ("CC_DESK_CODEX_PORT".into(), "1234".into()),
            ("CC_DESK_CODEX_RUN".into(), "run-one".into()),
            ("CC_DESK_CODEX_GENERATION".into(), "7".into()),
            (
                "CC_DESK_CODEX_CAPABILITY".into(),
                "0123456789abcdef0123456789abcdef".into(),
            ),
        ]),
    };
    let hooks = crate::codex_observer::hook_args(std::path::Path::new(
        r"C:\中文 空格 & (Desk)\cc-desk.exe",
    ))
    .unwrap();
    let prepared = original.with_codex_identity(&observer, &hooks).unwrap();
    if user_hooks {
        let envelope: Value = serde_json::from_str(
            prepared.environment()[OsStr::new("CC_DESK_CODEX_ARGV")]
                .to_str()
                .unwrap(),
        )
        .unwrap();
        let merged_arg = envelope["args"][1].as_str().unwrap();
        assert!(
            merged_arg.starts_with("hooks="),
            "same-layer user hooks must be combined with the Desk handler before transport"
        );
        let merged = merged_arg.strip_prefix("hooks=").unwrap();
        let document: toml::Table = format!("hooks={merged}").parse().unwrap();
        let groups = document["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups[0]["hooks"][0]["command"].as_str(),
            Some("user-owned")
        );
        let original_group: toml::Table = format!(
            "value={}",
            hooks[1]
                .to_str()
                .unwrap()
                .strip_prefix("hooks.SessionStart=")
                .unwrap()
        )
        .parse()
        .unwrap();
        assert_eq!(
            &groups[1], &original_group["value"][0],
            "append the exact Desk handler, including encoded safe paths"
        );
        assert_eq!(
            document["hooks"]["state"]["user-key"]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            document["hooks"]["state"]["user-key"]["trusted_hash"].as_str(),
            Some("user-trust")
        );
        assert!(document["hooks"]["state"]
            .as_table()
            .unwrap()
            .keys()
            .any(|key| key.ends_with(":session_start:1:0")));
    }
    let invocation = build_invocation(&f.request, &prepared).unwrap();
    assert!(
        crate::platform::launch::resolve_process(&invocation).is_ok(),
        "official cmd must keep hooks without placing TOML in CALL"
    );
    assert_eq!(
        invocation.args(),
        words(&["resume", LOCATOR, "--model", "user-model"])
    );
    assert!(!original.environment().keys().any(|k| k
        .to_string_lossy()
        .eq_ignore_ascii_case("CC_DESK_CODEX_ARGV")));
    let node = crate::platform::find_executable("node").unwrap();
    let output = std::process::Command::new(&node)
        .env_clear()
        .envs(prepared.environment())
        .arg(package.join("bin/codex.js"))
        .args(invocation.args())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "node preload probe: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed: Value = serde_json::from_slice(&output.stdout).unwrap();
    let envelope: Value = serde_json::from_str(
        prepared.environment()[OsStr::new("CC_DESK_CODEX_ARGV")]
            .to_str()
            .unwrap(),
    )
    .unwrap();
    let mut expected = envelope["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    expected.extend(
        invocation
            .args()
            .iter()
            .map(|v| v.to_str().unwrap().to_owned()),
    );
    assert_eq!(observed["argv"], json!(expected));
    assert!(observed["options"].is_null());
    assert!(observed["transport"].is_null());
    let read_back = |environment: &EnvMap, main: &std::path::Path| -> Value {
        let output = std::process::Command::new(&node)
            .env_clear()
            .envs(environment)
            .arg(main)
            .args(invocation.args())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixed Node transport probe must finish"
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    // 错入口和损坏载荷都必须还原环境、保留原参数并移除观察能力。
    for (wrong_entry, corrupt_payload, missing_binding) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let mut environment = prepared.environment().clone();
        if corrupt_payload {
            environment.insert("CC_DESK_CODEX_ARGV".into(), "not-json".into());
        }
        if missing_binding {
            environment.remove(OsStr::new("CC_DESK_CODEX_GENERATION"));
        }
        let main = if wrong_entry {
            npm.join("wrong.js")
        } else {
            package.join("bin/codex.js")
        };
        if user_hooks && wrong_entry {
            let output = std::process::Command::new(&node)
                .env_clear()
                .envs(&environment)
                .arg(&main)
                .args(invocation.args())
                .output()
                .unwrap();
            assert!(
                !output.status.success(),
                "never execute an unbound entry after removing user hook policy"
            );
            assert!(
                output.stdout.is_empty(),
                "wrong entry marker must not execute"
            );
            continue;
        }
        let result = read_back(&environment, &main);
        if user_hooks {
            assert_eq!(
                result["argv"],
                json!([
                    "resume",
                    LOCATOR,
                    "--model",
                    "user-model",
                    "-c",
                    r#"hooks.SessionStart=[{hooks=[{type="command",command="user-owned",timeout=1}]}]"#,
                    "-c",
                    r#"hooks.state={user-key={enabled=false,trusted_hash="user-trust"}}"#
                ]),
                "runtime enhancement failure must restore every original user policy argument"
            );
        } else {
            assert_eq!(
                result["argv"],
                json!(["resume", LOCATOR, "--model", "user-model"])
            );
        }
        assert!(result["options"].is_null());
        assert!(result["transport"].is_null());
        assert!(result["capability"].is_null());
    }
    f.inherited.insert("NODE_OPTIONS".into(), "".into());
    let empty = f.freeze().with_codex_identity(&observer, &hooks).unwrap();
    let empty_result = read_back(empty.environment(), &package.join("bin/codex.js"));
    assert_eq!(empty_result["options"], "");
    assert_eq!(empty_result["argv"], json!(expected));
    drop(empty);
    f.inherited.remove(OsStr::new("NODE_OPTIONS"));
    // 只转移后端 hooks；用户危险参数仍经过原 CALL 校验，过长传输仍经过 env 上限。
    f.request.extra_args.push("unsafe&marker".into());
    let unsafe_snapshot = f.freeze().with_codex_identity(&observer, &hooks).unwrap();
    let unsafe_invocation = build_invocation(&f.request, &unsafe_snapshot).unwrap();
    assert_eq!(
        crate::platform::launch::resolve_process(&unsafe_invocation)
            .unwrap_err()
            .code,
        "ARG_NOT_REPRESENTABLE"
    );
    drop(unsafe_invocation);
    drop(unsafe_snapshot);
    f.request.extra_args.clear();
    let mut oversized = hooks.clone();
    oversized[1] = format!(
        "hooks.SessionStart=[{{hooks=[{{type=\"command\",command=\"{}\"}}]}}]",
        "x".repeat(9000)
    )
    .into();
    let oversized = f
        .freeze()
        .with_codex_identity(&observer, &oversized)
        .unwrap();
    assert_eq!(
        crate::platform::launch::resolve_process(
            &build_invocation(&f.request, &oversized).unwrap()
        )
        .unwrap_err()
        .code,
        "ENV_NOT_REPRESENTABLE"
    );
    drop(oversized);
    let options = prepared.environment()[OsStr::new("NODE_OPTIONS")]
        .to_str()
        .unwrap();
    let loader = std::path::PathBuf::from(
        options
            .strip_prefix("--require \"")
            .unwrap()
            .strip_suffix('"')
            .unwrap(),
    );
    assert!(
        fs::write(&loader, "tamper").is_err(),
        "preload stays immutable while owned snapshot is retained"
    );
    assert!(fs::remove_file(&loader).is_err());
    assert!(fs::write(package.join("bin/codex.js"), "tamper").is_err());
    drop(invocation);
    drop(prepared);
    assert!(
        !loader.exists(),
        "owned snapshot releases its temporary artifact"
    );
    let alias = std::path::Path::new(direct_root).join("npm-junction");
    let link = std::process::Command::new(std::env::var_os("COMSPEC").unwrap())
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&alias)
        .arg(&npm)
        .output()
        .unwrap();
    assert!(link.status.success(), "junction fixture must be created");
    f.profile.program_path = Override::Set(alias.join("codex.cmd").to_str().unwrap().into());
    assert!(
        f.freeze().with_codex_identity(&observer, &hooks).is_err(),
        "a replaceable lexical junction must retain ordinary launch without the transport"
    );
    f.profile.program_path = Override::Set(npm.join("codex.cmd").to_str().unwrap().into());
    f.inherited
        .insert("NODE_OPTIONS".into(), "--permission".into());
    assert!(
        f.freeze().with_codex_identity(&observer, &hooks).is_err(),
        "pre-Node permission failures must preserve baseline launch"
    );
    f.inherited.remove(OsStr::new("NODE_OPTIONS"));
    fs::write(npm.join("codex.cmd"), "@echo custom").unwrap();
    assert!(
        f.freeze().with_codex_identity(&observer, &hooks).is_err(),
        "do not adapt a user-custom launcher"
    );
}
