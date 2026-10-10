use super::super::scoped_fs::Limits;
use super::*;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
fn put(root: &Path, file: &str, content: &str) {
    let p = root.join(file);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}
fn project(root: &Path) {
    fs::create_dir_all(root).unwrap();
}
fn list(
    root: &Path,
    cli: CliKind,
    selected: Option<&Path>,
    kind: ResourceKind,
    query: Option<&str>,
    id: Option<&str>,
) -> ReadResult<Value> {
    let r = Root::open(root)?;
    let p = selected.map(Root::open).transpose()?;
    let paths = selected
        .map(Path::to_path_buf)
        .into_iter()
        .collect::<Vec<_>>();
    let items = read(
        &Catalog {
            cli,
            root: &r,
            project: p.as_ref(),
            project_paths: &paths,
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind,
            query,
            session_id: id,
        },
        &mut Budget::new(Limits::default()),
    )?;
    Ok(serde_json::to_value(items).unwrap())
}
fn claude(root: &Path, title: &str, cwd: &Path) {
    put(
        root,
        "projects/encoded/same-id.jsonl",
        &format!(
            "{}\n{}\n{}\n",
            json!({"cwd":cwd.to_str().unwrap(),"type":"user","message":{"role":"user","content":"needle from Claude"},"timestamp":"2026-09-23T01:00:00Z"}),
            json!({"type":"custom-title","customTitle":title}),
            json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"answer"}]}})
        ),
    );
}
fn codex(root: &Path, cwd: &Path) {
    put(
        root,
        "sessions/2026/09/23/rollout-fixture.jsonl",
        &format!(
            "{}\n{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":"same-id","cwd":cwd.to_str().unwrap()}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"needle from Codex"},"timestamp":"2026-09-23T02:00:00Z"}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Codex answer"}]}})
        ),
    );
}
#[test]
fn real_history_same_id_never_crosses_native_roots() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    let cwd = t.path().join("work");
    project(&cwd);
    claude(&a, "LEFT", &cwd);
    claude(&b, "RIGHT", &cwd);
    let left = list(
        &a,
        CliKind::Claude,
        Some(&cwd),
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    let right = list(
        &b,
        CliKind::Claude,
        Some(&cwd),
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(left[0]["title"], "LEFT");
    assert_eq!(right[0]["title"], "RIGHT");
    assert_ne!(left[0]["sessionKey"], right[0]["sessionKey"]);
    assert_eq!(left[0]["nativeSessionId"], right[0]["nativeSessionId"]);
}
#[test]
fn codex_rollout_is_not_parsed_as_claude_history() {
    let t = tempfile::tempdir().unwrap();
    codex(t.path(), t.path());
    claude(t.path(), "must-not-appear", t.path());
    let items = list(
        t.path(),
        CliKind::Codex,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["title"], "needle from Codex");
}
// 官方文档中的 orphaned 旁文件不出现在主会话列表。
// https://code.claude.com/docs/en/claude-directory#cleaned-up-automatically (2026-10-10)
#[test]
#[allow(non_snake_case)]
fn HistoryOrphan_KeepMainSession_001() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main", t.path());
    put(
        t.path(),
        "projects/encoded/same-id.orphaned-20261010-replaced.jsonl",
        "{\"type\":\"custom-title\",\"customTitle\":\"orphan\"}\n",
    );
    let items = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["nativeSessionId"], "same-id");
}
// orphaned 标记缺少 timestamp-suffix 结构时不静默排除。
#[test]
#[allow(non_snake_case)]
fn HistoryOrphan_KeepNearMatch_002() {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        "projects/encoded/same-id.orphaned-y.jsonl",
        "{\"type\":\"custom-title\",\"customTitle\":\"valid session\"}\n",
    );
    let items = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["title"], "valid session");
}
// orphaned 文件只在 Claude 项目子层排除，不能在会话子层套用该规则。
#[test]
#[allow(non_snake_case)]
fn HistoryOrphan_KeepOtherDepth_005() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main", t.path());
    put(
        t.path(),
        "projects/encoded/same-id/other.orphaned-20261010-old.jsonl",
        "{\"type\":\"custom-title\",\"customTitle\":\"other depth\"}\n",
    );
    let items = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 2);
}
// orphaned 名称的符号链接不能因文件名规则被隐藏。
#[cfg(unix)]
#[test]
#[allow(non_snake_case)]
fn HistoryOrphan_RejectSymlink_003() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main", t.path());
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("history.jsonl"), "{}").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("history.jsonl"),
        t.path()
            .join("projects/encoded/same-id.orphaned-20261010-old.jsonl"),
    )
    .unwrap();
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::History,
            None,
            None
        )
        .err(),
        Some("SOURCE_NOT_REGULAR")
    );
}
// 单文件略过不能绕过整个历史读取的总字节硬上限。
#[test]
#[allow(non_snake_case)]
fn HistoryPartial_KeepAggregateCap_004() {
    let t = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] {
        put(
            t.path(),
            &format!("projects/p/{name}.jsonl"),
            &"x".repeat(768),
        );
    }
    let root = Root::open(t.path()).unwrap();
    let mut budget = Budget::new(Limits {
        file_bytes: 512,
        total_bytes: 1024,
        entries: 4096,
    });
    let result = read(
        &Catalog {
            cli: CliKind::Claude,
            root: &root,
            project: None,
            project_paths: &[],
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut budget,
    );
    assert_eq!(result.err(), Some("SOURCE_TOO_LARGE"));
}
// 无法枚举的目录不能略过，以免隐藏尚未检查的兄弟链接和有效会话。
#[cfg(unix)]
#[test]
#[allow(non_snake_case)]
fn HistoryPartial_RejectBadFilename_006() {
    use std::os::unix::ffi::OsStringExt;
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "known", t.path());
    fs::write(
        t.path()
            .join("projects/encoded")
            .join(std::ffi::OsString::from_vec(vec![0xff])),
        "x",
    )
    .unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), t.path().join("projects/encoded/linked")).unwrap();
    put(
        t.path(),
        "projects/other/other.jsonl",
        "{\"type\":\"custom-title\",\"customTitle\":\"other\"}\n",
    );
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::History,
            None,
            None
        )
        .err(),
        Some("SOURCE_INVALID_TEXT")
    );
}
#[test]
fn message_search_and_details_use_the_same_confined_reader() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "title", t.path());
    let search = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::Search,
        Some("needle"),
        None,
    )
    .unwrap();
    assert_eq!(search.as_array().unwrap().len(), 1);
    assert_eq!(search[0]["text"], "needle from Claude");
    let details = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::Messages,
        None,
        Some("same-id"),
    )
    .unwrap();
    assert_eq!(details.as_array().unwrap().len(), 2);
    assert!(list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::Messages,
        None,
        Some("other")
    )
    .unwrap()
    .as_array()
    .unwrap()
    .is_empty());
}
#[test]
fn project_filter_does_not_authorize_transcript_cwd() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    project(&a);
    project(&b);
    claude(t.path(), "belongs-to-a", &a);
    assert!(list(
        t.path(),
        CliKind::Claude,
        Some(&b),
        ResourceKind::History,
        None,
        None
    )
    .unwrap()
    .as_array()
    .unwrap()
    .is_empty());
}
#[test]
fn settings_and_mcp_never_return_environment_headers_or_commands() {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        "settings.json",
        r#"{"model":"claude-test","env":{"TOKEN":"TOP-SECRET"},"hooks":{"x":"TOP-SECRET"},"mcpServers":{"local":{"command":"TOP-SECRET","args":["TOP-SECRET"],"env":{"X":"TOP-SECRET"}}}}"#,
    );
    let settings = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::Config,
        None,
        None,
    )
    .unwrap();
    assert_eq!(settings[0]["value"], "claude-test");
    assert!(!settings.to_string().contains("TOP-SECRET"));
    let mcp = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::Mcp,
        None,
        None,
    )
    .unwrap();
    assert_eq!(mcp[0]["name"], "local");
    assert_eq!(mcp[0]["transport"], "stdio");
    assert!(!mcp.to_string().contains("TOP-SECRET"));
}
#[test]
fn codex_toml_metadata_is_separate_and_secret_free() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "config.toml", "model = 'codex-test'\n[mcp_servers.docs]\nurl = 'https://TOP-SECRET.invalid/'\nbearer_token_env_var = 'TOP-SECRET'\n[agents.reviewer]\ndescription = 'Review changes'\n[plugins.audit]\nenabled = true\n");
    assert_eq!(
        list(
            t.path(),
            CliKind::Codex,
            None,
            ResourceKind::Config,
            None,
            None
        )
        .unwrap()[0]["value"],
        "codex-test"
    );
    let mcp = list(
        t.path(),
        CliKind::Codex,
        None,
        ResourceKind::Mcp,
        None,
        None,
    )
    .unwrap();
    assert_eq!(mcp[0]["name"], "docs");
    assert!(!mcp.to_string().contains("TOP-SECRET"));
    assert_eq!(
        list(
            t.path(),
            CliKind::Codex,
            None,
            ResourceKind::Agents,
            None,
            None
        )
        .unwrap()[0]["name"],
        "reviewer"
    );
    let plugins = list(
        t.path(),
        CliKind::Codex,
        None,
        ResourceKind::Plugins,
        None,
        None,
    )
    .unwrap();
    assert_eq!(plugins[0]["id"], "audit");
    assert_eq!(plugins[0]["enabled"], true);
    assert!(plugins[0]["installed"].is_null());
}
#[test]
fn global_and_registered_project_resources_are_both_explicit() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("native");
    let cwd = t.path().join("project");
    project(&cwd);
    put(
        &root,
        "skills/global/SKILL.md",
        "---\nname: global\ndescription: global skill\n---\nbody",
    );
    put(
        &cwd,
        ".claude/skills/local/SKILL.md",
        "---\nname: local\ndescription: project skill\n---\nbody",
    );
    put(
        &root,
        "agents/check.md",
        "---\nname: check\ndescription: Check code\nmodel: test\n---\nbody",
    );
    let skills = list(
        &root,
        CliKind::Claude,
        Some(&cwd),
        ResourceKind::Skills,
        None,
        None,
    )
    .unwrap();
    assert_eq!(skills.as_array().unwrap().len(), 2);
    assert_eq!(skills[0]["origin"], "global");
    assert_eq!(skills[1]["origin"], "project");
    assert_eq!(
        list(
            &root,
            CliKind::Claude,
            Some(&cwd),
            ResourceKind::Agents,
            None,
            None
        )
        .unwrap()[0]["model"],
        "test"
    );
}
#[test]
fn plugin_registry_is_observed_without_spawning_a_cli() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "plugins/installed_plugins.json", &json!({"version":2,"plugins":{"audit@local":[{"scope":"user","installPath":t.path().join("plugins/cache/audit").to_str().unwrap(),"version":"1.0"}]}}).to_string());
    put(
        t.path(),
        "settings.json",
        r#"{"enabledPlugins":{"audit@local":false}}"#,
    );
    let plugins = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::Plugins,
        None,
        None,
    )
    .unwrap();
    assert_eq!(plugins[0]["id"], "audit@local");
    assert_eq!(plugins[0]["installed"], true);
    assert_eq!(plugins[0]["enabled"], false);
}
#[test]
fn corrupt_or_unknown_native_schema_never_becomes_empty_success() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "settings.json", "{broken");
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::Config,
            None,
            None
        )
        .err(),
        Some("SOURCE_INVALID")
    );
    put(
        t.path(),
        "plugins/installed_plugins.json",
        r#"{"version":99,"plugins":{}}"#,
    );
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::Plugins,
            None,
            None
        )
        .err(),
        Some("SOURCE_UNSUPPORTED")
    );
}
#[test]
fn instructions_are_documents_not_fabricated_agents() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "AGENTS.md", "project instructions");
    let docs = list(
        t.path(),
        CliKind::Codex,
        None,
        ResourceKind::Instructions,
        None,
        None,
    )
    .unwrap();
    assert_eq!(docs[0]["type"], "document");
    assert_eq!(docs[0]["name"], "AGENTS.md");
}
#[test]
fn revocation_precedes_native_reads() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "settings.json", "{bad");
    let root = Root::open(t.path()).unwrap();
    let result = read(
        &Catalog {
            cli: CliKind::Claude,
            root: &root,
            project: None,
            project_paths: &[],
            user_config: None,
            check: &|| Err("SCOPE_REVOKED"),
        },
        &Options {
            kind: ResourceKind::Config,
            query: None,
            session_id: None,
        },
        &mut Budget::new(Limits::default()),
    );
    assert_eq!(result.err(), Some("SCOPE_REVOKED"));
}

#[test]
fn escaped_nul_and_mixed_codex_streams_do_not_become_plausible_partial_data() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "settings.json", r#"{"model":"bad\u0000model"}"#);
    assert!(list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::Config,
        None,
        None
    )
    .is_err());
    codex(t.path(), t.path());
    let p = t.path().join("sessions/2026/09/23/rollout-fixture.jsonl");
    let mut content = fs::read_to_string(&p).unwrap();
    content.push_str("{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"different unpaired turn\"}}\n");
    fs::write(p, content).unwrap();
    assert!(list(
        t.path(),
        CliKind::Codex,
        None,
        ResourceKind::Messages,
        None,
        Some("same-id")
    )
    .is_err());
}
#[test]
fn resource_documents_are_guarded_after_read_not_just_at_admission() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "CLAUDE.md", "text");
    let r = Root::open(t.path()).unwrap();
    let calls = AtomicUsize::new(0);
    let check = || {
        if calls.fetch_add(1, Ordering::SeqCst) < 2 {
            Ok(())
        } else {
            Err("SCOPE_REVOKED")
        }
    };
    assert!(read(
        &Catalog {
            cli: CliKind::Claude,
            root: &r,
            project: None,
            project_paths: &[],
            user_config: None,
            check: &check
        },
        &Options {
            kind: ResourceKind::Instructions,
            query: None,
            session_id: None
        },
        &mut Budget::new(Limits::default())
    )
    .is_err());
}
#[test]
fn unsupported_depth_keeps_known_history() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "visible", t.path());
    put(
        t.path(),
        "projects/a/b/c/hidden.jsonl",
        "{\"type\":\"custom-title\",\"customTitle\":\"hidden\"}\n",
    );
    let items = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["nativeSessionId"], "same-id");
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::Messages,
            None,
            Some("same-id")
        )
        .err(),
        Some("SOURCE_UNSUPPORTED")
    );
}

#[test]
#[allow(non_snake_case)]
fn HistoryMetadata_ClaudeSubagentsAreNotMainSessions_001() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main session", t.path());
    put(
        t.path(),
        "projects/encoded/same-id/subagents/agent-child.jsonl",
        "{\"type\":\"user\",\"message\":{\"content\":\"child task\"}}\n",
    );
    let items = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["nativeSessionId"], "same-id");
    assert_eq!(items[0]["title"], "main session");
}

// 会话旁存在 tool-results 目录时，三个历史读取入口仍返回主会话内容。
#[test]
#[allow(non_snake_case)]
fn HistoryTools_KeepMainSession_001() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main session", t.path());
    fs::create_dir_all(t.path().join("projects/encoded/same-id/tool-results")).unwrap();
    for (kind, query, id, expected) in [
        (ResourceKind::History, None, None, 1),
        (ResourceKind::Messages, None, Some("same-id"), 2),
        (ResourceKind::Search, Some("needle"), None, 1),
    ] {
        let items = list(t.path(), CliKind::Claude, None, kind, query, id).unwrap();
        assert_eq!(items.as_array().unwrap().len(), expected);
        assert_eq!(items[0]["nativeSessionId"], "same-id");
    }
}

// tool-results 中损坏的 JSONL 和深层目录不会进入主会话扫描。
#[test]
#[allow(non_snake_case)]
fn HistoryTools_SkipStoredResults_002() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main session", t.path());
    put(
        t.path(),
        "projects/encoded/same-id/tool-results/broken.jsonl",
        "{broken",
    );
    put(
        t.path(),
        "projects/encoded/same-id/tool-results/nested/deeper/result.jsonl",
        "{broken",
    );
    let items = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["title"], "main session");
}

// 同层未知目录不删除正向历史；完整消息读取仍拒绝未知布局。
#[test]
#[allow(non_snake_case)]
fn HistoryTools_RejectUnknownSibling_003() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main session", t.path());
    fs::create_dir_all(t.path().join("projects/encoded/same-id/tool-results")).unwrap();
    fs::create_dir_all(t.path().join("projects/encoded/same-id/unknown-directory")).unwrap();
    let items = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["nativeSessionId"], "same-id");
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::Messages,
            None,
            Some("same-id")
        )
        .err(),
        Some("SOURCE_UNSUPPORTED")
    );
}

// Codex 未知深层目录明确使历史不完整；消息读取仍拒绝。
#[test]
#[allow(non_snake_case)]
fn HistoryTools_KeepCodexDepthGuard_004() {
    let t = tempfile::tempdir().unwrap();
    codex(t.path(), t.path());
    fs::create_dir_all(t.path().join("sessions/2026/09/23/session/tool-results")).unwrap();
    let items = list(
        t.path(),
        CliKind::Codex,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["nativeSessionId"], "same-id");
    assert_eq!(
        list(
            t.path(),
            CliKind::Codex,
            None,
            ResourceKind::Messages,
            None,
            Some("same-id")
        )
        .err(),
        Some("SOURCE_UNSUPPORTED")
    );
}

// 名为 tool-results 的符号链接仍被拒绝，不被当作可排除的真实目录。
#[cfg(unix)]
#[test]
#[allow(non_snake_case)]
fn HistoryTools_RejectSymlink_005() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main session", t.path());
    fs::create_dir_all(t.path().join("projects/encoded/same-id")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        outside.path(),
        t.path().join("projects/encoded/same-id/tool-results"),
    )
    .unwrap();
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::History,
            None,
            None
        )
        .err(),
        Some("SOURCE_NOT_REGULAR")
    );
}

// 项目自动记忆目录不是会话；其深层目录和 JSONL 不参与历史读取。
#[test]
#[allow(non_snake_case)]
fn HistoryMemory_KeepMainSession_001() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main session", t.path());
    put(t.path(), "projects/encoded/memory/notes.jsonl", "{broken");
    put(
        t.path(),
        "projects/encoded/memory/topics/nested/note.md",
        "memory",
    );
    for (kind, query, id, expected) in [
        (ResourceKind::History, None, None, 1),
        (ResourceKind::Messages, None, Some("same-id"), 2),
        (ResourceKind::Search, Some("needle"), None, 1),
    ] {
        let items = list(t.path(), CliKind::Claude, None, kind, query, id).unwrap();
        assert_eq!(items.as_array().unwrap().len(), expected);
        assert_eq!(items[0]["nativeSessionId"], "same-id");
    }
}

// 项目 memory 链接必须拒绝，不能因为名称已知而绕过文件身份检查。
#[cfg(unix)]
#[test]
#[allow(non_snake_case)]
fn HistoryMemory_RejectSymlink_002() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main session", t.path());
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), t.path().join("projects/encoded/memory")).unwrap();
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::History,
            None,
            None
        )
        .err(),
        Some("SOURCE_NOT_REGULAR")
    );
}

// memory 只在项目层排除；会话层同名目录仍是未支持的历史子树。
#[test]
#[allow(non_snake_case)]
fn HistoryMemory_KeepDepthGuard_003() {
    let t = tempfile::tempdir().unwrap();
    claude(t.path(), "main session", t.path());
    fs::create_dir_all(t.path().join("projects/encoded/same-id/memory")).unwrap();
    let items = list(
        t.path(),
        CliKind::Claude,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["nativeSessionId"], "same-id");
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::Messages,
            None,
            Some("same-id")
        )
        .err(),
        Some("SOURCE_UNSUPPORTED")
    );
}

#[test]
#[allow(non_snake_case)]
fn HistoryMetadata_LargeCodexTranscriptStillListsSession_002() {
    let t = tempfile::tempdir().unwrap();
    let body = format!(
        "{}\n{}\n{}\n",
        json!({"type":"session_meta","payload":{"id":"large-session","cwd":t.path().to_str().unwrap()}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"large task"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"x".repeat(2 * 1024 * 1024)}]}}),
    );
    put(t.path(), "sessions/2026/10/04/rollout-large.jsonl", &body);
    let items = list(
        t.path(),
        CliKind::Codex,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["nativeSessionId"], "large-session");
    assert_eq!(items[0]["title"], "large task");
    assert_eq!(items[0]["truncated"], true);
    assert!(items[0]["updatedAt"].is_null());
    assert_eq!(
        list(
            t.path(),
            CliKind::Codex,
            None,
            ResourceKind::Messages,
            None,
            Some("large-session")
        )
        .err(),
        Some("SOURCE_TOO_LARGE")
    );
}

#[test]
#[allow(non_snake_case)]
fn HistoryMetadata_LongHeaderAndCutRecordRemainBounded_003() {
    let t = tempfile::tempdir().unwrap();
    let header = json!({"type":"session_meta","payload":{"id":"long-header","cwd":t.path().to_str().unwrap(),"base_instructions":"x".repeat(70 * 1024)}});
    let body = format!(
        "{header}\n{}\n{}\n",
        json!({"type":"event_msg","payload":{"type":"user_message","message":"long header task"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":"中".repeat(50 * 1024)}})
    );
    put(t.path(), "sessions/2026/10/04/rollout-long.jsonl", &body);
    let items = list(
        t.path(),
        CliKind::Codex,
        None,
        ResourceKind::History,
        None,
        None,
    )
    .unwrap();
    assert_eq!(items[0]["nativeSessionId"], "long-header");
    assert_eq!(items[0]["title"], "long header task");
    assert_eq!(items[0]["truncated"], true);
    for invalid in [
        "{broken}\n",
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"different\"}}\n",
    ] {
        put(
            t.path(),
            "sessions/2026/10/04/rollout-long.jsonl",
            &format!("{header}\n{invalid}{}", "x".repeat(200 * 1024)),
        );
        assert!(list(
            t.path(),
            CliKind::Codex,
            None,
            ResourceKind::History,
            None,
            None
        )
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
        assert_eq!(
            list(
                t.path(),
                CliKind::Codex,
                None,
                ResourceKind::Messages,
                None,
                Some("long-header")
            )
            .err(),
            Some("SOURCE_INVALID")
        );
    }
}
#[test]
fn unknown_plugin_scope_never_becomes_global() {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        "plugins/installed_plugins.json",
        r#"{"version":2,"plugins":{"future@market":[{"scope":"future","installPath":"ignored"}]}}"#,
    );
    assert_eq!(
        list(
            t.path(),
            CliKind::Claude,
            None,
            ResourceKind::Plugins,
            None,
            None
        )
        .err(),
        Some("SOURCE_UNSUPPORTED")
    );
}
