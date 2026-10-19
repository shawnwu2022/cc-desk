// 回归：历史 force 重载（historyCue 路径，scope 往返期间 entry 处于 loading 过渡态）
// 不得丢失已证明的 sourceRootKey —— 否则同源折叠与归档判断（bare/legacy 路径依赖
// root 相等）失效一个往返：列表闪出「打开行 + 关闭行」双行、归档行漏进主列表再消失。
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useProjectsStateStore } from '@/stores/projectsState'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import type { HistorySession, TerminalTab } from '@/stores/session'
import type { UnifiedSession } from '@/types/unifiedSession'

const PROJECT = '/repo'
const SESSION = 'sess-1'
const ROOT = 'local:windows:1:2:3'

const clients: any[] = []
vi.mock('@/api/tauri', () => ({
  createNativeProjectionClient: () => clients.shift(),
}))

function nativeSource() {
  return {
    scopeId: 'scope_1', instanceId: 'instance_1', cli: 'claude', sourceRootKey: ROOT, identityEpoch: '1',
    profileId: 'cx', profileRevision: '7',
    target: { kind: 'profile' as const, profileId: 'cx', expectedProfileRevision: '7', projectIds: ['project'] },
    basis: 'configured-profile' as const,
  }
}

function historyItem() {
  return { type: 'session', sessionKey: 'key-1', nativeSessionId: SESSION, title: 'Task', truncated: false, archived: false, cwd: PROJECT, updatedAt: '2026-10-19T00:00:00Z' }
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

beforeEach(() => {
  setActivePinia(createPinia())
  useProjectsStateStore().loaded = true
  clients.length = 0
})

describe('history force-reload transition keeps root', () => {
  it('SessionList_ReloadTransitionKeepsFoldAndArchive_071', async () => {
    const legacyStore = new FakeLegacyStore()
    legacyStore.history.set(PROJECT, [{ sessionId: SESSION, name: 'Task', projectPath: PROJECT, lastActiveAt: 100 }])

    const history = useNativeHistoryStore()
    const tabs = useNativeTabsStore()
    const nativeAdapter = createNativeCliAdapter({
      tabs, history,
      legacyClaudeRootKey: async () => ROOT,
      runtime: {
        createTab: input => tabs.create({ cli: input.cli, projectId: 'project', projectPath: input.projectPath,
          profileId: 'cx', profileRevision: '7', action: input.action, sourceSessionKey: input.sourceSessionKey, title: input.title }),
        restartTab: async id => tabs.tab(id)!,
        stopTab: async () => undefined,
        renameTab: async () => undefined,
      },
      archive: { getArchivedSessions: legacyStore.getArchivedSessions.bind(legacyStore), archiveSession: async () => undefined, restoreSession: async () => undefined },
    })
    const legacyAdapter = createLegacyClaudeAdapter({
      store: legacyStore,
      runtime: { startTab: vi.fn(), stopTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn() },
      projectPaths: () => [PROJECT],
    })
    const catalog = useUnifiedSessionsStore()
    catalog.configureAdapters([nativeAdapter, legacyAdapter], { legacyClaudeRootKey: ROOT })

    // 第一轮：scope 成功 + 会话在历史里 → entry 证明 root。
    clients.push({ scope: vi.fn().mockResolvedValue(nativeSource()),
      read: vi.fn().mockResolvedValue({ state: 'ready', items: [historyItem()], hasMore: false }) })
    const context = { cli: 'claude' as const, profileId: 'cx', profileRevision: '7', projectId: 'project', projectPath: PROJECT }
    await history.load(context)
    expect(history.get(context)?.sourceRootKey).toBe(ROOT)

    // 打开的 native tab（resume-id）认领该会话。
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: PROJECT,
      profileId: 'cx', profileRevision: '7', action: { kind: 'resume-id', nativeSessionId: SESSION },
      sourceSessionKey: 'key-1', title: 'Task' })
    tabs.tab(tab.tabId)!.status = 'running'
    await catalog.refresh(PROJECT)
    expect(catalog.sessions.filter(row => row.projectPath === PROJECT)).toHaveLength(1)

    // historyCue 的 force 重载：scope 往返期间 entry 处于 loading 过渡态。
    clients.push({ scope: vi.fn(() => new Promise(() => {})), read: vi.fn() })
    void history.loadGrouped({ cli: 'claude', profileId: 'cx', profileRevision: '7',
      projects: [{ projectId: 'project', projectPath: PROJECT }] }, { force: true })
    const entry = history.get(context)
    expect(entry?.loading).toBe(true)

    // 过渡态的投影：同源折叠保持单行。
    await catalog.refresh(PROJECT)
    const rows = catalog.sessions.filter((row: UnifiedSession) => row.projectPath === PROJECT)
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({ runtime: 'native-cli', opened: true, archived: false, nativeRootKey: ROOT })
  })

  // 归档注册表存裸 session ID（legacy 归档的历史形态）：归档判断的 bare 路径要求
  // entry root === legacy root。重载过渡态丢失 root 会让归档行以未归档姿态漏进主
  // 列表，下一次投影又消失 —— 用户看到的「归档会话先渲染再消失」。
  it('SessionList_ReloadTransitionKeepsArchiveState_072', async () => {
    const legacyStore = new FakeLegacyStore()
    legacyStore.archived.set(PROJECT, [SESSION])

    const history = useNativeHistoryStore()
    const nativeAdapter = createNativeCliAdapter({
      tabs: useNativeTabsStore(), history,
      legacyClaudeRootKey: async () => ROOT,
      runtime: {
        createTab: () => { throw new Error('unused') },
        restartTab: async () => { throw new Error('unused') },
        stopTab: async () => undefined,
        renameTab: async () => undefined,
      },
      archive: { getArchivedSessions: legacyStore.getArchivedSessions.bind(legacyStore), archiveSession: async () => undefined, restoreSession: async () => undefined },
    })
    const legacyAdapter = createLegacyClaudeAdapter({
      store: legacyStore,
      runtime: { startTab: vi.fn(), stopTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn() },
      projectPaths: () => [PROJECT],
    })
    const catalog = useUnifiedSessionsStore()
    catalog.configureAdapters([nativeAdapter, legacyAdapter], { legacyClaudeRootKey: ROOT })

    clients.push({ scope: vi.fn().mockResolvedValue(nativeSource()),
      read: vi.fn().mockResolvedValue({ state: 'ready', items: [historyItem()], hasMore: false }) })
    const context = { cli: 'claude' as const, profileId: 'cx', profileRevision: '7', projectId: 'project', projectPath: PROJECT }
    await history.load(context)
    await catalog.refresh(PROJECT)
    expect(catalog.sessions.filter(row => row.projectPath === PROJECT && !row.archived)).toHaveLength(0)

    // 重载过渡态：归档状态不得翻转成未归档。
    clients.push({ scope: vi.fn(() => new Promise(() => {})), read: vi.fn() })
    void history.loadGrouped({ cli: 'claude', profileId: 'cx', profileRevision: '7',
      projects: [{ projectId: 'project', projectPath: PROJECT }] }, { force: true })
    await catalog.refresh(PROJECT)
    const leaked = catalog.sessions.filter(row => row.projectPath === PROJECT && !row.archived)
    expect(leaked.map(row => `${row.runtime}|${row.id.slice(0, 40)}`)).toEqual([])
  })
})
