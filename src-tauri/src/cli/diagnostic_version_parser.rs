//! Strict diagnostic output parser. No unmatched output crosses this boundary.
pub(crate) fn parse_version(cli: &str, bytes: &[u8]) -> Option<String> {
    if bytes.len() > 160 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let line = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    let version = match cli {
        "claude" => line.strip_suffix(" (Claude Code)")?,
        "codex" => line.strip_prefix("codex-cli ")?,
        "opencode" => line
            .strip_prefix("opencode v")
            .or_else(|| line.strip_prefix("opencode "))
            .unwrap_or(line),
        _ => return None,
    };
    if version.len() > 64 {
        return None;
    }
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    fn number(part: &str) -> bool {
        !part.is_empty()
            && part.len() <= 6
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    }
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| number(p)) {
        return None;
    }
    if let Some(pre) = pre {
        let (kind, n) = pre.split_once('.')?;
        if !matches!(kind, "alpha" | "beta" | "rc" | "preview") || !number(n) {
            return None;
        }
    }
    Some(version.to_owned())
}
