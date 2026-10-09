use super::*;
use std::ffi::OsString;
fn home() -> &'static str {
    if cfg!(windows) {
        "USERPROFILE"
    } else {
        "HOME"
    }
}
#[test]
fn independent_roots_and_default_sibling_authority_are_explicit() {
    let t = tempfile::tempdir().unwrap();
    let mut env = EnvMap::new();
    env.insert(home().into(), t.path().into());
    let c = locations(CliKind::Claude, &env).unwrap();
    assert_eq!(c.root, t.path().join(".claude"));
    assert!(c.user_config.is_some());
    let x = locations(CliKind::Codex, &env).unwrap();
    assert_eq!(x.root, t.path().join(".codex"));
    assert!(x.user_config.is_none());
    env.insert(
        "CLAUDE_CONFIG_DIR".into(),
        t.path().join("private").into_os_string(),
    );
    let c = locations(CliKind::Claude, &env).unwrap();
    assert_eq!(c.root, t.path().join("private"));
    assert!(c.user_config.is_none());
    assert_eq!(
        locations(CliKind::Codex, &env).unwrap().root,
        t.path().join(".codex")
    );
}
#[test]
fn explicit_invalid_root_never_falls_back_to_home() {
    let t = tempfile::tempdir().unwrap();
    let mut env = EnvMap::new();
    env.insert(home().into(), t.path().into());
    for value in ["", "relative", "nul\0root"] {
        env.insert("CODEX_HOME".into(), value.into());
        assert!(locations(CliKind::Codex, &env).is_err());
    }
    assert!(locations(CliKind::Shell, &env).is_err());
    assert!(locations(CliKind::Claude, &EnvMap::new()).is_err());
}
#[test]
fn explicit_root_does_not_need_or_grant_an_unrelated_home() {
    let t = tempfile::tempdir().unwrap();
    let env = EnvMap::from([(OsString::from("CODEX_HOME"), t.path().into())]);
    assert_eq!(locations(CliKind::Codex, &env).unwrap().root, t.path());
}
