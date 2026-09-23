use super::*;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

type PluginObservation = (Option<String>, Option<bool>, Option<bool>);

pub(super) fn read(
    c: &Catalog<'_>,
    kind: ResourceKind,
    b: &mut Budget,
) -> ReadResult<Vec<ResourceItem>> {
    if c.cli == CliKind::Shell {
        return Err("SOURCE_UNSUPPORTED");
    }
    let mut out = Vec::new();
    match kind {
        ResourceKind::Instructions => {
            let name = if c.cli == CliKind::Claude {
                "CLAUDE.md"
            } else {
                "AGENTS.md"
            };
            document(c, c.root, name, "global", b, &mut out)?;
            if let Some(project) = c.project {
                document(c, project, name, "project", b, &mut out)?;
                if c.cli == CliKind::Claude {
                    document(c, project, ".claude/CLAUDE.md", "project", b, &mut out)?;
                    document(c, project, "CLAUDE.local.md", "project-local", b, &mut out)?;
                }
            }
        }
        ResourceKind::Skills => {
            skills(c, c.root, "skills", "global", b, &mut out)?;
            if c.cli == CliKind::Claude {
                markdown(
                    c,
                    c.root,
                    "commands",
                    "global-command",
                    false,
                    6,
                    b,
                    &mut out,
                )?;
            }
            if let Some(project) = c.project {
                let dir = if c.cli == CliKind::Claude {
                    ".claude/skills"
                } else {
                    ".agents/skills"
                };
                skills(c, project, dir, "project", b, &mut out)?;
                if c.cli == CliKind::Claude {
                    markdown(
                        c,
                        project,
                        ".claude/commands",
                        "project-command",
                        false,
                        6,
                        b,
                        &mut out,
                    )?;
                }
            }
            if c.cli == CliKind::Claude {
                plugin_resources(c, kind, b, &mut out)?;
            }
        }
        ResourceKind::Agents if c.cli == CliKind::Claude => {
            markdown(c, c.root, "agents", "global", true, 6, b, &mut out)?;
            if let Some(project) = c.project {
                markdown(
                    c,
                    project,
                    ".claude/agents",
                    "project",
                    true,
                    6,
                    b,
                    &mut out,
                )?;
            }
            plugin_resources(c, kind, b, &mut out)?;
        }
        ResourceKind::Plugins if c.cli == CliKind::Claude => {
            let installed = installed(c, b)?;
            let config = json(c, c.root, "settings.json", b)?;
            let enabled = object_field(&config, "enabledPlugins")?;
            let mut records: BTreeMap<String, PluginObservation> = BTreeMap::new();
            for (id, value) in installed {
                records.insert(
                    id,
                    (
                        value["version"].as_str().map(str::to_owned),
                        None,
                        Some(true),
                    ),
                );
            }
            if let Some(enabled) = enabled {
                for (id, value) in enabled {
                    let flag = value.as_bool().ok_or("SOURCE_INVALID")?;
                    records.entry(id.clone()).or_insert((None, None, None)).1 = Some(flag);
                }
            }
            for (id, (version, enabled, installed)) in records {
                out.push(ResourceItem::Plugin {
                    name: id.split('@').next().unwrap_or(&id).into(),
                    id,
                    version,
                    enabled,
                    installed,
                    origin: "global".into(),
                });
            }
        }
        ResourceKind::Config | ResourceKind::Mcp | ResourceKind::Agents | ResourceKind::Plugins => {
            if c.cli == CliKind::Claude {
                let global = json(c, c.root, "settings.json", b)?;
                settings(&global, kind, "global", &mut out)?;
                if kind == ResourceKind::Mcp {
                    claude_user_mcp(
                        c,
                        &json(c, c.root, ".claude.json", b)?,
                        "root-user-config",
                        &mut out,
                    )?;
                    let mcps = json(c, c.root, ".mcp.json", b)?;
                    settings(&mcps, kind, "global", &mut out)?;
                    if let Some(user) = c.user_config {
                        let config = json(c, user, ".claude.json", b)?;
                        claude_user_mcp(c, &config, "user-config", &mut out)?;
                    }
                }
                if let Some(project) = c.project {
                    for (path, origin) in [
                        (".claude/settings.json", "project"),
                        (".claude/settings.local.json", "project-local"),
                    ] {
                        settings(&json(c, project, path, b)?, kind, origin, &mut out)?;
                    }
                    if kind == ResourceKind::Mcp {
                        settings(
                            &json(c, project, ".mcp.json", b)?,
                            kind,
                            "project",
                            &mut out,
                        )?;
                    }
                }
                if kind == ResourceKind::Mcp {
                    plugin_resources(c, kind, b, &mut out)?;
                }
            } else {
                codex_settings(
                    &toml(c, c.root, "config.toml", b)?,
                    kind,
                    "global",
                    &mut out,
                )?;
                if let Some(project) = c.project {
                    codex_settings(
                        &toml(c, project, ".codex/config.toml", b)?,
                        kind,
                        "project",
                        &mut out,
                    )?;
                }
            }
        }
        _ => return Err("SOURCE_UNSUPPORTED"),
    }
    Ok(out)
}
// Project keys are filter data, never a source of filesystem authority.
fn claude_user_mcp(
    c: &Catalog<'_>,
    config: &Value,
    origin: &str,
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    settings(config, ResourceKind::Mcp, origin, out)?;
    if let Some(projects) = object_field(config, "projects")? {
        for path in c.project_paths {
            if let Some(project) = path.to_str().and_then(|key| projects.get(key)) {
                settings(project, ResourceKind::Mcp, "project-local", out)?;
            }
        }
    }
    Ok(())
}

fn json(c: &Catalog<'_>, r: &Root, path: &str, b: &mut Budget) -> ReadResult<Value> {
    let Some(bytes) = c.bytes(r, path, b)? else {
        return Ok(Value::Object(Map::new()));
    };
    let value: Value = serde_json::from_str(text(&bytes)?).map_err(|_| "SOURCE_INVALID")?;
    validate_value(&value)?;
    if !value.is_object() {
        return Err("SOURCE_INVALID");
    }
    Ok(value)
}
fn toml(c: &Catalog<'_>, r: &Root, path: &str, b: &mut Budget) -> ReadResult<Value> {
    let Some(bytes) = c.bytes(r, path, b)? else {
        return Ok(Value::Object(Map::new()));
    };
    let value: toml::Value = toml::from_str(text(&bytes)?).map_err(|_| "SOURCE_INVALID")?;
    let value = serde_json::to_value(value).map_err(|_| "SOURCE_INVALID")?;
    validate_value(&value)?;
    Ok(value)
}
fn object_field<'a>(v: &'a Value, key: &str) -> ReadResult<Option<&'a Map<String, Value>>> {
    v.get(key)
        .map(|x| x.as_object().ok_or("SOURCE_INVALID"))
        .transpose()
}
fn settings(
    v: &Value,
    kind: ResourceKind,
    origin: &str,
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    if kind == ResourceKind::Config {
        for name in ["model", "language", "outputStyle"] {
            if let Some(value) = v.get(name) {
                let value = value.as_str().ok_or("SOURCE_INVALID")?;
                out.push(ResourceItem::Setting {
                    name: name.into(),
                    value: bounded(value, 1024).0,
                    origin: origin.into(),
                });
            }
        }
    } else if kind == ResourceKind::Mcp {
        mcp(v, "mcpServers", origin, out)?;
    }
    Ok(())
}
fn mcp(v: &Value, field: &str, origin: &str, out: &mut Vec<ResourceItem>) -> ReadResult<()> {
    if let Some(servers) = object_field(v, field)? {
        for (name, entry) in servers {
            if !entry.is_object() {
                return Err("SOURCE_INVALID");
            }
            let transport = match entry.get("type").and_then(Value::as_str) {
                Some("stdio") => "stdio",
                Some("http") => "http",
                Some("sse") => "sse",
                Some(_) => "unknown",
                None if entry.get("command").is_some() => "stdio",
                None if entry.get("url").is_some() => "http",
                None => "unknown",
            };
            // Values/headers/env/argv are deliberately not part of the projection DTO.
            out.push(ResourceItem::Mcp {
                name: bounded(name, 256).0,
                transport: transport.into(),
                origin: origin.into(),
            });
        }
    }
    Ok(())
}
fn codex_settings(
    v: &Value,
    kind: ResourceKind,
    origin: &str,
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    match kind {
        ResourceKind::Config => {
            for name in [
                "model",
                "model_reasoning_effort",
                "approval_policy",
                "sandbox_mode",
            ] {
                if let Some(value) = v.get(name) {
                    if let Some(value) = value.as_str() {
                        out.push(ResourceItem::Setting {
                            name: name.into(),
                            value: bounded(value, 1024).0,
                            origin: origin.into(),
                        });
                    } else {
                        return Err("SOURCE_UNSUPPORTED");
                    }
                }
            }
        }
        ResourceKind::Mcp => mcp(v, "mcp_servers", origin, out)?,
        ResourceKind::Agents => {
            if let Some(agents) = object_field(v, "agents")? {
                for (name, entry) in agents {
                    if !entry.is_object() {
                        continue;
                    } // max_threads etc. are not agents.
                    out.push(ResourceItem::Agent {
                        name: bounded(name, 256).0,
                        description: bounded(entry["description"].as_str().unwrap_or(""), 2048).0,
                        model: None,
                        origin: origin.into(),
                    });
                }
            }
        }
        ResourceKind::Plugins => {
            if let Some(plugins) = object_field(v, "plugins")? {
                for (id, entry) in plugins {
                    if !entry.is_object() {
                        return Err("SOURCE_INVALID");
                    }
                    out.push(ResourceItem::Plugin {
                        id: id.clone(),
                        name: bounded(id, 256).0,
                        version: None,
                        enabled: entry["enabled"].as_bool(),
                        installed: None,
                        origin: origin.into(),
                    });
                }
            }
        }
        _ => return Err("SOURCE_UNSUPPORTED"),
    }
    Ok(())
}
fn document(
    c: &Catalog<'_>,
    root: &Root,
    path: &str,
    origin: &str,
    b: &mut Budget,
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    if let Some(bytes) = c.bytes(root, path, b)? {
        let (text, truncated) = bounded(text(&bytes)?, 16 * 1024);
        out.push(ResourceItem::Document {
            name: path.into(),
            text,
            truncated,
            origin: origin.into(),
        });
    }
    Ok(())
}
fn frontmatter(input: &str, fallback: &str) -> (String, String, Option<String>) {
    // Deliberately small scalar subset. No YAML tags, expressions or file includes.
    let mut values = BTreeMap::new();
    if input.lines().next() == Some("---") {
        for line in input.lines().skip(1).take_while(|l| *l != "---") {
            if let Some((k, v)) = line.split_once(':') {
                values.insert(k.trim(), v.trim().trim_matches(['\'', '"']));
            }
        }
    }
    (
        bounded(values.get("name").copied().unwrap_or(fallback), 256).0,
        bounded(
            values.get("description").copied().unwrap_or_else(|| {
                input
                    .lines()
                    .find(|l| !l.trim().is_empty() && *l != "---")
                    .unwrap_or("")
            }),
            2048,
        )
        .0,
        values.get("model").map(|v| bounded(v, 256).0),
    )
}
fn skills(
    c: &Catalog<'_>,
    root: &Root,
    path: &str,
    origin: &str,
    b: &mut Budget,
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    for entry in c.entries(root, path, b)? {
        if !entry.is_file && !entry.is_dir {
            return Err("SOURCE_NOT_REGULAR");
        }
        if !entry.is_dir {
            continue;
        }
        if let Some(bytes) = c.bytes(root, &child(&child(path, &entry.name), "SKILL.md"), b)? {
            let (name, description, _) = frontmatter(text(&bytes)?, &entry.name);
            out.push(ResourceItem::Skill {
                name,
                description,
                origin: origin.into(),
            });
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn markdown(
    c: &Catalog<'_>,
    root: &Root,
    path: &str,
    origin: &str,
    agent: bool,
    depth: usize,
    b: &mut Budget,
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    for entry in c.entries(root, path, b)? {
        let p = child(path, &entry.name);
        if entry.is_dir {
            if depth == 0 {
                return Err("SOURCE_UNSUPPORTED");
            }
            markdown(c, root, &p, origin, agent, depth - 1, b, out)?;
        } else if entry.is_file && entry.name.ends_with(".md") {
            let Some(bytes) = c.bytes(root, &p, b)? else {
                return Err("SOURCE_CHANGED");
            };
            let (name, description, model) =
                frontmatter(text(&bytes)?, entry.name.trim_end_matches(".md"));
            if agent {
                out.push(ResourceItem::Agent {
                    name,
                    description,
                    model,
                    origin: origin.into(),
                });
            } else {
                out.push(ResourceItem::Skill {
                    name,
                    description,
                    origin: origin.into(),
                });
            }
        } else if !entry.is_dir && !entry.is_file {
            return Err("SOURCE_NOT_REGULAR");
        }
    }
    Ok(())
}
fn installed(c: &Catalog<'_>, b: &mut Budget) -> ReadResult<Vec<(String, Value)>> {
    let registry = json(c, c.root, "plugins/installed_plugins.json", b)?;
    if registry.as_object().is_some_and(Map::is_empty) {
        return Ok(Vec::new());
    }
    if !matches!(registry["version"].as_u64(), Some(1 | 2)) {
        return Err("SOURCE_UNSUPPORTED");
    }
    let entries = object_field(&registry, "plugins")?.ok_or("SOURCE_INVALID")?;
    let mut out = Vec::new();
    for (id, records) in entries {
        let records = records
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_else(|| vec![records]);
        for record in records {
            if !record.is_object() {
                return Err("SOURCE_INVALID");
            }
            match record.get("scope") {
                None => {} // v1 registry predates per-project installation scopes.
                Some(Value::String(scope))
                    if ["user", "project", "local"].contains(&scope.as_str()) => {}
                _ => return Err("SOURCE_UNSUPPORTED"),
            }
            if let Some("project" | "local") = record["scope"].as_str() {
                if !record["projectPath"]
                    .as_str()
                    .is_some_and(|p| c.project_paths.iter().any(|known| known == Path::new(p)))
                {
                    continue;
                }
            }
            // The DTO has one observation per plugin ID. Do not invent precedence
            // between multiple matching installations or merge their resources.
            if out.iter().any(|(existing, _)| existing == id) {
                return Err("SOURCE_AMBIGUOUS");
            }
            out.push((id.clone(), record.clone()));
        }
    }
    Ok(out)
}
fn plugin_resources(
    c: &Catalog<'_>,
    kind: ResourceKind,
    b: &mut Budget,
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    for (id, record) in installed(c, b)? {
        let Some(path) = record["installPath"].as_str() else {
            return Err("SOURCE_INVALID");
        };
        let relative = c.root.descendant(Path::new(path))?;
        let manifest = json(
            c,
            c.root,
            &child(&relative, ".claude-plugin/plugin.json"),
            b,
        )?;
        let resource_fields: &[&str] = match kind {
            ResourceKind::Skills => &["skills", "commands"],
            ResourceKind::Agents => &["agents"],
            ResourceKind::Mcp => &["mcpServers"],
            _ => &[],
        };
        if resource_fields
            .iter()
            .any(|field| manifest.get(*field).is_some())
        {
            // Only the default layout is supported here. An explicit custom layout
            // cannot be reported as an empty or complete default-layout observation.
            return Err("SOURCE_UNSUPPORTED");
        }
        let origin = format!("plugin:{}", bounded(&id, 256).0);
        match kind {
            ResourceKind::Skills => {
                skills(c, c.root, &child(&relative, "skills"), &origin, b, out)?;
                markdown(
                    c,
                    c.root,
                    &child(&relative, "commands"),
                    &origin,
                    false,
                    6,
                    b,
                    out,
                )?;
            }
            ResourceKind::Agents => markdown(
                c,
                c.root,
                &child(&relative, "agents"),
                &origin,
                true,
                6,
                b,
                out,
            )?,
            ResourceKind::Mcp => settings(
                &json(c, c.root, &child(&relative, ".mcp.json"), b)?,
                kind,
                &origin,
                out,
            )?,
            _ => {}
        }
    }
    Ok(())
}
