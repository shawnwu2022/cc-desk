#![allow(non_snake_case)]
#[path = "../cli/diagnostic_version_parser.rs"]
mod parser;
#[test]
fn CliVersion_CanonicalOnly_001() {
    for (cli, line, want) in [
        ("claude", "2.1.318 (Claude Code)\r\n", "2.1.318"),
        ("codex", "codex-cli 0.166.0\n", "0.166.0"),
        ("opencode", "opencode v2.2.5\n", "2.2.5"),
        ("opencode", "1.2.3\n", "1.2.3"),
        ("opencode", "opencode 2.2.5", "2.2.5"),
        ("codex", "codex-cli 1.2.3-rc.4", "1.2.3-rc.4"),
    ] {
        assert_eq!(
            parser::parse_version(cli, line.as_bytes()).as_deref(),
            Some(want)
        );
    }
}
#[test]
fn CliVersion_SensitiveOutputIsUnknown_002() {
    for line in [
        "codex-cli 1.2.3\nsecret-token",
        "codex-cli 999999999999.2.3",
        "codex-cli 01.2.3",
        "codex-cli 1.2.3+secret",
        "codex-cli 1.2.3-beta.secret",
        "codex-cli 1.2.3\x1b[0m",
        "codex-cli 1.2.3\n\n",
        " codex-cli 1.2.3",
        "codex-cli 1.2.3 ",
        "codex-cli 1.2.3-rc.01",
        "codex-cli 1.2.3-rc.9999999",
    ] {
        assert_eq!(parser::parse_version("codex", line.as_bytes()), None);
    }
}
