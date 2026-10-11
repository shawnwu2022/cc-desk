#![allow(non_snake_case)]

#[cfg(windows)]
use super::file_manager_argument;
use super::{open_in_file_manager, shell_quote_arg, ShellKind};

/// 恶意输入引用后必须保持单一字面量：整体包在单引号内，
/// 且内部单引号全部处于各 shell 的转义形态，不残留裸引号。
#[test]
fn ShellQuoteArg_MetacharInputs_StayInsideQuoteBoundary() {
    let cases = [
        "; rm -rf /x",
        "$(calc)",
        "`calc`",
        "| pipe",
        "double \"quote\"",
        "path with space",
        "会话 $HOME",
    ];
    for input in cases {
        for kind in [ShellKind::Posix, ShellKind::PowerShell] {
            let quoted = shell_quote_arg(input, kind);
            assert!(
                quoted.starts_with('\'') && quoted.ends_with('\''),
                "quoted output must be single-quote wrapped: {quoted}"
            );
            let inner = &quoted[1..quoted.len() - 1];
            let unescaped = match kind {
                // 还原各 shell 的转义形态后再检查是否残留裸引号
                ShellKind::Posix => inner.replace("'\\''", ""),
                ShellKind::PowerShell => inner.replace("''", ""),
            };
            assert!(
                !unescaped.contains('\''),
                "bare quote escaped its boundary: {quoted}"
            );
        }
    }
}

#[test]
fn ShellQuoteArg_ExactEscapedShape_PerShell() {
    assert_eq!(shell_quote_arg("it's", ShellKind::Posix), "'it'\\''s'");
    assert_eq!(shell_quote_arg("it's", ShellKind::PowerShell), "'it''s'");
    assert_eq!(shell_quote_arg("$(calc)", ShellKind::Posix), "'$(calc)'");
    assert_eq!(shell_quote_arg("", ShellKind::Posix), "''");
}

#[test]
fn OpenInFileManager_RejectsDashSwitchInput() {
    assert!(open_in_file_manager("-Trojan").is_err());
}

#[cfg(windows)]
#[test]
fn OpenInFileManager_RejectsExplorerSwitchInput() {
    assert!(open_in_file_manager("/select,C:\\Windows").is_err());
}

#[cfg(windows)]
#[test]
fn OpenInFileManager_NormalizedWindowsPaths_001() {
    // 前端统一路径使用正斜杠；进入 Explorer 前恢复 UNC 与长路径的字面量前缀。
    for (input, expected) in [
        ("//server/share/project", r"\\server\share\project"),
        (
            "//server/share/path with space",
            r"\\server\share\path with space",
        ),
        ("//?/C:/projects/demo", r"\\?\C:\projects\demo"),
        ("//?/UNC/server/share/demo", r"\\?\UNC\server\share\demo"),
        (r"\\server\share\project", r"\\server\share\project"),
        ("C:/projects/demo", "C:/projects/demo"),
    ] {
        assert_eq!(file_manager_argument(input).unwrap(), expected);
    }
}

#[cfg(windows)]
#[test]
fn OpenInFileManager_ExplorerSwitchesRemainRejected_002() {
    for input in ["/select,C:\\Windows", "/root,C:\\Windows", "/e", "-Trojan"] {
        assert!(
            file_manager_argument(input).is_err(),
            "accepted switch: {input}"
        );
    }
}
