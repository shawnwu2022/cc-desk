use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TitlePriority {
    Prompt,
    Ai,
    Custom,
}
impl TitlePriority {
    fn source(self) -> TitleSource {
        match self {
            Self::Prompt => TitleSource::Prompt,
            Self::Ai => TitleSource::Ai,
            Self::Custom => TitleSource::Custom,
        }
    }
}
struct Transcript {
    id: String,
    cwd: Option<String>,
    title: String,
    updated: Option<String>,
    messages: Vec<(String, String)>,
    metadata_incomplete: bool,
    title_priority: Option<TitlePriority>,
    source_path: String,
    observation_bytes: usize,
}
pub(super) fn read(
    c: &Catalog<'_>,
    o: &Options<'_>,
    b: &mut Budget,
) -> ReadResult<Vec<ResourceItem>> {
    if c.cli == CliKind::Shell {
        return Err("SOURCE_UNSUPPORTED");
    }
    let mut files = Vec::new();
    match c.cli {
        CliKind::Claude => walk(c, "projects", 2, o.kind, b, &mut files)?,
        CliKind::Codex => {
            walk(c, "sessions", 4, o.kind, b, &mut files)?;
            walk(c, "archived_sessions", 4, o.kind, b, &mut files)?;
        }
        CliKind::Shell => unreachable!(),
    }
    let mut transcripts = Vec::new();
    let mut seen = BTreeSet::new();
    for path in files {
        let mut observation_bytes = 0;
        let parsed = (|| {
            let observation = if o.kind == ResourceKind::History {
                c.history_prefix(&path, b)?
            } else {
                c.bytes(c.root, &path, b)?.map(|bytes| (bytes, false))
            };
            let Some((bytes, incomplete)) = observation else {
                return Err("SOURCE_CHANGED");
            };
            observation_bytes = bytes.len();
            if incomplete {
                let end = bytes
                    .iter()
                    .rposition(|b| *b == b'\n')
                    .ok_or("SOURCE_TOO_LARGE")?;
                let transcript = parse_metadata(&path, c.cli, text(&bytes[..=end])?)?;
                if c.cli == CliKind::Claude && transcript.cwd.is_none() {
                    // Adaptive observation exhausted its byte cap without cwd.
                    // Do not silently turn an unobserved identity into a foreign project.
                    return Err("SOURCE_TOO_LARGE");
                }
                Ok(transcript)
            } else if o.kind == ResourceKind::History {
                parse(&path, c.cli, text(&bytes)?)
            } else {
                parse_complete(&path, c.cli, text(&bytes)?, true)
            }
        })();
        let mut transcript = match parsed {
            Ok(transcript) => transcript,
            Err(code) if o.kind == ResourceKind::History && b.omit_history_entry(code) => {
                c.check()?;
                continue;
            }
            Err(code) => return Err(code),
        };
        transcript.observation_bytes = observation_bytes;
        if !seen.insert(transcript.id.clone()) {
            return Err("SOURCE_AMBIGUOUS");
        }
        if c.project.is_some()
            && !transcript
                .cwd
                .as_ref()
                .is_some_and(|cwd| c.project_paths.iter().any(|p| p == Path::new(cwd)))
        {
            if o.kind == ResourceKind::History && transcript.cwd.is_none() {
                // Even an EOF-complete metadata file cannot establish project
                // absence when no association was observed in its records.
                b.unobserved_history_association();
            }
            continue;
        }
        transcripts.push(transcript);
    }
    // Required headers/tree checks finish before optional display enrichment.
    // Only a positively associated requested-project row may spend spare bytes.
    if c.cli == CliKind::Claude && o.kind == ResourceKind::History && c.project.is_some() {
        for transcript in &mut transcripts {
            if !transcript.metadata_incomplete {
                continue;
            }
            let enriched = enrich_claude_title(c, transcript, b);
            match enriched {
                Ok(Some((title, priority)))
                    if transcript.title_priority.is_none_or(|old| priority >= old) =>
                {
                    transcript.title = title;
                    transcript.title_priority = Some(priority);
                }
                Ok(_) => {}
                Err(code) if b.omit_history_entry(code) => c.check()?,
                Err(code) => return Err(code),
            }
        }
    }
    transcripts.sort_by(|a, b| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
    let mut result = Vec::new();
    let query = o.query.map(str::to_lowercase);
    for t in transcripts {
        // Stable within a native source, not a pathname or a permission token.
        let key = serde_json::to_string(&["local", c.cli.as_str(), c.root.key(), &t.id])
            .map_err(|_| "SOURCE_INVALID")?;
        if o.kind == ResourceKind::History {
            let (title, truncated) = bounded(&t.title, 512);
            result.push(ResourceItem::Session {
                session_key: key,
                native_session_id: t.id,
                title,
                title_unknown: t.title_priority.is_none().then_some(true),
                title_source: t.title_priority.map(TitlePriority::source),
                metadata_incomplete: t.metadata_incomplete.then_some(true),
                truncated: truncated || t.metadata_incomplete,
                cwd: t.cwd,
                updated_at: t.updated,
            });
            continue;
        }
        if o.kind == ResourceKind::Messages && o.session_id != Some(t.id.as_str()) {
            continue;
        }
        for (role, message) in t.messages {
            if o.kind == ResourceKind::Search
                && !query
                    .as_ref()
                    .is_some_and(|q| message.to_lowercase().contains(q))
            {
                continue;
            }
            let (text, truncated) = bounded(&message, 16 * 1024);
            result.push(ResourceItem::Message {
                session_key: key.clone(),
                native_session_id: t.id.clone(),
                role,
                text,
                truncated,
            });
        }
    }
    Ok(result)
}
fn walk(
    c: &Catalog<'_>,
    path: &str,
    depth: usize,
    kind: ResourceKind,
    b: &mut Budget,
    files: &mut Vec<String>,
) -> ReadResult<()> {
    for entry in c.entries(c.root, path, b)? {
        let p = child(path, &entry.name);
        if entry.is_dir {
            // Documented project auto memory is not a session container. Only
            // this exact real-directory position is excluded; links still fail.
            if c.cli == CliKind::Claude && depth == 1 && entry.name == "memory" {
                continue;
            }
            // Subagent transcripts and spilled tool results live beneath a main
            // session; neither directory is main-session history to traverse.
            if c.cli == CliKind::Claude
                && depth == 0
                && matches!(entry.name.as_str(), "subagents" | "tool-results")
            {
                continue;
            }
            if depth == 0 {
                if kind == ResourceKind::History && b.omit_history_entry("SOURCE_UNSUPPORTED") {
                    continue;
                }
                return Err("SOURCE_UNSUPPORTED");
            }
            // A failed directory enumeration has not checked all siblings for
            // links/invalid paths. Only individual transcript reads are isolatable.
            walk(c, &p, depth - 1, kind, b, files)?;
        } else if entry.is_file
            && entry.name.ends_with(".jsonl")
            && !entry.name.starts_with("agent-")
        {
            // Documented set-aside transcripts are not shown by Claude's picker.
            // Exclude only regular project-child files, never a link or other depth.
            if c.cli == CliKind::Claude
                && depth == 1
                && entry
                    .name
                    .trim_end_matches(".jsonl")
                    .split_once(".orphaned-")
                    .is_some_and(|(session, suffix)| {
                        !session.is_empty()
                            && suffix.rsplit_once('-').is_some_and(|(timestamp, suffix)| {
                                !timestamp.is_empty() && !suffix.is_empty()
                            })
                    })
            {
                continue;
            }
            files.push(p);
        }
        // Link entries are explicitly rejected, not followed outside a held root.
        else if !entry.is_dir && !entry.is_file {
            return Err("SOURCE_NOT_REGULAR");
        }
    }
    Ok(())
}
fn parse(path: &str, cli: CliKind, input: &str) -> ReadResult<Transcript> {
    parse_complete(path, cli, input, false)
}

fn claude_observed_session_id(value: &Value, session_id: &str) -> ReadResult<()> {
    if matches!(
        value["type"].as_str(),
        Some("user" | "assistant" | "custom-title" | "ai-title")
    ) && value
        .get("sessionId")
        .is_some_and(|id| id.as_str() != Some(session_id))
    {
        return Err("SOURCE_AMBIGUOUS");
    }
    Ok(())
}

// Official SDK 0.2.165 gives aiTitle precedence over the first prompt, while
// customTitle wins. Only the observed ai-title schema with exact sessionId is used.
// https://github.com/anthropics/claude-agent-sdk-python/blob/b6e9d12fe1cc98dde988ab7b7713c1feeee50c6c/src/claude_agent_sdk/_internal/sessions.py#L441-L458
fn claude_ai_title(value: &Value, session_id: &str) -> ReadResult<String> {
    let observed_id = value["sessionId"].as_str().ok_or("SOURCE_INVALID")?;
    if observed_id != session_id {
        return Err("SOURCE_AMBIGUOUS");
    }
    value["aiTitle"]
        .as_str()
        .filter(|title| !title.trim().is_empty())
        .map(str::to_owned)
        .ok_or("SOURCE_INVALID")
}

fn enrich_claude_title(
    catalog: &Catalog<'_>,
    transcript: &Transcript,
    budget: &mut Budget,
) -> ReadResult<Option<(String, TitlePriority)>> {
    let Some((header, tail, cut_first)) = catalog.history_title_tail(
        &transcript.source_path,
        transcript.observation_bytes,
        budget,
    )?
    else {
        return Ok(None);
    };
    let end = header
        .iter()
        .rposition(|byte| *byte == b'\n')
        .ok_or("SOURCE_CHANGED")?;
    let observed = text(&header[..=end])
        .and_then(|input| parse_metadata(&transcript.source_path, CliKind::Claude, input))
        .map_err(|code| {
            if code == "SOURCE_AMBIGUOUS" {
                code
            } else {
                "SOURCE_CHANGED"
            }
        })?;
    if observed.id != transcript.id || observed.cwd != transcript.cwd {
        return Err("SOURCE_CHANGED");
    }
    // The initial tail fragment can begin inside UTF-8 or a JSON record. Only
    // subsequent complete records, including a validated EOF record, are used.
    let start = if cut_first {
        tail.iter()
            .position(|byte| *byte == b'\n')
            .map_or(tail.len(), |position| position + 1)
    } else {
        0
    };
    let mut candidate = None;
    for line in text(&tail[start..])?
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let value: Value = serde_json::from_str(line).map_err(|_| "SOURCE_INVALID")?;
        validate_value(&value)?;
        if !value.is_object() {
            return Err("SOURCE_INVALID");
        }
        claude_observed_session_id(&value, &transcript.id)?;
        if let Some(cwd) = value["cwd"].as_str() {
            if transcript.cwd.as_deref() != Some(cwd) {
                return Err("SOURCE_AMBIGUOUS");
            }
        }
        let title = match value["type"].as_str() {
            Some("ai-title") => Some((claude_ai_title(&value, &transcript.id)?, TitlePriority::Ai)),
            Some("custom-title") => value["customTitle"]
                .as_str()
                .map(|title| (title.to_owned(), TitlePriority::Custom)),
            _ => None,
        };
        if let Some((title, priority)) = title {
            if candidate.as_ref().is_none_or(|(_, old)| priority >= *old) {
                candidate = Some((title, priority));
            }
        }
    }
    Ok(candidate)
}

// Official SDK 0.2.165 (bundled CLI 2.1.296) demonstrates permission-mode
// before a user record. It carries no cwd and cannot end our observation.
// https://github.com/anthropics/claude-agent-sdk-python/blob/b6e9d12fe1cc98dde988ab7b7713c1feeee50c6c/tests/test_sessions.py#L1399-L1426
pub(super) fn claude_metadata_probe() -> impl FnMut(&[u8]) -> ReadResult<bool> {
    let mut searched = 0;
    let mut line_start = 0;
    let mut cwd_observed = false;
    move |bytes| {
        // Search only newly appended bytes. A long record is decoded once,
        // after its newline, rather than rescanned and parsed on each chunk.
        for position in searched..bytes.len() {
            if bytes[position] != b'\n' {
                continue;
            }
            let line = text(&bytes[line_start..position])?;
            line_start = position + 1;
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line).map_err(|_| "SOURCE_INVALID")?;
            validate_value(&value)?;
            if !value.is_object() {
                return Err("SOURCE_INVALID");
            }
            cwd_observed |= matches!(
                value["type"].as_str(),
                Some(
                    "user"
                        | "assistant"
                        | "custom-title"
                        | "summary"
                        | "file-history-snapshot"
                        | "queue-operation"
                        | "progress"
                        | "system"
                )
            ) && value["cwd"].as_str().is_some();
        }
        searched = bytes.len();
        Ok(cwd_observed)
    }
}
fn parse_complete(
    path: &str,
    cli: CliKind,
    input: &str,
    include_messages: bool,
) -> ReadResult<Transcript> {
    let fallback = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches(".jsonl");
    let mut t = Transcript {
        id: fallback.into(),
        cwd: None,
        title: String::new(),
        updated: None,
        messages: Vec::new(),
        metadata_incomplete: false,
        title_priority: None,
        source_path: path.into(),
        observation_bytes: 0,
    };
    let mut recognized = false;
    let mut explicit_title = None;
    let mut ai_title = None;
    let mut codex_id = None;
    let mut event_user = false;
    for line in input.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).map_err(|_| "SOURCE_INVALID")?;
        validate_value(&v)?;
        if !v.is_object() {
            return Err("SOURCE_INVALID");
        }
        match cli {
            CliKind::Claude => {
                claude_observed_session_id(&v, fallback)?;
                if let Some(cwd) = v.get("cwd").and_then(Value::as_str) {
                    t.cwd = Some(cwd.to_owned());
                }
                match v.get("type").and_then(Value::as_str) {
                    Some("ai-title") => {
                        ai_title = Some(claude_ai_title(&v, fallback)?);
                        recognized = true;
                    }
                    Some("custom-title") => {
                        recognized = true;
                        explicit_title = v
                            .get("customTitle")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    }
                    Some("user" | "assistant") => {
                        recognized = true;
                        if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
                            continue;
                        }
                        let role = v["type"].as_str().unwrap();
                        // History needs only the first user title. Every record is
                        // still decoded and validated; Messages/Search retain all bodies.
                        if include_messages || (role == "user" && t.messages.is_empty()) {
                            let message = content(&v["message"]["content"]);
                            if !message.is_empty() {
                                t.messages.push((role.into(), message));
                            }
                        }
                    }
                    // Known metadata doesn't create a fabricated conversation message.
                    Some(
                        "summary"
                        | "file-history-snapshot"
                        | "queue-operation"
                        | "progress"
                        | "system",
                    ) => {
                        recognized = true;
                    }
                    _ => {}
                }
            }
            CliKind::Codex => match v["type"].as_str() {
                Some("session_meta") => {
                    recognized = true;
                    let id = v["payload"]["id"].as_str().ok_or("SOURCE_INVALID")?;
                    if codex_id.as_ref().is_some_and(|old| old != id) {
                        return Err("SOURCE_INVALID");
                    }
                    codex_id = Some(id.to_owned());
                    t.cwd = v["payload"]["cwd"].as_str().map(str::to_owned);
                }
                Some("event_msg") if v["payload"]["type"] == "user_message" => {
                    let message = v["payload"]["message"].as_str().ok_or("SOURCE_INVALID")?;
                    // A rollout may also contain a response_item user record; prefer that
                    // canonical record when present, otherwise retain the event observation.
                    t.messages.push(("user-event".into(), message.into()));
                    event_user = true;
                }
                Some("response_item") if v["payload"]["type"] == "message" => {
                    if let Some(role @ ("user" | "assistant")) = v["payload"]["role"].as_str() {
                        // User streams remain available for the complete order and
                        // multiplicity check below, even for a metadata-only history read.
                        if include_messages || role == "user" {
                            let message = content(&v["payload"]["content"]);
                            if !message.is_empty() {
                                t.messages.push((role.into(), message));
                            }
                        }
                    }
                }
                _ => {}
            },
            CliKind::Shell => return Err("SOURCE_UNSUPPORTED"),
        }
        if let Some(ts) = v["timestamp"].as_str().filter(|s| s.len() <= 64) {
            t.updated = Some(ts.into());
        }
    }
    if !recognized {
        return Err("SOURCE_UNSUPPORTED");
    }
    if cli == CliKind::Codex {
        t.id = codex_id.ok_or("SOURCE_UNSUPPORTED")?;
    }
    if t.id.is_empty() || t.id.len() > 256 || t.id.chars().any(char::is_control) {
        return Err("SOURCE_INVALID");
    }
    if event_user {
        let events: Vec<_> = t
            .messages
            .iter()
            .filter(|(r, _)| r == "user-event")
            .map(|(_, m)| m)
            .collect();
        let canonical: Vec<_> = t
            .messages
            .iter()
            .filter(|(r, _)| r == "user")
            .map(|(_, m)| m)
            .collect();
        if !canonical.is_empty() {
            // Alternative complete record streams may represent the same turns. Compare order
            // AND multiplicity, never deduplicate by body. Mixed/partial streams are unavailable.
            if canonical != events {
                return Err("SOURCE_AMBIGUOUS");
            }
            t.messages.retain(|(role, _)| role != "user-event");
        } else {
            for (role, _) in &mut t.messages {
                if role == "user-event" {
                    *role = "user".into();
                }
            }
        }
    }
    t.title_priority = if explicit_title.is_some() {
        Some(TitlePriority::Custom)
    } else if ai_title.is_some() {
        Some(TitlePriority::Ai)
    } else if t.messages.iter().any(|(role, _)| role == "user") {
        Some(TitlePriority::Prompt)
    } else {
        None
    };
    t.title = explicit_title
        .or(ai_title)
        .or_else(|| {
            t.messages
                .iter()
                .find(|(role, _)| role == "user")
                .map(|(_, text)| text.clone())
        })
        .unwrap_or_else(|| "Untitled".into());
    if !include_messages {
        t.messages = Vec::new();
    }
    Ok(t)
}
// A bounded prefix is an observation of identity/title, not a complete message
// stream. Do not compare paired event/response streams or claim its latest time.
fn parse_metadata(path: &str, cli: CliKind, input: &str) -> ReadResult<Transcript> {
    let mut id = None;
    let mut cwd = None;
    let mut title = None;
    let mut explicit_title = None;
    let mut ai_title = None;
    let mut recognized = false;
    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|_| "SOURCE_INVALID")?;
        validate_value(&value)?;
        if !value.is_object() {
            return Err("SOURCE_INVALID");
        }
        let observed_cwd = match cli {
            CliKind::Claude => {
                claude_observed_session_id(
                    &value,
                    path.rsplit('/')
                        .next()
                        .unwrap_or("")
                        .trim_end_matches(".jsonl"),
                )?;
                if value["type"] == "ai-title" {
                    ai_title = Some(claude_ai_title(
                        &value,
                        path.rsplit('/')
                            .next()
                            .unwrap_or("")
                            .trim_end_matches(".jsonl"),
                    )?);
                    recognized = true;
                }
                recognized |= matches!(
                    value["type"].as_str(),
                    Some(
                        "user"
                            | "assistant"
                            | "custom-title"
                            | "summary"
                            | "file-history-snapshot"
                            | "queue-operation"
                            | "progress"
                            | "system",
                    )
                );
                if value["type"] == "custom-title" {
                    explicit_title = value["customTitle"].as_str().map(str::to_owned);
                }
                if title.is_none() && value["type"] == "user" && value["isMeta"] != true {
                    let candidate = content(&value["message"]["content"]);
                    if !candidate.is_empty() {
                        title = Some(candidate);
                    }
                }
                value["cwd"].as_str()
            }
            CliKind::Codex => {
                if value["type"] == "session_meta" {
                    recognized = true;
                    let observed_id = value["payload"]["id"].as_str().ok_or("SOURCE_INVALID")?;
                    if id.as_ref().is_some_and(|old| old != observed_id) {
                        return Err("SOURCE_INVALID");
                    }
                    id = Some(observed_id.to_owned());
                }
                if title.is_none() {
                    if value["type"] == "event_msg" && value["payload"]["type"] == "user_message" {
                        title = value["payload"]["message"].as_str().map(str::to_owned);
                    } else if value["type"] == "response_item"
                        && value["payload"]["type"] == "message"
                        && value["payload"]["role"] == "user"
                    {
                        let candidate = content(&value["payload"]["content"]);
                        if !candidate.is_empty() {
                            title = Some(candidate);
                        }
                    }
                }
                if value["type"] == "session_meta" {
                    value["payload"]["cwd"].as_str()
                } else {
                    None
                }
            }
            CliKind::Shell => return Err("SOURCE_UNSUPPORTED"),
        };
        if let Some(observed) = observed_cwd {
            if cwd.as_ref().is_some_and(|old| old != observed) {
                return Err("SOURCE_AMBIGUOUS");
            }
            cwd = Some(observed.to_owned());
        }
    }
    if !recognized {
        return Err("SOURCE_UNSUPPORTED");
    }
    let id = match cli {
        CliKind::Claude => path
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim_end_matches(".jsonl")
            .to_owned(),
        CliKind::Codex => id.ok_or("SOURCE_UNSUPPORTED")?,
        CliKind::Shell => return Err("SOURCE_UNSUPPORTED"),
    };
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err("SOURCE_INVALID");
    }
    let title_priority = if explicit_title.is_some() {
        Some(TitlePriority::Custom)
    } else if ai_title.is_some() {
        Some(TitlePriority::Ai)
    } else if title.is_some() {
        Some(TitlePriority::Prompt)
    } else {
        None
    };
    Ok(Transcript {
        id,
        cwd,
        title: explicit_title
            .or(ai_title)
            .or(title)
            .unwrap_or_else(|| "Untitled".into()),
        updated: None,
        messages: Vec::new(),
        metadata_incomplete: true,
        title_priority,
        source_path: path.into(),
        observation_bytes: 0,
    })
}
fn content(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.into();
    }
    v.as_array()
        .map(|a| {
            a.iter()
                .filter(|part| {
                    matches!(
                        part["type"].as_str(),
                        Some("text" | "input_text" | "output_text")
                    )
                })
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../../tests/native_cli_history_metadata.rs"]
mod tests;
