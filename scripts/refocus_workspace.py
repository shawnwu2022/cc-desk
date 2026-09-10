#!/usr/bin/env python3
"""One-shot migration: make Claude capability panels projection-only."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8", newline="\n")


def replace_required(content: str, old: str, new: str, path: str) -> str:
    if old not in content:
        raise RuntimeError(f"expected text not found in {path}: {old!r}")
    return content.replace(old, new)


def regex_required(content: str, pattern: str, replacement: str, path: str) -> str:
    updated, count = re.subn(pattern, replacement, content, count=1, flags=re.S)
    if count != 1:
        raise RuntimeError(f"expected pattern not found in {path}: {pattern!r}")
    return updated


def remove(path: str) -> None:
    target = ROOT / path
    if target.exists():
        target.unlink()


# Frontend IPC: keep only read-only list calls.
path = "src/api/tauri.ts"
content = read(path)
content = content.replace("  McpServerDetail,\n", "")
content = regex_required(
    content,
    r"\n// 切换用户级 skill/agent/mcp/plugin 启用状态.*?(?=\n// ============================================\n// File Management)",
    "\n",
    path,
)
write(path, content)

# Tauri command registry: remove the independent MCP runtime and native mutations.
path = "src-tauri/src/lib.rs"
content = read(path)
content = replace_required(content, "mod mcp;\n", "", path)
for command in [
    "commands::set_skill_enabled",
    "commands::set_agent_enabled",
    "commands::set_mcp_server_enabled",
    "commands::set_plugin_enabled",
    "commands::get_mcp_server_detail",
]:
    content = replace_required(content, f"            {command},\n", "", path)
write(path, content)

path = "src-tauri/src/commands.rs"
content = read(path)
start = content.find("/// 切换用户级 Skill 启用状态")
end = content.find("// ==================== Logging Commands ====================")
if start < 0 or end < 0 or end <= start:
    raise RuntimeError("native mutation/MCP command block not found")
content = content[:start] + content[end:]
write(path, content)

path = "src-tauri/src/tests/mod.rs"
content = read(path)
content = replace_required(content, "#[cfg(test)]\nmod mcp;\n", "", path)
write(path, content)

# Authentication material remains available to native parsers but never crosses into the WebView.
path = "src-tauri/src/store.rs"
content = read(path)
content = replace_required(
    content,
    "    /// 环境变量（stdio server）\n    pub env: Option<HashMap<String, String>>,\n    /// HTTP Headers（用于认证）\n    pub headers: Option<HashMap<String, String>>,\n",
    "    /// 环境变量仅供原生配置解析，不序列化到 WebView。\n    #[serde(skip_serializing)]\n    pub env: Option<HashMap<String, String>>,\n    /// HTTP Headers 可能包含凭据，不序列化到 WebView。\n    #[serde(skip_serializing)]\n    pub headers: Option<HashMap<String, String>>,\n",
    path,
)
write(path, content)

for path in [
    "src-tauri/src/mcp.rs",
    "src-tauri/src/tests/mcp.rs",
    "src/components/mcp/McpSubItem.vue",
    "tests/api/setEnabled.test.ts",
]:
    remove(path)

# Remove the one-shot migration machinery from the resulting product branch.
remove(".github/workflows/refocus-workspace.yml")
remove("scripts/refocus_workspace.py")
