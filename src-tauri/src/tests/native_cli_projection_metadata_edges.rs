use super::super::scoped_fs::Limits;
use super::*;
use serde_json::{json, Value};
use std::{fs, path::Path};

fn put(root: &Path, path: &str, content: Value) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, content.to_string()).unwrap();
}
fn items(root: &Path, project: Option<&Path>, kind: ResourceKind) -> ReadResult<Value> {
    let root = Root::open(root)?;
    let granted = project.map(Root::open).transpose()?;
    let paths = project
        .map(Path::to_path_buf)
        .into_iter()
        .collect::<Vec<_>>();
    let output = read(
        &Catalog {
            cli: CliKind::Claude,
            root: &root,
            project: granted.as_ref(),
            project_paths: &paths,
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind,
            query: None,
            session_id: None,
        },
        &mut Budget::new(Limits::default()),
    )?;
    Ok(serde_json::to_value(output).unwrap())
}

// Removing project-local expansion from the custom-root user config must fail this test.
#[test]
fn custom_root_mcp_includes_only_registered_project_local_entries() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("custom");
    let project = t.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let foreign = t.path().join("foreign");
    put(
        &root,
        ".claude.json",
        json!({
            "mcpServers": {"global-docs": {"type":"http", "headers":{"Authorization":"SECRET"}}},
            "projects": {
                project.to_str().unwrap(): {"mcpServers":{"local-docs":{"command":"SECRET","env":{"TOKEN":"SECRET"}}}},
                foreign.to_str().unwrap(): {"mcpServers":{"foreign-docs":{"command":"SECRET"}}}
            }
        }),
    );
    let result = items(&root, Some(&project), ResourceKind::Mcp).unwrap();
    let rows = result.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|x| x["name"] == "global-docs"));
    assert!(rows
        .iter()
        .any(|x| x["name"] == "local-docs" && x["origin"] == "project-local"));
    assert!(!result.to_string().contains("SECRET"));
    assert!(!result.to_string().contains("foreign-docs"));
    assert_eq!(
        items(&root, None, ResourceKind::Mcp)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

// Without manifest validation an unsupported layout looks like a valid empty resource list.
#[test]
fn custom_plugin_layout_is_unavailable_not_silently_empty() {
    let t = tempfile::tempdir().unwrap();
    let install = t.path().join("plugins/cache/audit");
    put(
        t.path(),
        "plugins/installed_plugins.json",
        json!({"version":2,"plugins":{"audit@local":[{
            "scope":"user", "installPath":install.to_str().unwrap(), "version":"1.0"
        }]}}),
    );
    for (field, kind) in [
        ("skills", ResourceKind::Skills),
        ("commands", ResourceKind::Skills),
        ("agents", ResourceKind::Agents),
        ("mcpServers", ResourceKind::Mcp),
    ] {
        put(
            &install,
            ".claude-plugin/plugin.json",
            json!({"name":"audit", field:"custom-location"}),
        );
        assert_eq!(
            items(t.path(), None, kind).err(),
            Some("SOURCE_UNSUPPORTED"),
            "{field}"
        );
    }
    put(
        &install,
        ".claude-plugin/plugin.json",
        json!({"name":"audit","version":"1.0"}),
    );
    fs::create_dir_all(install.join("skills/check")).unwrap();
    fs::write(
        install.join("skills/check/SKILL.md"),
        "---\nname: check\ndescription: audit code\n---\nbody",
    )
    .unwrap();
    let result = items(t.path(), None, ResourceKind::Skills).unwrap();
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["name"], "check");
    assert_eq!(result[0]["origin"], "plugin:audit@local");
}

#[test]
fn conflicting_plugin_installations_are_not_collapsed_into_one_version() {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        "plugins/installed_plugins.json",
        json!({"version":2,"plugins":{"audit@local":[
            {"scope":"user", "installPath":t.path().join("first").to_str().unwrap(),"version":"1.0"},
            {"scope":"user", "installPath":t.path().join("second").to_str().unwrap(),"version":"2.0"}
        ]}}),
    );
    assert_eq!(
        items(t.path(), None, ResourceKind::Plugins).err(),
        Some("SOURCE_AMBIGUOUS")
    );
}
