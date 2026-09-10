#!/usr/bin/env python3
"""Remove fork-era Claude/Git distribution while retaining CC Desk updates."""

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


def regex_required(content: str, pattern: str, replacement: str, path: str, *, flags: int = 0) -> str:
    updated, count = re.subn(pattern, replacement, content, count=1, flags=flags)
    if count != 1:
        raise RuntimeError(f"expected pattern not found in {path}: {pattern[:120]!r}")
    return updated


def remove(path: str) -> None:
    target = ROOT / path
    if target.exists():
        target.unlink()


def update_app_shell() -> None:
    path = "src/App.vue"
    content = read(path)
    content = regex_required(
        content,
        r"\n\s*<!-- 安装进度显示（多任务） -->.*?<!-- 按钮：Auto Install 和 Retry -->\n\s*<div class=\"check-btn-row\">.*?</div>\n",
        "\n      <div class=\"check-btn-row\">\n"
        "        <button class=\"check-retry-btn\" @click=\"retryChecks\">\n"
        "          {{ t('retry') }}\n"
        "        </button>\n"
        "      </div>\n",
        path,
        flags=re.S,
    )
    for line in [
        "  getInstalledClaudeVersion,\n",
        "  downloadAndInstallClaude,\n",
        "  downloadAndInstallGit,\n",
        "  onInstallProgress,\n",
    ]:
        content = content.replace(line, "")
    content = regex_required(
        content,
        r"\n// 自动安装状态\nconst isInstalling = ref\(false\).*?const installTasks = ref<InstallTask\[\]>\(\[\]\)\n",
        "\n",
        path,
        flags=re.S,
    )
    content = content.replace("let unlistenInstallProgress: (() => void) | null = null\n", "")
    content = content.replace("  unlistenInstallProgress?.()\n", "")
    content = regex_required(
        content,
        r"\n  // 启动只读本地 Claude CLI 版本号，不发 HTTP 请求对比 OSS\n  getInstalledClaudeVersion\(\).*?\.catch\(\(\) => \{\}\)\n",
        "\n",
        path,
        flags=re.S,
    )
    content = regex_required(
        content,
        r"\n// 自动安装（并发执行）\nasync function autoInstall\(\) \{.*?\n\}\n</script>",
        "\n</script>",
        path,
        flags=re.S,
    )
    write(path, content)


def update_api_and_types() -> None:
    path = "src/api/tauri.ts"
    content = read(path)
    for line in [
        "  ClaudeCliUpdateInfo,\n",
        "  ClaudeVersionEntry,\n",
        "  ClaudeVersions,\n",
    ]:
        content = content.replace(line, "")
    content = regex_required(
        content,
        r"\n// ============================================\n// Dependency Installation\n// ============================================.*?(?=\n// 右键菜单打开目录)",
        "\n",
        path,
        flags=re.S,
    )
    write(path, content)

    path = "src/types/app.ts"
    content = read(path)
    content = regex_required(
        content,
        r"\n// Claude CLI 更新信息.*?(?=\n/// 启动摘要)",
        "\n",
        path,
        flags=re.S,
    )
    write(path, content)


def update_sidebar_store() -> None:
    path = "src/stores/sidebar.ts"
    content = read(path)
    content = content.replace(
        "import type { AgentInfo, SkillInfo, McpServerInfo, PluginInfo, UpdateInfo, ClaudeCliUpdateInfo } from '@/types'",
        "import type { AgentInfo, SkillInfo, McpServerInfo, PluginInfo, UpdateInfo } from '@/types'",
    )
    content = content.replace("  const claudeCliUpdateInfo = ref<ClaudeCliUpdateInfo | null>(null)\n", "")
    content = regex_required(
        content,
        r"\n  function setClaudeCliUpdateInfo\(info: ClaudeCliUpdateInfo\) \{\n    claudeCliUpdateInfo.value = info\n  \}\n",
        "\n",
        path,
    )
    content = content.replace("    claudeCliUpdateInfo,\n", "")
    content = content.replace("    setClaudeCliUpdateInfo,\n", "")
    write(path, content)


def write_update_store() -> None:
    write(
        "src/stores/update.ts",
        """import { defineStore } from 'pinia'\n"
        "import { computed, ref } from 'vue'\n"
        "import type { DownloadProgress, UpdateInfo } from '@/types'\n\n"
        "export type DownloadState = 'idle' | 'downloading' | 'installing' | 'error'\n\n"
        "export const useUpdateStore = defineStore('update', () => {\n"
        "  const updateInfo = ref<UpdateInfo | null>(null)\n"
        "  const downloadState = ref<DownloadState>('idle')\n"
        "  const downloadProgress = ref<DownloadProgress>({ downloaded: 0, total: 0, percent: 0 })\n"
        "  const downloadError = ref('')\n\n"
        "  const hasUpdate = computed(() => updateInfo.value?.hasUpdate ?? false)\n\n"
        "  function setUpdateInfo(info: UpdateInfo | null) {\n"
        "    updateInfo.value = info\n"
        "  }\n\n"
        "  function setDownloadState(state: DownloadState) {\n"
        "    downloadState.value = state\n"
        "  }\n\n"
        "  function setDownloadProgress(progress: DownloadProgress) {\n"
        "    downloadProgress.value = progress\n"
        "  }\n\n"
        "  function setDownloadError(error: string) {\n"
        "    downloadError.value = error\n"
        "  }\n\n"
        "  function clearError() {\n"
        "    downloadError.value = ''\n"
        "  }\n\n"
        "  function resetDownload() {\n"
        "    downloadState.value = 'idle'\n"
        "    downloadProgress.value = { downloaded: 0, total: 0, percent: 0 }\n"
        "    downloadError.value = ''\n"
        "  }\n\n"
        "  return {\n"
        "    updateInfo, downloadState, downloadProgress, downloadError, hasUpdate,\n"
        "    setUpdateInfo, setDownloadState, setDownloadProgress,\n"
        "    setDownloadError, clearError, resetDownload,\n"
        "  }\n"
        "})\n""",
    )


def write_update_section() -> None:
    write(
        "src/components/settings/sections/UpdateSection.vue",
        """<template>\n"
        "  <div class=\"section-content\">\n"
        "    <h2 class=\"section-heading\">{{ t('ccDeskUpdate') }}</h2>\n"
        "    <div class=\"update-card\">\n"
        "      <div class=\"version-row\">\n"
        "        <div class=\"version-info\">\n"
        "          <span class=\"version-label\">CC Desk</span>\n"
        "          <span class=\"version-value\">v{{ currentVersion }}</span>\n"
        "        </div>\n"
        "        <button class=\"check-btn\" :disabled=\"checking\" @click=\"handleCheckUpdate\">\n"
        "          {{ checking ? t('checking') : t('checkForUpdates') }}\n"
        "        </button>\n"
        "      </div>\n\n"
        "      <div v-if=\"errorMessage\" class=\"update-message error\">\n"
        "        {{ t('checkFailed', { error: errorMessage }) }}\n"
        "      </div>\n"
        "      <div v-else-if=\"updateStore.updateInfo && !updateStore.updateInfo.hasUpdate\" class=\"update-message success\">\n"
        "        {{ t('upToDate') }}\n"
        "      </div>\n\n"
        "      <div v-if=\"updateStore.updateInfo?.hasUpdate\" class=\"update-available\">\n"
        "        <div class=\"update-banner\">\n"
        "          <strong>{{ t('versionAvailable', { version: updateStore.updateInfo.version }) }}</strong>\n"
        "          <span>{{ t('yourVersion') }} v{{ updateStore.updateInfo.currentVersion }}</span>\n"
        "        </div>\n"
        "        <div v-if=\"updateStore.updateInfo.releaseNotes\" class=\"release-notes\">\n"
        "          <h4>{{ t('whatsNew') }}</h4>\n"
        "          <div class=\"notes-content\" v-html=\"renderedNotes\"></div>\n"
        "        </div>\n"
        "        <div v-if=\"updateStore.downloadState === 'downloading'\" class=\"progress-section\">\n"
        "          <div class=\"progress-bar\"><div class=\"progress-fill\" :style=\"{ width: updateStore.downloadProgress.percent + '%' }\"></div></div>\n"
        "          <span>{{ updateStore.downloadProgress.percent.toFixed(0) }}%</span>\n"
        "        </div>\n"
        "        <p v-else-if=\"updateStore.downloadState === 'installing'\" class=\"update-message\">{{ t('installingUpdate') }}</p>\n"
        "        <p v-else-if=\"updateStore.downloadState === 'error'\" class=\"update-message error\">{{ updateStore.downloadError }}</p>\n"
        "        <div class=\"action-row\">\n"
        "          <button v-if=\"updateStore.downloadState === 'idle'\" class=\"action-btn primary\" @click=\"handleDownloadAndInstall\">{{ t('downloadAndInstall') }}</button>\n"
        "          <button v-if=\"updateStore.downloadState === 'error'\" class=\"action-btn primary\" @click=\"handleRetry\">{{ t('retry') }}</button>\n"
        "          <button class=\"action-btn secondary\" @click=\"openReleases\">{{ t('manualDownload') }}</button>\n"
        "        </div>\n"
        "      </div>\n"
        "    </div>\n\n"
        "    <div v-if=\"showConfirm\" class=\"confirm-overlay\" @click.self=\"showConfirm = false\">\n"
        "      <div class=\"confirm-dialog\">\n"
        "        <p>{{ t('updateConfirmActivePtys') }}</p>\n"
        "        <div class=\"action-row\">\n"
        "          <button class=\"action-btn secondary\" @click=\"showConfirm = false\">{{ t('cancel') }}</button>\n"
        "          <button class=\"action-btn primary\" @click=\"confirmUpdate\">{{ t('downloadAndInstall') }}</button>\n"
        "        </div>\n"
        "      </div>\n"
        "    </div>\n"
        "  </div>\n"
        "</template>\n\n"
        "<script setup lang=\"ts\">\n"
        "import { computed, ref } from 'vue'\n"
        "import { useI18n } from 'vue-i18n'\n"
        "import { open } from '@tauri-apps/plugin-shell'\n"
        "import { check, checkForUpdates, relaunch } from '@/api/tauri'\n"
        "import { useSessionStore } from '@/stores/session'\n"
        "import { useSidebarStore } from '@/stores/sidebar'\n"
        "import { useUpdateStore } from '@/stores/update'\n\n"
        "const { t } = useI18n()\n"
        "const sessionStore = useSessionStore()\n"
        "const sidebarStore = useSidebarStore()\n"
        "const updateStore = useUpdateStore()\n"
        "const currentVersion = __APP_VERSION__\n"
        "const checking = ref(false)\n"
        "const errorMessage = ref('')\n"
        "const showConfirm = ref(false)\n\n"
        "const renderedNotes = computed(() => (updateStore.updateInfo?.releaseNotes ?? '')\n"
        "  .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')\n"
        "  .replace(/\\n/g, '<br>'))\n\n"
        "async function handleCheckUpdate() {\n"
        "  checking.value = true\n"
        "  errorMessage.value = ''\n"
        "  try {\n"
        "    const info = await checkForUpdates()\n"
        "    updateStore.setUpdateInfo(info)\n"
        "    sidebarStore.setUpdateInfo(info)\n"
        "  } catch (error) {\n"
        "    errorMessage.value = String(error)\n"
        "  } finally {\n"
        "    checking.value = false\n"
        "  }\n"
        "}\n\n"
        "function openReleases() {\n"
        "  open('https://github.com/shawnwu2022/cc-desk/releases')\n"
        "}\n\n"
        "function handleDownloadAndInstall() {\n"
        "  if (sessionStore.runningTabIds.length > 0) {\n"
        "    showConfirm.value = true\n"
        "    return\n"
        "  }\n"
        "  void startDownload()\n"
        "}\n\n"
        "async function confirmUpdate() {\n"
        "  showConfirm.value = false\n"
        "  await startDownload()\n"
        "}\n\n"
        "async function startDownload() {\n"
        "  updateStore.setDownloadState('downloading')\n"
        "  updateStore.clearError()\n"
        "  updateStore.setDownloadProgress({ downloaded: 0, total: 0, percent: 0 })\n"
        "  try {\n"
        "    const update = await check()\n"
        "    if (!update) throw new Error(t('noUpdateAvailable'))\n"
        "    let downloaded = 0\n"
        "    let total = 0\n"
        "    await update.downloadAndInstall((event) => {\n"
        "      if (event.event === 'Started') {\n"
        "        total = event.data.contentLength ?? 0\n"
        "      } else if (event.event === 'Progress') {\n"
        "        downloaded += event.data.chunkLength\n"
        "      } else if (event.event === 'Finished') {\n"
        "        updateStore.setDownloadState('installing')\n"
        "      }\n"
        "      updateStore.setDownloadProgress({\n"
        "        downloaded, total, percent: total > 0 ? downloaded / total * 100 : 0,\n"
        "      })\n"
        "    })\n"
        "    await relaunch()\n"
        "  } catch (error) {\n"
        "    updateStore.setDownloadError(t('updateFailed', { error: String(error) }))\n"
        "    updateStore.setDownloadState('error')\n"
        "  }\n"
        "}\n\n"
        "async function handleRetry() {\n"
        "  updateStore.resetDownload()\n"
        "  await startDownload()\n"
        "}\n"
        "</script>\n\n"
        "<style scoped>\n"
        ".section-content { max-width: 760px; }\n"
        ".section-heading { margin: 0 0 20px; color: var(--text-primary); }\n"
        ".update-card { padding: 20px; border: 1px solid var(--border-color); border-radius: 10px; background: var(--bg-secondary); }\n"
        ".version-row, .version-info, .action-row { display: flex; align-items: center; gap: 12px; }\n"
        ".version-row { justify-content: space-between; }\n"
        ".version-info { flex-direction: column; align-items: flex-start; gap: 2px; }\n"
        ".version-label, .version-value { color: var(--text-primary); }\n"
        ".version-value { font-size: 12px; color: var(--text-secondary); }\n"
        ".check-btn, .action-btn { border: 1px solid var(--border-color); border-radius: 6px; padding: 7px 12px; cursor: pointer; }\n"
        ".check-btn:disabled { cursor: default; opacity: .6; }\n"
        ".action-btn.primary { background: var(--accent-color); color: white; border-color: var(--accent-color); }\n"
        ".action-btn.secondary, .check-btn { background: var(--bg-primary); color: var(--text-primary); }\n"
        ".update-message { margin-top: 16px; color: var(--text-secondary); }\n"
        ".update-message.error { color: var(--status-error); }\n"
        ".update-message.success { color: var(--status-success); }\n"
        ".update-available { margin-top: 18px; display: grid; gap: 16px; }\n"
        ".update-banner { display: flex; flex-direction: column; gap: 4px; color: var(--text-primary); }\n"
        ".release-notes { color: var(--text-secondary); }\n"
        ".release-notes h4 { color: var(--text-primary); margin: 0 0 8px; }\n"
        ".notes-content { line-height: 1.6; }\n"
        ".progress-bar { height: 6px; overflow: hidden; border-radius: 3px; background: var(--bg-primary); }\n"
        ".progress-fill { height: 100%; background: var(--accent-color); }\n"
        ".confirm-overlay { position: fixed; inset: 0; z-index: 200; display: grid; place-items: center; background: rgba(0,0,0,.45); }\n"
        ".confirm-dialog { width: min(420px, 90vw); padding: 20px; border-radius: 10px; background: var(--bg-primary); color: var(--text-primary); }\n"
        "</style>\n""",
    )


def write_update_test() -> None:
    write(
        "tests/stores/update.test.ts",
        """import { createPinia, setActivePinia } from 'pinia'\n"
        "import { beforeEach, describe, expect, test } from 'vitest'\n"
        "import { useUpdateStore } from '@/stores/update'\n\n"
        "describe('update store', () => {\n"
        "  beforeEach(() => setActivePinia(createPinia()))\n\n"
        "  test('tracks only CC Desk update download state', () => {\n"
        "    const store = useUpdateStore()\n"
        "    store.setDownloadState('downloading')\n"
        "    store.setDownloadProgress({ downloaded: 25, total: 100, percent: 25 })\n"
        "    expect(store.downloadState).toBe('downloading')\n"
        "    expect(store.downloadProgress.percent).toBe(25)\n"
        "    expect('claudeVersionList' in store).toBe(false)\n"
        "  })\n\n"
        "  test('resets transient update state', () => {\n"
        "    const store = useUpdateStore()\n"
        "    store.setDownloadState('error')\n"
        "    store.setDownloadError('failed')\n"
        "    store.resetDownload()\n"
        "    expect(store.downloadState).toBe('idle')\n"
        "    expect(store.downloadError).toBe('')\n"
        "  })\n"
        "})\n""",
    )


def update_rust_registry() -> None:
    path = "src-tauri/src/lib.rs"
    content = read(path)
    content = replace_required(content, "mod installer;\n", "", path)
    installer_commands = [
        "installer::get_latest_versions",
        "installer::check_installed_versions",
        "installer::check_claude_cli_update",
        "installer::check_claude_running",
        "installer::kill_claude_processes",
        "installer::download_and_install_claude",
        "installer::get_installed_claude_version",
        "installer::list_claude_versions",
        "installer::download_claude_version",
        "installer::cancel_claude_download",
        "installer::install_claude_version",
        "installer::download_and_install_git",
    ]
    content = content.replace("            #[cfg(target_os = \"windows\")]\n            installer::download_and_install_git,\n", "")
    for command in installer_commands:
        content = content.replace(f"            {command},\n", "")
    write(path, content)

    path = "src-tauri/src/tests/mod.rs"
    content = read(path)
    content = replace_required(content, "#[cfg(test)]\nmod installer;\n", "", path)
    write(path, content)

    remove("src-tauri/src/installer.rs")
    remove("src-tauri/src/tests/installer.rs")


if __name__ == "__main__":
    update_app_shell()
    update_api_and_types()
    update_sidebar_store()
    write_update_store()
    write_update_section()
    write_update_test()
    update_rust_registry()
