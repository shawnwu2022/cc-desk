#!/usr/bin/env python3
"""Apply the current CC Desk scope migration phase.

This script is intentionally deterministic and only runs on the isolated
`codex/focus-claude-workspace` branch. It is removed before the PR is marked
ready for review.
"""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8", newline="\n")


def replace_required(content: str, old: str, new: str, path: str) -> str:
    if old not in content:
        raise RuntimeError(f"expected text not found in {path}: {old[:120]!r}")
    return content.replace(old, new)


def remove(path: str) -> None:
    target = ROOT / path
    if target.exists():
        target.unlink()


def remove_provider_management() -> None:
    path = "src/components/settings/SettingsView.vue"
    content = read(path)
    content = replace_required(
        content,
        "      <ProvidersSection v-if=\"sidebarStore.activeSettingsSection === 'providers'\" />\n",
        "",
        path,
    )
    content = replace_required(
        content,
        "import { useSidebarStore } from '@/stores/sidebar'\n",
        "import { useSidebarStore, type SettingsSection } from '@/stores/sidebar'\n",
        path,
    )
    content = replace_required(
        content,
        "import ProvidersSection from './sections/ProvidersSection.vue'\n",
        "",
        path,
    )
    content = replace_required(
        content,
        "const navItems = computed(() => [\n",
        "const navItems = computed<Array<{ id: SettingsSection; label: string }>>(() => [\n",
        path,
    )
    content = replace_required(
        content,
        "  { id: 'providers', label: t('providers') },\n",
        "",
        path,
    )
    write(path, content)

    path = "src/stores/sidebar.ts"
    content = read(path)
    content = replace_required(
        content,
        "export type SidebarPanelType = 'sessions' | 'skills' | 'agents' | 'mcp' | 'plugins' | null\n",
        "export type SidebarPanelType = 'sessions' | 'skills' | 'agents' | 'mcp' | 'plugins' | null\n"
        "export type SettingsSection = 'appearance' | 'startup' | 'shortcuts' | 'update' | 'about'\n\n"
        "const SETTINGS_SECTIONS: readonly SettingsSection[] = [\n"
        "  'appearance', 'startup', 'shortcuts', 'update', 'about'\n"
        "]\n\n"
        "function isSettingsSection(value: string): value is SettingsSection {\n"
        "  return SETTINGS_SECTIONS.includes(value as SettingsSection)\n"
        "}\n",
        path,
    )
    content = replace_required(
        content,
        "  const activeSettingsSection = ref<string>('appearance')\n",
        "  const activeSettingsSection = ref<SettingsSection>('appearance')\n",
        path,
    )
    content = replace_required(
        content,
        "  function openSettings(section?: string) {\n"
        "    panelVisible.value = false\n"
        "    activePanel.value = null\n"
        "    showSettings.value = true\n"
        "    if (section) activeSettingsSection.value = section\n"
        "  }\n",
        "  function openSettings(section?: string) {\n"
        "    panelVisible.value = false\n"
        "    activePanel.value = null\n"
        "    showSettings.value = true\n"
        "    if (section) {\n"
        "      activeSettingsSection.value = isSettingsSection(section) ? section : 'appearance'\n"
        "    }\n"
        "  }\n",
        path,
    )
    write(path, content)

    path = "src-tauri/src/lib.rs"
    content = read(path)
    content = replace_required(content, "mod providers;\n", "", path)
    provider_commands = [
        "commands::get_providers_config",
        "commands::save_providers_config",
        "commands::activate_provider",
        "commands::create_provider",
        "commands::update_provider",
        "commands::delete_provider",
        "commands::update_provider_sort_order",
        "commands::update_common_config",
        "commands::check_cc_switch_db_exists",
        "commands::import_from_cc_switch",
        "commands::test_provider_connection",
    ]
    content = content.replace("            // Provider Commands\n", "")
    for command in provider_commands:
        content = content.replace(f"            {command},\n", "")
    write(path, content)

    path = "src-tauri/src/commands.rs"
    content = read(path)
    content, count = re.subn(
        r"\nuse crate::providers::\{.*?\};\n",
        "\n",
        content,
        count=1,
        flags=re.S,
    )
    if count != 1:
        raise RuntimeError("failed to remove Provider imports from commands.rs")
    marker = "// ==================== Provider Commands ===================="
    if marker not in content:
        raise RuntimeError("Provider command marker not found")
    content = content.split(marker, 1)[0].rstrip() + "\n"
    write(path, content)

    path = "src-tauri/src/tests/mod.rs"
    content = read(path)
    content = replace_required(content, "#[cfg(test)]\nmod providers;\n", "", path)
    write(path, content)

    for path in [
        "src-tauri/src/providers.rs",
        "src-tauri/src/tests/providers.rs",
        "src/api/provider.ts",
        "src/stores/providers.ts",
        "src/types/provider.ts",
        "src/config/providerPresets.ts",
        "src/components/settings/sections/ProvidersSection.vue",
        "src/components/settings/providers/CommonConfigPanel.vue",
        "src/components/settings/providers/ProviderCard.vue",
        "src/components/settings/providers/ProviderEditPanel.vue",
        "src/components/settings/providers/ProviderList.vue",
        "src/components/settings/providers/ProviderPresetPanel.vue",
        "tests/stores/providers.test.ts",
        "tests/config/providerPresets.test.ts",
        "docs/provider-management.md",
        "docs/provider-test-cases.md",
    ]:
        remove(path)


if __name__ == "__main__":
    remove_provider_management()
