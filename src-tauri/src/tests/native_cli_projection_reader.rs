use super::*;
use super::super::scoped_fs::Limits;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
fn put(root: &Path, file: &str, content: &str) {
    let p = root.join(file); fs::create_dir_all(p.parent().unwrap()).unwrap(); fs::write(p, content).unwrap();
}
fn project(root: &Path) { fs::create_dir_all(root).unwrap(); }
fn list(root: &Path, cli: CliKind, selected: Option<&Path>, kind: ResourceKind, query: Option<&str>, id: Option<&str>) -> ReadResult<Value> {
    let r = Root::open(root)?;
    let p = selected.map(Root::open).transpose()?;
    let paths = selected.map(Path::to_path_buf).into_iter().collect::<Vec<_>>();
    let items = read(&Catalog { cli, root: &r, project: p.as_ref(), project_paths: &paths, check: &|| Ok(()) }, &Options { kind, query, session_id: id }, &mut Budget::new(Limits::default()))?;
    Ok(serde_json::to_value(items).unwrap())
}
fn claude(root: &Path, title: &str, cwd: &Path) {
    put(root, "projects/encoded/same-id.jsonl", &format!("{}\n{}\n{}\n",
        json!({"cwd":cwd.to_str().unwrap(),"type":"user","message":{"role":"user","content":"needle from Claude"},"timestamp":"2026-09-23T01:00:00Z"}),
        json!({"type":"custom-title","customTitle":title}),
        json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"answer"}]}})));
}
fn codex(root: &Path, cwd: &Path) {
    put(root, "sessions/2026/09/23/rollout-fixture.jsonl", &format!("{}\n{}\n{}\n",
        json!({"type":"session_meta","payload":{"id":"same-id","cwd":cwd.to_str().unwrap()}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"needle from Codex"},"timestamp":"2026-09-23T02:00:00Z"}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Codex answer"}]}})));
}
#[test]
fn real_history_same_id_never_crosses_native_roots() {
    let t = tempfile::tempdir().unwrap(); let a = t.path().join("a"); let b = t.path().join("b"); let cwd = t.path().join("work"); project(&cwd);
    claude(&a, "LEFT", &cwd); claude(&b, "RIGHT", &cwd);
    let left = list(&a, CliKind::Claude, Some(&cwd), ResourceKind::History, None, None).unwrap();
    let right = list(&b, CliKind::Claude, Some(&cwd), ResourceKind::History, None, None).unwrap();
    assert_eq!(left[0]["title"], "LEFT"); assert_eq!(right[0]["title"], "RIGHT");
    assert_ne!(left[0]["sessionKey"], right[0]["sessionKey"]);
    assert_eq!(left[0]["nativeSessionId"], right[0]["nativeSessionId"]);
}
#[test]
fn codex_rollout_is_not_parsed_as_claude_history() {
    let t = tempfile::tempdir().unwrap(); codex(t.path(), t.path());
    claude(t.path(), "must-not-appear", t.path());
    let items = list(t.path(), CliKind::Codex, None, ResourceKind::History, None, None).unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1); assert_eq!(items[0]["title"], "needle from Codex");
}
#[test]
fn message_search_and_details_use_the_same_confined_reader() {
    let t = tempfile::tempdir().unwrap(); claude(t.path(), "title", t.path());
    let search = list(t.path(), CliKind::Claude, None, ResourceKind::Search, Some("needle"), None).unwrap();
    assert_eq!(search.as_array().unwrap().len(), 1); assert_eq!(search[0]["text"], "needle from Claude");
    let details = list(t.path(), CliKind::Claude, None, ResourceKind::Messages, None, Some("same-id")).unwrap();
    assert_eq!(details.as_array().unwrap().len(), 2);
    assert!(list(t.path(), CliKind::Claude, None, ResourceKind::Messages, None, Some("other")).unwrap().as_array().unwrap().is_empty());
}
#[test]
fn project_filter_does_not_authorize_transcript_cwd() {
    let t = tempfile::tempdir().unwrap(); let a = t.path().join("a"); let b = t.path().join("b"); project(&a); project(&b);
    claude(t.path(), "belongs-to-a", &a);
    assert!(list(t.path(), CliKind::Claude, Some(&b), ResourceKind::History, None, None).unwrap().as_array().unwrap().is_empty());
}
#[test]
fn settings_and_mcp_never_return_environment_headers_or_commands() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "settings.json", r#"{"model":"claude-test","env":{"TOKEN":"TOP-SECRET"},"hooks":{"x":"TOP-SECRET"},"mcpServers":{"local":{"command":"TOP-SECRET","args":["TOP-SECRET"],"env":{"X":"TOP-SECRET"}}}}"#);
    let settings = list(t.path(), CliKind::Claude, None, ResourceKind::Config, None, None).unwrap();
    assert_eq!(settings[0]["value"], "claude-test"); assert!(!settings.to_string().contains("TOP-SECRET"));
    let mcp = list(t.path(), CliKind::Claude, None, ResourceKind::Mcp, None, None).unwrap();
    assert_eq!(mcp[0]["name"], "local"); assert_eq!(mcp[0]["transport"], "stdio"); assert!(!mcp.to_string().contains("TOP-SECRET"));
}
#[test]
fn codex_toml_metadata_is_separate_and_secret_free() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "config.toml", "model = 'codex-test'\n[mcp_servers.docs]\nurl = 'https://TOP-SECRET.invalid/'\nbearer_token_env_var = 'TOP-SECRET'\n[agents.reviewer]\ndescription = 'Review changes'\n[plugins.audit]\nenabled = true\n");
    assert_eq!(list(t.path(), CliKind::Codex, None, ResourceKind::Config, None, None).unwrap()[0]["value"], "codex-test");
    let mcp = list(t.path(), CliKind::Codex, None, ResourceKind::Mcp, None, None).unwrap();
    assert_eq!(mcp[0]["name"], "docs"); assert!(!mcp.to_string().contains("TOP-SECRET"));
    assert_eq!(list(t.path(), CliKind::Codex, None, ResourceKind::Agents, None, None).unwrap()[0]["name"], "reviewer");
    let plugins = list(t.path(), CliKind::Codex, None, ResourceKind::Plugins, None, None).unwrap();
    assert_eq!(plugins[0]["id"], "audit"); assert_eq!(plugins[0]["enabled"], true); assert!(plugins[0]["installed"].is_null());
}
#[test]
fn global_and_registered_project_resources_are_both_explicit() {
    let t = tempfile::tempdir().unwrap(); let root = t.path().join("native"); let cwd = t.path().join("project"); project(&cwd);
    put(&root, "skills/global/SKILL.md", "---\nname: global\ndescription: global skill\n---\nbody");
    put(&cwd, ".claude/skills/local/SKILL.md", "---\nname: local\ndescription: project skill\n---\nbody");
    put(&root, "agents/check.md", "---\nname: check\ndescription: Check code\nmodel: test\n---\nbody");
    let skills = list(&root, CliKind::Claude, Some(&cwd), ResourceKind::Skills, None, None).unwrap();
    assert_eq!(skills.as_array().unwrap().len(), 2);
    assert_eq!(skills[0]["origin"], "global"); assert_eq!(skills[1]["origin"], "project");
    assert_eq!(list(&root, CliKind::Claude, Some(&cwd), ResourceKind::Agents, None, None).unwrap()[0]["model"], "test");
}
#[test]
fn plugin_registry_is_observed_without_spawning_a_cli() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "plugins/installed_plugins.json", &json!({"version":2,"plugins":{"audit@local":[{"scope":"user","installPath":t.path().join("plugins/cache/audit").to_str().unwrap(),"version":"1.0"}]}}).to_string());
    put(t.path(), "settings.json", r#"{"enabledPlugins":{"audit@local":false}}"#);
    let plugins = list(t.path(), CliKind::Claude, None, ResourceKind::Plugins, None, None).unwrap();
    assert_eq!(plugins[0]["id"], "audit@local"); assert_eq!(plugins[0]["installed"], true); assert_eq!(plugins[0]["enabled"], false);
}
#[test]
fn corrupt_or_unknown_native_schema_never_becomes_empty_success() {
    let t = tempfile::tempdir().unwrap(); put(t.path(), "settings.json", "{broken");
    assert_eq!(list(t.path(), CliKind::Claude, None, ResourceKind::Config, None, None).err(), Some("SOURCE_INVALID"));
    put(t.path(), "plugins/installed_plugins.json", r#"{"version":99,"plugins":{}}"#);
    assert_eq!(list(t.path(), CliKind::Claude, None, ResourceKind::Plugins, None, None).err(), Some("SOURCE_UNSUPPORTED"));
}
#[test]
fn instructions_are_documents_not_fabricated_agents() {
    let t = tempfile::tempdir().unwrap(); put(t.path(), "AGENTS.md", "project instructions");
    let docs = list(t.path(), CliKind::Codex, None, ResourceKind::Instructions, None, None).unwrap();
    assert_eq!(docs[0]["type"], "document"); assert_eq!(docs[0]["name"], "AGENTS.md");
}
#[test]
fn revocation_precedes_native_reads() {
    let t = tempfile::tempdir().unwrap(); put(t.path(), "settings.json", "{bad");
    let root = Root::open(t.path()).unwrap();
    let result = read(&Catalog { cli: CliKind::Claude, root: &root, project: None, project_paths: &[], check: &|| Err("SCOPE_REVOKED") }, &Options { kind: ResourceKind::Config, query: None, session_id: None }, &mut Budget::new(Limits::default()));
    assert_eq!(result.err(), Some("SCOPE_REVOKED"));
}
