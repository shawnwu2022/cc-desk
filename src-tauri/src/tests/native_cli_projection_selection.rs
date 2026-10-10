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

// 只读来源必须按配置参数判定，不能整体拒绝 npm Cmd shim 的启动机制。
#[test]
#[allow(non_snake_case)]
fn ScopeLauncher_NoBlanketShimReject_001() {
    let service = include_str!("../cli/native_projection/service.rs");
    assert!(
        !service.contains("!matches!(profile.launcher, Launcher::Native)"),
        "profile read scope must not blanket-reject explicit shims"
    );
    assert!(
        !service.contains("!matches!(snapshot.launcher(), Launcher::Native)"),
        "frozen run read scope must not blanket-reject explicit shims"
    );
}

// 显式 Cmd shim 与原生启动读取同一后端根，已知不改变根的参数可读取。
#[test]
#[allow(non_snake_case)]
fn ScopeArgs_AdmitRootNeutral_002() {
    use crate::cli::profiles::{Dialect, Launcher};
    let shim = Launcher::Shim {
        runner: "cmd.exe".into(),
        dialect: Dialect::Cmd,
    };
    for launcher in [&Launcher::Native, &shim] {
        assert!(read_scope_known(
            CliKind::Codex,
            launcher,
            std::iter::empty::<&str>()
        ));
        for flag in [
            "--full-auto",
            "--no-alt-screen",
            "--dangerously-bypass-approvals-and-sandbox",
        ] {
            assert!(read_scope_known(CliKind::Codex, launcher, [flag]));
        }
        assert!(read_scope_known(
            CliKind::Claude,
            launcher,
            ["--dangerously-skip-permissions"]
        ));
    }
}

// 根改变、未知参数、位置参数、原始分隔符和其他 shell 启动仍未知。
#[test]
#[allow(non_snake_case)]
fn ScopeArgs_RejectUnknownRoots_003() {
    use crate::cli::profiles::{Dialect, Launcher};
    for args in [
        vec!["--cd", "other"],
        vec!["--config", "other"],
        vec!["-c", "cwd='other'"],
        vec!["-Cother"],
        vec!["--config=other"],
        vec!["--full-auto", "--unknown"],
        vec!["--", "--full-auto"],
        vec!["prompt"],
    ] {
        assert!(!read_scope_known(CliKind::Codex, &Launcher::Native, args));
    }
    for launcher in [
        Launcher::Shell {
            program: "cmd.exe".into(),
            dialect: Dialect::Cmd,
        },
        Launcher::Shim {
            runner: "bash".into(),
            dialect: Dialect::Bash,
        },
        Launcher::Shim {
            runner: "pwsh".into(),
            dialect: Dialect::PowerShell,
        },
    ] {
        assert!(!read_scope_known(
            CliKind::Codex,
            &launcher,
            std::iter::empty::<&str>()
        ));
    }
    assert!(!read_scope_known(
        CliKind::Shell,
        &Launcher::Native,
        std::iter::empty::<&str>()
    ));
    assert!(!read_scope_known(
        CliKind::Claude,
        &Launcher::Native,
        ["--full-auto"]
    ));
    assert!(!read_scope_known(
        CliKind::Codex,
        &Launcher::Native,
        ["--dangerously-skip-permissions"]
    ));
}

// 两种互斥的 Codex 权限快捷参数不能组合为已知配置。
#[test]
#[allow(non_snake_case)]
fn ScopeArgs_RejectConflictingFlags_004() {
    for args in [
        ["--full-auto", "--dangerously-bypass-approvals-and-sandbox"],
        ["--dangerously-bypass-approvals-and-sandbox", "--full-auto"],
    ] {
        assert!(!read_scope_known(CliKind::Codex, &Launcher::Native, args));
    }
}
