use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

struct Transcript {
    id: String,
    cwd: Option<String>,
    title: String,
    updated: Option<String>,
    messages: Vec<(String, String)>,
    metadata_incomplete: bool,
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
        let parsed = (|| {
            let observation = if o.kind == ResourceKind::History {
                c.history_prefix(&path, b)?
            } else {
                c.bytes(c.root, &path, b)?.map(|bytes| (bytes, false))
            };
            let Some((bytes, incomplete)) = observation else {
                return Err("SOURCE_CHANGED");
            };
            if incomplete {
                let end = bytes
                    .iter()
                    .rposition(|b| *b == b'\n')
                    .ok_or("SOURCE_TOO_LARGE")?;
                parse_metadata(&path, c.cli, text(&bytes[..=end])?)
            } else {
                parse(&path, c.cli, text(&bytes)?)
            }
        })();
        let transcript = match parsed {
            Ok(transcript) => transcript,
            Err(code) if o.kind == ResourceKind::History && b.omit_history_entry(code) => {
                c.check()?;
                continue;
            }
            Err(code) => return Err(code),
        };
        if !seen.insert(transcript.id.clone()) {
            return Err("SOURCE_AMBIGUOUS");
        }
        if c.project.is_some()
            && !transcript
                .cwd
                .as_ref()
                .is_some_and(|cwd| c.project_paths.iter().any(|p| p == Path::new(cwd)))
        {
            continue;
        }
        transcripts.push(transcript);
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
    };
    let mut recognized = false;
    let mut explicit_title = None;
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
                if let Some(cwd) = v.get("cwd").and_then(Value::as_str) {
                    t.cwd = Some(cwd.to_owned());
                }
                match v.get("type").and_then(Value::as_str) {
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
                        let message = content(&v["message"]["content"]);
                        if !message.is_empty() {
                            t.messages.push((role.into(), message));
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
                        let message = content(&v["payload"]["content"]);
                        if !message.is_empty() {
                            t.messages.push((role.into(), message));
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
    t.title = explicit_title
        .or_else(|| {
            t.messages
                .iter()
                .find(|(role, _)| role == "user")
                .map(|(_, text)| text.clone())
        })
        .unwrap_or_else(|| "Untitled".into());
    Ok(t)
}
// A bounded prefix is an observation of identity/title, not a complete message
// stream. Do not compare paired event/response streams or claim its latest time.
fn parse_metadata(path: &str, cli: CliKind, input: &str) -> ReadResult<Transcript> {
    let mut id = None;
    let mut cwd = None;
    let mut title = None;
    let mut explicit_title = None;
    let mut recognized = false;
    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|_| "SOURCE_INVALID")?;
        validate_value(&value)?;
        if !value.is_object() {
            return Err("SOURCE_INVALID");
        }
        let observed_cwd = match cli {
            CliKind::Claude => {
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
    Ok(Transcript {
        id,
        cwd,
        title: explicit_title
            .or(title)
            .unwrap_or_else(|| "Untitled".into()),
        updated: None,
        messages: Vec::new(),
        metadata_incomplete: true,
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
