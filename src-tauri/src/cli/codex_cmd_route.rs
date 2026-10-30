//! A bounded proof of npm's file-entry forwarding grammar, not publisher trust.

/// The selected wrapper still executes. This proof never selects Node or a core.
pub(crate) struct ForwardingShape {
    pub(crate) entry: String,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !value.as_bytes()[0].is_ascii_digit()
        && !["path", "pathext", "comspec", "node_options", "systemroot"]
            .iter()
            .any(|name| value.eq_ignore_ascii_case(name))
}

/// Lex complete lines; quoted atoms retain their quotes. No expansion/evaluation.
fn atoms(line: &str) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut atom = String::new();
    let mut quoted = false;
    for c in line.chars() {
        if c == '"' {
            quoted = !quoted;
        }
        if matches!(c, ' ' | '\t') && !quoted {
            if !atom.is_empty() {
                result.push(std::mem::take(&mut atom));
            }
        } else {
            atom.push(c);
        }
    }
    if quoted {
        return None;
    }
    if !atom.is_empty() {
        result.push(atom);
    }
    Some(result)
}

fn same(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn assignment(line: &str) -> Option<(&str, &str)> {
    let (command, value) = line.split_once([' ', '\t'])?;
    if !command.eq_ignore_ascii_case("set") {
        return None;
    }
    // Unquoted SET tail spaces/TAB are part of the value in CMD.
    let value = value.trim_start_matches([' ', '\t']);
    let value = if value.starts_with('"') {
        value.strip_prefix('"')?.strip_suffix('"')?
    } else {
        value
    };
    value.split_once('=')
}

pub(crate) fn prove(
    text: &str,
    environment_contains: impl Fn(&str) -> bool,
) -> Option<ForwardingShape> {
    if text.len() > 128 * 1024
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return None;
    }
    let mut lines = Vec::new();
    for line in text.lines() {
        let line = line.trim_start_matches([' ', '\t']);
        let line = line.strip_prefix('@').unwrap_or(line);
        if line.trim_matches([' ', '\t']).is_empty() {
            continue;
        }
        let words = atoms(line)?;
        if words
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case("rem"))
        {
            // Batch comments still participate in expansion and command parsing.
            if line.contains(['%', '!', '&', '|', '<', '>', '^', '(', ')']) {
                return None;
            }
            continue;
        }
        lines.push(line);
    }
    if lines.len() != 15 {
        return None;
    }
    let lex = lines
        .iter()
        .map(|line| atoms(line))
        .collect::<Option<Vec<_>>>()?;
    if !same(&lex[0], &["echo", "off"])
        || lex[1].len() != 2
        || !lex[1][0].eq_ignore_ascii_case("goto")
    {
        return None;
    }
    let start = &lex[1][1];
    let sub = lines[2].strip_prefix(':')?;
    if !identifier(start)
        || !identifier(sub)
        || start.eq_ignore_ascii_case(sub)
        || !lines[5].eq_ignore_ascii_case(&format!(":{start}"))
        || !same(&lex[4], &["exit", "/b"])
        || !same(&lex[6], &["setlocal"])
        || !same(&lex[7], &["call", &format!(":{sub}")])
    {
        return None;
    }
    let (directory, origin) = assignment(lines[3])?;
    if !identifier(directory)
        || environment_contains(directory)
        || !origin.eq_ignore_ascii_case("%~dp0")
    {
        return None;
    }
    let local_node = format!("%{directory}%\\node.exe");
    if !same(&lex[8], &["if", "exist", &format!("\"{local_node}\""), "("])
        || !same(&lex[10], &[")", "else", "("])
        || !same(&lex[13], &[")"])
    {
        return None;
    }
    let (node, local) = assignment(lines[9])?;
    let (fallback_node, fallback) = assignment(lines[11])?;
    if !identifier(node)
        || environment_contains(node)
        || node.eq_ignore_ascii_case(directory)
        || !node.eq_ignore_ascii_case(fallback_node)
        || !local.eq_ignore_ascii_case(&local_node)
        || !fallback.eq_ignore_ascii_case("node")
        || !lines[12].eq_ignore_ascii_case("SET PATHEXT=%PATHEXT:;.JS;=;%")
    {
        return None;
    }
    let tail = &lex[14];
    if tail.len() != 12
        || !same(
            &tail[..10],
            &[
                "endlocal",
                "&",
                "goto",
                "#_undefined_#",
                "2>NUL",
                "||",
                "title",
                "%COMSPEC%",
                "&",
                &format!("\"%{node}%\""),
            ],
        )
        || tail[11] != "%*"
    {
        return None;
    }
    let prefix = format!("\"%{directory}%\\");
    let entry = tail[10].get(prefix.len()..)?.strip_suffix('"')?;
    if !tail[10].get(..prefix.len())?.eq_ignore_ascii_case(&prefix)
        || entry.contains(['%', '!', '^', '"', '&', '|', '<', '>', ':', '(', ')', '/'])
        || entry.split('\\').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.ends_with([' ', '.'])
        })
        || !entry
            .to_ascii_lowercase()
            .starts_with("node_modules\\@openai\\codex\\")
    {
        return None;
    }
    Some(ForwardingShape {
        entry: entry.replace('\\', "/"),
    })
}
