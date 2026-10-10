import { createPinia, setActivePinia } from 'pinia'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { describe, expect, it, vi } from 'vitest'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import type { HistorySession, TerminalTab } from '@/stores/session'
import type {
  LegacyClaudeRuntimePort,
  LegacyClaudeStorePort,
} from '@/session/adapters/legacyClaudeAdapter'

class FakeLegacyStore implements LegacyClaudeStorePort {
  readonly tabs = new Map<string, TerminalTab>()
  readonly history = new Map<string, HistorySession[]>()
  readonly archived = new Map<string, string[]>()
  readonly events: string[] = []
  private nextId = 1
  activeTabId: string | null = null

  getCatalogHistoryFor(projectPath: string): HistorySession[] {
    return this.history.get(projectPath.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()) ?? []
  }

  getArchivedSessions(projectPath: string): string[] {
    return this.archived.get(projectPath.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()) ?? []
  }

  createTab(projectPath: string, opts?: { sessionId?: string; name?: string }): string {
    const tabId = `tab-${this.nextId++}`
    this.tabs.set(tabId, {
      tabId,
      projectPath,
      ptyId: null,
      sessionId: opts?.sessionId ?? null,
      name: opts?.name ?? 'New Session',
      status: 'stopped',
      createdAt: 10,
      lastActiveAt: 10,
      working: false,
      pending: false,
      isResume: Boolean(opts?.sessionId),
      cli: 'claude',
    })
    this.events.push(`create:${tabId}`)
    return tabId
  }

  setActiveTab(tabId: string | null): void {
    this.activeTabId = tabId
    this.events.push(`activate:${tabId}`)
  }

  removeTab(tabId: string): void {
    this.tabs.delete(tabId)
    this.events.push(`remove:${tabId}`)
  }

  async closeTab(tabId: string): Promise<void> {
    this.tabs.delete(tabId)
    this.events.push(`close:${tabId}`)
  }

  updateTabName(tabId: string, name: string): void {
    const tab = this.tabs.get(tabId)
    if (tab) tab.name = name
    this.events.push(`rename:${tabId}:${name}`)
  }

  async archiveSession(projectPath: string, sessionId: string): Promise<void> {
    const key = projectPath.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
    this.archived.set(key, [...(this.archived.get(key) ?? []), sessionId])
    this.events.push(`archive:${projectPath}:${sessionId}`)
  }

  async restoreSession(projectPath: string, sessionId: string): Promise<void> {
    const key = projectPath.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
    this.archived.set(key, (this.archived.get(key) ?? []).filter(id => id !== sessionId))
    this.events.push(`restore:${projectPath}:${sessionId}`)
  }
}

function runtime(store: FakeLegacyStore): LegacyClaudeRuntimePort {
  return {
    async startTab(tabId) {
      store.events.push(`start:${tabId}`)
      const tab = store.tabs.get(tabId)
      if (tab) {
        tab.status = 'running'
        tab.ptyId = `pty-${tabId}`
      }
    },
    async stopTab(tabId) {
      store.events.push(`stop:${tabId}`)
      const tab = store.tabs.get(tabId)
      if (tab) {
        tab.status = 'stopped'
        tab.ptyId = null
      }
    },
    async restartTab(tabId) {
      store.events.push(`restart:${tabId}`)
      const tab = store.tabs.get(tabId)
      if (tab) {
        tab.status = 'running'
        tab.ptyId = `pty-restart-${tabId}`
      }
    },
    async renameTab(tabId, title) {
      store.events.push(`runtime-rename:${tabId}:${title}`)
    },
  }
}

function activeTab(overrides: Partial<TerminalTab> = {}): TerminalTab {
  return {
    tabId: 'tab-active',
    projectPath: 'C:\\Work\\Game\\',
    ptyId: 'pty-active',
    sessionId: 'claimed-session',
    name: 'Active Claude',
    status: 'running',
    createdAt: 100,
    lastActiveAt: 300,
    working: false,
    pending: true,
    isResume: false,
    cli: 'claude',
    ...overrides,
  }
}

describe('legacy Claude adapter', () => {
  it('projects active tabs and unclaimed history into one normalized project', async () => {
    const store = new FakeLegacyStore()
    store.tabs.set('tab-active', activeTab())
    store.history.set('c:/work/game', [
      { sessionId: 'claimed-session', name: 'Duplicate', projectPath: 'c:/work/game', lastActiveAt: 250 },
      { sessionId: 'history-1', name: 'Historical', projectPath: 'c:/work/game', lastActiveAt: 200 },
      { sessionId: 'archived-1', name: 'Archived', projectPath: 'c:/work/game', lastActiveAt: 150 },
    ])
    store.archived.set('c:/work/game', ['archived-1'])

    const adapter = createLegacyClaudeAdapter({
      store,
      runtime: runtime(store),
      projectPaths: () => ['C:\\Work\\Game\\'],
    })

    const sessions = await adapter.listSessions('c:/work/game')

    expect(sessions.map(session => session.id)).toEqual([
      'legacy-tab:tab-active',
      'legacy-history:c:/work/game:archived-1',
      'legacy-history:c:/work/game:history-1',
    ])
    expect(sessions.find(row => row.id === 'legacy-history:c:/work/game:archived-1')!.archived).toBe(true)
    expect(sessions[0]).toMatchObject({
      projectKey: 'c:/work/game',
      runtime: 'legacy-claude',
      cli: 'claude',
      processState: 'running',
      attentionState: 'needs-user',
      resumable: true,
      nativeSessionId: 'claimed-session',
    })
    expect(sessions.find(row => row.id === 'legacy-history:c:/work/game:history-1')).toMatchObject({
      processState: 'stopped',
      attentionState: 'none',
      resumable: true,
      adapterSessionId: 'history-1',
    })
  })

  it('creates and resumes tabs through the runtime without duplicating an open session', async () => {
    const store = new FakeLegacyStore()
    const run = runtime(store)
    const adapter = createLegacyClaudeAdapter({
      store,
      runtime: run,
      projectPaths: () => ['/work/game'],
    })

    const created = await adapter.createSession({
      projectKey: '/work/game',
      projectPath: '/work/game',
      cli: 'claude',
      title: 'New Claude',
    })
    expect(created.id).toBe('legacy-tab:tab-1')
    expect(store.events).toEqual(['create:tab-1', 'activate:tab-1', 'start:tab-1'])

    const open = store.tabs.get('tab-1')!
    open.sessionId = 'native-1'
    store.events.length = 0
    const resumed = await adapter.resumeSession({
      projectKey: '/work/game',
      projectPath: '/work/game',
      cli: 'claude',
      adapterSessionId: 'native-1',
      nativeSessionId: 'native-1',
      title: 'Existing',
    })

    expect(resumed.id).toBe('legacy-tab:tab-1')
    expect(store.events).toEqual(['activate:tab-1'])
    expect(store.tabs.size).toBe(1)
  })

  it('archives a running tab only after stop succeeds', async () => {
    const store = new FakeLegacyStore()
    store.tabs.set('tab-active', activeTab({ projectPath: '/work/game' }))
    const run = runtime(store)
    const adapter = createLegacyClaudeAdapter({ store, runtime: run, projectPaths: () => ['/work/game'] })

    await adapter.archiveSession('legacy-tab:tab-active')
    expect(store.events).toEqual([
      'stop:tab-active',
      'archive:/work/game:claimed-session',
      'close:tab-active',
    ])

    const failedStore = new FakeLegacyStore()
    failedStore.tabs.set('tab-active', activeTab({ projectPath: '/work/game' }))
    const failingRuntime = runtime(failedStore)
    failingRuntime.stopTab = vi.fn().mockRejectedValue(new Error('stop failed'))
    const failingAdapter = createLegacyClaudeAdapter({
      store: failedStore,
      runtime: failingRuntime,
      projectPaths: () => ['/work/game'],
    })

    await expect(failingAdapter.archiveSession('legacy-tab:tab-active')).rejects.toThrow('stop failed')
    expect(failedStore.events).toEqual([])
  })

  it('delegates activate, stop, restart, rename, close, and archived restore', async () => {
    const store = new FakeLegacyStore()
    store.tabs.set('tab-active', activeTab({ projectPath: '/work/game', pending: false }))
    const adapter = createLegacyClaudeAdapter({
      store,
      runtime: runtime(store),
      projectPaths: () => ['/work/game'],
    })

    await adapter.activateSession('legacy-tab:tab-active')
    await adapter.stopSession('legacy-tab:tab-active')
    const restarted = await adapter.restartSession('legacy-tab:tab-active')
    await adapter.renameSession('legacy-tab:tab-active', 'Renamed')
    await adapter.closeSession('legacy-tab:tab-active')
    await adapter.restoreArchivedSession('legacy-history:/work/game:archived-1')

    expect(restarted.processState).toBe('running')
    expect(store.events).toEqual([
      'activate:tab-active',
      'stop:tab-active',
      'restart:tab-active',
      'rename:tab-active:Renamed',
      'stop:tab-active',
      'close:tab-active',
      'restore:/work/game:archived-1',
    ])
  })
  it('LegacyArchive_RefreshRestore_005', async () => {
    setActivePinia(createPinia())
    const source = new FakeLegacyStore()
    source.history.set('/repo', [{ sessionId: 'history', name: 'Old', projectPath: '/repo', lastActiveAt: 10 }])
    const adapter = createLegacyClaudeAdapter({ store: source, runtime: runtime(source), projectPaths: () => ['/repo'] })
    const store = useUnifiedSessionsStore()
    store.configureAdapters([adapter])
    await store.refresh()
    const id = store.sessions[0].id
    await store.archiveSession(id)
    await store.refresh()
    expect(store.sessions[0].archived).toBe(true)
    expect(store.projectGroups).toEqual([])
    await store.restoreArchivedSession(id)
    expect(store.projectGroups[0].sessions[0].archived).toBe(false)
  })

})
