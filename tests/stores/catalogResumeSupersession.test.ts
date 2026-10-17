// 回归：恢复/激活会话时，catalog 的每一次发布都必须只包含一条该会话。
// 此前的缺陷：resumeSession 先发布「历史行 + 打开行」两条同一会话，等随后的
// refresh() 折叠才消失 —— 用户看到列表先多出一条一模一样的会话再消失。
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { watch } from 'vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useProjectsStateStore } from '@/stores/projectsState'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import type { HistorySession, TerminalTab } from '@/stores/session'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import type { UnifiedSession } from '@/types/unifiedSession'

const PROJECT = '/repo'
const SESSION = 'history-id'
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

function nativeEntry(): NativeHistoryEntry {
  return {
    key: 'codex:root',
    context: { cli: 'codex', profileId: 'cx', profileRevision: '7', projectId: 'project', projectPath: PROJECT },
    sessions: [{ type: 'session', sessionKey: 'root-key', nativeSessionId: SESSION, title: 'History',
      truncated: false, archived: false, cwd: PROJECT, updatedAt: '2026-09-29T00:00:00Z' }],
    loading: false, loaded: true, error: null, requestEpoch: '1', sourceRootKey: 'root-key',
  }
}

class FakeLegacyStore {
  readonly tabs = new Map<string, TerminalTab>()
  readonly history = new Map<string, HistorySession[]>()
  readonly archived = new Map<string, string[]>()
  activeTabId: string | null = null
  private nextId = 1
  getCatalogHistoryFor(projectPath: string): HistorySession[] { return this.history.get(projectPath) ?? [] }
  getArchivedSessions(projectPath: string): string[] { return this.archived.get(projectPath) ?? [] }
  createTab(projectPath: string, opts?: { sessionId?: string; name?: string }): string {
    const tabId = `tab-${this.nextId++}`
    this.tabs.set(tabId, { tabId, projectPath, ptyId: null, sessionId: opts?.sessionId ?? null,
      name: opts?.name ?? 'New Session', status: 'stopped', createdAt: 10, lastActiveAt: 10,
      working: false, pending: false, isResume: Boolean(opts?.sessionId), cli: 'claude' })
    return tabId
  }
  setActiveTab(tabId: string | null): void { this.activeTabId = tabId }
  removeTab(tabId: string): void { this.tabs.delete(tabId) }
  async closeTab(tabId: string): Promise<void> { this.tabs.delete(tabId) }
  updateTabName(tabId: string, name: string): void { const tab = this.tabs.get(tabId); if (tab) tab.name = name }
  async archiveSession(projectPath: string, sessionId: string): Promise<void> {
    this.archived.set(projectPath, [...(this.archived.get(projectPath) ?? []), sessionId])
  }
  async restoreSession(projectPath: string, sessionId: string): Promise<void> {
    this.archived.set(projectPath, (this.archived.get(projectPath) ?? []).filter(id => id !== sessionId))
  }
}

// 同一会话的身份：CLI 会话 ID + 项目（打开行与历史行共享它）。
function identityOf(row: UnifiedSession): string {
  return `${row.cli}|${row.projectPath}|${row.nativeSessionId ?? row.id}`
}

function recordPublications(catalog: ReturnType<typeof useUnifiedSessionsStore>) {
  const states: { ids: string[]; duplicated: boolean }[] = []
  watch(() => catalog.sessions, rows => {
    const counts = new Map<string, number>()
    for (const row of rows) counts.set(identityOf(row), (counts.get(identityOf(row)) ?? 0) + 1)
    states.push({ ids: rows.map(row => row.id), duplicated: [...counts.values()].some(count => count > 1) })
  }, { flush: 'sync' })
  return states
}

// 刷新带真实延迟：模拟生产的 adapter.listSessions 往返，让中间发布可见。
function nativeHarness(latency = 120) {
  const entries = [nativeEntry()]
  const tabs = useNativeTabsStore()
  const adapter = createNativeCliAdapter({
    tabs,
    history: { all: () => entries, load: async () => entries[0] },
    runtime: {
      createTab: input => tabs.create({ cli: input.cli, projectId: 'project', projectPath: input.projectPath,
        profileId: 'cx', profileRevision: '7', action: input.action, sourceSessionKey: input.sourceSessionKey, title: input.title }),
      restartTab: async id => tabs.tab(id)!,
      stopTab: async () => undefined,
      renameTab: async () => undefined,
    },
    archive: { getArchivedSessions: () => [], archiveSession: async () => undefined, restoreSession: async () => undefined },
  })
  const list = adapter.listSessions.bind(adapter)
  adapter.listSessions = async (projectKey?: string) => { await sleep(latency); return list(projectKey) }
  const catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([adapter])
  return catalog
}

function legacyHarness(latency = 120) {
  const store = new FakeLegacyStore()
  store.history.set(PROJECT, [{ sessionId: SESSION, name: 'Legacy History', projectPath: PROJECT, lastActiveAt: 100 }])
  const adapter = createLegacyClaudeAdapter({
    store,
    runtime: { startTab: vi.fn(), stopTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn() },
    projectPaths: () => [PROJECT],
  })
  const list = adapter.listSessions.bind(adapter)
  adapter.listSessions = async (projectKey?: string) => { await sleep(latency); return list(projectKey) }
  const catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([adapter])
  return catalog
}

beforeEach(() => { setActivePinia(createPinia()); useProjectsStateStore().loaded = true })

describe('opened row supersedes its history row', () => {
  it('ResumeCatalog_NativeNeverPublishesSourceAndOpenedTogether_068', async () => {
    const catalog = nativeHarness()
    await catalog.refresh()
    const history = catalog.sessions[0]
    const states = recordPublications(catalog)
    await catalog.resumeCatalogSession(history)
    for (const state of states) console.log('[SUPERS-native]', JSON.stringify(state.ids.map(id => id.slice(0, 20))), state.duplicated ? '<-- DUPLICATE' : '')
    expect(states.filter(state => state.duplicated).map(state => state.ids)).toEqual([])
    expect(catalog.sessions).toHaveLength(1)
  })

  it('ResumeCatalog_LegacyNeverPublishesSourceAndOpenedTogether_069', async () => {
    const catalog = legacyHarness()
    await catalog.refresh()
    const history = catalog.sessions[0]
    const states = recordPublications(catalog)
    await catalog.resumeCatalogSession(history)
    for (const state of states) console.log('[SUPERS-legacy]', JSON.stringify(state.ids.map(id => id.slice(0, 20))), state.duplicated ? '<-- DUPLICATE' : '')
    expect(states.filter(state => state.duplicated).map(state => state.ids)).toEqual([])
    expect(catalog.sessions).toHaveLength(1)
  })
})
