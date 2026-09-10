#!/usr/bin/env python3
"""One-shot migration: make SessionStart hook monitoring optional."""

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
        raise RuntimeError(f"expected text not found in {path}: {old[:160]!r}")
    return content.replace(old, new)


def regex_required(content: str, pattern: str, replacement: str, path: str) -> str:
    updated, count = re.subn(pattern, replacement, content, count=1, flags=re.S)
    if count != 1:
        raise RuntimeError(f"expected pattern not found in {path}: {pattern[:160]!r}")
    return updated


path = "src/components/TerminalView.vue"
content = read(path)
content = replace_required(
    content,
    "        <!-- sessionStart 事务进行中提示（非阻塞，允许终端交互） -->\n"
    "        <div v-if=\"sessionStarting\" class=\"session-starting-hint\">{{ t('claudeStarting') }}</div>\n",
    "        <!-- Hook 监控是可选增强；不可用时终端仍保持运行。 -->\n"
    "        <div v-if=\"sessionStarting\" class=\"session-starting-hint\">{{ t('claudeStarting') }}</div>\n"
    "        <div v-else-if=\"showMonitoringUnavailable\" class=\"session-starting-hint\">\n"
    "          {{ t('monitoringUnavailable') }}\n"
    "        </div>\n",
    path,
)
content = replace_required(
    content,
    "import { openInFileManager, logMessage, ptyKill } from '@/api/tauri'\n",
    "import { openInFileManager, logMessage } from '@/api/tauri'\n",
    path,
)
content = replace_required(
    content,
    "import { reduceWaiter, isTimeoutError, STARTUP_TIMEOUT_CODE, type WaiterStatus, type WaiterEvent } from '@/composables/useSessionStartWaiter'\n",
    "import { reduceWaiter, PERSIST_FAILED_CODE, type WaiterStatus, type WaiterEvent } from '@/composables/useSessionStartWaiter'\n",
    path,
)

new_monitoring_block = r'''// ==================== optional SessionStart monitoring ====================
// PTY spawn success is the process-start authority. SessionStart hooks only enrich
// activity state. A missing hook resolves to "unavailable" and never kills a live PTY.
type MonitoringResult = 'monitored' | 'unavailable'

interface WaiterEntry {
  status: WaiterStatus
  resolve: (result: MonitoringResult) => void
  reject: (error: Error) => void
  timer: ReturnType<typeof setTimeout> | null
}

const sessionStartWaiters = new Map<string, WaiterEntry>()
const SESSION_START_TIMEOUT_MS = 30000
const sessionStarting = ref(false)
const monitoringUnavailableTabs = ref<Set<string>>(new Set())

const showMonitoringUnavailable = computed(() => {
  const tabId = sessionStore.activeTabId
  return tabId !== null && monitoringUnavailableTabs.value.has(tabId)
})

function setMonitoringUnavailable(tabId: string, unavailable: boolean) {
  const next = new Set(monitoringUnavailableTabs.value)
  if (unavailable) next.add(tabId)
  else next.delete(tabId)
  monitoringUnavailableTabs.value = next
}

function settleWaiter(tabId: string, event: WaiterEvent) {
  const entry = sessionStartWaiters.get(tabId)
  if (!entry) return

  const next = reduceWaiter(entry.status, event)
  if (next === entry.status) return

  entry.status = next
  if (entry.timer) clearTimeout(entry.timer)
  entry.timer = null
  sessionStartWaiters.delete(tabId)

  switch (next) {
    case 'started':
      entry.resolve('monitored')
      break
    case 'unavailable':
      entry.resolve('unavailable')
      break
    case 'exited':
    case 'failed':
      entry.reject(new Error(t('claudeStartFailed')))
      break
    case 'cancelled':
      entry.reject(new Error('cancelled'))
      break
  }
}

function registerWaiter(tabId: string): Promise<MonitoringResult> {
  const tab = sessionStore.tabs.get(tabId)
  if (tab?.sessionId) return Promise.resolve('monitored')

  return new Promise<MonitoringResult>((resolve, reject) => {
    const timer = setTimeout(
      () => settleWaiter(tabId, { type: 'timeout' }),
      SESSION_START_TIMEOUT_MS
    )
    sessionStartWaiters.set(tabId, {
      status: 'waiting',
      resolve,
      reject,
      timer,
    })
  })
}

/**
 * Add a project and start its first Claude process.
 * The PTY result decides whether launch succeeded. Hook monitoring may become
 * unavailable without changing process state or triggering another launch.
 */
async function startProjectSession(path: string): Promise<void> {
  const tabId = sessionStore.createTab(path)
  sessionStore.setActiveTab(tabId)
  setMonitoringUnavailable(tabId, false)
  const waiter = registerWaiter(tabId)

  let spawnError: Error | null = null
  try {
    if (!terminalRef.value) throw new Error('terminal not ready')
    const result = await terminalRef.value.startTab(tabId)
    if (!result.ok) throw new Error(result.error)
    appStore.setCwdLocal(path)
    sessionStarting.value = true
  } catch (error) {
    spawnError = error instanceof Error ? error : new Error(String(error))
    settleWaiter(tabId, { type: 'spawnFail' })
  }

  let monitoringResult: MonitoringResult
  try {
    monitoringResult = await waiter
  } catch (error) {
    sessionStarting.value = false
    setMonitoringUnavailable(tabId, false)
    sessionStore.removeTab(tabId)
    throw spawnError ?? error
  }

  sessionStarting.value = false

  // The process can still exit in the small interval after monitoring settles.
  // Do not persist a successful startup for a tab that is no longer running.
  const liveTab = sessionStore.tabs.get(tabId)
  if (!liveTab || liveTab.status !== 'running') {
    setMonitoringUnavailable(tabId, false)
    sessionStore.removeTab(tabId)
    throw new Error(t('claudeStartFailed'))
  }

  const monitoringUnavailable = monitoringResult === 'unavailable'
  setMonitoringUnavailable(tabId, monitoringUnavailable)
  if (monitoringUnavailable) {
    void logMessage(
      'warn',
      `SessionStart hook unavailable for tab ${tabId}; Claude PTY remains running`
    )
  }

  try {
    await appStore.setCurrentProject(path, { persist: true })
  } catch (error) {
    const persistError = new Error(PERSIST_FAILED_CODE) as Error & {
      code: typeof PERSIST_FAILED_CODE
      cause?: unknown
    }
    persistError.code = PERSIST_FAILED_CODE
    persistError.cause = error instanceof Error ? error : undefined
    throw persistError
  }
}

function handlePtyExited(tabId: string, _ptyId: string) {
  setMonitoringUnavailable(tabId, false)
  settleWaiter(tabId, { type: 'ptyExit' })
}

const sessionStartHandler: HookEventHandler = (payload: HookEventPayload) => {
  const ptyId = payload.ptyId
  if (!ptyId) return

  const tab = sessionStore.getTabByPtyId(ptyId)
  if (!tab) return

  setMonitoringUnavailable(tab.tabId, false)
  settleWaiter(tab.tabId, { type: 'sessionStart' })
}
let unsubscribeSessionStart: (() => void) | null = null'''

content = regex_required(
    content,
    r"// ==================== sessionStart 事务.*?let unsubscribeSessionStart: \(\(\) => void\) \| null = null",
    new_monitoring_block,
    path,
)
write(path, content)

for path, old, new in [
    (
        "src/i18n/locales/en.ts",
        "  claudeStarting: 'Claude is starting...',\n",
        "  claudeStarting: 'Claude is starting...',\n"
        "  monitoringUnavailable: 'Claude is running, but detailed activity monitoring is unavailable.',\n",
    ),
    (
        "src/i18n/locales/zh.ts",
        "  claudeStarting: 'Claude 启动中...',\n",
        "  claudeStarting: 'Claude 启动中...',\n"
        "  monitoringUnavailable: 'Claude 正在运行，但详细活动状态暂不可用。',\n",
    ),
]:
    content = read(path)
    content = replace_required(content, old, new, path)
    write(path, content)

# Remove the one-shot migration machinery from the resulting branch.
for relative in [
    ".github/workflows/hook-monitoring-migration.yml",
    "scripts/migrate_hook_monitoring.py",
]:
    target = ROOT / relative
    if target.exists():
        target.unlink()
