import { createPinia, setActivePinia } from 'pinia'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { describe, expect, it, vi } from 'vitest'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { captureNativeAttempt, useNativeTabsStore, type NativeCliTab } from '@/stores/nativeTabs'
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
    sessions: [{ type: 'session', sessionKey: JSON.stringify(['local', cli, root, nativeSessionId]), nativeSessionId, title: `${cli}-${root}`, truncated: false, archived: false, cwd: projectPath, updatedAt: '2026-09-29T00:00:00Z' }],
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
  const cliUnarchive = { unarchive: vi.fn().mockResolvedValue(undefined) }
  const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive, close, rename }, history: { all: () => entries }, runtime, archive, cliUnarchive })
  return { adapter, map, runtime, archive, cliUnarchive, setActive, archived }
}

describe('native CLI adapter', () => {
  // restart 必须撤销旧 run 的 observer 身份，避免新 attempt 认领旧历史行。
  it('Restart_DropsObservedClaim_032 — restart 后旧 observer 身份不再认领历史', async () => {
    setActivePinia(createPinia())
    const tabs = useNativeTabsStore()
    const active = tabs.create({ cli: 'claude', projectId: 'project-1', projectPath: '/repo', profileId: 'claude-main', profileRevision: '7', action: { kind: 'new' } })
    tabs.applyLaunchStatus(active.tabId, {
      instanceId: 'backend', requestId: active.requestId, run: { runId: active.runId, generation: active.generation },
      phase: 'running', revision: '1', failure: null,
    })
    tabs.applyObservationIdentity(active.tabId, captureNativeAttempt(active), 'session-1')
    tabs.applyLaunchStatus(active.tabId, {
      instanceId: 'backend', requestId: active.requestId, run: { runId: active.runId, generation: active.generation },
      phase: 'exited', revision: '2', failure: null,
    })
    tabs.restart(active.tabId, { profileId: 'claude-main', profileRevision: '7' })
    const adapter = createNativeCliAdapter({
      tabs,
      history: { all: () => [history('claude', '/root', 'session-1')] },
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() },
      archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
    })

    const rows = (await adapter.listSessions()).filter(row => row.nativeSessionId === 'session-1')
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({ opened: false })
  })

  // 认证 hook 回填身份后，运行中 new 会话认领自己的历史行，消除活动/历史双行。
  it('NativeObservedIdentity_ClaimsHistoryRow_030', async () => {
    const active = tab({ cli: 'claude', profileId: 'claude-main', action: { kind: 'new' }, observedSessionId: 'session-1' })
    const { adapter } = setup([active], [history('claude', '/root', 'session-1')])
    const matched = (await adapter.listSessions()).filter(row => row.nativeSessionId === 'session-1')
    expect(matched).toHaveLength(1)
    expect(matched[0]).toMatchObject({ opened: true })
  })

  // 无观察身份时维持不猜测立场：历史行照常投出，与活动行并存。
  it('NativeObservedIdentity_WithoutIdentityKeepsRow_031', async () => {
    const active = tab({ cli: 'claude', profileId: 'claude-main', action: { kind: 'new' } })
    const { adapter } = setup([active], [history('claude', '/root', 'session-1')])
    const matched = (await adapter.listSessions()).filter(row => row.nativeSessionId === 'session-1')
    expect(matched).toHaveLength(1)
    expect(matched[0]).toMatchObject({ opened: false })
  })

  // Native 关闭后刷新冻结来源，已结束终端先移除，迟到历史进入同一次关闭完成。
  it('NativeClose_RefreshesExactHistory_011', async () => {
    const value = tab({ status: 'exited' })
    const tabs = new Map([[value.tabId, value]])
    let entries: NativeHistoryEntry[] = []
    let complete!: (entry: NativeHistoryEntry) => void
    const pending = new Promise<NativeHistoryEntry>(resolve => { complete = resolve })
    const load = vi.fn(async () => { const entry = await pending; entries = [entry]; return entry })
    const adapter = createNativeCliAdapter({ tabs: { tabs, activeTabId: value.tabId,
      setActive: vi.fn(), close: id => { tabs.delete(id) }, rename: vi.fn() },
      history: { all: () => entries, load },
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() },
      archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
    })
    let finished = false
    const closing = adapter.closeSession('native-tab:tab-1').then(() => { finished = true })
    await Promise.resolve(); await Promise.resolve()
    expect(tabs.has(value.tabId)).toBe(false)
    expect(load).toHaveBeenCalledExactlyOnceWith({ cli: 'codex', profileId: 'codex-main', profileRevision: '7', projectId: 'project-1', projectPath: '/repo', force: true, retainOnFailure: true })
    expect(finished).toBe(false)
    complete(history('codex', '/root', 'session-1')); await closing
    expect((await adapter.listSessions())[0]).toMatchObject({ opened: false, nativeSessionId: 'session-1' })
  })

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

  // 关闭成功后必须清理 acceptedOpens：条目持有整个 tab 引用（长会话缓慢累积），
  // 且 tabId 复用时旧 entry 会让新 tab 用陈旧 accepted 时间重复写 openedAt。
  it('NativeClose_CleansAcceptedOpenEntry_012', async () => {
    const created = tab()
    const tabs = new Map([[created.tabId, created]])
    const sessionRecords = new Map()
    const upsertSessionRecord = vi.fn(async (key: string, record: unknown) => { sessionRecords.set(key, record) })
    const adapter = createNativeCliAdapter({
      tabs: { tabs, activeTabId: null, setActive: vi.fn(), close: id => { tabs.delete(id) }, rename: vi.fn() },
      history: { all: () => [] },
      runtime: { createTab: vi.fn(async () => created), restartTab: vi.fn(), stopTab: vi.fn() },
      archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
      metadata: { sessionRecords, upsertSessionRecord },
    })

    await adapter.createSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', title: 'Codex active', action: { kind: 'new' } })
    expect(upsertSessionRecord).toHaveBeenCalledTimes(1)

    await adapter.closeSession('native-tab:tab-1')

    // tabId 复用探测：新 tab 对象指向另一条历史来源；entry 已清理则不得再写 openedAt
    const reused = tab({ action: { kind: 'resume-id', nativeSessionId: 'session-2' }, sourceSessionKey: JSON.stringify(['local', 'codex', '/root-2', 'session-2']) })
    tabs.set(reused.tabId, reused)
    await adapter.listSessions()
    expect(upsertSessionRecord).toHaveBeenCalledTimes(1)
  })

  // codex 自身归档容器（item.archived）的行必须标记归档；其「恢复」语义是
  // 从归档中打开会话（resume），而不是写 Desk 的归档注册表。
  // codex 拒绝 resume 自己归档的会话（要求先 `codex unarchive`），所以 CLI 归档行
  // 的恢复必须请求 CLI 取消归档，而不是开一个注定失败的 resume 标签；Desk 也不
  // 搬动 CLI 的归档文件。
  it('CodexArchive_SourceDirRowRestoresThroughCliUnarchive_033', async () => {
    setActivePinia(createPinia())
    const entry = history('codex', '/root', 'session-9')
    entry.sessions[0].archived = true
    const { adapter, runtime, archive, cliUnarchive, setActive } = setup([], [entry])
    const rows = await adapter.listSessions()
    expect(rows).toHaveLength(1)
    expect(rows[0].archived).toBe(true)
    await adapter.restoreArchivedSession(rows[0].id)
    expect(cliUnarchive.unarchive).toHaveBeenCalledTimes(1)
    expect(cliUnarchive.unarchive.mock.calls[0][1]).toBe('session-9')
    expect(archive.restoreSession).not.toHaveBeenCalled()
    expect(runtime.createTab).not.toHaveBeenCalled()
    expect(setActive).not.toHaveBeenCalled()
  })

  // 没有 CLI 取消归档能力时不得静默退化成 resume：那会开出一个必然被 CLI 拒绝的
  // 标签，用户以为恢复成功。失败必须显式暴露。
  it('CodexArchive_MissingCliUnarchiveFailsClosed_034', async () => {
    setActivePinia(createPinia())
    const entry = history('codex', '/root', 'session-9')
    entry.sessions[0].archived = true
    const adapter = createNativeCliAdapter({
      tabs: { tabs: new Map(), activeTabId: null, setActive: vi.fn(), close: vi.fn(), rename: vi.fn() },
      history: { all: () => [entry] },
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() },
      archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
    })
    const rows = await adapter.listSessions()
    await expect(adapter.restoreArchivedSession(rows[0].id)).rejects.toThrow('CLI_UNARCHIVE_UNAVAILABLE')
  })

})
