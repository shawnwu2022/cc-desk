// 回归：单行状态变化后，projectGroups 必须保留未变化行的对象身份。
// 此前的缺陷：projectGroups 每次重算都对每行展开新对象，任何一行的状态变化
// 都会让所有 SessionItem 的 prop 换身份、整棵树重渲染。
import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useProjectsStateStore } from '@/stores/projectsState'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'

const PROJECT = '/repo'

function nativeEntry(): NativeHistoryEntry {
  return {
    key: 'codex:root',
    context: { cli: 'codex', profileId: 'cx', profileRevision: '7', projectId: 'project', projectPath: PROJECT },
    sessions: [
      { type: 'session', sessionKey: 'key-a', nativeSessionId: 'session-a', title: 'Alpha',
        truncated: false, archived: false, cwd: PROJECT, updatedAt: '2026-09-29T00:00:00Z' },
      { type: 'session', sessionKey: 'key-b', nativeSessionId: 'session-b', title: 'Beta',
        truncated: false, archived: false, cwd: PROJECT, updatedAt: '2026-09-28T00:00:00Z' },
    ],
    loading: false, loaded: true, error: null, requestEpoch: '1', sourceRootKey: 'root-key',
  }
}

function nativeHarness(entries: NativeHistoryEntry[]) {
  const adapter = createNativeCliAdapter({
    tabs: useNativeTabsStore(),
    history: { all: () => entries, load: async () => entries[0] },
    runtime: {
      createTab: () => { throw new Error('unused') },
      restartTab: async () => { throw new Error('unused') },
      stopTab: async () => undefined,
      renameTab: async () => undefined,
    },
    archive: { getArchivedSessions: () => [], archiveSession: async () => undefined, restoreSession: async () => undefined },
  })
  const catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([adapter])
  return catalog
}

beforeEach(() => { setActivePinia(createPinia()); useProjectsStateStore().loaded = true })

describe('projectGroups preserves unchanged row identity', () => {
  it('ProjectGroups_KeepsUnchangedRowIdentityOnScopedRefresh_070', async () => {
    const entries = [nativeEntry()]
    const catalog = nativeHarness(entries)
    await catalog.refresh()
    const before = new Map(catalog.projectGroups.flatMap(group => group.sessions).map(row => [row.title, row]))
    expect([...before.keys()].sort()).toEqual(['Alpha', 'Beta'])

    entries[0].sessions[1].updatedAt = '2026-10-01T00:00:00Z'
    await catalog.refresh(PROJECT)

    const after = new Map(catalog.projectGroups.flatMap(group => group.sessions).map(row => [row.title, row]))
    expect(after.size).toBe(2)
    expect(after.get('Beta')!.lastActivityAt).not.toBe(before.get('Beta')!.lastActivityAt)
    expect(after.get('Alpha')!.lastActivityAt).toBe(before.get('Alpha')!.lastActivityAt)
    expect(after.get('Alpha')).toBe(before.get('Alpha'))
    expect(after.get('Beta')).not.toBe(before.get('Beta'))
  })
})
