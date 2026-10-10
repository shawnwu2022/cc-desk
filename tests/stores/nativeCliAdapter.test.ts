import { createPinia, setActivePinia } from 'pinia'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { describe, expect, it, vi } from 'vitest'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { NativeCliTab } from '@/stores/nativeTabs'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'

function tab(overrides: Partial<NativeCliTab> = {}): NativeCliTab {
  return {
    tabId: 'tab-1', cli: 'codex', projectId: 'project-1', projectPath: '/repo', profileId: 'codex-main', profileRevision: '7',
    requestId: 'req-1', runId: 'run-1', generation: 1, action: { kind: 'resume-id', nativeSessionId: 'session-1' },
    status: 'running', errorCode: null, launchRevision: '7', title: 'Codex active', createdAt: 1, lastActivityAt: 30,
    ...overrides,
  }
}

function history(cli: 'claude' | 'codex', root: string, nativeSessionId: string, projectPath = '/repo'): NativeHistoryEntry {
  const profileId = cli === 'claude' ? 'claude-main' : 'codex-main'
  return {
    key: `${cli}:${root}`,
    context: { cli, profileId, profileRevision: '7', projectId: 'project-1', projectPath },
    sessions: [{ type: 'session', sessionKey: JSON.stringify(['local', cli, root, nativeSessionId]), nativeSessionId, title: `${cli}-${root}`, truncated: false, cwd: projectPath, updatedAt: '2026-09-29T00:00:00Z' }],
    loading: false, loaded: true, error: null, requestEpoch: '1',
  }
}

function setup(tabs: NativeCliTab[], entries: NativeHistoryEntry[]) {
  const map = new Map(tabs.map(value => [value.tabId, value]))
  const setActive = vi.fn()
  const close = vi.fn((id: string) => map.delete(id))
  const rename = vi.fn((id: string, title: string) => { const value = map.get(id); if (value) value.title = title })
  const runtime = {
    createTab: vi.fn((input: any) => tab({ tabId: 'created', cli: input.cli, projectPath: input.projectPath, action: input.action, title: input.title ?? 'Created' })),
    restartTab: vi.fn((id: string) => tab({ ...(map.get(id) ?? {}), tabId: id, generation: 2, status: 'stopped' })),
    stopTab: vi.fn().mockResolvedValue(undefined),
  }
  const archived = new Map<string, string[]>()
  const archive = { getArchivedSessions: (path: string) => archived.get(path) ?? [], archiveSession: vi.fn(async (path: string, id: string) => { archived.set(path, [...(archived.get(path) ?? []), id]) }), restoreSession: vi.fn(async (path: string, id: string) => { archived.set(path, (archived.get(path) ?? []).filter(value => value !== id)) }) }
  const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive, close, rename }, history: { all: () => entries }, runtime, archive })
  return { adapter, map, runtime, archive, setActive, archived }
}

describe('native CLI adapter', () => {
  // Claude 的观察侧通道不得通过可选字段叠加到 Codex、Shell 或 raw 启动。
  it.each([
    { cli: 'codex', action: { kind: 'new' } },
    { cli: 'claude', action: { kind: 'raw', argv: ['shell-like-program'] } },
    { cli: 'shell', action: { kind: 'new' } },
  ] as const)('NativeObservation_RejectsUnsupportedSource_020 $cli $action.kind', async ({ cli, action }) => {
    const value = tab({ cli: cli as NativeCliTab['cli'], action: action as NativeCliTab['action'],
      activityState: 'waiting', observationState: 'active', attentionState: 'needs-user' })
    const { adapter } = setup([value], [])
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'unknown', observationState: 'off', attentionState: 'none' })
  })
  it.each(['working', 'waiting'] as const)('NativeObservation_PreservesClaudeOrderedActivity_021 %s', async activityState => {
    const { adapter } = setup([tab({ cli: 'claude', action: { kind: 'new' }, activityState, observationState: 'active',
      attentionState: activityState === 'waiting' ? 'needs-user' : 'none' })], [])
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState, observationState: 'active',
      attentionState: activityState === 'waiting' ? 'needs-user' : 'none' })
  })
  it.each(['off', 'connecting', 'unavailable'] as const)('NativeObservation_UnreliableActivityStaysUnknown_022 %s', async observationState => {
    const { adapter } = setup([tab({ cli: 'claude', action: { kind: 'new' }, activityState: 'waiting',
      observationState, attentionState: 'needs-user' })], [])
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'unknown', attentionState: 'none', observationState })
  })
  it('mixes Claude and Codex history and preserves duplicate native ids across source roots', async () => {
    const { adapter } = setup([], [history('claude', '/claude-a', 'same'), history('codex', '/codex-a', 'same'), history('codex', '/codex-b', 'same')])
    const sessions = await adapter.listSessions('/repo')
    expect(sessions).toHaveLength(3)
    expect(new Set(sessions.map(x => x.id)).size).toBe(3)
    expect(sessions.map(x => x.cli).sort()).toEqual(['claude', 'codex', 'codex'])
  })

  it('active tab claims only exact cli/profile/project/native identity', async () => {
    const active = tab()
    const { adapter } = setup([active], [history('codex', '/root', 'session-1'), history('claude', '/root', 'session-1')])
    const sessions = await adapter.listSessions('/repo')
    expect(sessions.filter(x => x.nativeSessionId === 'session-1')).toHaveLength(2)
    expect(sessions.some(x => x.id === 'native-tab:tab-1')).toBe(true)
    expect(sessions.some(x => x.cli === 'claude')).toBe(true)
  })

  it('resuming an already-open native session activates instead of duplicating', async () => {
    const active = tab()
    const { adapter, runtime, setActive } = setup([active], [])
    const resumed = await adapter.resumeSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', adapterSessionId: 'key', nativeSessionId: 'session-1' })
    expect(resumed.id).toBe('native-tab:tab-1')
    expect(setActive).toHaveBeenCalledWith('tab-1')
    expect(runtime.createTab).not.toHaveBeenCalled()
  })

  it('rejects restart from unknown state and archives only after stop succeeds', async () => {
    const unknown = tab({ status: 'unknown' })
    const one = setup([unknown], [])
    await expect(one.adapter.restartSession('native-tab:tab-1')).rejects.toThrow('LAUNCH_STATE_UNKNOWN')

    const running = tab({ tabId: 'tab-2' })
    const entry = history('codex', '/root', 'session-1')
    const two = setup([running], [entry])
    await two.adapter.archiveSession('native-tab:tab-2')
    expect(two.runtime.stopTab).toHaveBeenCalledTimes(1)
    expect(two.archive.archiveSession).toHaveBeenCalledWith('/repo', expect.stringContaining('native-history:'), expect.any(Function))
  })
  it('NativeAdapter_ProjectsArchivedHistory_005', async () => {
    const { adapter, archived } = setup([], [history('codex', '/root', 'session-1')])
    const [record] = await adapter.listSessions('/repo')
    archived.set('/repo', [record.id])
    const sessions = await adapter.listSessions('/repo')
    expect(sessions).toHaveLength(1)
    expect(sessions[0].archived).toBe(true)
  })

  it('NativeArchive_RefreshRestore_006', async () => {
    setActivePinia(createPinia())
    const { adapter } = setup([], [history('codex', '/root', 'session-1')])
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

  it('NativeArchive_IsolatesCatalogIdentity_007', async () => {
    const { adapter, archived } = setup([], [history('claude', '/claude', 'same'), history('codex', '/root-a', 'same'), history('codex', '/root-b', 'same')])
    const before = await adapter.listSessions('/repo')
    const target = before.find(value => value.title === 'codex-/root-a')!
    await adapter.archiveSession(target.id)
    const after = await adapter.listSessions('/repo')
    expect(after.filter(value => value.archived).map(value => value.id)).toEqual([target.id])
    expect(archived.get('/repo')).not.toContain('same')
    await adapter.restoreArchivedSession(target.id)
    expect((await adapter.listSessions('/repo')).every(value => !value.archived)).toBe(true)
  })

  it('NativeResume_DistinguishesProfiles_008', async () => {
    const entry = history('codex', '/root-b', 'session-1')
    entry.context.profileId = 'profile-b'
    const { adapter, runtime, setActive } = setup([tab({ profileId: 'profile-a' })], [entry])
    await adapter.resumeSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', adapterSessionId: entry.sessions[0].sessionKey, nativeSessionId: 'session-1', launchConfigId: 'profile-b' })
    expect(runtime.createTab).toHaveBeenCalledWith(expect.objectContaining({ launchConfigId: 'profile-b' }))
    expect(setActive).not.toHaveBeenCalledWith('tab-1')
  })

  it('NativeProjection_ClaimsExactSource_009', async () => {
    const first = history('codex', '/root-a', 'session-1')
    const second = history('codex', '/root-b', 'session-1')
    const { adapter } = setup([tab({ sourceSessionKey: first.sessions[0].sessionKey })], [first, second])
    const sessions = await adapter.listSessions('/repo')
    expect(sessions.map(value => value.title).sort()).toEqual(['Codex active', 'codex-/root-b'])
  })

  it('NativeResume_DistinguishesRevision_010', async () => {
    const entry = history('codex', '/root', 'session-1')
    entry.context.profileRevision = '8'
    const { adapter, runtime, setActive } = setup([tab({ profileRevision: '7' })], [entry])
    await adapter.resumeSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', adapterSessionId: entry.sessions[0].sessionKey, nativeSessionId: 'session-1', launchConfigId: 'codex-main' })
    expect(runtime.createTab).toHaveBeenCalledTimes(1)
    expect(setActive).not.toHaveBeenCalledWith('tab-1')
  })

})
