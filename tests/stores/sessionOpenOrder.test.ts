import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useSessionStore } from '@/stores/session'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNotificationsStore } from '@/stores/notifications'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import type { ProjectsState } from '@/types/app'

let time = 1000
let disk: ProjectsState
let failMetadataWrites = false
const writes: any[] = []
const source = (id: string): NativeHistoryEntry => ({ key: id,
  context: { cli: 'codex', profileId: 'cx', profileRevision: '7', projectId: 'project', projectPath: '/repo' },
  sessions: [{ type: 'session', sessionKey: JSON.stringify(['local', 'codex', 'root', id]), nativeSessionId: id,
    title: id, cwd: '/repo', updatedAt: '2026-10-10T00:00:00Z', truncated: false }],
  loaded: true, loading: false, error: null, requestEpoch: '1' })

beforeEach(() => {
  setActivePinia(createPinia()); clearMocks(); time = 1000; writes.length = 0; failMetadataWrites = false
  vi.spyOn(Date, 'now').mockImplementation(() => time)
  let uuid = 0
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => `legacy-${++uuid}` })
  disk = { pinnedProjects: [], archivedSessions: {}, displayNames: {}, sessionRecords: {} }
  mockIPC((command, args: any) => {
    if (command === 'get_projects_state') return structuredClone(disk)
    if (command === 'upsert_session_ui_record') {
      writes.push(structuredClone(args))
      if (failMetadataWrites) throw new Error('private path and transport details must not appear')
      disk.sessionRecords![args.recordKey] = structuredClone(args.record)
      return structuredClone(disk)
    }
    if (command === 'restore_session') {
      disk.archivedSessions[args.projectPath] = (disk.archivedSessions[args.projectPath] ?? []).filter(id => id !== args.sessionId)
      return structuredClone(disk)
    }
    if (command === 'get_sessions') return []
    throw new Error(`unexpected IPC ${command}`)
  })
})
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); clearMocks() })

function native(entries = [source('a'), source('b')]) {
  const tabs = useNativeTabsStore(), projects = useProjectsStateStore(), catalog = useUnifiedSessionsStore()
  const adapter = createNativeCliAdapter({ tabs, metadata: projects,
    history: { all: () => [{ ...entries[0], sessions: entries.flatMap(entry => entry.sessions) }] },
    archive: { getArchivedSessions: path => projects.archivedSessions.get(path) ?? [],
      archiveSession: projects.archiveSession, restoreSession: projects.restoreSession },
    runtime: {
      createTab(input) { return tabs.create({ cli: input.cli, projectId: 'project', projectPath: input.projectPath,
        profileId: 'cx', profileRevision: '7', action: input.action, sourceSessionKey: input.sourceSessionKey, title: input.title }) },
      restartTab: vi.fn(), stopTab: async tab => { tabs.tabs.get(tab.tabId)!.status = 'stopped' },
    } })
  catalog.configureAdapters([adapter])
  return { tabs, projects, catalog, adapter, entries }
}
const ids = (catalog: ReturnType<typeof useUnifiedSessionsStore>) => catalog.projectGroups[0].sessions.map(row => row.nativeSessionId ?? row.id)

describe('session order follows accepted opens instead of background activity', () => {
  it('keeps admitted runtimes after a failed metadata save and reports it safely without automatic retries', async () => {
    const { catalog, tabs } = native()
    await catalog.initialize(); failMetadataWrites = true
    const opened = await catalog.resumeCatalogSession(catalog.sessions.find(row => row.nativeSessionId === 'a')!)
    expect(tabs.tabs.has(opened.adapterSessionId)).toBe(true)
    expect(catalog.activeSessionId).toBe(opened.id)
    const notices = useNotificationsStore().toasts
    expect(notices).toMatchObject([{ kind: 'warning', messageKey: 'feedbackSessionOpenTimeNotSaved' }])
    expect(JSON.stringify(notices)).not.toContain('private path')
    for (let index = 0; index < 3; index++) {
      tabs.tabs.get(opened.adapterSessionId)!.lastActivityAt = ++time
      await catalog.refresh()
    }
    expect(writes).toHaveLength(1)
    expect(useNotificationsStore().toasts).toHaveLength(1)
    expect(disk.sessionRecords).toEqual({})
  })

  it('keeps parallel output, status and completed events in place without stealing selection', async () => {
    const { tabs, catalog } = native()
    await catalog.initialize()
    const a = await catalog.resumeCatalogSession(catalog.sessions.find(row => row.nativeSessionId === 'a')!)
    time = 2000
    const b = await catalog.resumeCatalogSession(catalog.sessions.find(row => row.nativeSessionId === 'b')!)
    expect(ids(catalog)).toEqual(['b', 'a'])
    await catalog.activateSession(a.id)
    for (let index = 0; index < 4; index++) {
      time += 1000
      const tab = tabs.tabs.get(index % 2 ? a.adapterSessionId : b.adapterSessionId)!
      tab.lastActivityAt = time; tab.status = index === 3 ? 'exited' : 'running'
      tab.activityState = index % 2 ? 'working' : 'waiting'
      await catalog.refresh()
      expect(ids(catalog)).toEqual(['b', 'a'])
      expect(catalog.activeSessionId).toBe(a.id)
    }
    expect(catalog.sessions.find(row => row.id === a.id)).toMatchObject({ lastOpenedAt: 1000 })
    expect(writes).toHaveLength(2)
  })

  it('persists reopen time for the exact source through close, refresh and application restart', async () => {
    const first = native()
    await first.catalog.initialize()
    const a = await first.catalog.resumeCatalogSession(first.catalog.sessions.find(row => row.nativeSessionId === 'a')!)
    time = 2000
    await first.catalog.resumeCatalogSession(first.catalog.sessions.find(row => row.nativeSessionId === 'b')!)
    await first.catalog.closeSession(a.id)
    time = 3000
    await first.catalog.resumeCatalogSession(first.catalog.sessions.find(row => row.nativeSessionId === 'a')!)
    expect(ids(first.catalog)).toEqual(['a', 'b'])
    setActivePinia(createPinia())
    const restarted = native()
    await restarted.catalog.initialize()
    expect(ids(restarted.catalog)).toEqual(['a', 'b'])
    expect(restarted.catalog.sessions.find(row => row.nativeSessionId === 'a')).toMatchObject({ lastOpenedAt: 3000 })
    time = 9000; restarted.entries[1].sessions[0].updatedAt = '2026-10-11T00:00:00Z'
    await restarted.catalog.refresh()
    expect(ids(restarted.catalog)).toEqual(['a', 'b'])
    expect(writes).toHaveLength(3)
  })

  it('new sessions move forward but switching an already open terminal does not', async () => {
    const { catalog } = native()
    await catalog.initialize()
    const a = await catalog.resumeCatalogSession(catalog.sessions.find(row => row.nativeSessionId === 'a')!)
    time = 2000
    const created = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', action: { kind: 'new' } })
    expect(catalog.projectGroups[0].sessions[0].id).toBe(created.id)
    time = 3000; await catalog.activateSession(a.id); await catalog.refresh()
    expect(catalog.projectGroups[0].sessions[0].id).toBe(created.id)
    expect(catalog.activeSessionId).toBe(a.id)
    expect(writes).toHaveLength(2)
  })

  it('restoring an archived history row moves it forward without opening or selecting its terminal', async () => {
    const { catalog, projects } = native()
    await catalog.initialize()
    await catalog.resumeCatalogSession(catalog.sessions.find(row => row.nativeSessionId === 'b')!)
    const a = catalog.sessions.find(row => row.nativeSessionId === 'a')!
    disk.archivedSessions['/repo'] = [a.id]; await projects.reload(); await catalog.refresh()
    const selected = catalog.activeSessionId
    time = 4000; await catalog.restoreArchivedSession(a.id)
    expect(ids(catalog)).toEqual(['a', 'b'])
    expect(catalog.activeSessionId).toBe(selected)
    expect(catalog.sessions.find(row => row.id === a.id)).toMatchObject({ opened: false, lastOpenedAt: 4000 })
  })

  it('uses a fixed missing-history fallback and stable identity for equal opening times', async () => {
    const { catalog, entries } = native([source('z'), source('a')])
    entries[0].sessions[0].updatedAt = '2027-01-01T00:00:00Z'
    await catalog.initialize()
    expect(ids(catalog)).toEqual(['a', 'z'])
    entries.reverse(); entries[1].sessions[0].updatedAt = '2028-01-01T00:00:00Z'
    await catalog.refresh()
    expect(ids(catalog)).toEqual(['a', 'z'])
    const a = await catalog.resumeCatalogSession(catalog.sessions.find(row => row.nativeSessionId === 'a')!)
    const z = await catalog.resumeCatalogSession(catalog.sessions.find(row => row.nativeSessionId === 'z')!)
    const expected = [a.id, z.id].sort()
    expect(catalog.projectGroups[0].sessions.map(row => row.id)).toEqual(expected)
    expect(writes).toHaveLength(2)
  })

  it('transfers a newly accepted legacy open time when its native history identity arrives later', async () => {
    const legacy = useSessionStore(), projects = useProjectsStateStore(), catalog = useUnifiedSessionsStore()
    await projects.ensureLoaded()
    const adapter = createLegacyClaudeAdapter({ store: legacy, metadata: projects, projectPaths: () => ['/repo'],
      runtime: { startTab: async () => {}, stopTab: async () => {}, restartTab: async () => {}, renameTab: async () => {} } })
    const opened = await adapter.createSession({ cli: 'claude', projectPath: '/repo', projectKey: '/repo' })
    time = 9000; legacy.setTabSessionId(opened.adapterSessionId, 'late-history-id')
    catalog.configureAdapters([adapter]); await catalog.refresh()
    expect(catalog.sessions[0]).toMatchObject({ lastOpenedAt: 1000 })
    const saved = Object.values(disk.sessionRecords!).find(row => row.nativeSessionId === 'late-history-id')
    expect(saved).toMatchObject({ lastOpenedAt: 1000 })
    await catalog.refresh()
    expect(writes).toHaveLength(2)
  })
})
