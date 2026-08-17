use serde_json::json;

use super::store_profiling::{benchmark_active_full_scan_real, profile_real_home_variants};
use crate::session_name_index::{
    FileStamp, IndexHealth, IndexLimits, IndexMutation, PendingIndexFlush, SessionNameEntry,
    SessionNameIndex, SessionNameIndexDelta, SessionNameIndexPaths, SessionNameIndexStore,
};
use crate::store::{
    acquire_lock, assemble_home_data, build_project_path_mapping_at,
    build_project_path_mapping_strict_at, canonicalize_state, compute_project_startup_state,
    delete_sessions_inner, expand_env_vars, extract_md_description,
    extract_project_path_from_jsonl, extract_session_name, find_valid_plugin_path,
    get_all_recent_sessions_indexed_at, get_home_data, get_home_data_indexed_at,
    get_projects_state_at, get_sessions_from_dirs, get_sessions_indexed_at, infer_server_type,
    invalidate_project_path_mapping, lookup_project_dirs, merge_json_values, normalize_path_inner,
    normalize_path_str, parse_agents_list_output, parse_mcp_server_entry, parse_skill_description,
    parse_timestamp, read_projects_state_locked, replace_file_atomic,
    resolve_marketplace_plugin_path_at, scan_home_projects_at, search_session_messages_in_dirs,
    set_agent_enabled_in, set_mcp_server_enabled_in, set_skill_enabled_in,
    validate_session_id_component, with_project_path_mapping, with_projects_state_locked,
    write_json_atomic, AgentInfo, AppConfig, Project, ProjectPathMapping, ProjectsState,
    SessionInfo,
};

use std::collections::HashMap;
use std::hint::black_box;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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

// 检查真实 ~/.claude/projects 数据集上的 home 聚合基准(需 CC_DESK_BENCH_REAL_HOME 门禁,默认跳过)
#[test]
#[ignore = "reads the real ~/.claude/projects dataset"]
fn BenchmarkHomeData_RealHistory_001() {
    assert_eq!(
        std::env::var("CC_DESK_BENCH_REAL_HOME").as_deref(),
        Ok("1"),
        "set CC_DESK_BENCH_REAL_HOME=1 explicitly"
    );

    let mut samples = Vec::with_capacity(5);
    for _ in 0..5 {
        invalidate_project_path_mapping();
        let started = Instant::now();
        let home = get_home_data(12, 20, "", &[]).expect("real home scan should succeed");
        assert!(
            !home.projects.is_empty(),
            "real home benchmark found no projects under {:?}",
            dirs::home_dir()
        );
        black_box(home);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "home_data_warm_p50_ms={:.1}; samples_ms={samples:?}",
        samples[2]
    );
}

// 对同一真实数据交错比较旧双扫、单快照旧名称解析、单快照流式名称解析。
#[test]
#[ignore = "reads the real ~/.claude/projects dataset"]
fn BenchmarkHomeBreakdown_Real_002() {
    assert_eq!(std::env::var("CC_DESK_BENCH_REAL_HOME").as_deref(), Ok("1"));
    let report = profile_real_home_variants(7).expect("profile should succeed");
    assert_eq!(report.rounds, 7);
    assert!(report.variants_are_equivalent);
    assert_eq!(report.samples.len(), 3);
    assert_eq!(report.p50.len(), 3);
    eprintln!(
        "{report:#?}\nsnapshot_overhead_ms={:.1}; snapshot_overhead_percent={:.1}; stream_overhead_ms={:.1}; stream_overhead_percent={:.1}; residual_p50_ms={:.1}; final_warm_limit_ms={:.1}; phase0a_should_stop={}",
        report.snapshot_overhead_ms,
        report.snapshot_overhead_percent,
        report.stream_overhead_ms,
        report.stream_overhead_percent,
        report.residual_p50_ms,
        report.final_warm_limit_ms,
        report.phase0a_should_stop,
    );
}

// 在真实近期会话的临时副本上量化 active-1/4/8 每次失效后的全量名称重扫。
#[test]
#[ignore = "reads the real ~/.claude/projects dataset and writes only temporary copies"]
fn BenchmarkActiveFullScan_Real_004() {
    assert_eq!(std::env::var("CC_DESK_BENCH_REAL_HOME").as_deref(), Ok("1"));
    let phase0a = profile_real_home_variants(7).expect("Phase 0A profile should succeed");
    assert!(
        !phase0a.phase0a_should_stop,
        "Phase 0A stop gate blocks active-session benchmarking"
    );
    let report =
        benchmark_active_full_scan_real(7, phase0a.residual_p50_ms, phase0a.final_warm_limit_ms)
            .expect("active full-scan benchmark should succeed");
    assert_eq!(report.rounds, 7);
    assert!(report.samples.values().all(|samples| samples.len() == 7));
    assert!(report
        .samples
        .iter()
        .all(|(k, samples)| samples.iter().all(|sample| sample.k == *k)));
    assert_eq!(report.source_file_sizes.len(), 8);
    assert_eq!(report.p50_name_parse_ms.len(), 3);
    assert_eq!(report.p50_jsonl_bytes_read.len(), 3);
    assert_eq!(report.estimated_active_total_p50_ms.len(), 3);
    eprintln!(
        "{report:#?}\nactive4_limit_ms={:.1}; append_resume_required={}",
        report.active4_limit_ms, report.append_resume_required
    );
}

fn percentile_ms(samples: &[f64], percentile_index: usize) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[percentile_index.min(sorted.len() - 1)]
}

fn normalized_session_rows(sessions: &[SessionInfo]) -> Vec<(String, String, String)> {
    sessions
        .iter()
        .map(|session| {
            (
                session.project_path.clone(),
                session.session_id.clone(),
                session.name.clone(),
            )
        })
        .collect()
}

// 真实 ~/.claude/projects 的 direct/cold/warm/history 对照；索引只写测试临时目录。
#[test]
#[ignore = "reads real ~/.claude/projects and writes only temporary indexes"]
fn BenchmarkHomeIndex_Real_003() {
    let projects_root = dirs::home_dir().unwrap().join(".claude").join("projects");
    let scan = scan_home_projects_at(&projects_root).unwrap();
    assert!(!scan.projects.is_empty());
    let history_project = scan
        .projects
        .iter()
        .find_map(|project| {
            scan.mapping
                .get(&project.path)
                .filter(|dirs| !dirs.is_empty())
                .map(|dirs| (project.path.clone(), dirs.clone()))
        })
        .expect("real benchmark requires one project with sessions");

    let mut direct_samples = Vec::new();
    let mut cold_samples = Vec::new();
    let mut warm_samples = Vec::new();
    let mut history_cold_samples = Vec::new();
    let mut history_warm_samples = Vec::new();
    let mut warm_jsonl_bytes = Vec::new();
    let mut history_warm_jsonl_bytes = Vec::new();
    let orders = [
        ["direct", "cold", "warm", "history-cold", "history-warm"],
        ["cold", "warm", "history-cold", "history-warm", "direct"],
        ["warm", "history-cold", "history-warm", "direct", "cold"],
        ["history-cold", "history-warm", "direct", "cold", "warm"],
        ["history-warm", "direct", "cold", "warm", "history-cold"],
    ];

    for round in 0usize..7 {
        let home_temp = tempfile::tempdir().unwrap();
        let home_paths = SessionNameIndexPaths {
            data: home_temp.path().join("session-name-index.json"),
            lock: home_temp.path().join("session-name-index.json.lock"),
        };
        let home_reads = Arc::new(AtomicU64::new(0));
        let home_health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
        let home_store = SessionNameIndexStore::new(
            home_paths.clone(),
            IndexLimits::default(),
            home_health,
            std::time::Duration::from_millis(100),
        )
        .with_snapshot_read_counter(Arc::clone(&home_reads));
        let history_temp = tempfile::tempdir().unwrap();
        let history_store = {
            let paths = SessionNameIndexPaths {
                data: history_temp.path().join("session-name-index.json"),
                lock: history_temp.path().join("session-name-index.json.lock"),
            };
            let reads = Arc::new(AtomicU64::new(0));
            let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
            SessionNameIndexStore::new(
                paths.clone(),
                IndexLimits::default(),
                health,
                std::time::Duration::from_millis(100),
            )
            .with_snapshot_read_counter(Arc::clone(&reads))
        };

        let cold_started = Instant::now();
        let cold = get_home_data_indexed_at(
            &projects_root,
            12,
            20,
            "",
            &[],
            &home_store,
            2_000 + round as u64,
            None,
        )
        .unwrap();
        let cold_elapsed = cold_started.elapsed().as_secs_f64() * 1000.0;
        let cold_rows = normalized_session_rows(&cold.value.recent_sessions);
        if let Some(pending) = cold.pending_flush {
            home_store.flush_pending(pending).unwrap();
        }

        let history_cold_started = Instant::now();
        let history_cold = get_sessions_indexed_at(
            &history_project.0,
            &history_project.1,
            20,
            0,
            &history_store,
            2_000 + round as u64,
            None,
        )
        .unwrap();
        let history_cold_elapsed = history_cold_started.elapsed().as_secs_f64() * 1000.0;
        let history_rows = normalized_session_rows(&history_cold.value);
        if let Some(pending) = history_cold.pending_flush {
            history_store.flush_pending(pending).unwrap();
        }

        for mode in orders[round % orders.len()] {
            match mode {
                "direct" => {
                    let started = Instant::now();
                    let direct = get_home_data(12, 20, "", &[]).unwrap();
                    direct_samples.push(started.elapsed().as_secs_f64() * 1000.0);
                    assert_eq!(normalized_session_rows(&direct.recent_sessions), cold_rows);
                }
                "cold" => cold_samples.push(cold_elapsed),
                "warm" => {
                    let started = Instant::now();
                    let warm = get_home_data_indexed_at(
                        &projects_root,
                        12,
                        20,
                        "",
                        &[],
                        &home_store,
                        3_000 + round as u64,
                        None,
                    )
                    .unwrap();
                    warm_samples.push(started.elapsed().as_secs_f64() * 1000.0);
                    warm_jsonl_bytes.push(warm.stats.jsonl_bytes_read);
                    assert_eq!(
                        normalized_session_rows(&warm.value.recent_sessions),
                        cold_rows
                    );
                    assert!(warm.pending_flush.is_none());
                }
                "history-cold" => history_cold_samples.push(history_cold_elapsed),
                "history-warm" => {
                    let started = Instant::now();
                    let warm = get_sessions_indexed_at(
                        &history_project.0,
                        &history_project.1,
                        20,
                        0,
                        &history_store,
                        3_000 + round as u64,
                        None,
                    )
                    .unwrap();
                    history_warm_samples.push(started.elapsed().as_secs_f64() * 1000.0);
                    history_warm_jsonl_bytes.push(warm.stats.jsonl_bytes_read);
                    assert_eq!(normalized_session_rows(&warm.value), history_rows);
                    assert!(warm.pending_flush.is_none());
                }
                _ => unreachable!(),
            }
        }
    }

    for (mode, samples) in [
        ("direct", &direct_samples),
        ("cold", &cold_samples),
        ("warm", &warm_samples),
        ("history-cold", &history_cold_samples),
        ("history-warm", &history_warm_samples),
    ] {
        eprintln!(
            "mode={mode}; samples_ms={samples:?}; p50_ms={:.3}; p95_ms={:.3}",
            percentile_ms(samples, 3),
            percentile_ms(samples, 6),
        );
    }
    eprintln!(
        "warm_jsonl_bytes={warm_jsonl_bytes:?}; history_warm_jsonl_bytes={history_warm_jsonl_bytes:?}"
    );

    let direct_p50 = percentile_ms(&direct_samples, 3);
    let cold_p50 = percentile_ms(&cold_samples, 3);
    let warm_p50 = percentile_ms(&warm_samples, 3);
    assert!(cold_p50 <= direct_p50 * 1.15);
    assert!(warm_p50 <= 250.0);
    assert!(warm_p50 <= 2_200.6 * 0.25);
    assert!(warm_jsonl_bytes.iter().all(|bytes| *bytes == 0));
    assert!(history_warm_jsonl_bytes.iter().all(|bytes| *bytes == 0));
}

// 真实近期 JSONL 的临时副本：追加项必须 full rebuild，未改项必须 exact hit。
#[test]
#[ignore = "reads real ~/.claude/projects and writes only temporary copies/indexes"]
fn BenchmarkActiveIndex_Real_005() {
    assert_eq!(std::env::var("CC_DESK_BENCH_REAL_HOME").as_deref(), Ok("1"));
    let projects_root = dirs::home_dir().unwrap().join(".claude").join("projects");
    let mut sources = Vec::new();
    for project in std::fs::read_dir(&projects_root).unwrap().flatten() {
        if !project.path().is_dir() {
            continue;
        }
        for file in std::fs::read_dir(project.path()).unwrap().flatten() {
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
                && !path
                    .file_name()
                    .map(|name| name.to_string_lossy().starts_with("agent-"))
                    .unwrap_or(false)
            {
                let modified = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok();
                sources.push((modified, path));
            }
        }
    }
    sources.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let sources = sources
        .into_iter()
        .map(|(_, path)| path)
        .take(8)
        .collect::<Vec<_>>();
    assert_eq!(sources.len(), 8);

    for k in [1usize, 4, 8] {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        let encoded = root.path().join(format!("encoded-{k}"));
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&encoded).unwrap();
        let mut copies = Vec::new();
        for (sequence, source) in sources.iter().enumerate() {
            let copy = encoded.join(format!("session-{sequence}.jsonl"));
            std::fs::copy(source, &copy).unwrap();
            copies.push(copy);
        }
        let paths = SessionNameIndexPaths {
            data: root.path().join("session-name-index.json"),
            lock: root.path().join("session-name-index.json.lock"),
        };
        let reads = Arc::new(AtomicU64::new(0));
        let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
        let store = SessionNameIndexStore::new(
            paths.clone(),
            IndexLimits::default(),
            health,
            std::time::Duration::from_millis(100),
        )
        .with_snapshot_read_counter(Arc::clone(&reads));
        let seeded = copies
            .iter()
            .map(|path| {
                (
                    encoded.as_path(),
                    path.as_path(),
                    extract_session_name(path),
                )
            })
            .collect::<Vec<_>>();
        let seed_refs = seeded
            .iter()
            .map(|(dir, path, name)| (*dir, *path, name.as_str()))
            .collect::<Vec<_>>();
        {
            let mut seed_index = SessionNameIndex::empty();
            for (project_dir, path, name) in &seed_refs {
                let stamp = FileStamp::read(path).unwrap();
                let project_key = normalize_path_str(&project_dir.to_string_lossy());
                let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
                seed_index.projects.entry(project_key).or_default().insert(
                    file_name,
                    SessionNameEntry {
                        name: (*name).to_string(),
                        observed_length: stamp.observed_length,
                        modified_secs: stamp.modified_secs,
                        modified_nanos: stamp.modified_nanos,
                        cached_at_ms: 1_000,
                    },
                );
            }
            std::fs::write(&paths.data, serde_json::to_vec(&seed_index).unwrap()).unwrap();
        }

        let mut samples = Vec::new();
        for round in 0..7 {
            let title = format!("active-{k}-round-{round}");
            for (sequence, path) in copies.iter().take(k).enumerate() {
                let event = if sequence == 0 {
                    format!(
                        "{{\"type\":\"custom-title\",\"customTitle\":{}}}\n",
                        serde_json::to_string(&title).unwrap()
                    )
                } else {
                    "{\"type\":\"assistant\",\"message\":{\"content\":[]}}\n".to_string()
                };
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(path)
                    .unwrap()
                    .write_all(event.as_bytes())
                    .unwrap();
            }

            let started = Instant::now();
            let result = get_sessions_indexed_at(
                real.to_string_lossy().as_ref(),
                std::slice::from_ref(&encoded),
                8,
                0,
                &store,
                10_000 + round,
                None,
            )
            .unwrap();
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(result.stats.full_rebuilds, k as u64);
            assert_eq!(result.stats.exact_hits, (8 - k) as u64);
            assert!(result.stats.jsonl_bytes_read > 0);
            assert!(result.value.iter().any(|session| session.name == title));
        }
        eprintln!(
            "active_k={k}; samples_ms={samples:?}; p50_ms={:.3}; p95_ms={:.3}",
            percentile_ms(&samples, 3),
            percentile_ms(&samples, 6)
        );
        if k == 4 {
            assert!(percentile_ms(&samples, 3) <= 350.0);
        }
        if k == 8 {
            assert!(percentile_ms(&samples, 3) <= 500.0);
        }
    }
}

// 四进程共享临时索引：real/8MiB disjoint、same-key stale base、CAS exhaustion。
#[test]
#[ignore = "reads real home for measured index size and runs four child processes"]
fn BenchmarkIndexMultiProcess_Real_006() {
    if let Ok(mode) = std::env::var("CC_DESK_INDEX_WORKER_MODE") {
        let dir = std::path::PathBuf::from(std::env::var("CC_DESK_INDEX_WORKER_DIR").unwrap());
        let id = std::env::var("CC_DESK_INDEX_WORKER_ID")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let request_compaction = std::env::var_os("CC_DESK_INDEX_WORKER_COMPACT").is_some();
        let paths = SessionNameIndexPaths {
            data: dir.join("session-name-index.json"),
            lock: dir.join("session-name-index.json.lock"),
        };
        let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
        let mut store = SessionNameIndexStore::new(
            paths.clone(),
            IndexLimits::default(),
            health,
            Duration::from_millis(100),
        );
        let session_path = dir.join(format!("worker-{id}.jsonl"));
        std::fs::write(&session_path, format!("worker-{id}")).unwrap();
        let snapshot = store.read_snapshot();
        let project_key = "worker-project".to_string();
        let file_name = if mode == "same" {
            "same.jsonl".to_string()
        } else {
            format!("worker-{id}.jsonl")
        };
        let base = snapshot
            .index
            .projects
            .get(&project_key)
            .and_then(|bucket| bucket.get(&file_name))
            .cloned();
        let stamp = FileStamp::read(&session_path).unwrap();
        let replacement = SessionNameEntry {
            name: format!("worker-{id}"),
            observed_length: stamp.observed_length,
            modified_secs: stamp.modified_secs,
            modified_nanos: stamp.modified_nanos,
            cached_at_ms: u64::MAX - id as u64,
        };
        let pending = PendingIndexFlush {
            base_raw: snapshot.raw,
            delta: SessionNameIndexDelta {
                mutations: vec![IndexMutation {
                    project_key,
                    file_name,
                    path: session_path,
                    base,
                    replacement,
                }],
                request_compaction,
                ..SessionNameIndexDelta::default()
            },
        };
        if mode == "cas" {
            let data_path = paths.data.clone();
            store = store.with_flush_test_config(
                Duration::from_secs(1),
                4,
                None,
                Some(Arc::new(move |_| {
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(&data_path)
                        .unwrap()
                        .write_all(b" ")
                        .unwrap();
                })),
                None,
            );
        }
        std::fs::write(dir.join(format!("ready-{id}")), "1").unwrap();
        wait_for_file(&dir.join("start-flush"), Duration::from_secs(10));
        let result = store.flush_pending(pending);
        let report = match result {
            Ok(metrics) => serde_json::json!({
                "ok": true,
                "exclusiveHoldMs": metrics.exclusive_hold.as_secs_f64() * 1000.0,
                "attempts": metrics.attempts,
            }),
            Err(error) => serde_json::json!({
                "ok": false,
                "error": error.to_string(),
            }),
        };
        std::fs::write(
            dir.join(format!("result-{id}.json")),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        std::process::exit(0);
    }
    assert_eq!(std::env::var("CC_DESK_BENCH_REAL_HOME").as_deref(), Ok("1"));
    let projects_root = dirs::home_dir().unwrap().join(".claude").join("projects");
    let real_temp = tempfile::tempdir().unwrap();
    let real_paths = SessionNameIndexPaths {
        data: real_temp.path().join("session-name-index.json"),
        lock: real_temp.path().join("session-name-index.json.lock"),
    };
    let real_reads = Arc::new(AtomicU64::new(0));
    let real_health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let real_store = SessionNameIndexStore::new(
        real_paths.clone(),
        IndexLimits::default(),
        real_health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&real_reads));
    let cold = get_home_data_indexed_at(&projects_root, 12, 20, "", &[], &real_store, 2_000, None)
        .unwrap();
    real_store
        .flush_pending(cold.pending_flush.unwrap())
        .unwrap();
    let real_bytes = std::fs::read(&real_paths.data).unwrap();
    let eight_mib = {
        let target_bytes = 8 * 1024 * 1024;
        let mut count = (target_bytes / 180).max(1);
        let mut closest = Vec::new();
        for _ in 0..8 {
            let mut index = SessionNameIndex::empty();
            let bucket = index.projects.entry("synthetic".to_string()).or_default();
            for sequence in 0..count {
                bucket.insert(
                    format!("session-{sequence:08}.jsonl"),
                    SessionNameEntry {
                        name: format!("Synthetic {sequence:08} xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
                        observed_length: sequence as u64,
                        modified_secs: 1_700_000_000,
                        modified_nanos: sequence as u32,
                        cached_at_ms: sequence as u64,
                    },
                );
            }
            let bytes = serde_json::to_vec(&index).unwrap();
            if closest.is_empty()
                || bytes.len().abs_diff(target_bytes) < closest.len().abs_diff(target_bytes)
            {
                closest = bytes.clone();
            }
            if bytes.len().abs_diff(target_bytes) <= target_bytes / 100 {
                break;
            }
            count = count
                .saturating_mul(target_bytes)
                .checked_div(bytes.len().max(1))
                .unwrap_or(1)
                .max(1);
        }
        closest
    };
    eprintln!(
        "multi_process_real_input_bytes={}; eight_mib_input_bytes={}",
        real_bytes.len(),
        eight_mib.len()
    );

    for (label, initial, compact) in [
        ("real", real_bytes.as_slice(), false),
        ("8mib", eight_mib.as_slice(), true),
    ] {
        for mode in ["disjoint", "same", "cas"] {
            let (dir, reports): (tempfile::TempDir, Vec<serde_json::Value>) = {
                let dir = tempfile::tempdir().unwrap();
                std::fs::write(dir.path().join("session-name-index.json"), initial).unwrap();
                let exe = std::env::current_exe().unwrap();
                let mut children = Vec::new();
                for id in 0..4 {
                    let mut command = std::process::Command::new(&exe);
                    command
                        .arg("BenchmarkIndexMultiProcess_Real_006")
                        .arg("--ignored")
                        .arg("--test-threads=1")
                        .env("CC_DESK_INDEX_WORKER_MODE", mode)
                        .env("CC_DESK_INDEX_WORKER_DIR", dir.path())
                        .env("CC_DESK_INDEX_WORKER_ID", id.to_string());
                    if compact {
                        command.env("CC_DESK_INDEX_WORKER_COMPACT", "1");
                    }
                    children.push(command.spawn().unwrap());
                }
                for id in 0..4 {
                    wait_for_file(
                        &dir.path().join(format!("ready-{id}")),
                        Duration::from_secs(10),
                    );
                }
                std::fs::write(dir.path().join("start-flush"), "1").unwrap();
                for child in &mut children {
                    wait_for_child(child, Duration::from_secs(30));
                }
                let reports = (0..4)
                    .map(|id| {
                        serde_json::from_slice(
                            &std::fs::read(dir.path().join(format!("result-{id}.json"))).unwrap(),
                        )
                        .unwrap()
                    })
                    .collect();
                (dir, reports)
            };
            eprintln!("multi_process_size={label}; mode={mode}; reports={reports:?}");
            assert!(std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .all(|entry| {
                    !entry
                        .file_name()
                        .to_string_lossy()
                        .contains("session-name-index.json.tmp.")
                }));
            if mode == "cas" {
                assert!(reports.iter().all(|report| {
                    !report["ok"].as_bool().unwrap()
                        && report["error"].as_str().unwrap().contains("CAS exhausted")
                }));
                continue;
            }
            assert!(reports.iter().all(|report| report["ok"].as_bool().unwrap()));
            let persisted: SessionNameIndex = serde_json::from_slice(
                &std::fs::read(dir.path().join("session-name-index.json")).unwrap(),
            )
            .unwrap();
            let bucket = &persisted.projects["worker-project"];
            assert_eq!(bucket.len(), if mode == "disjoint" { 4 } else { 1 });
            let holds = reports
                .iter()
                .map(|report| report["exclusiveHoldMs"].as_f64().unwrap())
                .collect::<Vec<_>>();
            if label == "real" {
                assert!(percentile_ms(&holds, 3) <= 100.0);
            } else {
                assert!(holds.iter().all(|hold| *hold <= 150.0));
            }
        }
    }
}

// cwd 在无效字节尾之前：流式早停读到 cwd，不读坏尾。
#[test]
fn ExtractProjectPath_StopBeforeBadTail_001() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session.jsonl");
    let mut bytes = b"{\"cwd\":\"C:/work/project\"}\n".to_vec();
    bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
    std::fs::write(file, bytes).unwrap();

    assert_eq!(
        extract_project_path_from_jsonl(dir.path()).as_deref(),
        Some("C:/work/project")
    );
}

// cwd 在无效 UTF-8 行之后：逐行容错跳过坏行，仍读到后续 cwd。
#[test]
fn ExtractProjectPath_SkipBadUtf8_002() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session.jsonl");
    let mut bytes = vec![0xff, 0xfe, b'\n'];
    bytes.extend_from_slice(b"{\"cwd\":\"C:/work/project\"}\n");
    std::fs::write(file, bytes).unwrap();

    assert_eq!(
        extract_project_path_from_jsonl(dir.path()).as_deref(),
        Some("C:/work/project")
    );
}

// 多个编码目录映射同一真实路径时，扫描结果按真实路径合并。
#[test]
fn ScanHomeProjects_MergeByRealPath_001() {
    let root = tempfile::tempdir().unwrap();
    let real = tempfile::tempdir().unwrap();
    let first = {
        let dir = root.path().join("old-encoding");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("old-encoding.jsonl"),
            format!(
                "{{\"cwd\":{}}}\n",
                serde_json::to_string(&real.path().to_string_lossy()).unwrap()
            ),
        )
        .unwrap();
        dir
    };
    let second = {
        let dir = root.path().join("new-encoding");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("new-encoding.jsonl"),
            format!(
                "{{\"cwd\":{}}}\n",
                serde_json::to_string(&real.path().to_string_lossy()).unwrap()
            ),
        )
        .unwrap();
        dir
    };

    let scan = scan_home_projects_at(root.path()).unwrap();
    let dirs = scan
        .mapping
        .get(real.path().to_string_lossy().as_ref())
        .unwrap();

    assert_eq!(scan.projects.len(), 2);
    assert_eq!(dirs.len(), 2);
    assert!(dirs.contains(&first));
    assert!(dirs.contains(&second));
}

// 跨多个编码目录合并会话后，排序与分页结果正确。
#[test]
fn GetSessionsFromDirs_SortAcrossDirs_001() {
    use std::time::{Duration, SystemTime};

    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let old = first.join("old.jsonl");
    let new = second.join("new.jsonl");
    std::fs::write(
        &old,
        "{\"type\":\"user\",\"message\":{\"content\":\"Old\"}}\n",
    )
    .unwrap();
    std::fs::write(
        &new,
        "{\"type\":\"user\",\"message\":{\"content\":\"New\"}}\n",
    )
    .unwrap();
    let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(base))
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&new)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(base + Duration::from_secs(10)))
        .unwrap();

    let dirs = vec![first, second];
    assert_eq!(
        get_sessions_from_dirs("C:/project", &dirs, 1, 0).unwrap()[0].session_id,
        "new"
    );
    assert_eq!(
        get_sessions_from_dirs("C:/project", &dirs, 1, 1).unwrap()[0].session_id,
        "old"
    );
}

// home 请求无论包含多少项目，都只能读取一次索引快照。
#[test]
fn HomeIndex_ReadsOnce_020() {
    let root = tempfile::tempdir().unwrap();
    let projects_root = root.path().join("projects");
    let real_one = root.path().join("real-one");
    let real_two = root.path().join("real-two");
    std::fs::create_dir_all(&real_one).unwrap();
    std::fs::create_dir_all(&real_two).unwrap();
    {
        std::fs::create_dir_all(projects_root.join("encoded-one")).unwrap();
        let path = projects_root.join("encoded-one").join("one.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real_one.to_string_lossy()).unwrap(),
                serde_json::to_string("One").unwrap(),
            ),
        )
        .unwrap();
    }
    {
        std::fs::create_dir_all(projects_root.join("encoded-two")).unwrap();
        let path = projects_root.join("encoded-two").join("two.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real_two.to_string_lossy()).unwrap(),
                serde_json::to_string("Two").unwrap(),
            ),
        )
        .unwrap();
    }
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));

    let result =
        get_home_data_indexed_at(&projects_root, 12, 20, "", &[], &store, 2_000, None).unwrap();

    assert_eq!(result.value.recent_sessions.len(), 2);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

// 单个项目目录在 resolve 阶段读取失败（目录被替换为同名文件 -> read_dir ENOTDIR）时，
// 首页不整体失败，仍返回其他项目的会话（spec §7 失败隔离）。
#[test]
fn HomeIndex_SkipFailedProject_022() {
    let root = tempfile::tempdir().unwrap();
    let projects_root = root.path().join("projects");
    let real_one = root.path().join("real-one");
    let real_two = root.path().join("real-two");
    std::fs::create_dir_all(&real_one).unwrap();
    std::fs::create_dir_all(&real_two).unwrap();
    let encoded_one = projects_root.join("encoded-one");
    {
        std::fs::create_dir_all(&encoded_one).unwrap();
        let path = encoded_one.join("one.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real_one.to_string_lossy()).unwrap(),
                serde_json::to_string("One").unwrap(),
            ),
        )
        .unwrap();
    }
    {
        std::fs::create_dir_all(projects_root.join("encoded-two")).unwrap();
        let path = projects_root.join("encoded-two").join("two.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real_two.to_string_lossy()).unwrap(),
                serde_json::to_string("Two").unwrap(),
            ),
        )
        .unwrap();
    }
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));

    // scan 完成后、首个项目 resolve 前破坏 encoded-one：删目录 + 建同名文件 -> read_dir 失败。
    let broken = std::sync::atomic::AtomicBool::new(false);
    let before_resolve = std::sync::Arc::new(move || {
        if !broken.swap(true, Ordering::SeqCst) {
            std::fs::remove_dir_all(&encoded_one).unwrap();
            std::fs::write(&encoded_one, b"not a dir").unwrap();
        }
    });

    let result = get_home_data_indexed_at(
        &projects_root,
        12,
        20,
        "",
        &[],
        &store,
        2_000,
        Some(before_resolve),
    )
    .unwrap();

    // 首页整体成功：失败项目会话被跳过（不出现"One"），成功项目会话仍返回（"Two"在）。
    let names: Vec<&str> = result
        .value
        .recent_sessions
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        names.contains(&"Two"),
        "surviving project missing: {names:?}"
    );
    assert!(
        !names.contains(&"One"),
        "failed project must be skipped: {names:?}"
    );
}

// all-recent 同语义：单项目 resolve 失败时仍返回其他项目会话。
#[test]
fn AllRecentIndex_SkipFailedProject_023() {
    let root = tempfile::tempdir().unwrap();
    let projects_root = root.path().join("projects");
    let real_one = root.path().join("real-one");
    let real_two = root.path().join("real-two");
    std::fs::create_dir_all(&real_one).unwrap();
    std::fs::create_dir_all(&real_two).unwrap();
    let encoded_one = projects_root.join("encoded-one");
    {
        std::fs::create_dir_all(&encoded_one).unwrap();
        let path = encoded_one.join("one.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real_one.to_string_lossy()).unwrap(),
                serde_json::to_string("One").unwrap(),
            ),
        )
        .unwrap();
    }
    {
        std::fs::create_dir_all(projects_root.join("encoded-two")).unwrap();
        let path = projects_root.join("encoded-two").join("two.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real_two.to_string_lossy()).unwrap(),
                serde_json::to_string("Two").unwrap(),
            ),
        )
        .unwrap();
    }
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));

    let broken = std::sync::atomic::AtomicBool::new(false);
    let before_resolve = std::sync::Arc::new(move || {
        if !broken.swap(true, Ordering::SeqCst) {
            std::fs::remove_dir_all(&encoded_one).unwrap();
            std::fs::write(&encoded_one, b"not a dir").unwrap();
        }
    });

    let result =
        get_all_recent_sessions_indexed_at(&projects_root, 20, &store, 2_000, Some(before_resolve))
            .unwrap();

    let names: Vec<&str> = result.value.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Two"),
        "surviving project missing: {names:?}"
    );
    assert!(
        !names.contains(&"One"),
        "failed project must be skipped: {names:?}"
    );
}

// home 仍只为每个真实项目解析排序后的前三条会话。
#[test]
fn HomeIndex_RecentThree_021() {
    let root = tempfile::tempdir().unwrap();
    let projects_root = root.path().join("projects");
    let real = root.path().join("real");
    let encoded = projects_root.join("encoded");
    std::fs::create_dir_all(&real).unwrap();
    for sequence in 0..5 {
        let path = {
            std::fs::create_dir_all(&encoded).unwrap();
            let inner_path = encoded.join(format!("session-{sequence}.jsonl"));
            std::fs::write(
                &inner_path,
                format!(
                    "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                    serde_json::to_string(&real.to_string_lossy()).unwrap(),
                    serde_json::to_string(&format!("Session {sequence}")).unwrap(),
                ),
            )
            .unwrap();
            inner_path
        };
        let modified =
            std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + sequence);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));

    let result =
        get_home_data_indexed_at(&projects_root, 12, 20, "", &[], &store, 2_000, None).unwrap();

    assert_eq!(result.value.recent_sessions.len(), 3);
    assert_eq!(result.stats.full_rebuilds, 3);
}

// warm history page 只解析分页命中的条目，并返回缓存名称与零 JSONL bytes。
#[test]
fn SessionsIndex_WarmPage_022() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let encoded = root.path().join("encoded");
    std::fs::create_dir_all(&real).unwrap();
    let first = {
        std::fs::create_dir_all(&encoded).unwrap();
        let inner_path = encoded.join("first.jsonl");
        std::fs::write(
            &inner_path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real.to_string_lossy()).unwrap(),
                serde_json::to_string("Disk first").unwrap(),
            ),
        )
        .unwrap();
        inner_path
    };
    let second = {
        std::fs::create_dir_all(&encoded).unwrap();
        let inner_path = encoded.join("second.jsonl");
        std::fs::write(
            &inner_path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real.to_string_lossy()).unwrap(),
                serde_json::to_string("Disk second").unwrap(),
            ),
        )
        .unwrap();
        inner_path
    };
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));
    {
        let mut seed_index = SessionNameIndex::empty();
        for (project_dir, path, name) in [
            (&encoded, &first, "Cached first"),
            (&encoded, &second, "Cached second"),
        ] {
            let stamp = FileStamp::read(path).unwrap();
            let project_key = normalize_path_str(&project_dir.to_string_lossy());
            let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
            seed_index.projects.entry(project_key).or_default().insert(
                file_name,
                SessionNameEntry {
                    name: (*name).to_string(),
                    observed_length: stamp.observed_length,
                    modified_secs: stamp.modified_secs,
                    modified_nanos: stamp.modified_nanos,
                    cached_at_ms: 1_000,
                },
            );
        }
        std::fs::write(&paths.data, serde_json::to_vec(&seed_index).unwrap()).unwrap();
    }

    let result = get_sessions_indexed_at(
        real.to_string_lossy().as_ref(),
        &[encoded],
        1,
        0,
        &store,
        2_000,
        None,
    )
    .unwrap();

    assert_eq!(result.value.len(), 1);
    assert!(result.value[0].name.starts_with("Cached"));
    assert_eq!(result.stats.exact_hits, 1);
    assert_eq!(result.stats.jsonl_bytes_read, 0);
    assert!(result.pending_flush.is_none());
}

// append 后必须走 full rebuild 并返回新增 custom-title，不能使用增量 cursor。
#[test]
fn SessionsIndex_AppendFullRebuild_023() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let encoded = root.path().join("encoded");
    std::fs::create_dir_all(&real).unwrap();
    let path = {
        std::fs::create_dir_all(&encoded).unwrap();
        let inner_path = encoded.join("session.jsonl");
        std::fs::write(
            &inner_path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real.to_string_lossy()).unwrap(),
                serde_json::to_string("Old").unwrap(),
            ),
        )
        .unwrap();
        inner_path
    };
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));
    {
        let mut seed_index = SessionNameIndex::empty();
        let (project_dir, path, name) = (&encoded, &path, "Old cached");
        let stamp = FileStamp::read(path).unwrap();
        let project_key = normalize_path_str(&project_dir.to_string_lossy());
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        seed_index.projects.entry(project_key).or_default().insert(
            file_name,
            SessionNameEntry {
                name: (*name).to_string(),
                observed_length: stamp.observed_length,
                modified_secs: stamp.modified_secs,
                modified_nanos: stamp.modified_nanos,
                cached_at_ms: 1_000,
            },
        );
        std::fs::write(&paths.data, serde_json::to_vec(&seed_index).unwrap()).unwrap();
    }
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"type\":\"custom-title\",\"customTitle\":\"New title\"}\n")
        .unwrap();

    let result = get_sessions_indexed_at(
        real.to_string_lossy().as_ref(),
        &[encoded],
        20,
        0,
        &store,
        2_000,
        None,
    )
    .unwrap();

    assert_eq!(result.value[0].name, "New title");
    assert_eq!(result.stats.full_rebuilds, 1);
    assert!(result.stats.jsonl_bytes_read > 0);
    assert!(result.pending_flush.is_some());
}

// 同一真实 cwd 的多个编码目录必须在一次 history 请求中同时返回且索引键隔离。
#[test]
fn SessionsIndex_MultiDir_024() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let old = root.path().join("encoded-old");
    let new = root.path().join("encoded-new");
    std::fs::create_dir_all(&real).unwrap();
    {
        std::fs::create_dir_all(&old).unwrap();
        let path = old.join("old.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real.to_string_lossy()).unwrap(),
                serde_json::to_string("Old encoding").unwrap(),
            ),
        )
        .unwrap();
    }
    {
        std::fs::create_dir_all(&new).unwrap();
        let path = new.join("new.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real.to_string_lossy()).unwrap(),
                serde_json::to_string("New encoding").unwrap(),
            ),
        )
        .unwrap();
    }
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));

    let result = get_sessions_indexed_at(
        real.to_string_lossy().as_ref(),
        &[old, new],
        20,
        0,
        &store,
        2_000,
        None,
    )
    .unwrap();

    assert_eq!(result.value.len(), 2);
    let pending = result.pending_flush.unwrap();
    assert_eq!(pending.delta.mutations.len(), 2);
    assert_ne!(
        pending.delta.mutations[0].project_key,
        pending.delta.mutations[1].project_key
    );
}

// all-recent 与 home 一样必须跨项目共享一个 resolver/索引快照。
#[test]
fn AllRecentIndex_OneResolver_025() {
    let root = tempfile::tempdir().unwrap();
    let projects_root = root.path().join("projects");
    for sequence in 0..2 {
        let real = root.path().join(format!("real-{sequence}"));
        std::fs::create_dir_all(&real).unwrap();
        {
            std::fs::create_dir_all(projects_root.join(format!("encoded-{sequence}"))).unwrap();
            let path = projects_root
                .join(format!("encoded-{sequence}"))
                .join(format!("session-{sequence}.jsonl"));
            std::fs::write(
                &path,
                format!(
                    "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                    serde_json::to_string(&real.to_string_lossy()).unwrap(),
                    serde_json::to_string(&format!("Session {sequence}")).unwrap(),
                ),
            )
            .unwrap();
        }
    }
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));

    let result =
        get_all_recent_sessions_indexed_at(&projects_root, 20, &store, 2_000, None).unwrap();

    assert_eq!(result.value.len(), 2);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

// metadata 枚举后、名称扫描前发生变化时可返回新名称，但不得产生持久 replacement。
#[test]
fn SessionsIndex_Unstable_NoFlush_026() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let encoded = root.path().join("encoded");
    std::fs::create_dir_all(&real).unwrap();
    let path = {
        std::fs::create_dir_all(&encoded).unwrap();
        let inner_path = encoded.join("session.jsonl");
        std::fs::write(
            &inner_path,
            format!(
                "{{\"cwd\":{}}}\n{{\"type\":\"user\",\"message\":{{\"content\":{}}}}}\n",
                serde_json::to_string(&real.to_string_lossy()).unwrap(),
                serde_json::to_string("Before").unwrap(),
            ),
        )
        .unwrap();
        inner_path
    };
    let mutate_path = path.clone();
    let before_resolve = Arc::new(move || {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&mutate_path)
            .unwrap()
            .write_all(b"{\"type\":\"custom-title\",\"customTitle\":\"After\"}\n")
            .unwrap();
    });
    let paths = SessionNameIndexPaths {
        data: root.path().join("session-name-index.json"),
        lock: root.path().join("session-name-index.json.lock"),
    };
    let reads = Arc::new(AtomicU64::new(0));
    let health = Arc::new(IndexHealth::new(|| 1_000, |_| {}));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        std::time::Duration::from_millis(100),
    )
    .with_snapshot_read_counter(Arc::clone(&reads));

    let result = get_sessions_indexed_at(
        real.to_string_lossy().as_ref(),
        &[encoded],
        20,
        0,
        &store,
        2_000,
        Some(before_resolve),
    )
    .unwrap();

    assert_eq!(result.value[0].name, "After");
    assert_eq!(result.stats.full_rebuilds, 1);
    assert!(result.pending_flush.is_none());
}

// 扫描发布快照后，排队的失效请求仍能正确清空缓存（不快照不覆盖失效）。
#[test]
fn ProjectPathMapping_QueueWinsRace_001() {
    use std::sync::{mpsc, Mutex};
    use std::thread;

    static TEST_LOCK: Mutex<()> = Mutex::new(());
    let _test_guard = TEST_LOCK.lock().unwrap();
    with_project_path_mapping(|cache| *cache = None);

    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let publisher = thread::spawn(move || {
        with_project_path_mapping(|cache| {
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            *cache = Some(ProjectPathMapping::from([(
                "C:/project".to_string(),
                vec![std::path::PathBuf::from("encoded-project")],
            )]));
        });
    });

    locked_rx.recv().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let invalidator = thread::spawn(move || {
        started_tx.send(()).unwrap();
        invalidate_project_path_mapping();
    });
    started_rx.recv().unwrap();
    release_tx.send(()).unwrap();
    publisher.join().unwrap();
    invalidator.join().unwrap();

    assert!(with_project_path_mapping(|cache| cache.is_none()));
}

// 多条用户消息时返回第一条有效消息，而非最后一条。
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

// custom-title 优先级高于用户消息。
#[test]
fn ExtractSessionName_CustomTitlePriority_002() {
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

// 文件后部的 custom-title 覆盖前部首条用户消息（后部优先语义）。
#[test]
fn ExtractSessionName_LateTitleWins_003() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session.jsonl");
    std::fs::write(
        &file,
        concat!(
            "{\"type\":\"user\",\"message\":{\"content\":\"First prompt\"},\"isMeta\":false}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":\"reply\"}}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"Late title\"}\n"
        ),
    )
    .unwrap();

    assert_eq!(extract_session_name(&file), "Late title");
}

// isMeta=true 的消息被过滤，不作为名称。
#[test]
fn ExtractSessionName_SkipMeta_004() {
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

// 以 < 开头的系统注入消息被过滤。
#[test]
fn ExtractSessionName_SkipSystemInject_005() {
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

// 超过 50 字符的消息被截断并加省略号。
#[test]
fn ExtractSessionName_TruncateLong_006() {
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

// 无用户消息也无 custom-title 时返回 "Unnamed session"。
#[test]
fn ExtractSessionName_NoMessages_007() {
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
        let n = normalize_path_str("E:/A");
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

// ==================== validate_session_id_component ====================

// 检查空 sessionId 被拒
#[test]
fn SessionId_RejectsEmpty_001() {
    assert!(validate_session_id_component("").is_err());
}

// 检查含路径分隔符、冒号、空字符的 sessionId 被拒
#[test]
fn SessionId_RejectsSep_002() {
    assert!(validate_session_id_component("a/b").is_err());
    assert!(validate_session_id_component("a\\b").is_err());
    assert!(validate_session_id_component("a:b").is_err()); // Windows 冒号/ADS
    assert!(validate_session_id_component("a\0b").is_err());
}

// 检查 "." 与 ".." 被拒
#[test]
fn SessionId_RejectsDot_003() {
    assert!(validate_session_id_component(".").is_err());
    assert!(validate_session_id_component("..").is_err());
}

// 检查 Windows 保留设备名(含扩展名)被拒
#[test]
fn SessionId_RejectsReserved_004() {
    for name in [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM9", "LPT1", "com1", "con",
    ] {
        assert!(
            validate_session_id_component(name).is_err(),
            "{} should be rejected",
            name
        );
    }
    assert!(validate_session_id_component("CON.txt").is_err()); // 保留名 + 扩展
}

// 检查普通合法 sessionId 被接受
#[test]
fn SessionId_AcceptsNormal_005() {
    assert!(validate_session_id_component("abc123").is_ok());
    assert!(validate_session_id_component("a-b_c.2026").is_ok());
}

// 检查 agent- 前缀的 sessionId 被拒
#[test]
fn SessionId_RejectsAgent_006() {
    assert!(validate_session_id_component("agent-a1b2").is_err());
    assert!(validate_session_id_component("agent-").is_err());
}

// ==================== build_project_path_mapping_at / lookup_project_dirs ====================

// 等价 key 全集合并:两个 cwd 大小写/斜杠规范化后相等,一次查找两目录都命中。
// 用 normalize_path_inner(_, false) 构造确定等价的 key:normalize_path_str 在 Linux
// 大小写敏感,原始 "E:\Foo" 与 "e:/foo" 在 Linux 不等价;先强制不敏感语义归一,
// 保证断言在任何宿主平台(含 Linux CI)成立。
#[test]
fn LookupDirs_MergesEquiv_004() {
    let root = tempfile::tempdir().unwrap();
    let upper_key = normalize_path_inner("E:\\Foo", false);
    let lower_key = normalize_path_inner("e:/foo", false);
    assert_eq!(upper_key, lower_key, "两 key 必须在任意宿主平台等价");
    let d1 = root.path().join("a-1");
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::write(
        d1.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string(&upper_key).unwrap()
        ),
    )
    .unwrap();
    let d2 = root.path().join("a-2");
    std::fs::create_dir_all(&d2).unwrap();
    std::fs::write(
        d2.join("s2.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string(&lower_key).unwrap()
        ),
    )
    .unwrap();
    let mapping = build_project_path_mapping_at(root.path());
    assert_eq!(lookup_project_dirs(&mapping, &upper_key).len(), 2);
    assert_eq!(lookup_project_dirs(&mapping, &lower_key).len(), 2);
}

// 锁定语义(strict 版,删除路径用):projects_root 是文件而非目录时扫描报错,
// 必须整体传播为 Err(不折叠为空映射)
#[test]
fn MappingStrict_RootIsFile_001() {
    let root = tempfile::tempdir().unwrap();
    let not_a_dir = root.path().join("not-a-dir");
    std::fs::write(&not_a_dir, b"plain file").unwrap();
    assert!(build_project_path_mapping_strict_at(&not_a_dir).is_err());
}

// 容错映射:一个正常项目 + 一个含「s.jsonl 是目录(File::open 失败,真实 IO 错误)」的
// 损坏兄弟目录,映射仍保留正常项目。若 strict 泄漏回生产(任一目录 IO 错整体空映射),
// 此断言红——真正锁死「单文件读取失败不能清空正常映射」。
#[test]
fn MappingAt_BrokenSibling_KeepsGood_001() {
    let root = tempfile::tempdir().unwrap();
    let good = root.path().join("good");
    std::fs::create_dir_all(&good).unwrap();
    std::fs::write(good.join("s.jsonl"), "{\"cwd\":\"/real/proj\"}\n").unwrap();
    // 损坏兄弟:把 s.jsonl 做成子目录,File::open 对目录在 Windows/Linux 均失败
    let broken = root.path().join("broken");
    std::fs::create_dir_all(broken.join("s.jsonl")).unwrap();
    let mapping = build_project_path_mapping_at(root.path());
    assert_eq!(mapping.get("/real/proj").map(|d| d.len()), Some(1));
}

// 检查精确原始路径命中,规范化 key 兜底也能命中
#[test]
fn LookupDirs_ExactAndNorm_001() {
    let root = tempfile::tempdir().unwrap();
    let dir_a = root.path().join("proj-a");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::write(
        dir_a.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let mapping = build_project_path_mapping_at(root.path());
    assert_eq!(lookup_project_dirs(&mapping, "E:\\Foo").len(), 1);
    let norm = normalize_path_str("E:\\Foo");
    assert_eq!(lookup_project_dirs(&mapping, &norm).len(), 1);
}

// 检查同一项目映射到多个编码目录时,一次查找全部命中
#[test]
fn LookupDirs_MultiDir_002() {
    let root = tempfile::tempdir().unwrap();
    let d1 = root.path().join("a-1");
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::write(
        d1.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let d2 = root.path().join("a-2");
    std::fs::create_dir_all(&d2).unwrap();
    std::fs::write(
        d2.join("s2.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let mapping = build_project_path_mapping_at(root.path());
    assert_eq!(lookup_project_dirs(&mapping, "E:\\Foo").len(), 2);
}

// 检查不存在的项目路径查找返回空
#[test]
fn LookupDirs_MissingEmpty_003() {
    let root = tempfile::tempdir().unwrap();
    let d = root.path().join("a");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let mapping = build_project_path_mapping_at(root.path());
    assert!(lookup_project_dirs(&mapping, "E:\\Bar").is_empty());
}

// ==================== delete_sessions_inner ====================

// 检查多目录、多扩展名的会话全部删除且全局标记清空
#[test]
fn Delete_MultiDirBothExt_001() {
    let dir = tempfile::tempdir().unwrap();
    let a1 = dir.path().join("a1");
    std::fs::create_dir_all(&a1).unwrap();
    std::fs::write(
        a1.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    std::fs::write(
        a1.join("s2.txt"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let a2 = dir.path().join("a2");
    std::fs::create_dir_all(&a2).unwrap();
    std::fs::write(
        a2.join("s1.txt"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1", "s2"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");

    let state = delete_sessions_inner(
        &data,
        &lock,
        dir.path(),
        "E:\\Foo",
        &["s1".into(), "s2".into()],
    )
    .unwrap();
    assert!(state.archived_sessions.is_empty());
    assert!(!dir.path().join("a1/s1.jsonl").exists());
    assert!(!dir.path().join("a1/s2.txt").exists());
    assert!(!dir.path().join("a2/s1.txt").exists());
}

// 检查不存在的会话文件容错跳过,仍清除标记
#[test]
fn Delete_MissingFile_002() {
    let dir = tempfile::tempdir().unwrap();
    let a1 = dir.path().join("a1");
    std::fs::create_dir_all(&a1).unwrap();
    std::fs::write(
        a1.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1", "ghost"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");
    // ghost 文件不存在 -> 容错跳过,仍清两标记
    let state = delete_sessions_inner(
        &data,
        &lock,
        dir.path(),
        "E:\\Foo",
        &["s1".into(), "ghost".into()],
    )
    .unwrap();
    assert!(state.archived_sessions.is_empty());
    assert!(!dir.path().join("a1/s1.jsonl").exists());
}

// 检查未存档的会话被删除请求整体拒绝,状态与文件不变
#[test]
fn Delete_NotArchived_003() {
    let dir = tempfile::tempdir().unwrap();
    let a1 = dir.path().join("a1");
    std::fs::create_dir_all(&a1).unwrap();
    std::fs::write(
        a1.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");
    // s2 未存档 -> 整体 Err,projects.json 不变,s1 文件不动
    let before = get_projects_state_at(&data).unwrap();
    assert!(delete_sessions_inner(&data, &lock, dir.path(), "E:\\Foo", &["s2".into()]).is_err());
    let after = get_projects_state_at(&data).unwrap();
    assert_eq!(before.archived_sessions, after.archived_sessions);
    assert!(dir.path().join("a1/s1.jsonl").exists());
}

// 检查含非法路径字符的 sessionId 被拒
#[test]
fn Delete_InvalidId_004() {
    let dir = tempfile::tempdir().unwrap();
    let a1 = dir.path().join("a1");
    std::fs::create_dir_all(&a1).unwrap();
    std::fs::write(
        a1.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");
    for bad in ["../evil", "a/b", "a\\b", "C:evil", "CON"] {
        assert!(
            delete_sessions_inner(&data, &lock, dir.path(), "E:\\Foo", &[bad.into()]).is_err(),
            "{}",
            bad
        );
    }
}

// 检查项目目录已不存在时,无文件可删,清标记收敛
#[test]
fn Delete_ProjectGone_005() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");
    let state = delete_sessions_inner(&data, &lock, dir.path(), "E:\\Foo", &["s1".into()]).unwrap();
    assert!(state.archived_sessions.is_empty());
}

// 检查 root 是文件(扫描报错)时整体 Err,不误清标记
#[test]
fn Delete_RootScanErr_008() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");
    // root 路径是个文件(非目录)→ read_dir 失败 → Err 不动状态,不误清标记
    let root_file = dir.path().join("not-a-dir");
    std::fs::write(&root_file, b"x").unwrap();
    assert!(delete_sessions_inner(&data, &lock, &root_file, "E:\\Foo", &["s1".into()]).is_err());
    let after = get_projects_state_at(&data).unwrap();
    assert_eq!(after.archived_sessions.len(), 1);
}

// 检查 root 整个不存在时清标记收敛,不 canonicalize root
#[test]
fn Delete_RootMissing_009() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");
    // root 整个不存在(全新机器/目录被移走)→ 无处可删,清标记收敛;
    // 此时不得 canonicalize root(会失败),命中 dirs.is_empty() 短路
    let missing_root = dir.path().join("no-such-root");
    let state =
        delete_sessions_inner(&data, &lock, &missing_root, "E:\\Foo", &["s1".into()]).unwrap();
    assert!(state.archived_sessions.is_empty());
}

// 检查传规范化 key 也能命中目录并删除会话
#[test]
fn Delete_NormPath_006() {
    let dir = tempfile::tempdir().unwrap();
    let a1 = dir.path().join("a1");
    std::fs::create_dir_all(&a1).unwrap();
    std::fs::write(
        a1.join("s1.jsonl"),
        format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string("E:\\Foo").unwrap()
        ),
    )
    .unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");
    // 传规范化 key e:/foo 也能命中目录并删除
    let state = delete_sessions_inner(
        &data,
        &lock,
        dir.path(),
        &normalize_path_str("E:\\Foo"),
        &["s1".into()],
    )
    .unwrap();
    assert!(state.archived_sessions.is_empty());
    assert!(!dir.path().join("a1/s1.jsonl").exists());
}

// 检查空删除列表返回原状态,标记不清
#[test]
fn Delete_EmptyList_007() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("projects.json");
    let mut archived = serde_json::Map::new();
    archived.insert(normalize_path_str("E:\\Foo"), json!(["s1"]));
    std::fs::write(&data, json!({ "archivedSessions": archived }).to_string()).unwrap();
    let lock = dir.path().join("projects.json.lock");
    let state = delete_sessions_inner(&data, &lock, dir.path(), "E:\\Foo", &[]).unwrap();
    assert_eq!(state.archived_sessions.len(), 1);
}
