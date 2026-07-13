use serde_json::json;

use crate::store::{
    compute_project_startup_state, expand_env_vars, extract_md_description, extract_session_name,
    find_valid_plugin_path, get_projects_state_at, infer_server_type, merge_json_values,
    normalize_path_inner, parse_agents_list_output, parse_mcp_server_entry, parse_skill_description,
    parse_timestamp, resolve_marketplace_plugin_path, search_session_messages_in_dirs,
    set_agent_enabled_in, set_mcp_server_enabled_in, set_skill_enabled_in,
    update_projects_state_at, write_json_atomic, AgentInfo, AppConfig, Project, ProjectsState,
};

use std::collections::HashMap;
use std::path::Path;

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

// ==================== parse_mcp_server_entry ====================

// 解析 stdio server：带 command/args/env
#[test]
fn ParseMcpEntry_StdioServer_001() {
    let config = json!({
        "command": "npx",
        "args": ["-y", "chrome-devtools-mcp@latest"],
        "env": { "CHROME_PATH": "/usr/bin/chrome" }
    });
    let result = parse_mcp_server_entry("chrome-devtools", &config, "user", None);
    assert!(result.is_some());
    let info = result.unwrap();
    assert_eq!(info.name, "chrome-devtools");
    assert_eq!(info.command.as_deref(), Some("npx"));
    assert_eq!(info.args.as_ref().unwrap().len(), 2);
    assert_eq!(info.args.as_ref().unwrap()[0], "-y");
    assert_eq!(info.env.as_ref().unwrap().get("CHROME_PATH").unwrap(), "/usr/bin/chrome");
    assert_eq!(info.server_type.as_deref(), Some("stdio"));
    assert_eq!(info.source_type, "user");
    assert!(info.url.is_none());
}

// 解析 HTTP server：带 url/headers
#[test]
fn ParseMcpEntry_HttpServer_001() {
    let config = json!({
        "type": "http",
        "url": "https://api.example.com/mcp",
        "headers": { "Authorization": "Bearer token123" }
    });
    let result = parse_mcp_server_entry("zread", &config, "user", None);
    assert!(result.is_some());
    let info = result.unwrap();
    assert_eq!(info.name, "zread");
    assert_eq!(info.url.as_deref(), Some("https://api.example.com/mcp"));
    assert_eq!(info.server_type.as_deref(), Some("http"));
    assert_eq!(info.headers.as_ref().unwrap().get("Authorization").unwrap(), "Bearer token123");
    assert!(info.command.is_none());
}

// 解析 SSE server：带 type:"sse"
#[test]
fn ParseMcpEntry_SseServer_001() {
    let config = json!({
        "type": "sse",
        "url": "https://mcp.example.com/sse"
    });
    let result = parse_mcp_server_entry("slack", &config, "project", None);
    assert!(result.is_some());
    let info = result.unwrap();
    assert_eq!(info.server_type.as_deref(), Some("sse"));
    assert_eq!(info.source_type, "project");
}

// 非对象配置返回 None
#[test]
fn ParseMcpEntry_NotObject_001() {
    let config = json!("just a string");
    let result = parse_mcp_server_entry("test", &config, "user", None);
    assert!(result.is_none());
}

// ==================== infer_server_type ====================

// 有 command 字段无 type → stdio
#[test]
fn InferType_Stdio_001() {
    let config = json!({ "command": "npx", "args": ["-y", "some-package"] });
    assert_eq!(infer_server_type(&config), "stdio");
}

// 有 url + type:"sse" → sse
#[test]
fn InferType_Sse_001() {
    let config = json!({ "type": "sse", "url": "https://example.com/sse" });
    assert_eq!(infer_server_type(&config), "sse");
}

// 有 url + type:"http" → http
#[test]
fn InferType_Http_001() {
    let config = json!({ "type": "http", "url": "https://example.com/mcp" });
    assert_eq!(infer_server_type(&config), "http");
}

// 有 url 无 type → http（默认）
#[test]
fn InferType_UrlNoType_001() {
    let config = json!({ "url": "https://example.com/mcp" });
    assert_eq!(infer_server_type(&config), "http");
}

// 无 command/url → stdio（兜底）
#[test]
fn InferType_Default_001() {
    let config = json!({});
    assert_eq!(infer_server_type(&config), "stdio");
}

// 非 JSON 对象 → stdio（兜底）
#[test]
fn InferType_NonObject_001() {
    let config = json!("string");
    assert_eq!(infer_server_type(&config), "stdio");
}

// ==================== expand_env_vars ====================

// extra_env 中的变量被展开
#[test]
fn ExpandEnvVars_ExtraEnv_001() {
    let mut extra = HashMap::new();
    extra.insert("CLAUDE_PLUGIN_ROOT".to_string(), "C:/plugins/paper".to_string());
    let result = expand_env_vars("${CLAUDE_PLUGIN_ROOT}/sub", Some(&extra));
    assert_eq!(result, "C:/plugins/paper/sub");
}

// ${VAR:-default} 使用默认值
#[test]
fn ExpandEnvVars_Default_001() {
    let result = expand_env_vars("${NONEXISTENT_VAR:-fallback}", None);
    assert_eq!(result, "fallback");
}

// 不含变量的字符串不变
#[test]
fn ExpandEnvVars_NoVars_001() {
    let result = expand_env_vars("plain string", None);
    assert_eq!(result, "plain string");
}

// 多个变量同时展开
#[test]
fn ExpandEnvVars_Multiple_001() {
    std::env::set_var("CC_BOX_TEST_A", "hello");
    let mut extra = HashMap::new();
    extra.insert("CC_BOX_TEST_B".to_string(), "world".to_string());
    let result = expand_env_vars("${CC_BOX_TEST_A}-${CC_BOX_TEST_B}", Some(&extra));
    assert_eq!(result, "hello-world");
    std::env::remove_var("CC_BOX_TEST_A");
}

// plugin scope 中 CLAUDE_PLUGIN_ROOT 被展开到 args
#[test]
fn ParseMcpEntry_PluginEnvExpand_001() {
    let config = json!({
        "command": "uv",
        "args": ["run", "--directory", "${CLAUDE_PLUGIN_ROOT}/paper-search", "mcp_server.py"]
    });
    let mut extra = HashMap::new();
    extra.insert("CLAUDE_PLUGIN_ROOT".to_string(), "C:/plugins/paper-tool".to_string());
    let result = parse_mcp_server_entry("plugin:paper-tool:paper", &config, "plugin", Some(&extra));
    assert!(result.is_some());
    let info = result.unwrap();
    let args = info.args.unwrap();
    assert_eq!(args[2], "C:/plugins/paper-tool/paper-search");
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

// frontmatter 长描述完整返回，不截断不加省略号
#[test]
fn ExtractMd_FrontmatterLongDesc_001() {
    let long_desc: String = "x".repeat(250);
    let content = format!("---\ndescription: {}\n---\nBody", long_desc);
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, content).unwrap();
    let result = extract_md_description(&file_path).unwrap();
    assert_eq!(result, long_desc);
    assert!(!result.ends_with("..."));
}

// 正文长描述完整返回，不截断不加省略号
#[test]
fn ExtractMd_BodyLongDesc_001() {
    let long_body: String = "a".repeat(150);
    let content = format!("# Title\n\n{}", long_body);
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.md");
    std::fs::write(&file_path, content).unwrap();
    let result = extract_md_description(&file_path).unwrap();
    assert_eq!(result, long_body);
    assert!(!result.ends_with("..."));
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

// ==================== parse_skill_description ====================

// frontmatter 中 description 字段完整返回
#[test]
fn ParseSkill_Frontmatter_001() {
    let content = "---\ndescription: Build skill\n---\n# Title\nBody";
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("SKILL.md");
    std::fs::write(&file_path, content).unwrap();
    let result = parse_skill_description(&file_path).unwrap();
    assert_eq!(result, "Build skill");
}

// 无 frontmatter 时取正文第一行非空非标题行
#[test]
fn ParseSkill_BodyFallback_001() {
    let content = "# Title\n\nFirst body line\nMore text";
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("SKILL.md");
    std::fs::write(&file_path, content).unwrap();
    let result = parse_skill_description(&file_path).unwrap();
    assert_eq!(result, "First body line");
}

// frontmatter 长描述完整返回不截断
#[test]
fn ParseSkill_FrontmatterLongDesc_001() {
    let long_desc: String = "y".repeat(300);
    let content = format!("---\ndescription: {}\n---\nBody", long_desc);
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("SKILL.md");
    std::fs::write(&file_path, content).unwrap();
    let result = parse_skill_description(&file_path).unwrap();
    assert_eq!(result, long_desc);
    assert!(!result.ends_with("..."));
}

// 无 frontmatter 时正文长描述完整返回不截断
#[test]
fn ParseSkill_BodyLongDesc_001() {
    let long_body: String = "b".repeat(200);
    let content = format!("# Title\n\n{}", long_body);
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("SKILL.md");
    std::fs::write(&file_path, content).unwrap();
    let result = parse_skill_description(&file_path).unwrap();
    assert_eq!(result, long_body);
    assert!(!result.ends_with("..."));
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

// ==================== find_valid_plugin_path ====================
// 使用本机真实路径验证完整查找链路

// frontend-design cache 路径存在，直接返回
#[test]
fn FindPlugin_CacheExists_001() {
    let result = find_valid_plugin_path(
        "C:\\Users\\orczh\\.claude\\plugins\\cache\\claude-plugins-official\\frontend-design\\104d39be10b7",
        "frontend-design@claude-plugins-official",
    );
    assert!(result.is_some());
    assert!(result.unwrap().contains("frontend-design"));
}

// paper-tool cache 路径不存在，回退到 marketplace source 找到真实路径
#[test]
fn FindPlugin_CacheMissingFallsBackToMarketplace_001() {
    let result = find_valid_plugin_path(
        "C:\\Users\\orczh\\.claude\\plugins\\cache\\orczh\\paper-tool\\2.4.1",
        "paper-tool@orczh",
    );
    assert!(result.is_some());
    let path = result.unwrap();
    assert!(path.contains("paper-tool"));
    // 路径存在且包含 plugin.json
    assert!(std::path::Path::new(&path).join(".claude-plugin").join("plugin.json").exists());
}

// pyright-lsp cache 路径存在
#[test]
fn FindPlugin_PyrightCacheExists_001() {
    let result = find_valid_plugin_path(
        "C:\\Users\\orczh\\.claude\\plugins\\cache\\claude-plugins-official\\pyright-lsp\\1.0.0",
        "pyright-lsp@claude-plugins-official",
    );
    assert!(result.is_some());
}

// claude-scientific-writer cache 路径存在
#[test]
fn FindPlugin_ScientificWriterCacheExists_001() {
    let result = find_valid_plugin_path(
        "C:\\Users\\orczh\\.claude\\plugins\\cache\\claude-scientific-writer\\claude-scientific-writer\\5bf6b597e2af",
        "claude-scientific-writer@claude-scientific-writer",
    );
    assert!(result.is_some());
}

// 不存在的路径 + 无效 marketplace name 返回 None
#[test]
fn FindPlugin_InvalidId_001() {
    let result = find_valid_plugin_path(
        "C:\\nonexistent\\path",
        "fake-plugin@fake-marketplace",
    );
    assert!(result.is_none());
}

// ==================== resolve_marketplace_plugin_path ====================

// 通过 known_marketplaces.json 解析 paper-tool@orczh 的真实路径
#[test]
fn ResolveMarketplace_LocalDirectory_001() {
    let result = resolve_marketplace_plugin_path("paper-tool@orczh");
    assert!(result.is_some());
    let path = result.unwrap();
    assert!(std::path::Path::new(&path).exists());
    assert!(std::path::Path::new(&path).join(".claude-plugin").join("plugin.json").exists());
}

// 通过 github marketplace 解析 frontend-design@claude-plugins-official
#[test]
fn ResolveMarketplace_GithubMarketplace_001() {
    let result = resolve_marketplace_plugin_path("frontend-design@claude-plugins-official");
    assert!(result.is_some());
    let path = result.unwrap();
    assert!(std::path::Path::new(&path).exists());
}

// 无效 marketplace name 返回 None
#[test]
fn ResolveMarketplace_UnknownMarketplace_001() {
    let result = resolve_marketplace_plugin_path("plugin@nonexistent-marketplace");
    assert!(result.is_none());
}

// 格式错误的 plugin_id（无 @ 分隔）返回 None
#[test]
fn ResolveMarketplace_BadFormat_001() {
    let result = resolve_marketplace_plugin_path("no-at-sign");
    assert!(result.is_none());
}

// ==================== search_session_messages_in_dirs ====================

// 构造一行 JSONL 消息（user/assistant，content 为 string）
fn build_jsonl_line(msg_type: &str, content: &str) -> String {
    let t = if msg_type == "user" { "user" } else { "assistant" };
    format!(r#"{{"type":"{}","message":{{"content":"{}"}}}}"#, t, content)
}

// 单文件单消息按 query 匹配，返回 snippet
#[test]
fn SearchSession_BasicMatch_001() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("session-abc.jsonl");
    std::fs::write(
        &file_path,
        format!("{}\n", build_jsonl_line("user", "hello world")),
    )
    .unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "hello", 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].session_id, "session-abc");
    assert!(results[0].snippet.contains("hello"));
}

// 大小写不敏感匹配
#[test]
fn SearchSession_CaseInsensitive_001() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("s1.jsonl");
    std::fs::write(
        &file_path,
        format!("{}\n", build_jsonl_line("assistant", "Hello WORLD")),
    )
    .unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "HELLO", 10);
    assert_eq!(results.len(), 1);
    assert!(results[0].snippet.to_lowercase().contains("hello"));
}

// 超过 200 行的文件，老消息（前 200 行之外）也能被匹配
#[test]
fn SearchSession_LongFile_OldMessage_001() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("long.jsonl");

    // 前 250 行是不匹配的填充，第 1 行（最老）才是目标
    let mut content = String::new();
    content.push_str(&format!("{}\n", build_jsonl_line("user", "TARGET_KEYWORD_HERE")));
    for i in 0..250 {
        content.push_str(&format!("{}\n", build_jsonl_line("assistant", &format!("filler {}", i))));
    }
    std::fs::write(&file_path, content).unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "TARGET_KEYWORD", 10);
    assert_eq!(results.len(), 1, "old message outside newest 200 lines should be matched");
    assert!(results[0].snippet.contains("TARGET_KEYWORD"));
}

// 同一文件多条匹配，snippet 取最新（最末尾的匹配）
#[test]
fn SearchSession_LatestMatchFirst_001() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("multi.jsonl");
    let mut content = String::new();
    content.push_str(&format!("{}\n", build_jsonl_line("user", "KEYWORD old match")));
    content.push_str(&format!("{}\n", build_jsonl_line("assistant", "no match here")));
    content.push_str(&format!("{}\n", build_jsonl_line("user", "KEYWORD new match")));
    std::fs::write(&file_path, content).unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "KEYWORD", 10);
    assert_eq!(results.len(), 1);
    assert!(results[0].snippet.contains("new match"));
    assert!(!results[0].snippet.contains("old match"));
}

// agent- 开头的文件被跳过
#[test]
fn SearchSession_AgentFilesSkipped_001() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("agent-sub.jsonl"),
        format!("{}\n", build_jsonl_line("user", "secret keyword")),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("normal.jsonl"),
        format!("{}\n", build_jsonl_line("user", "no match")),
    )
    .unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "secret", 10);
    assert_eq!(results.len(), 0, "agent-* files must be skipped");
}

// limit 截断生效
#[test]
fn SearchSession_LimitApplied_001() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        std::fs::write(
            dir.path().join(format!("s{}.jsonl", i)),
            format!("{}\n", build_jsonl_line("user", "shared keyword")),
        )
        .unwrap();
    }

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "shared", 3);
    assert_eq!(results.len(), 3);
}

// 非 .jsonl / .txt 文件被忽略
#[test]
fn SearchSession_NonJsonlIgnored_001() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.md"),
        format!("{}\n", build_jsonl_line("user", "keyword in md")),
    )
    .unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "keyword", 10);
    assert_eq!(results.len(), 0);
}

// content 为 array（多模态）的消息目前不匹配（仅 string content 才匹配）
#[test]
fn SearchSession_ArrayContentSkipped_001() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("multi-modal.jsonl");
    std::fs::write(
        &file_path,
        r#"{"type":"user","message":{"content":[{"type":"text","text":"keyword in array"}]}}
"#,
    )
    .unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "keyword", 10);
    assert_eq!(results.len(), 0, "array content not yet supported");
}

// ==================== set_skill_enabled_in ====================

// 禁用 skill：目录从 active 移到 disabled
#[test]
fn SetSkillEnabled_Disable_MovesDir_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    let skill_dir = active.path().join("deploy");
    std::fs::create_dir(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "---\ndescription: x\n---\n").unwrap();

    set_skill_enabled_in(active.path(), disabled.path(), "deploy", false).unwrap();

    assert!(!skill_dir.exists(), "active skill dir should be gone");
    assert!(
        disabled.path().join("deploy").join("SKILL.md").exists(),
        "disabled backup should exist"
    );
}

// 启用 skill：目录从 disabled 移回 active
#[test]
fn SetSkillEnabled_Enable_MovesBack_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    let backup = disabled.path().join("deploy");
    std::fs::create_dir(&backup).unwrap();
    std::fs::write(backup.join("SKILL.md"), "content").unwrap();

    set_skill_enabled_in(active.path(), disabled.path(), "deploy", true).unwrap();

    assert!(active.path().join("deploy").join("SKILL.md").exists());
    assert!(!backup.exists());
}

// 禁用不存在的 skill → Err
#[test]
fn SetSkillEnabled_Disable_NotFound_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    let r = set_skill_enabled_in(active.path(), disabled.path(), "ghost", false);
    assert!(r.is_err());
}

// 重复禁用（disabled 已存在）→ Err
#[test]
fn SetSkillEnabled_Disable_AlreadyDisabled_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    std::fs::create_dir(active.path().join("deploy")).unwrap();
    std::fs::create_dir(disabled.path().join("deploy")).unwrap();

    let r = set_skill_enabled_in(active.path(), disabled.path(), "deploy", false);
    assert!(r.is_err());
}

// 启用时 active 已存在同名 → conflict Err
#[test]
fn SetSkillEnabled_Enable_Conflict_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    std::fs::create_dir(active.path().join("deploy")).unwrap();
    std::fs::create_dir(disabled.path().join("deploy")).unwrap();

    let r = set_skill_enabled_in(active.path(), disabled.path(), "deploy", true);
    assert!(r.is_err());
}

// 启用未禁用的 skill（backup 不存在）→ Err
#[test]
fn SetSkillEnabled_Enable_NotDisabled_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();

    let r = set_skill_enabled_in(active.path(), disabled.path(), "deploy", true);
    assert!(r.is_err());
}

// 路径穿越 → Err
#[test]
fn SetSkillEnabled_PathTraversal_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();

    let r1 = set_skill_enabled_in(active.path(), disabled.path(), "../escape", false);
    let r2 = set_skill_enabled_in(active.path(), disabled.path(), "a/b", false);
    let r3 = set_skill_enabled_in(active.path(), disabled.path(), "a\\b", false);

    assert!(r1.is_err());
    assert!(r2.is_err());
    assert!(r3.is_err());
}

// ==================== set_agent_enabled_in ====================

// 禁用 agent：文件从 active 移到 disabled
#[test]
fn SetAgentEnabled_Disable_MovesFile_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    std::fs::write(active.path().join("reviewer.md"), "content").unwrap();

    set_agent_enabled_in(active.path(), disabled.path(), "reviewer", false).unwrap();

    assert!(!active.path().join("reviewer.md").exists());
    assert!(disabled.path().join("reviewer.md").exists());
}

// 启用 agent：文件从 disabled 移回 active
#[test]
fn SetAgentEnabled_Enable_MovesBack_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    std::fs::write(disabled.path().join("reviewer.md"), "content").unwrap();

    set_agent_enabled_in(active.path(), disabled.path(), "reviewer", true).unwrap();

    assert!(active.path().join("reviewer.md").exists());
    assert!(!disabled.path().join("reviewer.md").exists());
}

// 禁用不存在的 agent → Err
#[test]
fn SetAgentEnabled_Disable_NotFound_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    let r = set_agent_enabled_in(active.path(), disabled.path(), "ghost", false);
    assert!(r.is_err());
}

// 重复禁用 → Err
#[test]
fn SetAgentEnabled_Disable_AlreadyDisabled_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    std::fs::write(active.path().join("reviewer.md"), "x").unwrap();
    std::fs::write(disabled.path().join("reviewer.md"), "x").unwrap();

    let r = set_agent_enabled_in(active.path(), disabled.path(), "reviewer", false);
    assert!(r.is_err());
}

// 启用冲突 → Err
#[test]
fn SetAgentEnabled_Enable_Conflict_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();
    std::fs::write(active.path().join("reviewer.md"), "x").unwrap();
    std::fs::write(disabled.path().join("reviewer.md"), "x").unwrap();

    let r = set_agent_enabled_in(active.path(), disabled.path(), "reviewer", true);
    assert!(r.is_err());
}

// 路径穿越 → Err
#[test]
fn SetAgentEnabled_PathTraversal_001() {
    let active = tempfile::tempdir().unwrap();
    let disabled = tempfile::tempdir().unwrap();

    let r = set_agent_enabled_in(active.path(), disabled.path(), "../escape", false);
    assert!(r.is_err());
}

// ==================== set_mcp_server_enabled_in ====================

// 禁用 MCP：从 ~/.claude.json::mcpServers.<name> 剪切到 backup，其他字段保留
#[test]
fn SetMcpEnabled_Disable_CutsEntry_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(
        &claude_json,
        r#"{
            "otherConfig": {"keepMe": true},
            "mcpServers": {
                "zread": {"type":"http","url":"https://x"},
                "other": {"command":"foo"}
            }
        }"#,
    )
    .unwrap();

    set_mcp_server_enabled_in(&claude_json, &disabled_dir, "zread", false).unwrap();

    // backup 文件含单条 server 配置
    let backup = std::fs::read_to_string(disabled_dir.join("zread.json")).unwrap();
    assert!(backup.contains("https://x"), "backup should contain url content");
    assert!(!backup.contains("\"other\""), "backup should only contain zread");

    // 主配置保留其他字段和其他 server
    let main = std::fs::read_to_string(&claude_json).unwrap();
    assert!(main.contains("\"keepMe\""), "other config must be preserved");
    assert!(main.contains("\"other\""), "other server must be preserved");
    assert!(!main.contains("zread"), "zread should be removed from main config");
}

// 启用 MCP：backup 内容贴回 mcpServers，backup 文件删除
#[test]
fn SetMcpEnabled_Enable_PastesBack_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(
        &claude_json,
        r#"{"otherConfig":{"keepMe":true},"mcpServers":{"other":{"command":"foo"}}}"#,
    )
    .unwrap();
    std::fs::write(
        disabled_dir.join("zread.json"),
        r#"{"type":"http","url":"https://x"}"#,
    )
    .unwrap();

    set_mcp_server_enabled_in(&claude_json, &disabled_dir, "zread", true).unwrap();

    let main = std::fs::read_to_string(&claude_json).unwrap();
    assert!(main.contains("zread"), "zread should be back in main config");
    assert!(main.contains("https://x"), "zread config content should be intact");
    assert!(main.contains("\"keepMe\""), "other config preserved");
    assert!(main.contains("\"other\""), "other server preserved");
    assert!(!disabled_dir.join("zread.json").exists(), "backup file should be removed");
}

// 禁用不存在的 server → Err
#[test]
fn SetMcpEnabled_Disable_NotFound_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(&claude_json, r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();

    let r = set_mcp_server_enabled_in(&claude_json, &disabled_dir, "ghost", false);
    assert!(r.is_err());
}

// 重复禁用（backup 已存在）→ Err
#[test]
fn SetMcpEnabled_Disable_AlreadyDisabled_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(
        &claude_json,
        r#"{"mcpServers":{"zread":{"url":"x"}}}"#,
    )
    .unwrap();
    std::fs::write(disabled_dir.join("zread.json"), r#"{"url":"x"}"#).unwrap();

    let r = set_mcp_server_enabled_in(&claude_json, &disabled_dir, "zread", false);
    assert!(r.is_err());
}

// 启用时主配置已有同名 → conflict Err
#[test]
fn SetMcpEnabled_Enable_Conflict_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(
        &claude_json,
        r#"{"mcpServers":{"zread":{"url":"old"}}}"#,
    )
    .unwrap();
    std::fs::write(disabled_dir.join("zread.json"), r#"{"url":"new"}"#).unwrap();

    let r = set_mcp_server_enabled_in(&claude_json, &disabled_dir, "zread", true);
    assert!(r.is_err());
}

// 启用时 backup 不存在 → Err
#[test]
fn SetMcpEnabled_Enable_NotDisabled_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(&claude_json, r#"{"mcpServers":{}}"#).unwrap();

    let r = set_mcp_server_enabled_in(&claude_json, &disabled_dir, "zread", true);
    assert!(r.is_err());
}

// .claude.json 不存在时禁用 → Err
#[test]
fn SetMcpEnabled_Disable_NoClaudeJson_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();

    let r = set_mcp_server_enabled_in(&claude_json, &disabled_dir, "zread", false);
    assert!(r.is_err());
}

// 启用时主配置文件不存在，会自动创建并加入
#[test]
fn SetMcpEnabled_Enable_CreatesClaudeJson_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(disabled_dir.join("zread.json"), r#"{"url":"x"}"#).unwrap();

    set_mcp_server_enabled_in(&claude_json, &disabled_dir, "zread", true).unwrap();

    let main = std::fs::read_to_string(&claude_json).unwrap();
    assert!(main.contains("zread"));
    assert!(main.contains("mcpServers"));
}

// 路径穿越 → Err
#[test]
fn SetMcpEnabled_PathTraversal_001() {
    let tmp = tempfile::tempdir().unwrap();
    let claude_json = tmp.path().join(".claude.json");
    let disabled_dir = tmp.path().join("disabled_mcp");
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(&claude_json, r#"{"mcpServers":{}}"#).unwrap();

    let r = set_mcp_server_enabled_in(&claude_json, &disabled_dir, "../escape", false);
    assert!(r.is_err());
}

// AppConfig 序列化包含 terminalTheme（camelCase rename），反序列化还原
#[test]
fn AppConfig_TerminalTheme_SerializeDeserialize_001() {
    let config = AppConfig {
        terminal_theme: Some("dracula".to_string()),
        ..Default::default()
    };
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("\"terminalTheme\":\"dracula\""));

    let parsed: AppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.terminal_theme, Some("dracula".to_string()));
}

// terminal_theme 默认为 None（首次返回不设默认，迁移交前端）
#[test]
fn AppConfig_TerminalTheme_DefaultNone_001() {
    let config = AppConfig::default();
    assert_eq!(config.terminal_theme, None);
}

// ==================== get_projects_state_at / update_projects_state_at ====================

// 文件不存在 -> 返回默认空状态（pinned 为空 Vec，archived 为空 Map）
#[test]
fn GetProjectsState_NoFile_DefaultEmpty_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    let state = get_projects_state_at(&path).unwrap();
    assert!(state.pinned_projects.is_empty(), "pinned 应为空");
    assert!(state.archived_sessions.is_empty(), "archived 应为空");
}

// 文件不存在时返回的默认状态与 ProjectsState::default() 一致（字段级校验，struct 未 derive PartialEq）
#[test]
fn GetProjectsState_NoFile_MatchesDefault_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    let state = get_projects_state_at(&path).unwrap();
    let default = ProjectsState::default();
    assert_eq!(state.pinned_projects, default.pinned_projects);
    assert_eq!(state.archived_sessions, default.archived_sessions);
}

// update 写入后 get 读回一致（pinnedProjects + archivedSessions 双字段）
#[test]
fn UpdateProjectsState_WriteThenReadBack_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    let updates = json!({
        "pinnedProjects": ["E:/proj/a", "E:/proj/b"],
        "archivedSessions": {"E:/proj/a": ["sess-1", "sess-2"]}
    });
    update_projects_state_at(&path, updates).unwrap();

    let state = get_projects_state_at(&path).unwrap();
    assert_eq!(state.pinned_projects, vec!["E:/proj/a", "E:/proj/b"]);
    let archived = state.archived_sessions.get("E:/proj/a").unwrap();
    assert_eq!(*archived, vec!["sess-1".to_string(), "sess-2".to_string()]);
}

// merge：只更新 pinnedProjects，已有的 archivedSessions 不丢失
#[test]
fn UpdateProjectsState_PartialMergeKeepsArchived_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    // 先写入两个字段
    update_projects_state_at(
        &path,
        json!({"pinnedProjects": ["a"], "archivedSessions": {"a": ["s1"]}}),
    )
    .unwrap();
    // 只更新 pinnedProjects
    update_projects_state_at(&path, json!({"pinnedProjects": ["b", "c"]})).unwrap();

    let state = get_projects_state_at(&path).unwrap();
    assert_eq!(state.pinned_projects, vec!["b", "c"], "pinned 应被覆盖");
    assert!(state.archived_sessions.contains_key("a"), "archived 应保留");
    assert_eq!(
        state.archived_sessions.get("a").unwrap(),
        &vec!["s1".to_string()],
        "archived 内容不变"
    );
}

// merge：只更新 archivedSessions，已有的 pinnedProjects 不丢失
#[test]
fn UpdateProjectsState_PartialMergeKeepsPinned_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    update_projects_state_at(
        &path,
        json!({"pinnedProjects": ["a", "b"], "archivedSessions": {"a": ["s1"]}}),
    )
    .unwrap();
    update_projects_state_at(&path, json!({"archivedSessions": {"b": ["s2", "s3"]}})).unwrap();

    let state = get_projects_state_at(&path).unwrap();
    assert_eq!(state.pinned_projects, vec!["a", "b"], "pinned 应保留");
    // archivedSessions 整体被覆盖为 updates 中的值（merge 顶层替换语义）
    assert!(!state.archived_sessions.contains_key("a"), "a 被覆盖");
    assert!(state.archived_sessions.contains_key("b"), "b 为新值");
    assert_eq!(
        state.archived_sessions.get("b").unwrap(),
        &vec!["s2".to_string(), "s3".to_string()]
    );
}

// 父目录不存在时 update 自动创建
#[test]
fn UpdateProjectsState_CreatesParentDir_001() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("nested").join("deep").join("projects.json");
    update_projects_state_at(&nested, json!({"pinnedProjects": ["x"]})).unwrap();
    assert!(nested.exists(), "文件应被创建");
    let state = get_projects_state_at(&nested).unwrap();
    assert_eq!(state.pinned_projects, vec!["x"]);
}

// 文件存在但为空对象 {} -> 反序列化为默认空状态（serde default 生效）
#[test]
fn GetProjectsState_EmptyObject_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    std::fs::write(&path, "{}").unwrap();
    let state = get_projects_state_at(&path).unwrap();
    assert!(state.pinned_projects.is_empty());
    assert!(state.archived_sessions.is_empty());
}

// 文件存在但缺一个字段 -> 缺失字段用默认值（serde default 生效）
#[test]
fn GetProjectsState_MissingField_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    std::fs::write(&path, r#"{"pinnedProjects":["only"]}"#).unwrap();
    let state = get_projects_state_at(&path).unwrap();
    assert_eq!(state.pinned_projects, vec!["only"]);
    assert!(state.archived_sessions.is_empty(), "缺失字段应默认空");
}

// update 后文件内容是合法 JSON 且字段名为 camelCase
#[test]
fn UpdateProjectsState_WritesCamelCase_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    update_projects_state_at(&path, json!({"pinnedProjects": ["a"]})).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("\"pinnedProjects\""), "应使用 camelCase 字段名");
    assert!(!content.contains("pinned_projects"), "不应出现 snake_case");
    // 内容整体可被解析回 ProjectsState
    let reparsed: ProjectsState = serde_json::from_str(&content).unwrap();
    assert_eq!(reparsed.pinned_projects, vec!["a"]);
}

// 首次 update（无现存文件）-> 读默认空再 merge，结果只含 updates 字段
#[test]
fn UpdateProjectsState_FirstWrite_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json");
    update_projects_state_at(&path, json!({"archivedSessions": {"p": ["s1"]}})).unwrap();
    let state = get_projects_state_at(&path).unwrap();
    assert!(state.pinned_projects.is_empty(), "未提供 pinned 应为空");
    assert_eq!(
        state.archived_sessions.get("p").unwrap(),
        &vec!["s1".to_string()]
    );
}

// ==================== compute_project_startup_state ====================

// 无项目：has_any=false, has_visible=false, last_info=None
#[test]
fn ComputeStartup_NoProjects_001() {
    let state = compute_project_startup_state(&[], "", &[]);
    assert!(!state.has_any_project);
    assert!(!state.has_visible_project);
    assert!(state.last_opened_project_info.is_none());
}

// 有项目全可见：has_any=true, has_visible=true
#[test]
fn ComputeStartup_AllVisible_001() {
    let projects = vec![Project {
        path: "/p-a".into(),
        name: "p-a".into(),
        last_session_id: None,
        last_cost: None,
        last_duration: None,
    }];
    let state = compute_project_startup_state(&projects, "", &[]);
    assert!(state.has_any_project);
    assert!(state.has_visible_project);
}

// 有项目但全隐藏：has_any=true, has_visible=false
#[test]
fn ComputeStartup_AllHidden_001() {
    let projects = vec![Project {
        path: "/p-a".into(),
        name: "p-a".into(),
        last_session_id: None,
        last_cost: None,
        last_duration: None,
    }];
    let state = compute_project_startup_state(&projects, "", &["/p-a".to_string()]);
    assert!(state.has_any_project);
    assert!(!state.has_visible_project);
}

// 部分隐藏：has_visible=true（仍有可见项目）
#[test]
fn ComputeStartup_PartialHidden_001() {
    let projects = vec![
        Project {
            path: "/p-a".into(),
            name: "p-a".into(),
            last_session_id: None,
            last_cost: None,
            last_duration: None,
        },
        Project {
            path: "/p-b".into(),
            name: "p-b".into(),
            last_session_id: None,
            last_cost: None,
            last_duration: None,
        },
    ];
    let state = compute_project_startup_state(&projects, "", &["/p-a".to_string()]);
    assert!(state.has_any_project);
    assert!(state.has_visible_project); // /p-b 仍可见
}

// lastOpened 命中真实路径（含分页外项目）：last_info 填充 exists=true
#[test]
fn ComputeStartup_LastOpenedExists_001() {
    let projects = vec![Project {
        path: "/p-deep".into(),
        name: "p-deep".into(),
        last_session_id: None,
        last_cost: None,
        last_duration: None,
    }];
    let state = compute_project_startup_state(&projects, "/p-deep", &[]);
    let info = state.last_opened_project_info.expect("info should exist");
    assert_eq!(info.path, "/p-deep");
    assert_eq!(info.name, "p-deep");
    assert!(info.exists);
}

// lastOpened 不在项目集合：exists=false（info 仍填充，供前端提示）
#[test]
fn ComputeStartup_LastOpenedMissing_001() {
    let projects = vec![Project {
        path: "/p-a".into(),
        name: "p-a".into(),
        last_session_id: None,
        last_cost: None,
        last_duration: None,
    }];
    let state = compute_project_startup_state(&projects, "/p-gone", &[]);
    let info = state.last_opened_project_info.expect("info should exist");
    assert!(!info.exists);
}

// lastOpened 为空：last_info=None（首次启动）
#[test]
fn ComputeStartup_LastOpenedEmpty_001() {
    let projects = vec![Project {
        path: "/p-a".into(),
        name: "p-a".into(),
        last_session_id: None,
        last_cost: None,
        last_duration: None,
    }];
    let state = compute_project_startup_state(&projects, "", &[]);
    assert!(state.last_opened_project_info.is_none());
}

// 规范化比较：Windows 反斜杠 + 大小写差异仍能命中/隐藏
#[test]
fn ComputeStartup_NormalizePath_001() {
    let projects = vec![Project {
        path: "E:\\source\\Foo".into(),
        name: "Foo".into(),
        last_session_id: None,
        last_cost: None,
        last_duration: None,
    }];
    // lastOpened 用正斜杠小写仍命中
    let state = compute_project_startup_state(&projects, "e:/source/foo", &[]);
    let info = state.last_opened_project_info.unwrap();
    assert!(info.exists);
    // hidden 用正斜杠小写仍隐藏
    let state2 = compute_project_startup_state(&projects, "", &["e:/source/foo".to_string()]);
    assert!(!state2.has_visible_project);
}

// ==================== ProjectsState displayNames ====================

// displayNames 序列化往返：camelCase 字段名 + 中文别名可还原
#[test]
fn ProjectsState_DisplayNames_Roundtrip_001() {
    let mut m = HashMap::new();
    m.insert("/p-a".to_string(), "主项目".to_string());
    let state = ProjectsState {
        pinned_projects: vec!["/p-a".into()],
        archived_sessions: HashMap::new(),
        display_names: m,
    };
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains("\"displayNames\""), "字段名须为 camelCase displayNames");
    let back: ProjectsState = serde_json::from_str(&json).unwrap();
    assert_eq!(back.display_names.get("/p-a"), Some(&"主项目".to_string()));
}

// 旧文件无 displayNames 字段 -> 默认空 map（向后兼容，旧 projects.json 不挂）
#[test]
fn ProjectsState_DisplayNames_Default_001() {
    let json = r#"{"pinnedProjects":["/p-a"],"archivedSessions":{}}"#;
    let state: ProjectsState = serde_json::from_str(json).unwrap();
    assert!(state.display_names.is_empty());
}

// displayNames 为 null -> 容错返空（不整体解析失败）
#[test]
fn ProjectsState_MalformedDisplayNames_Null_001() {
    let json = r#"{"pinnedProjects":[],"archivedSessions":{},"displayNames":null}"#;
    let state: ProjectsState = serde_json::from_str(json).unwrap();
    assert!(state.display_names.is_empty());
}

// displayNames 为数组 -> 容错返空
#[test]
fn ProjectsState_MalformedDisplayNames_Array_001() {
    let json = r#"{"pinnedProjects":[],"archivedSessions":{},"displayNames":["a","b"]}"#;
    let state: ProjectsState = serde_json::from_str(json).unwrap();
    assert!(state.display_names.is_empty());
}

// displayNames 内某条目值非 string（数字）-> 跳过该条目，其余保留
#[test]
fn ProjectsState_MalformedDisplayNames_NonStringValue_001() {
    let json = r#"{"pinnedProjects":[],"archivedSessions":{},"displayNames":{"/p-a":"别名","/p-b":123}}"#;
    let state: ProjectsState = serde_json::from_str(json).unwrap();
    assert_eq!(state.display_names.get("/p-a"), Some(&"别名".to_string()));
    assert!(state.display_names.get("/p-b").is_none(), "非 string 值条目跳过");
}

// ==================== write_json_atomic（含 Windows target exists） ====================

// 原子写：写后目标文件内容正确
#[test]
fn WriteJsonAtomic_Content_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let val = serde_json::json!({"displayNames": {"/p-a": "别名"}});
    write_json_atomic(&path, &val).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["displayNames"]["/p-a"], "别名");
}

// 原子写：写后无 .json.tmp 残留（证明走了 tmp+rename 清理路径，fs::write 不产生 tmp）
#[test]
fn WriteJsonAtomic_NoTmpLeftover_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    write_json_atomic(&path, &serde_json::json!({"a":1})).unwrap();
    let tmp = path.with_extension("json.tmp");
    assert!(!tmp.exists(), "rename 成功后 .tmp 不应残留");
}

// 原子写：原文件存在时完整替换（读回 == 新值，非旧+新拼接）
#[test]
fn WriteJsonAtomic_ReplacesFully_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    std::fs::write(&path, serde_json::to_string_pretty(&serde_json::json!({"old": true})).unwrap()).unwrap();
    write_json_atomic(&path, &serde_json::json!({"displayNames": {"/p-a": "新"}})).unwrap();
    let back: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back["displayNames"]["/p-a"], "新");
    assert!(back.get("old").is_none(), "完整替换，旧 key 不残留");
}

// 原子写：目标已存在时二次写入成功（Windows fs::rename 目标存在失败 -> remove+rename 修复）。
// 这是 codex 致命#1 的核心场景：projects.json 首次写入后，后续 update 必须仍能覆盖。
#[test]
fn WriteJsonAtomic_ReplacesExisting_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    // 第一次写（目标不存在）
    write_json_atomic(&path, &serde_json::json!({"displayNames": {"/p-a": "first"}})).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap().contains("first"), true);
    // 第二次写（目标已存在）--Windows 上裸 fs::rename 会失败，remove+rename 修复后须成功
    write_json_atomic(&path, &serde_json::json!({"displayNames": {"/p-a": "second"}})).unwrap();
    let back: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back["displayNames"]["/p-a"], "second", "目标已存在时二次写入须覆盖成功");
    // 三次写仍成功（连续覆盖）
    write_json_atomic(&path, &serde_json::json!({"displayNames": {"/p-a": "third"}})).unwrap();
    let back3: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back3["displayNames"]["/p-a"], "third");
}

// ==================== update_projects_state_at 原子写 ====================

// update 用 (path, updates) 签名（path first），displayNames 持久化往返
#[test]
fn UpdateProjectsState_DisplayNames_Persist_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    update_projects_state_at(&path, serde_json::json!({"displayNames": {"/p-a": "别名"}})).unwrap();
    let state: ProjectsState = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state.display_names.get("/p-a"), Some(&"别名".to_string()));
}

// update 二次写入（目标已存在）仍成功：Windows remove+rename 闭环
#[test]
fn UpdateProjectsState_OverwriteExisting_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    update_projects_state_at(&path, serde_json::json!({"displayNames": {"/p-a": "旧"}})).unwrap();
    update_projects_state_at(&path, serde_json::json!({"displayNames": {"/p-a": "新"}})).unwrap();
    let state: ProjectsState = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state.display_names.get("/p-a"), Some(&"新".to_string()));
}

// 故障注入：预先把 .json.tmp 建成目录 -> write_json_atomic 写 tmp 失败 ->
// update_projects_state_at 返 Err 且原 projects.json 内容不变（原子性：失败不破坏旧文件）。
// 裸 fs::write(path) 不经 tmp，此场景下会成功覆盖 -> 与 expect Err 冲突，故能区分（非 false green）。
#[test]
fn UpdateProjectsState_AtomicWrite_FailPreservesOriginal_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    // 1) 先写入合法旧内容
    update_projects_state_at(&path, serde_json::json!({"displayNames": {"/p-a": "旧别名"}})).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    // 2) 把 tmp 路径占为目录，迫使 write_json_atomic 写 tmp 失败
    let tmp = path.with_extension("json.tmp");
    std::fs::create_dir(&tmp).unwrap();
    // 3) 再次写入应失败（fs::write(&tmp) 对目录失败，remove/rename 未到达）
    let res = update_projects_state_at(&path, serde_json::json!({"displayNames": {"/p-a": "新别名"}}));
    assert!(res.is_err(), "tmp 写失败须传播 Err");
    // 4) 原文件未被破坏
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original, "原子写：失败不破坏原文件");
}

// ==================== normalize_path_inner（平台感知规范化） ====================

// Windows/macOS（case_sensitive=false）：反斜杠规范 + 去尾斜杠 + 小写
#[test]
fn NormalizePath_CaseInsensitive_Normalize_001() {
    assert_eq!(normalize_path_inner("E:\\Source\\Foo\\", false), "e:/source/foo");
}

// Windows/macOS：大小写不敏感 -> E:\Repo 与 e:/repo 归一为同身份
#[test]
fn NormalizePath_CaseInsensitive_MergesIdentity_001() {
    assert_eq!(
        normalize_path_inner("E:\\Repo", false),
        normalize_path_inner("e:/repo", false)
    );
}

// Windows drive 根：C:\ / C: / C:/ 均归一为 c:（盘符小写 + 去尾斜杠）
#[test]
fn NormalizePath_CaseInsensitive_DriveRoot_001() {
    assert_eq!(normalize_path_inner("C:\\", false), "c:");
    assert_eq!(normalize_path_inner("C:", false), "c:");
    assert_eq!(normalize_path_inner("C:/", false), "c:");
}

// Linux（case_sensitive=true）：不 lower，保留大小写身份
#[test]
fn NormalizePath_CaseSensitive_PreservesCase_001() {
    assert_eq!(normalize_path_inner("/work/Foo/", true), "/work/Foo");
}

// Linux：大小写敏感 -> /work/Foo 与 /work/foo 不同身份（不误并）
#[test]
fn NormalizePath_CaseSensitive_DistinctIdentity_001() {
    assert_ne!(
        normalize_path_inner("/work/Foo", true),
        normalize_path_inner("/work/foo", true)
    );
}

// Linux：反斜杠规范 + 去尾斜杠仍生效（仅大小写保留）
#[test]
fn NormalizePath_CaseSensitive_NormalizeSlash_001() {
    assert_eq!(normalize_path_inner("/work/Foo\\Bar/", true), "/work/Foo/Bar");
}

// POSIX 根 '/' 去尾斜杠后恢复 '/'（非空串 key），两支平台一致
#[test]
fn NormalizePath_PosixRoot_Recovered_001() {
    assert_eq!(normalize_path_inner("/", true), "/");
    assert_eq!(normalize_path_inner("///", false), "/");
}
