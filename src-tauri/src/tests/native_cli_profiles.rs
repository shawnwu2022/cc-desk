use crate::cli::profiles::{resolve_override, EnvValue, Override, Profile};
use crate::cli::types::CliKind;
use serde_json::json;

#[test]
fn D06_Override_FalseUnsetEmpty_01() {
    assert_eq!(resolve_override(Override::Set(false), Some(true)), Some(false));
    assert_eq!(resolve_override(Override::<bool>::Unset, Some(true)), None);
    assert_eq!(resolve_override(Override::Inherit, Some(true)), Some(true));
    assert_eq!(resolve_override(Override::Set(String::new()), Some("old".into())), Some(String::new()));
    assert_eq!(resolve_override(Override::Set(Vec::<String>::new()), Some(vec!["old".into()])), Some(vec![]));
}

#[test]
fn D06_Override_WireShape_02() {
    let cases = [(json!({"mode":"inherit"}), Override::Inherit), (json!({"mode":"set","value":false}), Override::Set(false)), (json!({"mode":"unset"}), Override::Unset)];
    for (value, expected) in cases {
        let parsed: Override<bool> = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
    for value in [json!({"mode":"set"}), json!({"mode":"unset","value":true}), json!(null), json!({"mode":"other"})] {
        assert!(serde_json::from_value::<Override<bool>>(value).is_err());
    }
}

#[test]
fn D06_Legacy_IsClaudeOnly_03() {
    let legacy = json!({"defaultSkipPermissions":true,"claudeEnvVars":{"SYNTHETIC_SECRET":"not-real"},"future":"preserve"});
    let mut claude = Profile::new("legacyClaude", CliKind::Claude);
    assert_eq!(claude.resolve_skip_permissions(Some(&legacy)), Some(true));
    claude.skip_permissions = Override::Set(false);
    assert_eq!(claude.resolve_skip_permissions(Some(&legacy)), Some(false));
    claude.skip_permissions = Override::Unset;
    assert_eq!(claude.resolve_skip_permissions(Some(&legacy)), None);
    assert_eq!(Profile::new("codex", CliKind::Codex).resolve_skip_permissions(Some(&legacy)), None);
    assert_eq!(Profile::new("newClaude", CliKind::Claude).resolve_skip_permissions(Some(&legacy)), None);
    assert!(!serde_json::to_string(&claude).unwrap().contains("not-real"));
}

#[test]
fn D06_Profile_RejectsImplicitSecretAndCodexBypass_04() {
    let mut p = Profile::new("codex", CliKind::Codex);
    p.skip_permissions = Override::Set(true);
    assert!(p.validate().is_err());
    p.skip_permissions = Override::Inherit;
    p.env.insert("TOKEN".into(), Override::Set(EnvValue::Literal { value: "fixture".into(), non_secret: false }));
    assert!(p.validate().is_err());
    p.env.insert("TOKEN".into(), Override::Set(EnvValue::HostRef { name: "EXTERNAL_TOKEN".into() }));
    assert!(p.validate().is_ok());
    p.env.insert("BAD=KEY".into(), Override::Unset);
    assert!(p.validate().is_err());
}

#[test]
fn D06_Profile_RejectsWrongLegacyOwnerAndNul_05() {
    assert!(Profile::new("legacyClaude", CliKind::Codex).validate().is_err());
    let mut p = Profile::new("claude", CliKind::Claude);
    p.default_args = Override::Set(vec!["fixture\0secret".into()]);
    let error = p.validate().unwrap_err();
    assert!(!error.to_string().contains("fixture"));
}
