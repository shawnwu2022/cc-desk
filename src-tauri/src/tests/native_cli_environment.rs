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
