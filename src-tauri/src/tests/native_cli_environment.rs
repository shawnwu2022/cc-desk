use crate::cli::environment::{build_environment, EnvMap, ObserverEnv};
use crate::cli::profiles::{EnvValue, Override, Profile};
use crate::cli::types::CliKind;
use serde_json::json;
use std::ffi::OsStr;

fn env(values: &[(&str, &str)]) -> EnvMap {
    values
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect()
}

fn literal(value: &str) -> Override<EnvValue> {
    Override::Set(EnvValue::Literal {
        value: value.into(),
        non_secret: true,
    })
}

#[test]
fn D08_Environment_NoCrossProfileLeak_01() {
    let inherited = env(&[
        ("OPENAI_API_KEY", "host-key"),
        ("ANTHROPIC_API_KEY", "user-key"),
    ]);
    let mut claude = Profile::new("claude-one", CliKind::Claude);
    claude
        .env
        .insert("ANTHROPIC_BASE_URL".into(), literal("profile-only"));
    let codex = Profile::new("codex-one", CliKind::Codex);
    let a = build_environment(&inherited, &EnvMap::new(), &claude, None, None).unwrap();
    let b = build_environment(&inherited, &EnvMap::new(), &codex, None, None).unwrap();
    assert_eq!(
        a.get(OsStr::new("ANTHROPIC_BASE_URL")).unwrap(),
        "profile-only"
    );
    assert!(!b.contains_key(OsStr::new("ANTHROPIC_BASE_URL")));
    assert_eq!(b, inherited);
}

#[test]
fn D08_Environment_EmptyUnsetAndPrecedence_02() {
    let inherited = env(&[("TERM", "old"), ("REMOVE_ME", "host"), ("EMPTY", "old")]);
    let mut profile = Profile::new("one", CliKind::Codex);
    profile.env.insert("TERM".into(), literal("profile-term"));
    profile.env.insert("REMOVE_ME".into(), Override::Unset);
    profile.env.insert("EMPTY".into(), literal(""));
    let result = build_environment(
        &inherited,
        &env(&[("TERM", "terminal")]),
        &profile,
        None,
        None,
    )
    .unwrap();
    assert_eq!(result.get(OsStr::new("TERM")).unwrap(), "profile-term");
    assert_eq!(result.get(OsStr::new("EMPTY")).unwrap(), "");
    assert!(!result.contains_key(OsStr::new("REMOVE_ME")));
    assert_eq!(inherited.get(OsStr::new("REMOVE_ME")).unwrap(), "host");
}

#[test]
fn D08_Environment_HostReferenceUsesOriginalHost_03() {
    let inherited = env(&[("SOURCE", "original")]);
    let mut profile = Profile::new("one", CliKind::Codex);
    profile.env.insert("SOURCE".into(), literal("replacement"));
    profile.env.insert(
        "TARGET".into(),
        Override::Set(EnvValue::HostRef {
            name: "SOURCE".into(),
        }),
    );
    let result = build_environment(&inherited, &EnvMap::new(), &profile, None, None).unwrap();
    assert_eq!(result.get(OsStr::new("TARGET")).unwrap(), "original");
    assert_eq!(result.get(OsStr::new("SOURCE")).unwrap(), "replacement");
}

#[test]
fn D08_Environment_MissingReferenceIsSafe_04() {
    let mut profile = Profile::new("one", CliKind::Codex);
    profile.env.insert(
        "TARGET".into(),
        Override::Set(EnvValue::HostRef {
            name: "sensitive-source".into(),
        }),
    );
    let error =
        build_environment(&EnvMap::new(), &EnvMap::new(), &profile, None, None).unwrap_err();
    assert_eq!(error.code, "ENV_SOURCE_MISSING");
    assert!(!format!("{error:?}").contains("sensitive-source"));
}

#[test]
fn D08_Environment_LegacyOnlyAndExplicitOverride_05() {
    let legacy = json!({"claudeEnvVars": {"LEGACY": "legacy-only", "EMPTY": "legacy"}});
    let mut old = Profile::new("legacyClaude", CliKind::Claude);
    old.env.insert("EMPTY".into(), literal(""));
    let a = build_environment(&EnvMap::new(), &EnvMap::new(), &old, Some(&legacy), None).unwrap();
    assert_eq!(a.get(OsStr::new("LEGACY")).unwrap(), "legacy-only");
    assert_eq!(a.get(OsStr::new("EMPTY")).unwrap(), "");
    for cli in [CliKind::Codex, CliKind::Claude, CliKind::Shell] {
        let new = Profile::new("independent", cli);
        let b = build_environment(
            &EnvMap::new(),
            &EnvMap::new(),
            &new,
            Some(&json!({"claudeEnvVars": false})),
            None,
        )
        .unwrap();
        assert!(b.is_empty());
    }
}

#[test]
fn D08_Environment_ObserverIsOptInAndMinimal_06() {
    let observer = ObserverEnv {
        values: env(&[("CC_BOX_SESSION_ID", "run-only")]),
    };
    let mut profile = Profile::new("one", CliKind::Claude);
    let off = build_environment(
        &EnvMap::new(),
        &EnvMap::new(),
        &profile,
        None,
        Some(&observer),
    )
    .unwrap();
    assert!(off.is_empty());
    profile.observer = Override::Set(true);
    let on = build_environment(
        &EnvMap::new(),
        &EnvMap::new(),
        &profile,
        None,
        Some(&observer),
    )
    .unwrap();
    assert_eq!(on.get(OsStr::new("CC_BOX_SESSION_ID")).unwrap(), "run-only");
    let unsafe_observer = ObserverEnv {
        values: env(&[("OPENAI_API_KEY", "observer-secret")]),
    };
    let error = build_environment(
        &EnvMap::new(),
        &EnvMap::new(),
        &profile,
        None,
        Some(&unsafe_observer),
    )
    .unwrap_err();
    assert_eq!(error.code, "INVALID_REQUEST");
    assert!(!format!("{error:?}").contains("observer-secret"));
}

#[test]
fn D08_Environment_RejectsUnsafeValuesWithoutMutation_07() {
    let inherited = env(&[("KEEP", "original")]);
    let before = inherited.clone();
    let mut profile = Profile::new("one", CliKind::Codex);
    profile.env.insert("BAD".into(), literal("secret\0tail"));
    assert!(build_environment(&inherited, &EnvMap::new(), &profile, None, None).is_err());
    assert_eq!(inherited, before);
    assert!(build_environment(
        &env(&[("BAD=NAME", "secret")]),
        &EnvMap::new(),
        &Profile::new("one", CliKind::Codex),
        None,
        None
    )
    .is_err());
}

#[test]
fn D08_Environment_TerminalCannotOverrideProvider_08() {
    let error = build_environment(
        &EnvMap::new(),
        &env(&[("OPENAI_API_KEY", "secret")]),
        &Profile::new("one", CliKind::Codex),
        None,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, "INVALID_REQUEST");
    assert!(!format!("{error:?}").contains("secret"));
}

#[cfg(windows)]
#[test]
fn D08_Environment_WindowsCaseConflictAndDeletion_09() {
    let profile = Profile::new("one", CliKind::Codex);
    assert!(build_environment(
        &env(&[("PATH", "first"), ("Path", "second")]),
        &EnvMap::new(),
        &profile,
        None,
        None
    )
    .is_err());
    let mut profile = profile;
    profile.env.insert("PATH".into(), Override::Unset);
    let result = build_environment(
        &env(&[("Path", "old")]),
        &EnvMap::new(),
        &profile,
        None,
        None,
    )
    .unwrap();
    assert!(result.is_empty());
}

#[cfg(windows)]
#[test]
fn D08_Environment_WindowsHostRefAndProfileAliasConflict_10() {
    let mut profile = Profile::new("one", CliKind::Codex);
    profile.env.insert(
        "TARGET".into(),
        Override::Set(EnvValue::HostRef {
            name: "path".into(),
        }),
    );
    let result = build_environment(
        &env(&[("Path", "host")]),
        &EnvMap::new(),
        &profile,
        None,
        None,
    )
    .unwrap();
    assert_eq!(result.get(OsStr::new("TARGET")).unwrap(), "host");
    profile.env.insert("PATH".into(), literal("a"));
    profile.env.insert("Path".into(), literal("b"));
    assert!(build_environment(&EnvMap::new(), &EnvMap::new(), &profile, None, None).is_err());
}

#[cfg(unix)]
#[test]
fn D08_Environment_UnixPreservesCaseAndNonUnicodeValues_11() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let mut inherited = env(&[("PATH", "a"), ("Path", "b")]);
    inherited.insert("BYTES".into(), OsString::from_vec(vec![0xff, 0xfe]));
    let result = build_environment(
        &inherited,
        &EnvMap::new(),
        &Profile::new("one", CliKind::Codex),
        None,
        None,
    )
    .unwrap();
    assert_eq!(result, inherited);
}

#[test]
fn D13_Observer_LegacyInheritanceNewDefaultAndCodexIsolation_012() {
    let observer = ObserverEnv {
        values: env(&[
            ("CC_BOX_HOOK_PORT", "43123"),
            (
                "CC_DESK_OBSERVER_CAPABILITY",
                "0123456789abcdef0123456789abcdef",
            ),
            ("CC_DESK_OBSERVER_RUN", "run-current"),
            ("CC_DESK_OBSERVER_GENERATION", "2"),
        ]),
    };

    let legacy = Profile::new("legacyClaude", CliKind::Claude);
    let inherited = build_environment(
        &EnvMap::new(),
        &EnvMap::new(),
        &legacy,
        None,
        Some(&observer),
    )
    .unwrap();
    assert_eq!(
        inherited
            .get(OsStr::new("CC_DESK_OBSERVER_CAPABILITY"))
            .unwrap(),
        "0123456789abcdef0123456789abcdef"
    );

    let fresh = Profile::new("fresh-claude", CliKind::Claude);
    let off = build_environment(
        &EnvMap::new(),
        &EnvMap::new(),
        &fresh,
        None,
        Some(&observer),
    )
    .unwrap();
    assert!(!off.contains_key(OsStr::new("CC_DESK_OBSERVER_CAPABILITY")));

    let mut explicit = fresh;
    explicit.observer = Override::Set(true);
    let on = build_environment(
        &EnvMap::new(),
        &EnvMap::new(),
        &explicit,
        None,
        Some(&observer),
    )
    .unwrap();
    assert_eq!(
        on.get(OsStr::new("CC_DESK_OBSERVER_RUN")).unwrap(),
        "run-current"
    );

    let mut codex = Profile::new("codex", CliKind::Codex);
    codex.observer = Override::Set(true);
    let isolated = build_environment(
        &EnvMap::new(),
        &EnvMap::new(),
        &codex,
        None,
        Some(&observer),
    )
    .unwrap();
    assert!(!isolated.contains_key(OsStr::new("CC_DESK_OBSERVER_CAPABILITY")));
}

#[test]
fn D13_Observer_ParentCapabilityNeverLeaksIntoAnotherRun_013() {
    let inherited = env(&[
        ("CC_DESK_OBSERVER_CAPABILITY", "private-parent"),
        ("CC_DESK_OBSERVER_RUN", "parent"),
        ("CC_DESK_OBSERVER_GENERATION", "1"),
        ("CC_BOX_HOOK_PORT", "4321"),
        ("OPENAI_API_KEY", "user-owned"),
        ("KEEP", "yes"),
    ]);
    for cli in [CliKind::Claude, CliKind::Codex, CliKind::Shell] {
        let profile = Profile::new("new", cli);
        let result = build_environment(&inherited, &EnvMap::new(), &profile, None, None).unwrap();
        assert!(!result.contains_key(OsStr::new("CC_DESK_OBSERVER_CAPABILITY")));
        assert!(!result.contains_key(OsStr::new("CC_BOX_HOOK_PORT")));
        assert_eq!(result[OsStr::new("OPENAI_API_KEY")], "user-owned");
        assert_eq!(result[OsStr::new("KEEP")], "yes");
    }
}

/// Opt-in host probe: never print inherited names, values, or private profiles.
#[cfg(windows)]
#[test]
#[ignore = "reads only the test process environment; emits shape counts and fixed categories"]
fn D08_Environment_WindowsHostDefaultProfiles_014() {
    use std::os::windows::ffi::OsStrExt;
    let raw: Vec<_> = std::env::vars_os().collect();
    let inherited: EnvMap = raw.iter().cloned().collect();
    let mut empty_names = 0;
    let mut nul_names = 0;
    let mut drive_names = 0;
    let mut other_leading_equals = 0;
    let mut pseudo_drive_names = 0;
    let mut command_status_names = 0;
    let mut interior_equals = 0;
    let mut non_unicode_names = 0;
    let mut empty_values = 0;
    let mut nul_values = 0;
    let mut non_unicode_values = 0;
    let mut alias_conflicts = 0;
    let mut first_alias_matches_os = 0;
    for (index, (name, value)) in inherited.iter().enumerate() {
        let units: Vec<_> = name.encode_wide().collect();
        empty_names += usize::from(units.is_empty());
        nul_names += usize::from(units.contains(&0));
        if units.first() == Some(&(b'=' as u16)) {
            if units.len() == 3
                && u8::try_from(units[1]).is_ok_and(|c| c.is_ascii_alphabetic())
                && units[2] == b':' as u16
            {
                drive_names += 1;
            } else {
                other_leading_equals += 1;
                pseudo_drive_names += usize::from(name == "=::");
                command_status_names += usize::from(crate::cli::environment::same_name(
                    name,
                    OsStr::new("=ExitCode"),
                ));
            }
        } else {
            interior_equals += usize::from(units.contains(&(b'=' as u16)));
        }
        non_unicode_names += usize::from(name.to_str().is_none());
        empty_values += usize::from(value.is_empty());
        nul_values += usize::from(value.encode_wide().any(|unit| unit == 0));
        non_unicode_values += usize::from(value.to_str().is_none());
        alias_conflicts += inherited
            .iter()
            .take(index)
            .filter(|(previous, old)| {
                crate::cli::environment::same_name(previous, name) && *old != value
            })
            .count();
    }
    eprintln!("host_shape entries={} empty_names={empty_names} nul_names={nul_names} drive_names={drive_names} other_leading_equals={other_leading_equals} interior_equals={interior_equals} non_unicode_names={non_unicode_names} empty_values={empty_values} nul_values={nul_values} non_unicode_values={non_unicode_values} alias_conflicts={alias_conflicts}", inherited.len());
    for (index, (name, value)) in raw.iter().enumerate() {
        if raw
            .iter()
            .take(index)
            .any(|(previous, _)| crate::cli::environment::same_name(previous, name))
        {
            continue;
        }
        if raw
            .iter()
            .skip(index + 1)
            .any(|(next, other)| crate::cli::environment::same_name(next, name) && other != value)
        {
            first_alias_matches_os += usize::from(std::env::var_os(name).as_ref() == Some(value));
        }
    }
    eprintln!("host_resolution first_alias_matches_os={first_alias_matches_os}");
    let captured =
        crate::cli::environment::capture_windows_environment(raw, |name| std::env::var_os(name));
    let mut passed = true;
    let mut outcomes = Vec::new();
    for cli in [CliKind::Claude, CliKind::Codex] {
        let result = captured
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|environment| {
                build_environment(
                    environment,
                    &EnvMap::new(),
                    &Profile::new("synthetic", cli),
                    None,
                    None,
                )
            });
        let category = match result
            .as_ref()
            .err()
            .and_then(|error| error.field.as_deref())
        {
            None if result.is_ok() => "ok",
            Some("environment.name") => "environment.name",
            Some("environment.value") => "environment.value",
            Some("environment.aliasConflict") => "environment.aliasConflict",
            Some("environment.changed") => "environment.changed",
            _ => "other-fixed-category",
        };
        eprintln!("default_profile cli={} category={category}", cli.as_str());
        outcomes.push(json!({"cli":cli.as_str(), "category":category}));
        passed &= result.is_ok();
    }
    // A caller-owned marker opts into a bounded report for hidden Explorer launches.
    // Never read application settings or serialize inherited entries.
    let directory = std::env::current_dir().unwrap();
    if directory.join("ccdesk-host-probe.request").is_file() {
        use windows::Win32::{
            Foundation::CloseHandle,
            System::Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
        };
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.unwrap();
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut parent_pid = None;
        let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };
        while next.is_ok() {
            if entry.th32ProcessID == std::process::id() {
                parent_pid = Some(entry.th32ParentProcessID);
                break;
            }
            next = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        unsafe { CloseHandle(snapshot) }.unwrap();
        let report = json!({"pid":std::process::id(), "parentPid":parent_pid,
            "entries":inherited.len(), "emptyNames":empty_names, "nulNames":nul_names,
            "driveNames":drive_names, "otherLeadingEquals":other_leading_equals,
            "pseudoDriveNames":pseudo_drive_names, "commandStatusNames":command_status_names,
            "interiorEquals":interior_equals, "nonUnicodeNames":non_unicode_names,
            "emptyValues":empty_values, "nulValues":nul_values, "nonUnicodeValues":non_unicode_values,
            "aliasConflicts":alias_conflicts, "profiles":outcomes});
        std::fs::write(
            directory.join("ccdesk-host-environment.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    assert!(
        passed,
        "default profiles rejected inherited host environment; see fixed categories"
    );
}

// Windows 继承别名使用 OS 生效值，不能按名称排序覆盖，两个默认 CLI 都可构建环境。
#[cfg(windows)]
#[test]
fn D08_Environment_HostAliases_015() {
    use crate::cli::environment::{capture_windows_environment, lookup};
    for names in [["Path", "PATH"], ["PATH", "PATH"]] {
        for effective in ["first", ""] {
            let mut reads = 0;
            let captured = capture_windows_environment(
                vec![
                    (names[0].into(), "first".into()),
                    (names[1].into(), "".into()),
                ],
                |_| {
                    reads += 1;
                    Some(effective.into())
                },
            )
            .unwrap();
            assert_eq!(reads, 1);
            assert_eq!(captured.len(), 1);
            for cli in [CliKind::Claude, CliKind::Codex] {
                let result = build_environment(
                    &captured,
                    &EnvMap::new(),
                    &Profile::new("default", cli),
                    None,
                    None,
                )
                .unwrap();
                assert_eq!(lookup(&result, OsStr::new("path")).unwrap(), effective);
            }
        }
    }
}

// OS 值不属于已捕获冲突组时失败，不猜测环境值。
#[cfg(windows)]
#[test]
fn D08_Environment_HostChanged_016() {
    use crate::cli::environment::capture_windows_environment;
    for effective in [None, Some("changed".into())] {
        let error = capture_windows_environment(
            vec![
                ("Path".into(), "first".into()),
                ("PATH".into(), "second".into()),
            ],
            |_| effective.clone(),
        )
        .unwrap_err();
        assert_eq!(error.code, "INVALID_REQUEST");
        assert_eq!(error.field.as_deref(), Some("environment.changed"));
    }
}

// 唯一项、同值别名、盘符项及非 Unicode 值保留快照，不重新读取全局环境。
#[cfg(windows)]
#[test]
fn D08_Environment_HostSnapshot_017() {
    use crate::cli::environment::capture_windows_environment;
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let raw = vec![
        ("PATH".into(), "same".into()),
        ("Path".into(), "same".into()),
        ("=C:".into(), r"C:\fixture".into()),
        ("EMPTY".into(), "".into()),
        (
            OsString::from_wide(&[0xd800]),
            OsString::from_wide(&[0xdc00]),
        ),
    ];
    let captured = capture_windows_environment(raw, |_| panic!("no conflicting values")).unwrap();
    assert_eq!(captured.len(), 4);
    let result = build_environment(
        &captured,
        &EnvMap::new(),
        &Profile::new("default", CliKind::Codex),
        None,
        None,
    )
    .unwrap();
    assert_eq!(result, captured);
}

// Explorer 可继承非盘符形式的内部变量；两个 CLI 应保留其 OS 字符串。
#[cfg(windows)]
#[test]
fn D08_Environment_WindowsReserved_018() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let mut inherited = env(&[
        ("=::", r"::\"),
        ("=ExitCode", "00000000"),
        ("=Reserved", "fixture"),
    ]);
    inherited.insert(OsString::from_wide(&[b'=' as u16, 0xd800]), "opaque".into());
    for cli in [CliKind::Claude, CliKind::Codex] {
        let result = build_environment(
            &inherited,
            &EnvMap::new(),
            &Profile::new("synthetic", cli),
            None,
            None,
        )
        .unwrap();
        assert_eq!(result, inherited);
    }
}

// 前导等号仅供 Windows 继承层使用，配置、terminal、legacy、observer 不能注入。
#[cfg(windows)]
#[test]
fn D08_Environment_ReservedBoundary_019() {
    let inherited = env(&[("=::", "fixture")]);
    let mut profile = Profile::new("synthetic", CliKind::Claude);
    assert!(build_environment(&EnvMap::new(), &inherited, &profile, None, None).is_err());
    profile.env.insert("=::".into(), literal("fixture"));
    assert!(build_environment(&EnvMap::new(), &EnvMap::new(), &profile, None, None).is_err());
    let legacy = Profile::new("legacyClaude", CliKind::Claude);
    assert!(build_environment(
        &EnvMap::new(),
        &EnvMap::new(),
        &legacy,
        Some(&json!({"claudeEnvVars":{"=::":"fixture"}})),
        None
    )
    .is_err());
    assert!(crate::cli::environment::overlay_observer(
        &EnvMap::new(),
        &ObserverEnv { values: inherited }
    )
    .is_err());
    for name in ["=", "==x", "=x=y", "x=y", "=x\0tail", ""] {
        assert!(build_environment(
            &env(&[(name, "fixture")]),
            &EnvMap::new(),
            &legacy,
            None,
            None
        )
        .is_err());
    }
}

// 非 Windows 环境不接受 Windows 内部变量名。
#[cfg(not(windows))]
#[test]
fn D08_Environment_ReservedWindowsOnly_020() {
    assert!(build_environment(
        &env(&[("=::", "fixture")]),
        &EnvMap::new(),
        &Profile::new("synthetic", CliKind::Codex),
        None,
        None
    )
    .is_err());
}
