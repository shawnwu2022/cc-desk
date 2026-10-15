use super::super::cache;
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
/// 有界标题尾读的结果：优先标题候选，以及尾段观察到的最新记录时间。
type BoundedTitleResult = (Option<(String, TitlePriority)>, Option<String>);
struct Transcript {
    id: String,
    cwd: Option<String>,
    title: String,
    updated: Option<String>,
    messages: Vec<(String, String)>,
    metadata_incomplete: bool,
    title_priority: Option<TitlePriority>,
    /// codex 来源标记表明该 rollout 由 subagent 或自动流程创建（非用户主动创建）。
    auto_session: bool,
    /// wrapper-only 且观察不完整：不占用列表行，由调用方按单条省略披露。
    unconfirmable_wrapper_only: bool,
    /// 完整观察到的输入仅为退出命令，且无用户任务或 assistant/工具工作。
    envelope_only: bool,
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
    // 用户自己设定的会话名优先于现抽标题；只在历史列表展示中生效。
    let named = if c.cli == CliKind::Codex && o.kind == ResourceKind::History {
        codex_session_names(c, b)
    } else {
        std::collections::HashMap::new()
    };
    let mut files = Vec::new();
    match c.cli {
        CliKind::Claude => walk(c, "projects", 2, o.kind, b, &mut files)?,
        CliKind::Codex => {
            walk(c, "sessions", 4, o.kind, b, &mut files)?;
            walk(c, "archived_sessions", 4, o.kind, b, &mut files)?;
        }
        CliKind::Shell => unreachable!(),
    }
    let mut transcripts: Vec<(Transcript, Option<String>)> = Vec::new();
    let mut seen = BTreeSet::new();
    // codex fork/compact 为同一 thread 追加 rollout，同 id 是合法 thread 模型；
    // Claude 文件名即身份，跨目录同 id 仍是来源级异常。
    let mut codex_representatives = std::collections::HashMap::<String, usize>::new();
    for path in files {
        let mut observation_bytes = 0;
        // Ok(None) 表示这一行不是用户主动创建的会话，直接跳过而不计入任何失败。
        let parsed: ReadResult<Option<Transcript>> = (|| {
            // Codex 历史走缓存 + 一次打开的头窗口观察：身份取自 session_meta，
            // 标题取自首条用户消息，时间取自尾部。命中缓存仍校验当前身份。
            if o.kind == ResourceKind::History && c.cli == CliKind::Codex {
                return codex_history(c, &path, b, &mut observation_bytes);
            }
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
                Ok(Some(transcript))
            } else if o.kind == ResourceKind::History {
                parse(&path, c.cli, text(&bytes)?).map(Some)
            } else {
                parse_complete(&path, c.cli, text(&bytes)?, true).map(Some)
            }
        })();
        let mut transcript = match parsed {
            Ok(Some(transcript)) => transcript,
            Ok(None) => continue,
            Err(code) if o.kind == ResourceKind::History && b.omit_history_entry(code) => {
                continue;
            }
            Err(code) => return Err(code),
        };
        // 行是否展示不能削弱全源身份检查，退出行也可能与另一目录的会话重名。
        if c.cli == CliKind::Claude && !seen.insert(transcript.id.clone()) {
            return Err("SOURCE_AMBIGUOUS");
        }
        // 只有完整观察才能证明「从未有过真实用户输入」；有界观察未覆盖全程时
        // 照常列出（fail-open），不因未读到标题而隐藏真实会话。
        if o.kind == ResourceKind::History
            && transcript.envelope_only
            && !transcript.metadata_incomplete
        {
            continue;
        }
        transcript.observation_bytes = observation_bytes;
        if let Some(name) = named.get(&transcript.id) {
            // 用户自己起的名字就是这条会话的身份：优先于任何现抽标题，且不受
            // 头窗口是否覆盖用户消息影响。
            transcript.title = name.clone();
            transcript.title_priority = Some(TitlePriority::Custom);
        }
        // 一次观察服务多个已注册项目：cwd 唯一归属一个项目；无归属行不进入任何项目。
        let owner = if c.projects.is_empty() {
            None
        } else {
            match transcript.cwd.as_ref().and_then(|cwd| {
                c.projects
                    .iter()
                    .find(|p| p.paths.iter().any(|path| path == Path::new(cwd)))
            }) {
                Some(project) => Some(project.id.to_owned()),
                None => {
                    if o.kind == ResourceKind::History && transcript.cwd.is_none() {
                        // 无 cwd 观察的行无法证明不属于任何注册项目：按单文件省略
                        // 披露（与 wrapper-only 同语义），只置 incomplete 不记失败
                        // 会让「列表可能不完整」横幅对这种真实省略失明。
                        b.omit_history_entry("SOURCE_UNSUPPORTED");
                    }
                    continue;
                }
            }
        };
        if c.cli == CliKind::Codex {
            if let Some(&index) = codex_representatives.get(&transcript.id) {
                // 最新文件代表 thread 当前状态，与 codex resume 语义一致；
                // updated 相同（含都缺失）时保留先到的代表。
                let replaces = match (&transcript.updated, &transcripts[index].0.updated) {
                    (Some(new), Some(old)) => new > old,
                    (Some(_), None) => true,
                    (None, _) => false,
                };
                if replaces {
                    transcripts[index] = (transcript, owner);
                }
                continue;
            }
            codex_representatives.insert(transcript.id.clone(), transcripts.len());
        }
        transcripts.push((transcript, owner));
    }
    // Required headers/tree checks finish before optional display enrichment.
    // Only a positively associated requested-project row may spend spare bytes.
    if c.cli == CliKind::Claude && o.kind == ResourceKind::History && !c.projects.is_empty() {
        for (transcript, _) in transcripts.iter_mut() {
            if !transcript.metadata_incomplete {
                continue;
            }
            let enriched = enrich_claude_title(c, transcript, b);
            match enriched {
                Ok((candidate, tail_updated)) => {
                    // 尾读观察到的更新时间总是可采用的证据，与标题候选无关。
                    if let Some(newer) = tail_updated {
                        if transcript.updated.as_ref().is_none_or(|old| newer > *old) {
                            transcript.updated = Some(newer);
                        }
                    }
                    if let Some((title, priority)) = candidate {
                        if transcript.title_priority.is_none_or(|old| priority >= old) {
                            transcript.title = title;
                            transcript.title_priority = Some(priority);
                        }
                    }
                }
                Err(code) if b.omit_history_entry(code) => {}
                Err(code) => return Err(code),
            }
        }
    }
    transcripts.sort_by(|(a, _), (b, _)| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
    let mut result = Vec::new();
    let query = o.query.map(str::to_lowercase);
    for (t, owner) in transcripts {
        // Stable within a native source, not a pathname or a permission token.
        let key = serde_json::to_string(&["local", c.cli.as_str(), c.root.key(), &t.id])
            .map_err(|_| "SOURCE_INVALID")?;
        if o.kind == ResourceKind::History {
            let (title, truncated) = bounded(&t.title, 512);
            // Codex 自身把归档会话移进 archived_sessions/（与 sessions/ 零重叠的
            // move 语义）；代表 rollout 的容器位置就是该 thread 的归档状态。
            let archived = t.source_path.starts_with("archived_sessions/");
            result.push(ResourceItem::Session {
                session_key: key,
                native_session_id: t.id,
                project_id: owner,
                title,
                title_unknown: t.title_priority.is_none().then_some(true),
                title_source: t.title_priority.map(TitlePriority::source),
                metadata_incomplete: t.metadata_incomplete.then_some(true),
                truncated: truncated || t.metadata_incomplete,
                archived,
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
    // 显示元数据缓存只影响后续扫描的读取成本，不改变本次观察结果。
    let _ = cache::with(|cache| cache.flush());
    Ok(result)
}

/// Codex 在 `session_index.jsonl` 里记录用户设定的会话名（每行
/// `{id, thread_name, updated_at}`）。这是**用户意图**，优先级高于从 rollout 现抽
/// 的标题；同一 id 出现多行时（实测 317 行里 10 个 id 有 2 行）取 `updated_at`
/// 最大者，与"最后改名生效"一致。它是展示增强，不是完整性判据：文件只覆盖
/// Desktop/CLI 记录过名字的会话（本机 418 个 rollout 里仅 34 个），缺失一律回落
/// 到现抽标题；损坏行逐行跳过，整个文件不可用则等同于没有该文件。
fn codex_session_names(
    c: &Catalog<'_>,
    b: &mut Budget,
) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    // 身份文件本身不可读（CLI 版本没有该文件）不是历史读取失败。
    let Ok(Some(bytes)) = c.bytes(c.root, "session_index.jsonl", b) else {
        return HashMap::new();
    };
    let Ok(text) = text(&bytes) else {
        return HashMap::new();
    };
    // id -> (updated_at, name)：只保留 updated_at 最大的一行。
    let mut best: HashMap<String, (String, String)> = HashMap::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(id) = value["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        else {
            continue;
        };
        let Some(name) = value["thread_name"]
            .as_str()
            .filter(|name| !name.contains('\0'))
        else {
            continue;
        };
        let display = display_title(name);
        // display_title 已折叠空白；残留的控制字符（ESC 等）不是会话名，
        // 与 id 校验同样拒绝，回落到现抽标题。
        if display.is_empty() || display.chars().any(char::is_control) {
            continue;
        }
        let stamp = value["updated_at"].as_str().unwrap_or_default().to_owned();
        match best.get_mut(id) {
            // 同一 id 多行：只有更新的行才能覆盖，否则保留先到的标题。
            Some(entry) => {
                if stamp > entry.0 {
                    *entry = (stamp, display);
                }
            }
            None => {
                best.insert(id.to_owned(), (stamp, display));
            }
        }
    }
    best.into_iter().map(|(id, (_, name))| (id, name)).collect()
}

/// Codex 缓存只复用显示字段：命中后仍从持有文件读取首个完整身份记录，
/// 身份/归属/来源不匹配即重新观察，不能把 size/mtime 相等当作缺失证明。
fn codex_history(
    c: &Catalog<'_>,
    path: &str,
    b: &mut Budget,
    observation_bytes: &mut usize,
) -> ReadResult<Option<Transcript>> {
    let relative = Path::new(path);
    let Some((len, modified)) = c.root.stat(relative)? else {
        return Ok(None);
    };
    let Some(stamps) = cache::Stamps::from_system(len, modified) else {
        return Err("SOURCE_CHANGED");
    };
    let key = cache::key(c.root.key(), c.cli.as_str(), relative);
    if let Some(metadata) = cache::with(|cache| cache.lookup(&key, &stamps).cloned()) {
        // 恢复/同步可保留 size/mtime；每次从已准入且持有的文件证明当前首个身份。
        let mut identity_probe = |bytes: &[u8]| {
            Ok(bytes.split_inclusive(|byte| *byte == b'\n').any(|line| {
                line.last() == Some(&b'\n')
                    && serde_json::from_slice::<Value>(line).is_ok_and(|value| {
                        value["type"] == "session_meta" && value["payload"]["id"].is_string()
                    })
            }))
        };
        let Some((header, incomplete)) = c.root.history_until(relative, b, &mut identity_probe)?
        else {
            cache::with(|cache| cache.forget(&key));
            return Err("SOURCE_CHANGED");
        };
        // 整个块仍计入预算，只解析最后完整记录前的字节，避免后续 UTF-8 字符切半。
        let complete = header
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        let observed = parse_codex_metadata(text(&header[..complete])?, None, !incomplete)?;
        if observed.id == metadata.native_id
            && observed.cwd == metadata.cwd
            && !observed.auto_session
            && !observed.unconfirmable_wrapper_only
        {
            *observation_bytes = header.len();
            let mut transcript = cached_transcript(metadata);
            // 归档状态由代表 rollout 的容器位置决定，必须保留真实相对路径。
            transcript.source_path = path.to_owned();
            // 缓存命中的行若上次只观察到文件前缀，这个事实必须回到来源级标记：
            // 否则同一文件冷读标记不完整、暖读漏记，部分观察被升级成缺失证明。
            if transcript.metadata_incomplete {
                b.retain_history_metadata_incomplete();
            }
            return Ok(Some(transcript));
        }
        cache::with(|cache| cache.forget(&key));
    }
    // 观察者在确认「非用户创建的会话」时立即停止，省下该文件其余头窗口的读取。
    let auto = std::cell::Cell::new(false);
    let mut probe = codex_metadata_probe(&auto);
    let Some((head, tail, incomplete)) = c.root.history_codex_metadata(relative, b, &mut probe)?
    else {
        cache::with(|cache| cache.forget(&key));
        return Err("SOURCE_CHANGED");
    };
    let mut transcript = parse_codex_metadata(text(&head)?, text(&tail).ok(), !incomplete)?;
    if transcript.auto_session {
        cache::with(|cache| cache.forget(&key));
        return Ok(None);
    }
    // wrapper-only 且观察不完整：几乎全为 exec/SDK 自动会话，Untitled 行只是
    // 噪声；但前缀不能证明全会话只含包装，因此按单文件省略披露（来源不完整 +
    // 读失败计数）而不是伪装成无此会话，Messages 仍可按显式 id 读取。
    if transcript.unconfirmable_wrapper_only {
        cache::with(|cache| cache.forget(&key));
        b.omit_history_entry("SOURCE_UNSUPPORTED");
        return Ok(None);
    }
    transcript.metadata_incomplete = incomplete;
    transcript.source_path = path.to_owned();
    *observation_bytes = head.len();
    cache::with(|cache| {
        cache.record(
            key,
            stamps,
            cache::CachedMetadata {
                native_id: transcript.id.clone(),
                cwd: transcript.cwd.clone(),
                title: transcript.title.clone(),
                title_priority: transcript.title_priority.map(|p| p as u8),
                updated: transcript.updated.clone(),
                metadata_incomplete: incomplete,
            },
        )
    });
    Ok(Some(transcript))
}

fn cached_transcript(metadata: cache::CachedMetadata) -> Transcript {
    let title_priority = match metadata.title_priority {
        Some(0) => Some(TitlePriority::Prompt),
        Some(1) => Some(TitlePriority::Ai),
        Some(2) => Some(TitlePriority::Custom),
        _ => None,
    };
    Transcript {
        id: metadata.native_id,
        cwd: metadata.cwd,
        title: metadata.title,
        updated: metadata.updated,
        messages: Vec::new(),
        metadata_incomplete: metadata.metadata_incomplete,
        title_priority,
        auto_session: false,
        unconfirmable_wrapper_only: false,
        envelope_only: false,
        source_path: String::new(),
        observation_bytes: 0,
    }
}

/// Codex 头窗口观察者：确认已见 session_meta（身份）与首条用户消息（标题来源）
/// 后即可停止读取。逐行解析失败只跳过该行，不使整次观察失败。若首个
/// session_meta 已表明该 rollout 由 subagent 或自动流程创建，则不再继续读取，
/// 并把该结论写入 `auto`（这类会话不进历史列表，其头窗口无需展开）。
pub(super) fn codex_metadata_probe(
    auto: &std::cell::Cell<bool>,
) -> impl FnMut(&[u8]) -> ReadResult<bool> + '_ {
    let mut line_start = 0usize;
    let mut scanned = 0usize;
    let mut seen_meta = false;
    let mut seen_user = false;
    move |bytes: &[u8]| {
        for position in scanned..bytes.len() {
            if bytes[position] != b'\n' {
                continue;
            }
            let Ok(line) = std::str::from_utf8(&bytes[line_start..position]) else {
                line_start = position + 1;
                continue;
            };
            line_start = position + 1;
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            match value["type"].as_str() {
                Some("session_meta") => {
                    seen_meta = true;
                    if is_auto_session(&value["payload"]) {
                        auto.set(true);
                        return Ok(true);
                    }
                }
                Some("event_msg") if value["payload"]["type"] == "user_message" => {
                    let message = value["payload"]["message"].as_str().unwrap_or_default();
                    if !message.is_empty()
                        && !is_injected_user_context(message)
                        && !is_automation_wrapper(message)
                    {
                        seen_user = true;
                    }
                }
                Some("response_item") if value["payload"]["role"] == "user" => {
                    // 注入的上下文（AGENTS.md/环境块）与自动评审/安全扫描的脚手架
                    // 都不算标题来源，不能据此提前停止读取，否则真正的用户消息
                    // （通常在其后）永远读不到。
                    let candidate = content(&value["payload"]["content"]);
                    if !candidate.is_empty()
                        && !is_injected_user_context(&candidate)
                        && !is_automation_wrapper(&candidate)
                    {
                        seen_user = true;
                    }
                }
                _ => {}
            }
        }
        scanned = bytes.len();
        Ok(seen_meta && seen_user)
    }
}

/// 该 rollout 是否由 subagent、自动流程（守护评审、安全扫描、代码评审）或
/// 非交互执行模式创建。判据取自 codex 自己在 session_meta 上写的来源标记：
/// `thread_source` 与 `source.subagent` 是权威取值；`source == "exec"` 是
/// `codex exec` 等非交互入口，CC Desk 只启动交互式 codex，这类 rollout 永远
/// 不是用户在 Desk 里打开的会话，其无 prompt 的 `Untitled` 行只是噪声。
/// 未知取值一律按用户创建处理——隐藏真实用户会话的代价远大于多列一行噪声。
fn is_auto_session(payload: &Value) -> bool {
    if matches!(
        payload["thread_source"].as_str(),
        Some("subagent" | "guardian_review" | "security_scan" | "code_review")
    ) {
        return true;
    }
    if payload["source"].as_str() == Some("exec") {
        return true;
    }
    payload["source"].get("subagent").is_some()
}

/// codex 会把 AGENTS.md、环境上下文等以 role=user 注入到 rollout 中。这些记录
/// 只作展示标题时是噪声，按已知包装前缀排除；真实用户输入不会是这些形式。
fn is_injected_user_context(text: &str) -> bool {
    let head = text.trim_start();
    head.starts_with('<')
        || head.starts_with("# AGENTS.md instructions")
        || head.starts_with("# Environment")
}

/// codex 的自动评审、代码评审与安全扫描插件把整段任务以 role=user 写进 rollout。
/// 这些会话的 thread_source 通常已标为自动来源，但由 `codex exec`/SDK 驱动时仍会
/// 记为 user，因此这里再按实测前缀识别：开头是这些包装、且窗口内没有任何真实用户
/// 消息的会话视为非用户创建。只保留明确任务脚手架的前缀；未知包装最坏作为
/// 会话名显示。普通人工文件引用、评审和限制说明不作为来源判据。
// 文件引用、人工评审或读取限制可由普通用户直接输入，不能作为自动来源证据。
fn is_automation_wrapper(text: &str) -> bool {
    const WRAPPERS: [&str; 5] = [
        "The following is the Codex agent history",
        "## Code review guidelines:",
        "Run one Standard security scan",
        "You are the single serial semantic reducer",
        "You are one independent discovery worker",
    ];
    let head = text.trim_start();
    WRAPPERS.iter().any(|prefix| head.starts_with(prefix))
}

/// 展示标题：取首个非空行、折叠空白并限长。首条用户消息常是整段 prompt，
/// 直接展示会是整屏文本；完整原文仍可经消息读取获得。
fn display_title(raw: &str) -> String {
    const MAX_CHARS: usize = 96;
    let line = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let mut out: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// 从头窗口提取身份/归属/标题，从尾部提取最近原生时间戳。头窗口覆盖任意记录，
/// 而真实 codex rollout 的字符串值可能含裸换行（把一条记录拆成无法解析的片段），
/// 因此逐行解析失败只跳过该行。身份只取首条 session_meta（与文件名、
/// `codex resume <id>` 一致）；compact/fork 续写的 rollout 可能内嵌其他 thread 的
/// meta，后续出现一律跳过，不视为来源级错误。
fn parse_codex_metadata(
    head: &str,
    tail: Option<&str>,
    observed_completely: bool,
) -> ReadResult<Transcript> {
    let mut id = None;
    let mut cwd = None;
    let mut title = None;
    let mut auto_session = false;
    let mut wrapper_only = false;
    for line in head.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if !value.is_object() {
            continue;
        }
        match value["type"].as_str() {
            Some("session_meta") if id.is_none() => {
                let Some(observed) = value["payload"]["id"].as_str() else {
                    continue;
                };
                id = Some(observed.to_owned());
                auto_session = is_auto_session(&value["payload"]);
                cwd = value["payload"]["cwd"]
                    .as_str()
                    .filter(|path| !path.contains('\0'))
                    .map(str::to_owned)
                    .or(cwd);
            }
            Some("event_msg") if title.is_none() && value["payload"]["type"] == "user_message" => {
                title = value["payload"]["message"]
                    .as_str()
                    .filter(|text| !text.contains('\0'))
                    .and_then(|text| {
                        let display = user_title(text, &mut wrapper_only);
                        (!display.is_empty()).then_some(display)
                    });
            }
            Some("response_item") if title.is_none() && value["payload"]["role"] == "user" => {
                let candidate = content(&value["payload"]["content"]);
                if !candidate.is_empty() && !candidate.contains('\0') {
                    let display = user_title(&candidate, &mut wrapper_only);
                    if !display.is_empty() {
                        title = Some(display);
                    }
                }
            }
            _ => {}
        }
    }
    let id = id.ok_or("SOURCE_UNSUPPORTED")?;
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err("SOURCE_INVALID");
    }
    let updated = tail.and_then(latest_timestamp);
    let title_priority = title.as_ref().map(|_| TitlePriority::Prompt);
    Ok(Transcript {
        id,
        cwd,
        title: title.unwrap_or_else(|| "Untitled".into()),
        updated,
        messages: Vec::new(),
        metadata_incomplete: false,
        title_priority,
        // 只含自动评审/安全扫描脚手架、且观察到完整文件的会话，与 thread_source
        // 已标记的自动会话同等处理。头窗口只是前缀：部分观察的 wrapper-only 不能
        // 证明也不该直接列出——由调用方按省略披露处理。
        auto_session: auto_session
            || (wrapper_only && title_priority.is_none() && observed_completely),
        unconfirmable_wrapper_only: wrapper_only
            && title_priority.is_none()
            && !observed_completely,
        envelope_only: false,
        source_path: String::new(),
        observation_bytes: 0,
    })
}

/// 展示标题候选：注入上下文与自动脚手架都不是标题来源，`wrapper_only` 记录本次
/// 观察中是否只见到这两类记录（用于判定会话是否由用户主动创建）。
fn user_title(text: &str, wrapper_only: &mut bool) -> String {
    if is_injected_user_context(text) {
        return String::new();
    }
    if is_automation_wrapper(text) {
        *wrapper_only = true;
        return String::new();
    }
    display_title(text)
}

/// 尾部最近时间戳。尾部读取已保证从完整行开始（不完整的首行在字节层丢弃），
/// 这里逐行解析并取最大时间戳。
fn latest_timestamp(tail: &str) -> Option<String> {
    tail.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| value["timestamp"].as_str().map(str::to_owned))
        .filter(|stamp| stamp.len() <= 64)
        .max()
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

fn claude_injected_user_message(message: &str) -> bool {
    let head = message.trim_start();
    head.starts_with("<local-command-caveat>") || head.starts_with("<local-command-stdout>")
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

/// Claude Code 把 slash 命令调用（/model、/clear、/diagnosing-bugs 等）写成首条
/// user 记录的 XML 封套。原始封套不是可读标题；提取命令名与参数合成紧凑标题。
/// 返回 None 表示该记录不是命令封套（可能仍是注入噪声，由调用方按 `<` 前缀排除）。
fn claude_envelope_title(text: &str) -> Option<String> {
    let head = text.trim_start();
    if !head.starts_with("<command-name>") && !head.starts_with("<command-message>") {
        return None;
    }
    let field = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = head.find(&open)? + open.len();
        let end = head[start..].find(&close)? + start;
        Some(head[start..end].trim().to_owned())
    };
    let name = field("command-name").or_else(|| field("command-message"))?;
    let args = field("command-args").unwrap_or_default();
    let compact = if args.is_empty() {
        name
    } else {
        format!("{name} {args}")
    };
    Some(display_title(&compact))
}

fn enrich_claude_title(
    catalog: &Catalog<'_>,
    transcript: &Transcript,
    budget: &mut Budget,
) -> ReadResult<BoundedTitleResult> {
    let Some((header, tail, cut_first)) = catalog.history_title_tail(
        &transcript.source_path,
        transcript.observation_bytes,
        budget,
    )?
    else {
        return Ok((None, None));
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
    let mut tail_updated: Option<String> = None;
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
        // 同一转录内的 cwd 会随会话进入子目录而变化（例如 root/web、root/mobile/android），
        // 首段观察到的只是其中一个取值。这里不再比对 cwd：行归属已由首段观察决定，
        // 尾段只用于提取标题，其身份约束由上面的 sessionId 校验承担。
        if let Some(stamp) = value["timestamp"]
            .as_str()
            .filter(|stamp| stamp.len() <= 64)
        {
            tail_updated = std::cmp::max(tail_updated, Some(stamp.to_owned()));
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
    Ok((candidate, tail_updated))
}

// Official SDK 0.2.165 (bundled CLI 2.1.296) demonstrates permission-mode
// before a user record. It carries no cwd and cannot end our observation.
// https://github.com/anthropics/claude-agent-sdk-python/blob/b6e9d12fe1cc98dde988ab7b7713c1feeee50c6c/tests/test_sessions.py#L1399-L1426
//
// 观察终点从「看到 cwd」后移到「cwd + 可用标题来源」。真实会话的首条用户消息常
// 在 cwd 之后很远（实测 depth-2 中位 12.9KiB、p90 965KiB）：旧探针在 cwd 处早停
// 会让标题回落到命令封套或 Untitled。标题来源 = 显式标题记录（ai-title/custom-title）
// 或首条真实用户消息（非 slash 命令封套、非注入噪声）。
fn claude_project_cwd(value: &Value) -> Option<&str> {
    match value["type"].as_str() {
        Some(
            "user"
            | "assistant"
            | "custom-title"
            | "summary"
            | "file-history-snapshot"
            | "queue-operation"
            | "progress"
            | "system",
        ) => value["cwd"].as_str(),
        _ => None,
    }
}

pub(super) fn claude_metadata_probe() -> impl FnMut(&[u8]) -> ReadResult<bool> {
    let mut searched = 0;
    let mut line_start = 0;
    let mut cwd_observed = false;
    let mut titled = false;
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
            let record = value["type"].as_str();
            cwd_observed |= claude_project_cwd(&value).is_some();
            titled |= matches!(record, Some("ai-title" | "custom-title"));
            if !titled && record == Some("user") && value["isMeta"] != true {
                let message = content(&value["message"]["content"]);
                if !message.trim().is_empty()
                    && claude_envelope_title(&message).is_none()
                    && !claude_injected_user_message(&message)
                {
                    titled = true;
                }
            }
        }
        searched = bytes.len();
        Ok(cwd_observed && titled)
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
        auto_session: false,
        unconfirmable_wrapper_only: false,
        envelope_only: false,
        source_path: path.into(),
        observation_bytes: 0,
    };
    let mut recognized = false;
    let mut explicit_title = None;
    let mut ai_title = None;
    let mut codex_id = None;
    let mut event_user = false;
    let mut envelope_fallback: Option<String> = None;
    let mut real_user = false;
    let mut saw_envelope = false;
    let mut only_exit_commands = true;
    let mut substantive_reply = false;
    for line in input.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).map_err(|_| "SOURCE_INVALID")?;
        validate_value(&v)?;
        if !v.is_object() {
            return Err("SOURCE_INVALID");
        }
        match cli {
            CliKind::Claude => {
                claude_observed_session_id(&v, fallback)?;
                if t.cwd.is_none() {
                    // 与有界读取一致：后续子目录或未知记录不能改变会话的项目归属。
                    t.cwd = claude_project_cwd(&v).map(str::to_owned);
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
                        // 工具调用没有可提取的纯文本，但仍然证明会话已经做过工作。
                        if role == "assistant" {
                            let body = &v["message"]["content"];
                            substantive_reply |=
                                body.as_str().is_some_and(|s| !s.trim().is_empty())
                                    || body.as_array().is_some_and(|blocks| !blocks.is_empty());
                        }
                        // History needs only the first user title. Every record is
                        // still decoded and validated; Messages/Search retain all bodies.
                        if include_messages || (role == "user" && t.messages.is_empty()) {
                            let message = content(&v["message"]["content"]);
                            if include_messages {
                                if !message.is_empty() {
                                    t.messages.push((role.into(), message));
                                }
                            } else if let Some(envelope) = claude_envelope_title(&message) {
                                // 命令封套只作兜底：先跳过，等不到真实用户消息时再使用。
                                saw_envelope = true;
                                only_exit_commands &=
                                    matches!(envelope.as_str(), "/exit" | "/quit");
                                if envelope_fallback.is_none() {
                                    envelope_fallback = Some(envelope);
                                }
                            } else if !message.is_empty() {
                                // 注入噪声（<local-command-caveat> 等）不占标题槽位，
                                // 让后续真实用户消息成为标题。
                                if !claude_injected_user_message(&message) {
                                    real_user = true;
                                    t.messages.push((role.into(), message));
                                }
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
    } else if t.messages.iter().any(|(role, _)| role == "user") || envelope_fallback.is_some() {
        Some(TitlePriority::Prompt)
    } else {
        None
    };
    // 仅明确无工作的退出会话可省略。其他 slash 命令可以直接发起任务，
    // assistant 的工具调用及手写 XML 用户输入也不能被视作空会话。
    // 纯元数据文件（无任何 user 记录）不算空会话，保持既有列出行为。
    t.envelope_only = cli == CliKind::Claude
        && saw_envelope
        && only_exit_commands
        && !substantive_reply
        && !real_user
        && explicit_title.is_none()
        && ai_title.is_none()
        && t.messages.iter().all(|(role, _)| role != "user");
    t.title = explicit_title
        .or(ai_title)
        .or_else(|| {
            t.messages
                .iter()
                .find(|(role, _)| role == "user")
                .map(|(_, text)| text.clone())
        })
        .or(envelope_fallback)
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
    let mut observed_updated: Option<String> = None;
    let mut envelope_title: Option<String> = None;
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
                    // slash 命令封套不是可读标题，记为兜底；注入噪声跳过；
                    // 后续真实用户消息优先于封套。
                    if let Some(envelope) = claude_envelope_title(&candidate) {
                        if envelope_title.is_none() {
                            envelope_title = Some(envelope);
                        }
                    } else if !candidate.is_empty() && !claude_injected_user_message(&candidate) {
                        title = Some(candidate);
                    }
                }
                // 只有受支持记录类型的 cwd 才建立项目归属。未知类型（例如未来新增
                // 的记录）携带的 cwd 不作数，也不与已观察值冲突。
                claude_project_cwd(&value)
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
            // Claude 每条记录都带当轮 cwd，进入子目录后合法变化（实测同一转录
            // `E:\qgs\qgs` 与 `E:\qgs\qgs\docs\reviews\...` 并存）。首段观察到的
            // 取值即行归属，后续差异不是来源歧义——与尾段同一语义。Codex 的 cwd
            // 来自唯一 session_meta，其配对逻辑在解析处已单独校验。
            if cwd.is_none() {
                cwd = Some(observed.to_owned());
            }
        }
        // 与 codex 尾部时间同语义：已观察记录的时间是最新部分证据（ISO 字典序即时间序）。
        if let Some(stamp) = value["timestamp"]
            .as_str()
            .filter(|stamp| stamp.len() <= 64)
        {
            observed_updated = std::cmp::max(observed_updated, Some(stamp.to_owned()));
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
    } else if title.is_some() || envelope_title.is_some() {
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
            .or(envelope_title)
            .unwrap_or_else(|| "Untitled".into()),
        updated: observed_updated,
        messages: Vec::new(),
        metadata_incomplete: true,
        title_priority,
        unconfirmable_wrapper_only: false,
        envelope_only: false,
        auto_session: false,
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

