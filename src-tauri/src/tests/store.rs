use serde_json::json;

use crate::store::{
    acquire_lock, assemble_home_data, canonicalize_state, compute_project_startup_state,
    expand_env_vars, extract_md_description, extract_session_name, find_valid_plugin_path,
    get_projects_state_at, infer_server_type, merge_json_values, normalize_path_inner,
    normalize_path_str_pub, parse_agents_list_output, parse_mcp_server_entry,
    parse_skill_description, parse_timestamp, read_projects_state_locked, replace_file_atomic,
    resolve_marketplace_plugin_path_at, search_session_messages_in_dirs, set_agent_enabled_in,
    set_mcp_server_enabled_in, set_skill_enabled_in, with_projects_state_locked, write_json_atomic,
    AgentInfo, AppConfig, Project, ProjectsState, SessionInfo,
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
    assert_eq!(
        info.env.as_ref().unwrap().get("CHROME_PATH").unwrap(),
        "/usr/bin/chrome"
    );
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
    assert_eq!(
        info.headers.as_ref().unwrap().get("Authorization").unwrap(),
        "Bearer token123"
    );
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
    extra.insert(
        "CLAUDE_PLUGIN_ROOT".to_string(),
        "C:/plugins/paper".to_string(),
    );
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
    extra.insert(
        "CLAUDE_PLUGIN_ROOT".to_string(),
        "C:/plugins/paper-tool".to_string(),
    );
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
    let lines = [
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
    let lines = [
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
    let lines = [
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
    let lines = [
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
    let lines = [format!(
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

#[test]
fn FindPlugin_CacheExists_001() {
    let temp = tempfile::tempdir().unwrap();
    let cache_path = temp.path().join("cache").join("frontend-design");
    std::fs::create_dir_all(&cache_path).unwrap();

    let result = find_valid_plugin_path(cache_path.to_str().unwrap(), "ignored@fixture");

    assert_eq!(result.as_deref(), cache_path.to_str());
}

// 不存在的路径 + 无效 marketplace name 返回 None
#[test]
fn FindPlugin_InvalidId_001() {
    let result = find_valid_plugin_path("C:\\nonexistent\\path", "fake-plugin@fake-marketplace");
    assert!(result.is_none());
}

// ==================== resolve_marketplace_plugin_path ====================

fn create_marketplace_fixture(
    source: serde_json::Value,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let install_location = temp.path().join("marketplace");
    let plugin_path = install_location.join("plugins").join("fixture-plugin");
    std::fs::create_dir_all(plugin_path.join(".claude-plugin")).unwrap();
    std::fs::write(plugin_path.join(".claude-plugin").join("plugin.json"), "{}").unwrap();
    std::fs::create_dir_all(install_location.join(".claude-plugin")).unwrap();
    std::fs::write(
        install_location
            .join(".claude-plugin")
            .join("marketplace.json"),
        serde_json::to_string(&json!({"plugins": [{"name": "fixture-plugin", "source": source}]}))
            .unwrap(),
    )
    .unwrap();
    let known_dir = temp.path().join(".claude").join("plugins");
    std::fs::create_dir_all(&known_dir).unwrap();
    std::fs::write(
        known_dir.join("known_marketplaces.json"),
        serde_json::to_string(&json!({"fixture": {"installLocation": install_location}})).unwrap(),
    )
    .unwrap();
    (temp, plugin_path)
}

#[test]
fn ResolveMarketplace_LocalDirectory_001() {
    let (temp, plugin_path) = create_marketplace_fixture(json!("./plugins/fixture-plugin"));
    let result = resolve_marketplace_plugin_path_at(temp.path(), "fixture-plugin@fixture");

    let resolved = std::path::PathBuf::from(result.expect("fixture plugin path should resolve"));
    assert_eq!(
        resolved.canonicalize().unwrap(),
        plugin_path.canonicalize().unwrap()
    );
}

#[test]
fn ResolveMarketplace_SourceObject_001() {
    let (temp, plugin_path) =
        create_marketplace_fixture(json!({"source": "./plugins/fixture-plugin"}));
    let result = resolve_marketplace_plugin_path_at(temp.path(), "fixture-plugin@fixture");

    let resolved = std::path::PathBuf::from(result.expect("fixture plugin path should resolve"));
    assert_eq!(
        resolved.canonicalize().unwrap(),
        plugin_path.canonicalize().unwrap()
    );
}

#[test]
fn ResolveMarketplace_UnknownMarketplace_001() {
    let (temp, _) = create_marketplace_fixture(json!("./plugins/fixture-plugin"));
    assert!(resolve_marketplace_plugin_path_at(temp.path(), "plugin@missing").is_none());
}

#[test]
fn ResolveMarketplace_BadFormat_001() {
    let (temp, _) = create_marketplace_fixture(json!("./plugins/fixture-plugin"));
    assert!(resolve_marketplace_plugin_path_at(temp.path(), "no-at-sign").is_none());
}
// ==================== search_session_messages_in_dirs ====================

// 构造一行 JSONL 消息（user/assistant，content 为 string）
fn build_jsonl_line(msg_type: &str, content: &str) -> String {
    let t = if msg_type == "user" {
        "user"
    } else {
        "assistant"
    };
    format!(
        r#"{{"type":"{}","message":{{"content":"{}"}}}}"#,
        t, content
    )
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
    content.push_str(&format!(
        "{}\n",
        build_jsonl_line("user", "TARGET_KEYWORD_HERE")
    ));
    for i in 0..250 {
        content.push_str(&format!(
            "{}\n",
            build_jsonl_line("assistant", &format!("filler {}", i))
        ));
    }
    std::fs::write(&file_path, content).unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let results = search_session_messages_in_dirs(&dirs, "/proj", "TARGET_KEYWORD", 10);
    assert_eq!(
        results.len(),
        1,
        "old message outside newest 200 lines should be matched"
    );
    assert!(results[0].snippet.contains("TARGET_KEYWORD"));
}

// 同一文件多条匹配，snippet 取最新（最末尾的匹配）
#[test]
fn SearchSession_LatestMatchFirst_001() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("multi.jsonl");
    let mut content = String::new();
    content.push_str(&format!(
        "{}\n",
        build_jsonl_line("user", "KEYWORD old match")
    ));
    content.push_str(&format!(
        "{}\n",
        build_jsonl_line("assistant", "no match here")
    ));
    content.push_str(&format!(
        "{}\n",
        build_jsonl_line("user", "KEYWORD new match")
    ));
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
    assert!(
        backup.contains("https://x"),
        "backup should contain url content"
    );
    assert!(
        !backup.contains("\"other\""),
        "backup should only contain zread"
    );

    // 主配置保留其他字段和其他 server
    let main = std::fs::read_to_string(&claude_json).unwrap();
    assert!(
        main.contains("\"keepMe\""),
        "other config must be preserved"
    );
    assert!(main.contains("\"other\""), "other server must be preserved");
    assert!(
        !main.contains("zread"),
        "zread should be removed from main config"
    );
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
    assert!(
        main.contains("zread"),
        "zread should be back in main config"
    );
    assert!(
        main.contains("https://x"),
        "zread config content should be intact"
    );
    assert!(main.contains("\"keepMe\""), "other config preserved");
    assert!(main.contains("\"other\""), "other server preserved");
    assert!(
        !disabled_dir.join("zread.json").exists(),
        "backup file should be removed"
    );
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
    std::fs::write(&claude_json, r#"{"mcpServers":{"zread":{"url":"x"}}}"#).unwrap();
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
    std::fs::write(&claude_json, r#"{"mcpServers":{"zread":{"url":"old"}}}"#).unwrap();
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

// ==================== get_projects_state_at ====================

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

// 父目录不存在时 with_locked 自动创建 lock 文件 + 数据文件
#[test]
fn WithLocked_CreatesParentDir_001() {
    let tmp = tempfile::tempdir().unwrap();
    let nested_dir = tmp.path().join("nested").join("deep");
    let data = nested_dir.join("projects.json");
    let lock = nested_dir.join("projects.json.lock");
    with_projects_state_locked(&data, &lock, |s| {
        s.pinned_projects.push("e:/x".into());
        Ok::<(), anyhow::Error>(())
    })
    .unwrap();
    assert!(data.exists(), "数据文件应被创建");
    assert!(lock.exists(), "lock 文件应被创建");
    let state = get_projects_state_at(&data).unwrap();
    assert_eq!(state.pinned_projects, vec!["e:/x"]);
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

// with_locked 写后文件内容是合法 JSON 且字段名为 camelCase
#[test]
fn WithLocked_WritesCamelCase_001() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    with_projects_state_locked(&data, &lock, |s| {
        s.pinned_projects.push("a".into());
        Ok::<(), anyhow::Error>(())
    })
    .unwrap();
    let content = std::fs::read_to_string(&data).unwrap();
    assert!(
        content.contains("\"pinnedProjects\""),
        "应使用 camelCase 字段名"
    );
    assert!(!content.contains("pinned_projects"), "不应出现 snake_case");
    // 内容整体可被解析回 ProjectsState
    let reparsed: ProjectsState = serde_json::from_str(&content).unwrap();
    assert_eq!(reparsed.pinned_projects, vec!["a"]);
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

// ==================== assemble_home_data ====================

fn sample_project(path: &str, last_duration: Option<u64>) -> Project {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string();
    Project {
        path: path.into(),
        name,
        last_session_id: None,
        last_cost: None,
        last_duration,
    }
}

fn sample_session(id: &str, project: &str, last_active_at: u64) -> SessionInfo {
    SessionInfo {
        session_id: id.into(),
        name: id.into(),
        project_path: project.into(),
        last_active_at,
    }
}

// startup_state 基于全量 projects + lastOpened 正确填充
#[test]
fn AssembleHome_StartupStateFromProjects_001() {
    let projects = vec![sample_project("/p-a", Some(100))];
    let home = assemble_home_data(projects, vec![], "/p-a", &[], 12, 20);
    let info = home
        .startup_state
        .last_opened_project_info
        .expect("info 应存在");
    assert!(info.exists);
    assert!(home.startup_state.has_any_project);
    assert!(home.startup_state.has_visible_project);
}

// 分页：projects 超 limit -> has_more + 截断；sessions 截断到 session_limit
#[test]
fn AssembleHome_Pagination_001() {
    let projects = vec![
        sample_project("/p-a", Some(30)),
        sample_project("/p-b", Some(20)),
        sample_project("/p-c", Some(10)),
    ];
    let sessions = vec![
        sample_session("s1", "/p-a", 5),
        sample_session("s2", "/p-a", 4),
        sample_session("s3", "/p-a", 3),
    ];
    let home = assemble_home_data(projects, sessions, "", &[], 2, 2);
    assert!(home.has_more);
    assert_eq!(home.projects.len(), 2);
    assert_eq!(home.recent_sessions.len(), 2);
    assert_eq!(home.projects[0].path, "/p-a");
    assert_eq!(home.projects[1].path, "/p-b");
}

// 核心：startup_state 用全量 projects（含分页外），非分页结果
#[test]
fn AssembleHome_StartupUsesFullSetBeyondPagination_001() {
    let projects = vec![
        sample_project("/p-a", Some(100)),   // 分页内（limit=1）
        sample_project("/p-deep", Some(50)), // 分页外
    ];
    let home = assemble_home_data(projects, vec![], "/p-deep", &[], 1, 20);
    assert_eq!(home.projects.len(), 1);
    assert_eq!(home.projects[0].path, "/p-a");
    let info = home
        .startup_state
        .last_opened_project_info
        .expect("info 应存在");
    assert!(
        info.exists,
        "startup_state 应基于全量 projects，含分页外项目"
    );
}

// projects 按 last_duration 降序
#[test]
fn AssembleHome_SortByLastModifiedDesc_001() {
    let projects = vec![
        sample_project("/old", Some(10)),
        sample_project("/new", Some(100)),
        sample_project("/mid", Some(50)),
    ];
    let home = assemble_home_data(projects, vec![], "", &[], 12, 20);
    assert_eq!(home.projects[0].path, "/new");
    assert_eq!(home.projects[1].path, "/mid");
    assert_eq!(home.projects[2].path, "/old");
}

// hidden 影响可见性（startup_state 用全量 + hidden 判定）
#[test]
fn AssembleHome_HiddenAffectsVisibility_001() {
    let projects = vec![
        sample_project("/p-a", Some(100)),
        sample_project("/p-b", Some(50)),
    ];
    let home = assemble_home_data(projects, vec![], "", &["/p-a".to_string()], 12, 20);
    assert!(home.startup_state.has_any_project);
    assert!(home.startup_state.has_visible_project); // /p-b 仍可见
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
    assert!(
        json.contains("\"displayNames\""),
        "字段名须为 camelCase displayNames"
    );
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
    let json =
        r#"{"pinnedProjects":[],"archivedSessions":{},"displayNames":{"/p-a":"别名","/p-b":123}}"#;
    let state: ProjectsState = serde_json::from_str(json).unwrap();
    assert_eq!(state.display_names.get("/p-a"), Some(&"别名".to_string()));
    assert!(
        !state.display_names.contains_key("/p-b"),
        "非 string 值条目跳过"
    );
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
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({"old": true})).unwrap(),
    )
    .unwrap();
    write_json_atomic(&path, &serde_json::json!({"displayNames": {"/p-a": "新"}})).unwrap();
    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back["displayNames"]["/p-a"], "新");
    assert!(back.get("old").is_none(), "完整替换，旧 key 不残留");
}

// 原子写：目标已存在时二次写入成功（Windows 使用 ReplaceFileW，不制造 remove->rename 空窗）。
// 这是 codex 致命#1 的核心场景：projects.json 首次写入后，后续 update 必须仍能覆盖。
#[test]
fn WriteJsonAtomic_ReplacesExisting_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    // 第一次写（目标不存在）
    write_json_atomic(
        &path,
        &serde_json::json!({"displayNames": {"/p-a": "first"}}),
    )
    .unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("first"));
    // 第二次写（目标已存在）--Windows 上裸 fs::rename 会失败，原子 replace 后须成功
    write_json_atomic(
        &path,
        &serde_json::json!({"displayNames": {"/p-a": "second"}}),
    )
    .unwrap();
    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        back["displayNames"]["/p-a"], "second",
        "目标已存在时二次写入须覆盖成功"
    );
    // 三次写仍成功（连续覆盖）
    write_json_atomic(
        &path,
        &serde_json::json!({"displayNames": {"/p-a": "third"}}),
    )
    .unwrap();
    let back3: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back3["displayNames"]["/p-a"], "third");
}

// replacement 不可用时必须保留旧目标；不能先删目标再发现 replacement 无法 rename。
#[test]
fn ReplaceFileAtomic_MissingReplacementPreservesTarget_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let missing = dir.path().join("missing.tmp");
    std::fs::write(&path, "old-state").unwrap();

    assert!(replace_file_atomic(&missing, &path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old-state");
}

// ==================== with_projects_state_locked 原子写（含故障注入） ====================

// with_locked apply displayNames 后读回一致（持久化往返）
#[test]
fn WithLocked_DisplayNames_Persist_001() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("projects.json");
    let lock = dir.path().join("projects.json.lock");
    with_projects_state_locked(&data, &lock, |s| {
        s.display_names.insert("/p-a".into(), "别名".into());
        Ok::<(), anyhow::Error>(())
    })
    .unwrap();
    let state: ProjectsState =
        serde_json::from_str(&std::fs::read_to_string(&data).unwrap()).unwrap();
    assert_eq!(state.display_names.get("/p-a"), Some(&"别名".to_string()));
}

// with_locked 二次 apply（目标已存在）仍成功：Windows remove+rename 闭环
#[test]
fn WithLocked_OverwriteExisting_001() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("projects.json");
    let lock = dir.path().join("projects.json.lock");
    with_projects_state_locked(&data, &lock, |s| {
        s.display_names.insert("/p-a".into(), "旧".into());
        Ok::<(), anyhow::Error>(())
    })
    .unwrap();
    with_projects_state_locked(&data, &lock, |s| {
        s.display_names.insert("/p-a".into(), "新".into());
        Ok::<(), anyhow::Error>(())
    })
    .unwrap();
    let state: ProjectsState =
        serde_json::from_str(&std::fs::read_to_string(&data).unwrap()).unwrap();
    assert_eq!(state.display_names.get("/p-a"), Some(&"新".to_string()));
}

// 故障注入：预先把 .json.tmp 建成目录 -> write_json_atomic 写 tmp 失败 ->
// with_projects_state_locked 返 Err 且原 projects.json 内容不变（原子性：失败不破坏旧文件）。
// 裸 fs::write(path) 不经 tmp，此场景下会成功覆盖 -> 与 expect Err 冲突，故能区分（非 false green）。
#[test]
fn WithLocked_AtomicWrite_FailPreservesOriginal_001() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("projects.json");
    let lock = dir.path().join("projects.json.lock");
    // 1) 先写入合法旧内容
    with_projects_state_locked(&data, &lock, |s| {
        s.display_names.insert("/p-a".into(), "旧别名".into());
        Ok::<(), anyhow::Error>(())
    })
    .unwrap();
    let original = std::fs::read_to_string(&data).unwrap();
    // 2) 把 tmp 路径占为目录，迫使 write_json_atomic 写 tmp 失败
    let tmp = data.with_extension("json.tmp");
    std::fs::create_dir(&tmp).unwrap();
    // 3) 再次 with_locked 应失败（fs::write(&tmp) 对目录失败，remove/rename 未到达）
    let res = with_projects_state_locked(&data, &lock, |s| {
        s.display_names.insert("/p-a".into(), "新别名".into());
        Ok::<(), anyhow::Error>(())
    });
    assert!(res.is_err(), "tmp 写失败须传播 Err");
    // 4) 原文件未被破坏
    assert_eq!(
        std::fs::read_to_string(&data).unwrap(),
        original,
        "原子写：失败不破坏原文件"
    );
}

// ==================== normalize_path_inner（平台感知规范化） ====================

// Windows/macOS（case_sensitive=false）：反斜杠规范 + 去尾斜杠 + 小写
#[test]
fn NormalizePath_CaseInsensitive_Normalize_001() {
    assert_eq!(
        normalize_path_inner("E:\\Source\\Foo\\", false),
        "e:/source/foo"
    );
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
    assert_eq!(
        normalize_path_inner("/work/Foo\\Bar/", true),
        "/work/Foo/Bar"
    );
}

// POSIX 根 '/' 去尾斜杠后恢复 '/'（非空串 key），两支平台一致
#[test]
fn NormalizePath_PosixRoot_Recovered_001() {
    assert_eq!(normalize_path_inner("/", true), "/");
    assert_eq!(normalize_path_inner("///", false), "/");
}

// ==================== canonicalize_state（legacy 等价键合并）====================

// pinned：多等价路径（斜杠/大小写）合并为单一 canonical，去重
#[test]
fn Canonicalize_PinnedMergesEquivalent_001() {
    let mut s = ProjectsState {
        pinned_projects: vec!["E:\\Repo".into(), "e:/repo".into(), "E:/Other".into()],
        ..Default::default()
    };
    canonicalize_state(&mut s);
    assert_eq!(
        s.pinned_projects,
        vec!["e:/repo".to_string(), "e:/other".to_string()]
    );
}

// archivedSessions：等价 key 合并、sessionId 去重
#[test]
fn Canonicalize_ArchivedMergesEquivalent_001() {
    let mut a = std::collections::HashMap::new();
    a.insert(
        "E:\\P".to_string(),
        vec!["s1".to_string(), "s2".to_string()],
    );
    a.insert("e:/p".to_string(), vec!["s2".to_string(), "s3".to_string()]);
    let mut s = ProjectsState {
        archived_sessions: a,
        ..Default::default()
    };
    canonicalize_state(&mut s);
    let merged = s.archived_sessions.get("e:/p").unwrap();
    assert_eq!(
        merged,
        &vec!["s1".to_string(), "s2".to_string(), "s3".to_string()]
    );
    assert_eq!(s.archived_sessions.len(), 1, "等价 key 应合并为 1 个");
}

// displayNames：等价 key 冲突保留原始 key 字典序最小的值（确定性）
#[test]
fn Canonicalize_DisplayNamesConflictDeterministic_001() {
    let mut d = std::collections::HashMap::new();
    d.insert("E:\\B".to_string(), "beta".to_string()); // 原始 key "E:\B"
    d.insert("e:/b".to_string(), "alpha".to_string()); // 原始 key "e:/b"
    let mut s = ProjectsState {
        display_names: d,
        ..Default::default()
    };
    canonicalize_state(&mut s);
    // 原始 key 字典序最小："E:\B" < "e:/b"（ASCII 'E'=69 < 'e'=101）-> 保留 "beta"
    assert_eq!(s.display_names.get("e:/b"), Some(&"beta".to_string()));
    assert_eq!(s.display_names.len(), 1);
}

// canonicalize 幂等：再跑一次不变
#[test]
fn Canonicalize_Idempotent_001() {
    let mut s = ProjectsState {
        pinned_projects: vec!["E:/A".into(), "e:/a".into()],
        ..Default::default()
    };
    canonicalize_state(&mut s);
    let after1 = s.pinned_projects.clone();
    canonicalize_state(&mut s);
    assert_eq!(s.pinned_projects, after1);
}

// ==================== with_projects_state_locked / read_projects_state_locked ====================

// 首次（无数据文件）：apply 后写入，返回的状态含 apply 结果；数据文件被创建
#[test]
fn WithLocked_FirstWriteAppliesAndReturns_001() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    let state = with_projects_state_locked(&data, &lock, |s| {
        s.pinned_projects.push("e:/a".into());
        Ok::<(), anyhow::Error>(())
    })
    .unwrap();
    assert_eq!(state.pinned_projects, vec!["e:/a".to_string()]);
    assert!(data.exists());
    assert!(lock.exists(), "lock 文件应被创建");
}

// apply 闭包返 Err -> command 层错误，状态不写入（原子性：失败不破坏旧文件）
#[test]
fn WithLocked_ApplyErrDoesNotWrite_001() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    // 先写入基线
    with_projects_state_locked(&data, &lock, |s| {
        s.pinned_projects.push("e:/keep".into());
        Ok(())
    })
    .unwrap();
    // apply 返 Err
    let res = with_projects_state_locked(&data, &lock, |_s| {
        Err::<(), anyhow::Error>(anyhow::anyhow!("alias invalid"))
    });
    assert!(res.is_err());
    // 旧内容保留
    let state = read_projects_state_locked(&data, &lock).unwrap();
    assert_eq!(state.pinned_projects, vec!["e:/keep".to_string()]);
}

// read_locked：数据文件不存在 -> default，不报错（不因空文件解析失败）
#[test]
fn ReadLocked_MissingFileDefaults_001() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    let state = read_projects_state_locked(&data, &lock).unwrap();
    assert!(state.pinned_projects.is_empty());
    assert!(state.archived_sessions.is_empty());
}

// 共享锁读取返回前必须 canonicalize，前端不能收到 legacy 等价键或重复 pinned。
#[test]
fn ReadLocked_CanonicalizesLegacyKeys_001() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    std::fs::write(
        &data,
        r#"{"pinnedProjects":["E:\\A","e:/a"],"displayNames":{"E:\\A":"A"}}"#,
    )
    .unwrap();

    let state = read_projects_state_locked(&data, &lock).unwrap();

    assert_eq!(state.pinned_projects, vec!["e:/a".to_string()]);
    assert_eq!(state.display_names.get("e:/a"), Some(&"A".to_string()));
    assert_eq!(state.display_names.len(), 1);
}

// 锁内 canonicalize：预置 legacy 等价键，with_locked 操作后返回已合并状态
#[test]
fn WithLocked_CanonicalizesBeforeApply_001() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    // 预置双等价 pinned
    std::fs::write(&data, r#"{"pinnedProjects":["E:\\A","e:/a"]}"#).unwrap();
    let state = with_projects_state_locked(&data, &lock, |s| {
        s.pinned_projects.push("e:/b".into());
        Ok::<(), anyhow::Error>(())
    })
    .unwrap();
    assert_eq!(
        state.pinned_projects,
        vec!["e:/a".to_string(), "e:/b".to_string()]
    );
}

// ==================== command 行为单测（模拟 command apply 逻辑）====================

// 模拟 pin_project command 的 apply 逻辑：pin 幂等（已含 normalized 等价则不重复）
#[test]
fn PinProjectCommand_Idempotent_001() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    let apply = |s: &mut ProjectsState| {
        let n = normalize_path_str_pub("E:/A");
        if !s.pinned_projects.contains(&n) {
            s.pinned_projects.push(n);
        }
        Ok::<(), anyhow::Error>(())
    };
    with_projects_state_locked(&data, &lock, apply).unwrap();
    // 再 pin 等价路径（不同大小写/斜杠），normalized 后同一 key，不应增加
    let s2 = with_projects_state_locked(&data, &lock, apply).unwrap();
    assert_eq!(
        s2.pinned_projects,
        vec!["e:/a".to_string()],
        "重复 pin 等价路径不增加"
    );
}

// 模拟 set_display_name command 的 apply 逻辑：超长 alias -> Err，状态不变（校验失败不写入）
#[test]
fn SetDisplayNameCommand_RejectsTooLong_001() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    let long_alias = "x".repeat(40);
    let res = with_projects_state_locked(&data, &lock, |s| {
        let trimmed = long_alias.trim();
        if trimmed.chars().count() > 32 {
            return Err(anyhow::anyhow!("alias too long"));
        }
        s.display_names.insert("e:/a".into(), trimmed.into());
        Ok::<(), anyhow::Error>(())
    });
    assert!(res.is_err(), "超长 alias 应被拒绝");
    let state = read_projects_state_locked(&data, &lock).unwrap();
    assert!(state.display_names.is_empty(), "校验失败不应写入");
}

// ==================== 跨进程并发（re-exec self 子任务）====================
// 读改写正确性用真多进程验证，覆盖实际多实例的进程边界与文件系统行为。
// 子任务模式：CC_BOX_CONC_TEST=<mode> + CC_BOX_CONC_DIR=<dir> -> 执行单次操作后 exit(0)。
// 主测试：spawn 多子进程并发，等待后断言磁盘。

use std::env;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

fn conc_dirs() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("projects.json");
    let lock = tmp.path().join("projects.json.lock");
    (tmp, data, lock)
}

fn wait_for_file(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "等待文件超时: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_child(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "子进程异常退出: {status}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("等待子进程超时");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 子任务入口：若 CC_BOX_CONC_TEST 设置，按 mode 执行单次操作后 exit(0)；否则返回 false（主测试继续）。
fn run_conc_child_if_set() -> bool {
    let (Ok(mode), Ok(dir)) = (env::var("CC_BOX_CONC_TEST"), env::var("CC_BOX_CONC_DIR")) else {
        return false;
    };
    let data = std::path::Path::new(&dir).join("projects.json");
    let lock = std::path::Path::new(&dir).join("projects.json.lock");
    if env::var_os("CC_BOX_CONC_BARRIER").is_some() {
        wait_for_file(
            &std::path::Path::new(&dir).join("start"),
            Duration::from_secs(5),
        );
    }
    match mode.as_str() {
        "pin_a" => {
            with_projects_state_locked(&data, &lock, |s| {
                s.pinned_projects.push("e:/a".into());
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
        }
        "pin_b" => {
            with_projects_state_locked(&data, &lock, |s| {
                s.pinned_projects.push("e:/b".into());
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
        }
        "read" => {
            // 持共享锁读，结果写入 marker 文件供主测试校验「未返空」
            let s = read_projects_state_locked(&data, &lock).unwrap();
            let marker = std::path::Path::new(&dir).join("read_result.txt");
            std::fs::write(&marker, format!("pinned={}", s.pinned_projects.len())).unwrap();
        }
        "hold_lock" => {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock)
                .unwrap();
            acquire_lock(&file, true, Duration::from_secs(1)).unwrap();
            std::fs::write(std::path::Path::new(&dir).join("lock_held"), "1").unwrap();
            wait_for_file(
                &std::path::Path::new(&dir).join("release_lock"),
                Duration::from_secs(30),
            );
        }
        _ => {}
    }
    std::process::exit(0);
}

// 两子进程并发 pin 不同项目 -> 磁盘含两者（无 stale-write 丢失）
#[test]
fn Concurrent_TwoChildrenBothPreserved_001() {
    if run_conc_child_if_set() {
        return;
    }
    let (_tmp, data, lock) = conc_dirs();
    let dir = data.parent().unwrap();
    let exe = env::current_exe().unwrap();
    // 子进程只跑本测试单名 -> 单线程 -> 操作仅应用一次，避免默认 harness 并行跑全部 test 引入竞态
    let mut ha = Command::new(&exe)
        .arg("Concurrent_TwoChildrenBothPreserved_001")
        .env("CC_BOX_CONC_TEST", "pin_a")
        .env("CC_BOX_CONC_DIR", dir)
        .env("CC_BOX_CONC_BARRIER", "1")
        .spawn()
        .unwrap();
    let mut hb = Command::new(&exe)
        .arg("Concurrent_TwoChildrenBothPreserved_001")
        .env("CC_BOX_CONC_TEST", "pin_b")
        .env("CC_BOX_CONC_DIR", dir)
        .env("CC_BOX_CONC_BARRIER", "1")
        .spawn()
        .unwrap();
    std::fs::write(dir.join("start"), "1").unwrap();
    wait_for_child(&mut ha, Duration::from_secs(10));
    wait_for_child(&mut hb, Duration::from_secs(10));
    let state = read_projects_state_locked(&data, &lock).unwrap();
    let mut got = state.pinned_projects.clone();
    got.sort();
    assert_eq!(
        got,
        vec!["e:/a".to_string(), "e:/b".to_string()],
        "两子进程操作都应保留"
    );
}

// 首次（无数据文件）并发写不失败、不解析空文件
#[test]
fn Concurrent_FirstWriteNoFile_001() {
    if run_conc_child_if_set() {
        return;
    }
    let (_tmp, data, lock) = conc_dirs();
    assert!(!data.exists());
    let dir = data.parent().unwrap();
    let exe = env::current_exe().unwrap();
    let mut ha = Command::new(&exe)
        .arg("Concurrent_FirstWriteNoFile_001")
        .env("CC_BOX_CONC_TEST", "pin_a")
        .env("CC_BOX_CONC_DIR", dir)
        .env("CC_BOX_CONC_BARRIER", "1")
        .spawn()
        .unwrap();
    let mut hb = Command::new(&exe)
        .arg("Concurrent_FirstWriteNoFile_001")
        .env("CC_BOX_CONC_TEST", "pin_b")
        .env("CC_BOX_CONC_DIR", dir)
        .env("CC_BOX_CONC_BARRIER", "1")
        .spawn()
        .unwrap();
    std::fs::write(dir.join("start"), "1").unwrap();
    wait_for_child(&mut ha, Duration::from_secs(10));
    wait_for_child(&mut hb, Duration::from_secs(10));
    let state = read_projects_state_locked(&data, &lock).unwrap();
    let mut got = state.pinned_projects;
    got.sort();
    assert_eq!(got, vec!["e:/a".to_string(), "e:/b".to_string()]);
}

// writer 持排他锁时 reader（共享锁）阻塞到写完，不返 default 空
#[test]
fn Concurrent_ReaderDuringWrite_NotEmpty_001() {
    if run_conc_child_if_set() {
        return;
    }
    let (_tmp, data, _lock) = conc_dirs();
    let dir = data.parent().unwrap();
    let exe = env::current_exe().unwrap();
    // 先写入基线 pin_a
    let mut h0 = Command::new(&exe)
        .arg("Concurrent_ReaderDuringWrite_NotEmpty_001")
        .env("CC_BOX_CONC_TEST", "pin_a")
        .env("CC_BOX_CONC_DIR", dir)
        .spawn()
        .unwrap();
    wait_for_child(&mut h0, Duration::from_secs(5));
    // 子进程明确持排他锁，marker 出现后再启动 reader，证明共享读会等待。
    let mut holder = Command::new(&exe)
        .arg("Concurrent_ReaderDuringWrite_NotEmpty_001")
        .env("CC_BOX_CONC_TEST", "hold_lock")
        .env("CC_BOX_CONC_DIR", dir)
        .spawn()
        .unwrap();
    wait_for_file(&dir.join("lock_held"), Duration::from_secs(5));
    let mut hr = Command::new(&exe)
        .arg("Concurrent_ReaderDuringWrite_NotEmpty_001")
        .env("CC_BOX_CONC_TEST", "read")
        .env("CC_BOX_CONC_DIR", dir)
        .spawn()
        .unwrap();
    let marker = dir.join("read_result.txt");
    std::thread::sleep(Duration::from_millis(100));
    assert!(!marker.exists(), "writer 持排他锁期间 reader 不应完成");
    std::fs::write(dir.join("release_lock"), "1").unwrap();
    wait_for_child(&mut holder, Duration::from_secs(5));
    wait_for_child(&mut hr, Duration::from_secs(5));
    let content = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(content, "pinned=1", "reader 应在锁释放后读到完整基线状态");
}

// 持锁进程被杀后，OS 释放锁，另一实例可继续写。
#[test]
fn Concurrent_LockHolderExit_OtherProceeds_001() {
    if run_conc_child_if_set() {
        return;
    }
    let (_tmp, data, lock) = conc_dirs();
    let dir = data.parent().unwrap();
    let exe = env::current_exe().unwrap();
    let mut holder = Command::new(&exe)
        .arg("Concurrent_LockHolderExit_OtherProceeds_001")
        .env("CC_BOX_CONC_TEST", "hold_lock")
        .env("CC_BOX_CONC_DIR", dir)
        .spawn()
        .unwrap();
    wait_for_file(&dir.join("lock_held"), Duration::from_secs(5));
    holder.kill().unwrap();
    holder.wait().unwrap();
    let mut writer = Command::new(&exe)
        .arg("Concurrent_LockHolderExit_OtherProceeds_001")
        .env("CC_BOX_CONC_TEST", "pin_b")
        .env("CC_BOX_CONC_DIR", dir)
        .spawn()
        .unwrap();
    wait_for_child(&mut writer, Duration::from_secs(5));
    assert_eq!(
        read_projects_state_locked(&data, &lock)
            .unwrap()
            .pinned_projects,
        vec!["e:/b".to_string()]
    );
}

// 同一进程内第二个 handle 竞争同一文件锁时也必须有界超时。
#[test]
fn AcquireLock_Timeout_001() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("projects.json.lock");
    let first = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    let second = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    acquire_lock(&first, true, Duration::from_secs(1)).unwrap();
    let started = Instant::now();
    let err = acquire_lock(&second, true, Duration::from_millis(80)).unwrap_err();
    assert!(started.elapsed() >= Duration::from_millis(60));
    assert!(
        err.to_string().contains("lock timeout"),
        "实际错误: {err:#}"
    );
}
