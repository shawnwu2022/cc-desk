use serde_json::json;

use crate::store::{
    extract_md_description, extract_session_name, find_name_separator, merge_json_values,
    parse_agents_list_output, parse_mcp_list_output, parse_timestamp, AgentInfo,
};

// ==================== merge_json_values ====================

// updates 新增 key 到 base: merge({"a":1}, {"b":2}) 包含 a 和 b
#[test]
fn MergeJson_NewKey_001() {
    let base = json!({"a": 1});
    let updates = json!({"b": 2});
    let result = merge_json_values(base, updates);
    assert_eq!(result["a"], 1);
    assert_eq!(result["b"], 2);
}

// updates 覆盖已有 key: merge({"a":1}, {"a":2}) → {"a":2}
#[test]
fn MergeJson_Overwrite_001() {
    let base = json!({"a": 1});
    let updates = json!({"a": 2});
    let result = merge_json_values(base, updates);
    assert_eq!(result["a"], 2);
    assert_eq!(result.as_object().unwrap().len(), 1);
}

// null 值 update 删除 base 中对应的 key: merge({"a":1,"b":2}, {"a":null}) → {"b":2}
#[test]
fn MergeJson_NullDelete_001() {
    let base = json!({"a": 1, "b": 2});
    let updates = json!({"a": null});
    let result = merge_json_values(base, updates);
    assert_eq!(result.as_object().unwrap().len(), 1);
    assert_eq!(result["b"], 2);
    assert!(result.get("a").is_none());
}

// 非 object 的 updates 替换整个 base: merge({"a":1}, "text") → "text"
#[test]
fn MergeJson_PrimitiveReplace_001() {
    let base = json!({"a": 1});
    let updates = json!("text");
    let result = merge_json_values(base, updates);
    assert_eq!(result, json!("text"));
}

// 空 updates 返回 base 不变: merge({"a":1}, {}) → {"a":1}
#[test]
fn MergeJson_EmptyUpdate_001() {
    let base = json!({"a": 1});
    let updates = json!({});
    let result = merge_json_values(base, updates);
    assert_eq!(result["a"], 1);
    assert_eq!(result.as_object().unwrap().len(), 1);
}

// ==================== parse_mcp_list_output ====================

// 解析 HTTP 服务器行，提取 name、url、type 为 http
#[test]
fn ParseMcp_HttpServer_001() {
    let input = "zread: https://open.bigmodel.cn/api/mcp/zread/mcp (HTTP) - connected";
    let servers = parse_mcp_list_output(input);
    assert_eq!(servers.len(), 1);
    let s = &servers[0];
    assert_eq!(s.name, "zread");
    assert_eq!(
        s.url.as_deref(),
        Some("https://open.bigmodel.cn/api/mcp/zread/mcp")
    );
    assert_eq!(s.command, None);
}

// 解析 stdio 服务器行，提取 name、command，type 为 stdio
#[test]
fn ParseMcp_StdioServer_001() {
    let input = "myserver: npx -y @my/mcp-server - running";
    let servers = parse_mcp_list_output(input);
    assert_eq!(servers.len(), 1);
    let s = &servers[0];
    assert_eq!(s.name, "myserver");
    assert!(s.command.is_some());
    assert_eq!(s.url, None);
}

// 解析 plugin: 前缀的服务器，scope 为 plugin
#[test]
fn ParseMcp_PluginScope_001() {
    let input = "plugin:paper-tool:paper-search: uv run mcp_server.py - connected";
    let servers = parse_mcp_list_output(input);
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].scope, "plugin");
}

// 不含 " - " 分隔符的标题行被跳过
#[test]
fn ParseMcp_SkipHeader_001() {
    let input = "MCP Servers:\n  some header line\nzread: https://example.com (HTTP) - connected";
    let servers = parse_mcp_list_output(input);
    assert_eq!(servers.len(), 1);
}

// 空字符串输入返回空 Vec
#[test]
fn ParseMcp_EmptyInput_001() {
    let servers = parse_mcp_list_output("");
    assert!(servers.is_empty());
}

// ==================== find_name_separator ====================

// HTTP 模式跳过 :// 找到 name 与 url 之间的冒号
#[test]
fn FindSeparator_HttpMode_001() {
    let input = "zread: https://open.bigmodel.cn/api/mcp/zread/mcp (HTTP)";
    let pos = find_name_separator(input, "HTTP");
    assert!(pos.is_some());
    let idx = pos.unwrap();
    assert_eq!(&input[..idx], "zread");
}

// stdio 模式找到 name 与 command 之间的冒号
#[test]
fn FindSeparator_StdioMode_001() {
    let input = "myserver: npx -y @my/mcp-server";
    let pos = find_name_separator(input, "stdio");
    assert!(pos.is_some());
    let idx = pos.unwrap();
    assert_eq!(&input[..idx], "myserver");
}

// 跳过 Windows 盘符冒号 C: 不误判为分隔符
#[test]
fn FindSeparator_WindowsDrive_001() {
    let input = "myserver: C:/path/to/binary --arg1";
    let pos = find_name_separator(input, "stdio");
    assert!(pos.is_some());
    let idx = pos.unwrap();
    assert_eq!(&input[..idx], "myserver");
}

// 无冒号的字符串返回 None
#[test]
fn FindSeparator_NoColon_001() {
    let input = "just-plain-text no separator";
    let pos = find_name_separator(input, "stdio");
    assert!(pos.is_none());
}

// 跳过 :// 不在 URL 协议冒号处分割
#[test]
fn FindSeparator_UrlPrefix_001() {
    let input = "server: https://example.com/path (HTTP)";
    let pos = find_name_separator(input, "HTTP");
    assert!(pos.is_some());
    let idx = pos.unwrap();
    assert_eq!(&input[..idx], "server");
}

// ==================== parse_agents_list_output ====================

// 解析 Built-in agents 段，source_type 为 builtin
#[test]
fn ParseAgents_Builtin_001() {
    let input = "Built-in agents:\n  claude-code-guide · haiku\n  Explore · inherit";
    let mut agents: Vec<AgentInfo> = Vec::new();
    parse_agents_list_output(input, &mut agents);
    assert!(!agents.is_empty());
    let builtin_agents: Vec<&AgentInfo> = agents
        .iter()
        .filter(|a| a.source_type == "builtin")
        .collect();
    assert!(builtin_agents.len() >= 2);
    assert_eq!(builtin_agents[0].name, "claude-code-guide");
    assert_eq!(builtin_agents[0].model.as_deref(), Some("haiku"));
}

// 解析 Plugin agents 段，source_type 为 plugin
#[test]
fn ParseAgents_Plugin_001() {
    let input = "Plugin agents:\n  paper-tool:paper-search · inherit";
    let mut agents: Vec<AgentInfo> = Vec::new();
    parse_agents_list_output(input, &mut agents);
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].source_type, "plugin");
    assert_eq!(agents[0].name, "paper-tool:paper-search");
}

// 空输入返回空 Vec 不崩溃
#[test]
fn ParseAgents_EmptyInput_001() {
    let mut agents: Vec<AgentInfo> = Vec::new();
    parse_agents_list_output("", &mut agents);
    assert!(agents.is_empty());
}

// ==================== extract_md_description ====================

// YAML frontmatter 中有 description 字段时提取其值
#[test]
fn ExtractMd_Frontmatter_001() {
    let content = "---\ndescription: This is a skill\n---\n# Skill Title\nBody text";
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, content).unwrap();
    let result = extract_md_description(&file_path);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "This is a skill");
}

// 无 frontmatter 时取第一个非空非标题行作为描述
#[test]
fn ExtractMd_BodyFallback_001() {
    let content = "# Title\n\nFirst body line is the description\nMore text";
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, content).unwrap();
    let result = extract_md_description(&file_path);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "First body line is the description");
}

// frontmatter 描述超过 200 字符时截断并加省略号
#[test]
fn ExtractMd_FrontmatterTruncate_001() {
    let long_desc: String = "x".repeat(250);
    let content = format!("---\ndescription: {}\n---\nBody", long_desc);
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, content).unwrap();
    let result = extract_md_description(&file_path).unwrap();
    assert!(result.ends_with("..."));
    assert!(result.len() <= 203); // 200 chars + "..."
}

// 正文描述超过 100 字符时截断并加省略号
#[test]
fn ExtractMd_BodyTruncate_001() {
    let long_body: String = "a".repeat(150);
    let content = format!("# Title\n\n{}", long_body);
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, content).unwrap();
    let result = extract_md_description(&file_path).unwrap();
    assert!(result.ends_with("..."));
    assert!(result.len() <= 103); // 100 chars + "..."
}

// 空内容返回 "No description"
#[test]
fn ExtractMd_EmptyContent_001() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, "").unwrap();
    let result = extract_md_description(&file_path);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "No description");
}

// ==================== parse_timestamp ====================

// 有效 ISO 8601 时间戳返回非零毫秒值
#[test]
fn ParseTimestamp_ValidIso_001() {
    let ts = "2024-01-15T10:30:00Z";
    let result = parse_timestamp(ts);
    assert_ne!(result, 0);
}

// "not-a-date" 返回 0
#[test]
fn ParseTimestamp_InvalidString_001() {
    let result = parse_timestamp("not-a-date");
    assert_eq!(result, 0);
}

// 空字符串返回 0
#[test]
fn ParseTimestamp_EmptyString_001() {
    let result = parse_timestamp("");
    assert_eq!(result, 0);
}

// ==================== extract_session_name ====================

// 多条用户消息时返回第一条有效消息，而非最后一条
#[test]
fn ExtractSessionName_FirstUserMessage_001() {
    let lines = vec![
        r#"{"type":"user","message":{"content":"First prompt here"},"isMeta":false}"#,
        r#"{"type":"assistant","message":{"content":"response"}}"#,
        r#"{"type":"user","message":{"content":"Second prompt here"},"isMeta":false}"#,
        r#"{"type":"user","message":{"content":"Third prompt here"},"isMeta":false}"#,
    ];
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("session.jsonl");
    std::fs::write(&file_path, lines.join("\n")).unwrap();
    let result = extract_session_name(&file_path);
    assert_eq!(result, "First prompt here");
}

// custom-title 优先级高于用户消息
#[test]
fn ExtractSessionName_CustomTitlePriority_001() {
    let lines = vec![
        r#"{"type":"user","message":{"content":"User message"},"isMeta":false}"#,
        r#"{"type":"custom-title","customTitle":"My Custom Title"}"#,
    ];
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("session.jsonl");
    std::fs::write(&file_path, lines.join("\n")).unwrap();
    let result = extract_session_name(&file_path);
    assert_eq!(result, "My Custom Title");
}

// isMeta=true 的消息被过滤，不作为名称
#[test]
fn ExtractSessionName_SkipMeta_001() {
    let lines = vec![
        r#"{"type":"user","message":{"content":"meta prompt"},"isMeta":true}"#,
        r#"{"type":"user","message":{"content":"real prompt"},"isMeta":false}"#,
    ];
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("session.jsonl");
    std::fs::write(&file_path, lines.join("\n")).unwrap();
    let result = extract_session_name(&file_path);
    assert_eq!(result, "real prompt");
}

// 以 < 开头的系统注入消息被过滤
#[test]
fn ExtractSessionName_SkipSystemInject_001() {
    let lines = vec![
        r#"{"type":"user","message":{"content":"<system-reminder>some system text</system-reminder>"},"isMeta":false}"#,
        r#"{"type":"user","message":{"content":"actual user message"},"isMeta":false}"#,
    ];
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("session.jsonl");
    std::fs::write(&file_path, lines.join("\n")).unwrap();
    let result = extract_session_name(&file_path);
    assert_eq!(result, "actual user message");
}

// 超过 50 字符的消息被截断并加省略号
#[test]
fn ExtractSessionName_TruncateLong_001() {
    let long_msg: String = "a".repeat(60);
    let lines = vec![format!(
        r#"{{"type":"user","message":{{"content":"{}"}},"isMeta":false}}"#,
        long_msg
    )];
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("session.jsonl");
    std::fs::write(&file_path, lines.join("\n")).unwrap();
    let result = extract_session_name(&file_path);
    assert!(result.ends_with("..."));
    // 50 chars + "..." = 53
    assert_eq!(result.len(), 53);
}

// 无用户消息也无 custom-title 时返回 "Unnamed session"
#[test]
fn ExtractSessionName_NoMessages_001() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("session.jsonl");
    std::fs::write(&file_path, "").unwrap();
    let result = extract_session_name(&file_path);
    assert_eq!(result, "Unnamed session");
}
