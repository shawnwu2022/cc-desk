#![allow(non_snake_case)]

use super::{
    apply_configured_claude_environment, build_claude_command, legacy_claude_root_is_proven,
    prepare_legacy_claude_args, validate_resume_session_id, validate_resume_session_ids,
};
use crate::platform::{shell_quote_arg, ShellKind};

#[test]
fn BuildClaudeCommand_QuotesPathAndArgsAgainstShellMetachars() {
    let evils = [
        "; rm -rf /x",
        "$(calc)",
        "`calc`",
        "| pipe",
        "double \"quote\"",
        "a b",
    ];
    for evil in evils {
        for kind in [ShellKind::Posix, ShellKind::PowerShell] {
            let cmd = build_claude_command(
                Some("C:\\evil path\\claude.exe"),
                Some(&["--resume".to_string(), evil.to_string()]),
                kind,
            );
            // 每个动态片段都必须是引用后的字面量，不允许裸露在 shell 中
            let call = if kind == ShellKind::PowerShell {
                "& "
            } else {
                ""
            };
            let expected = format!(
                "{call}{} {} {}",
                shell_quote_arg("C:\\evil path\\claude.exe", kind),
                shell_quote_arg("--resume", kind),
                shell_quote_arg(evil, kind)
            );
            assert_eq!(cmd, expected, "unquoted fragment for {kind:?}: {cmd}");
        }
    }
}

#[test]
fn BuildClaudeCommand_WithoutArgs_KeepsProgramOnly() {
    assert_eq!(
        build_claude_command(Some("C:\\x\\claude.exe"), None, ShellKind::Posix),
        "'C:\\x\\claude.exe'"
    );
    assert_eq!(build_claude_command(None, None, ShellKind::Posix), "claude");
    assert_eq!(
        build_claude_command(None, Some(&[]), ShellKind::Posix),
        "claude"
    );
    // PowerShell 以调用操作符开头，裸引号路径才可执行
    assert_eq!(
        build_claude_command(Some("C:\\x\\claude.exe"), None, ShellKind::PowerShell),
        "& 'C:\\x\\claude.exe'"
    );
    assert_eq!(
        build_claude_command(
            None,
            Some(&["--resume".to_string(), "it's".to_string()]),
            ShellKind::PowerShell
        ),
        "& claude '--resume' 'it''s'"
    );
}

#[test]
fn ValidateResumeSessionIds_RejectsShellMetachars() {
    let evils = [
        "abc; rm -rf /x",
        "$(calc)",
        "a`id`b",
        "a|b",
        "a\"b",
        "a b",
        "会话",
        "a:b",
    ];
    for evil in evils {
        assert!(
            validate_resume_session_id(evil).is_err(),
            "must reject: {evil}"
        );
        let pair = ["--resume".to_string(), evil.to_string()];
        assert!(validate_resume_session_ids(&pair).is_err());
        let inline = [format!("--resume={evil}")];
        assert!(validate_resume_session_ids(&inline).is_err());
    }
    // --resume 缺值同样拒绝
    let missing = ["--resume".to_string()];
    assert!(validate_resume_session_ids(&missing).is_err());
}

#[test]
fn ValidateResumeSessionIds_AcceptsWellFormedIds() {
    assert!(validate_resume_session_id("cb2f0e991234456789ab001122334455").is_ok());
    assert!(validate_resume_session_id("a-b_c.2026").is_ok());
    let pair = ["--resume".to_string(), "abc123".to_string()];
    assert!(validate_resume_session_ids(&pair).is_ok());
    assert!(validate_resume_session_ids(&[]).is_ok());
}

#[test]
fn ValidateResumeSessionIds_RejectsPathEscapeForms() {
    // store 层文件名组件校验补充点路径逃逸
    assert!(validate_resume_session_id(".").is_err());
    assert!(validate_resume_session_id("..").is_err());
    assert!(validate_resume_session_id("a/b").is_err());
}

#[test]
fn LegacyResume_RawArgsStayOpaque_01() {
    let cases = [
        vec!["--resume".to_string()],
        vec!["--resume".to_string(), "--model".to_string()],
        vec!["-r".to_string(), "named session".to_string()],
        vec![
            "-r".to_string(),
            "C:\\Users\\me\\.claude\\projects\\repo\\session.jsonl".to_string(),
        ],
    ];
    for raw in cases {
        assert_eq!(
            prepare_legacy_claude_args(None, Some(&raw)).unwrap(),
            Some(raw)
        );
    }
}

#[test]
fn LegacyResume_OwnedPairBeforeRaw_02() {
    let raw = vec![
        "--append-system-prompt".to_string(),
        "quote ' and $literal".to_string(),
    ];
    let prepared = prepare_legacy_claude_args(Some("saved-session"), Some(&raw))
        .unwrap()
        .unwrap();
    assert_eq!(
        prepared,
        vec![
            "--resume",
            "saved-session",
            "--append-system-prompt",
            "quote ' and $literal",
        ]
    );
    assert_eq!(
        build_claude_command(None, Some(&prepared), ShellKind::Posix),
        "claude '--resume' 'saved-session' '--append-system-prompt' 'quote '\\'' and $literal'"
    );
}

#[test]
fn LegacyResume_BadOwnedIdRejected_03() {
    for invalid in ["", "..", "../escape", "CON", "--model", "name;calc"] {
        assert!(prepare_legacy_claude_args(Some(invalid), None).is_err());
    }
}

#[test]
fn LegacyRootProof_OnlyNonInteractiveGitBashPreservesEnvironmentProof_04() {
    #[cfg(target_os = "windows")]
    {
        assert!(legacy_claude_root_is_proven(
            true,
            Some("C:\\Program Files\\Git\\bin\\bash.exe")
        ));
        assert!(!legacy_claude_root_is_proven(
            false,
            Some("C:\\Program Files\\Git\\bin\\bash.exe")
        ));
        assert!(!legacy_claude_root_is_proven(true, None));
    }

    #[cfg(not(target_os = "windows"))]
    {
        assert!(!legacy_claude_root_is_proven(true, None));
        assert!(!legacy_claude_root_is_proven(true, Some("/bin/bash")));
    }
}

#[cfg(target_os = "windows")]
#[test]
fn LegacyRootProof_ConfiguredHomeKeepsEnvButRevokesProof_05() {
    for key in ["USERPROFILE", "userprofile"] {
        let mut cmd = portable_pty::CommandBuilder::new("claude");
        let value = "C:\\custom-claude-home";
        let vars = std::collections::HashMap::from([(key.to_owned(), value.to_owned())]);
        assert!(
            !apply_configured_claude_environment(&mut cmd, &vars),
            "{key}"
        );
        assert_eq!(cmd.get_env(key), Some(std::ffi::OsStr::new(value)));
    }
}

#[test]
fn LegacyRootProof_ConfiguredRedirectPreservesAppliedEnvironment_06() {
    for (key, value, proven) in [
        ("CLAUDE_CONFIG_DIR", "C:\\alternate-root", false),
        ("claude_config_dir", "", false),
        ("BASH_ENV", "startup.sh", false),
        ("bash_env", "", true),
        ("CC_DESK_TEST_ENV", "retained", true),
    ] {
        let mut cmd = portable_pty::CommandBuilder::new("claude");
        let vars = std::collections::HashMap::from([(key.to_owned(), value.to_owned())]);
        assert_eq!(apply_configured_claude_environment(&mut cmd, &vars), proven);
        assert_eq!(cmd.get_env(key), Some(std::ffi::OsStr::new(value)));
    }
}
