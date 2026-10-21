import { createPinia, setActivePinia } from 'pinia'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { describe, expect, it, vi } from 'vitest'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import { captureNativeAttempt, useNativeTabsStore, type NativeCliTab } from '@/stores/nativeTabs'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import { deriveSessionVisualState } from '@/utils/sessionPresentation'

function tab(overrides: Partial<NativeCliTab> = {}): NativeCliTab {
  return {
    tabId: 'tab-1', cli: 'codex', projectId: 'project-1', projectPath: '/repo', profileId: 'codex-main', profileRevision: '7',
    requestId: 'req-1', runId: 'run-1', generation: 1, action: { kind: 'resume-id', nativeSessionId: 'session-1' },
    status: 'running', errorCode: null, launchRevision: '7', title: 'Codex active', createdAt: 1, lastActivityAt: 30,
    ...overrides,
  }
}

function history(cli: 'claude' | 'codex' | 'opencode', root: string, nativeSessionId: string, projectPath = '/repo'): NativeHistoryEntry {
  const profileId = cli === 'claude' ? 'claude-main' : cli === 'codex' ? 'codex-main' : 'opencode-main'
  return {
    key: `${cli}:${root}`,
    context: { cli, profileId, profileRevision: '7', projectId: 'project-1', projectPath },
    sessions: [{ type: 'session', sessionKey: JSON.stringify(['local', cli, root, nativeSessionId]), nativeSessionId, title: `${cli}-${root}`, truncated: false, archived: false, cwd: projectPath, updatedAt: '2026-09-29T00:00:00Z' }],
    loading: false, loaded: true, error: null, requestEpoch: '1',
  }
}

it('CodexArchive_RestoreDoesNotValidateClaude_050', async () => {
  setActivePinia(createPinia())
  const entry = history('codex', '/root', 'session-9'); entry.sourceRootKey = '/root'
  const root = vi.fn().mockRejectedValue(new Error('unrelated unavailable'))
  const { adapter, archived, archive } = setup([], [entry], root)
  const row = (await adapter.listSessions(undefined, 'codex'))[0]
  archived.set('/repo', [row.id])
  await adapter.restoreArchivedSession(row.id)
  expect(root).not.toHaveBeenCalled()
  expect(archive.restoreSession).toHaveBeenCalledOnce()
  expect(archived.get('/repo')).toEqual([])
})

it.each(['claude', 'codex'] as const)('History_OpenedOwnerShowsCurrentReadFailure_051 %s', async cli => {
  const entry = history(cli, 'root', 'session-1'); entry.sourceRootKey = 'root'
  entry.sessions[0].readFailure = 'SOURCE_INVALID_TEXT'
  const active = tab({ cli, profileId: entry.context.profileId, sourceSessionKey: entry.sessions[0].sessionKey })
  const entries = [entry], { adapter } = setup([active], entries)
  let rows = await adapter.listSessions()
  expect(rows).toHaveLength(1)
  expect(rows[0]).toMatchObject({ opened: true, historyReadFailure: 'SOURCE_INVALID_TEXT' })
  delete entry.sessions[0].readFailure
  expect((await adapter.listSessions())[0].historyReadFailure).toBeUndefined()
  entry.sessions[0].readFailure = 'SOURCE_INVALID_TEXT'; await adapter.listSessions()
  entries.splice(0)
  rows = await adapter.listSessions()
  expect(rows[0].historyReadFailure).toBeUndefined()
  expect(rows[0].title).toBe(`${cli}-root`)
})

it.each(['loading', 'error', 'unloaded'] as const)('History_RestorePreflightsEveryExactAlias_052 %s', async failure => {
  const first = history('codex', 'root', 'same'), second = history('codex', 'root', 'same')
  first.sourceRootKey = second.sourceRootKey = 'root'; second.context.profileId = 'other-profile'
  const { adapter, archived, archive } = setup([], [first, second])
  const rows = await adapter.listSessions(undefined, 'codex'), keys = rows.map(row => row.id)
  archived.set('/repo', keys)
  if (failure === 'loading') second.loading = true
  else if (failure === 'error') second.error = 'SOURCE_UNAVAILABLE'
  else second.loaded = false
  await expect(adapter.restoreArchivedSession(rows[0].id)).rejects.toThrow('STALE_SESSION_ATTEMPT')
  expect(archive.restoreSession).not.toHaveBeenCalled()
  expect(archived.get('/repo')).toEqual(keys)
})

function setup(tabs: NativeCliTab[], entries: NativeHistoryEntry[], legacyRoot?: () => Promise<string | undefined>) {
  const map = new Map(tabs.map(value => [value.tabId, value]))
  const setActive = vi.fn()
  const close = vi.fn((id: string) => map.delete(id))
  const rename = vi.fn((id: string, title: string) => { const value = map.get(id); if (value) value.title = title })
  const runtime = {
    createTab: vi.fn((input: any) => tab({ tabId: 'created', cli: input.cli, projectPath: input.projectPath, action: input.action, title: input.title ?? 'Created' })),
    restartTab: vi.fn((id: string) => tab({ ...(map.get(id) ?? {}), tabId: id, generation: 2, status: 'stopped' })),
    stopTab: vi.fn().mockResolvedValue(undefined),
    renameTab: vi.fn().mockResolvedValue(undefined),
  }
  const archived = new Map<string, string[]>()
  const archive = { getArchivedSessions: (path: string) => archived.get(path) ?? [], archiveSession: vi.fn(async (path: string, id: string) => { archived.set(path, [...(archived.get(path) ?? []), id]) }), restoreSession: vi.fn(async (path: string, id: string, beforeMutation?: () => void, prepareMutation?: () => Promise<void>) => { if (prepareMutation) await prepareMutation(); beforeMutation?.(); archived.set(path, (archived.get(path) ?? []).filter(value => value !== id)) }) }
  const cliUnarchive = { unarchive: vi.fn().mockResolvedValue(undefined) }
  const historyLoad = vi.fn(async () => entries[0])
  const metadata = { sessionRecords: new Map(), recordSessionOpened: vi.fn(async () => undefined), upsertSessionRecord: vi.fn(async (_key: string, _record: any, _beforeMutation?: () => void) => undefined) }
  const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive, close, rename }, history: { all: () => entries, load: historyLoad }, runtime, archive, cliUnarchive, metadata, legacyClaudeRootKey:legacyRoot })
  return { adapter, map, runtime, archive, cliUnarchive, setActive, archived, historyLoad, metadata }
}

// 实际两个 adapter 使用不同历史键；原生行不能接管未知来源的 Legacy 归档或恢复。
it('History_KeepLegacyArchiveOwner_037', async () => {
  setActivePinia(createPinia())
  const native = setup([], [history('claude', 'independent-root', 'session-1')])
  native.archived.set('/repo', ['session-1'])
  const restore = vi.fn(async () => undefined)
  const legacy = createLegacyClaudeAdapter({
    store: { tabs: new Map(), getCatalogHistoryFor: () => [{ sessionId: 'session-1', name: 'Legacy archived', projectPath: '/repo', lastActiveAt: 1 }],
      getArchivedSessions: () => ['session-1'], createTab: vi.fn(), setActiveTab: vi.fn(), removeTab: vi.fn(), closeTab: vi.fn(), updateTabName: vi.fn(), archiveSession: vi.fn(), restoreSession: restore },
    runtime: { startTab: vi.fn(), stopTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn() }, projectPaths: () => ['/repo'],
  })
  const catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([legacy, native.adapter])
  await catalog.refresh()
  const archived = catalog.sessions.find(row => row.runtime === 'legacy-claude')
  expect(archived).toMatchObject({ archived: true, title: 'Legacy archived' })
  expect(catalog.sessions.find(row => row.runtime === 'native-cli')).toMatchObject({ archived: false })
  await catalog.restoreArchivedSession(archived!.id)
  expect(restore).toHaveBeenCalledWith('/repo', 'session-1', expect.any(Function), expect.any(Function))
  expect(native.archive.restoreSession).not.toHaveBeenCalled()
})

// 已恢复 tab 的 sourceSessionKey 仍应从唯一来源刷新 CLI 名称；Desk 自定义名保持优先。
it('History_ResumeFollowsCliTitle_038', async () => {
  const entry = history('claude', 'root', 'session-1')
  const active = tab({ cli: 'claude', profileId: 'claude-main', sourceSessionKey: entry.sessions[0].sessionKey, title: 'Old CLI title' })
  const state = setup([active], [entry])
  entry.sessions[0].title = 'New CLI title'
  expect((await state.adapter.listSessions())[0].title).toBe('New CLI title')
})

// 认证身份稍后出现时，已保存的 tab 显示名不能被新发现的 CLI 标题覆盖。
it.each([false, true])('History_PreserveEarlyTabRename_039 identity known: %s', async identityKnown => {
  const active = tab({ cli: 'claude', profileId: 'claude-main', action: { kind: 'new' }, title: 'Claude Code',
    ...(identityKnown ? { observedSessionId: 'session-1' } : {}) })
  const entries: NativeHistoryEntry[] = []
  const state = setup([active], entries)
  state.metadata.upsertSessionRecord.mockImplementation(async (key: string, record: any, beforeMutation?: () => void) => {
    beforeMutation?.(); state.metadata.sessionRecords.set(key, record)
  })
  const original=(await state.adapter.listSessions())[0]
  state.metadata.sessionRecords.set(original.id,{runtime:original.runtime,cli:original.cli,projectPath:original.projectPath,adapterSessionId:original.adapterSessionId,nativeSessionId:original.nativeSessionId,title:'Saved before identity',lastActivityAt:original.lastActivityAt})
  active.observedSessionId = 'session-1'
  entries.push(history('claude', 'root', 'session-1'))
  expect((await state.adapter.listSessions())[0].title).toBe('Saved before identity')
  entries[0].sessions[0].title = 'Later CLI title'
  expect((await state.adapter.listSessions())[0].title).toBe('Saved before identity')
  state.map.delete(active.tabId)
  const closed = (await state.adapter.listSessions())[0]
  expect(closed.title).toBe('Later CLI title')
  state.metadata.sessionRecords.set(closed.id,{runtime:closed.runtime,cli:closed.cli,projectPath:closed.projectPath,adapterSessionId:closed.adapterSessionId,nativeSessionId:closed.nativeSessionId,title:'Exact history name',lastActivityAt:closed.lastActivityAt})
  state.map.set(active.tabId, active)
  expect((await state.adapter.listSessions())[0].title).toBe('Exact history name')
})

describe('native CLI adapter', () => {
  // 关闭历史投影不应把同一已打开 attempt 的 CLI 标题退回占位名；新 attempt 不继承。
  it.each(['generation', 'profile', 'project', 'session', 'root', 'replacement'] as const)('History_ActiveTitleSurvivesHistoryRemoval_043 %s', async change => {
    const entry = history('codex', 'root', 'session-1'); entry.sourceRootKey = 'root'
    entry.sessions[0].title = 'Native renamed title'
    const active = tab({ sourceSessionKey:entry.sessions[0].sessionKey, title:'Codex CLI' })
    const entries = [entry], state = setup([active], entries)
    expect((await state.adapter.listSessions())[0].title).toBe('Native renamed title')
    entries.length = 0
    expect((await state.adapter.listSessions())[0].title).toBe('Native renamed title')
    if (change === 'generation') active.generation++
    if (change === 'profile') active.profileRevision = '8'
    if (change === 'project') active.projectId = 'replacement-project'
    if (change === 'session') active.sourceSessionKey = 'replacement-session'
    if (change === 'root') entries.push({ ...entry, sourceRootKey: 'replacement-root', sessions: [] })
    if (change === 'replacement') state.map.set(active.tabId, { ...active })
    expect((await state.adapter.listSessions())[0].title).toBe('Codex CLI')
  })
  // 恢复同一物理记录时先收集全部精确归档别名，不能刷新后又被另一 profile 的 key 归档。
  it('History_RestoreAllProvenPhysicalArchiveKeys_044', async () => {
    const a = history('claude','root','session-1'); a.sourceRootKey = 'root'
    const b = { ...a, key:'other-profile',context:{ ...a.context,profileId:'other-profile' } }
    const state = setup([], [a,b])
    const rows = await state.adapter.listSessions()
    for (const row of rows) await state.adapter.archiveSession(row.id)
    await state.adapter.restoreArchivedSession(rows[0].id)
    expect((await state.adapter.listSessions()).every(row => !row.archived)).toBe(true)
    expect(state.archive.restoreSession).toHaveBeenCalledTimes(2)
  })
  // 历史归档的显示证据保留，但失效 lease 不能授权恢复写入。
  it('History_RetiredLegacyArchiveProofIsDisplayOnly_045', async () => {
    const entry = history('claude','root','session-1'); entry.sourceRootKey = 'root'
    const root = vi.fn<() => Promise<string | undefined>>().mockResolvedValue('root')
    const state = setup([], [entry],root); state.archived.set('/repo',['session-1'])
    const row = (await state.adapter.listSessions())[0]; expect(row.archived).toBe(true)
    root.mockResolvedValue(undefined)
    expect((await state.adapter.listSessions())[0]).toMatchObject({ archived:true,safeErrorCode:'SOURCE_UNAVAILABLE' })
    await expect(state.adapter.restoreArchivedSession(row.id)).rejects.toThrow('SESSION_ORIGIN_AMBIGUOUS')
    expect(state.archive.restoreSession).not.toHaveBeenCalled()
  })
  // 同 root/UUID 也不能把 Claude 的裸归档键套给另一种 CLI。
  it('History_LegacyBareArchiveDoesNotCrossCli_046', async () => {
    const entry = history('codex','root','session-1'); entry.sourceRootKey = 'root'
    const state = setup([], [entry],async () => 'root'); state.archived.set('/repo',['session-1'])
    const row = (await state.adapter.listSessions())[0]
    expect(row.archived).toBe(false)
    await state.adapter.restoreArchivedSession(row.id)
    expect(state.archive.restoreSession).not.toHaveBeenCalled()
  })
  it('History_LegacyBareArchiveSharesProvenPhysicalProfiles_047', async () => {
    const a = history('claude', 'root', 'session-1'); a.sourceRootKey = 'root'
    const b = { ...a, key: 'other-profile', context: { ...a.context, profileId: 'other-profile' } }
    const state = setup([], [a, b], async () => 'root'); state.archived.set('/repo', ['session-1'])
    const rows = await state.adapter.listSessions()
    expect(rows.every(row => row.archived && !row.safeErrorCode)).toBe(true)
    await state.adapter.restoreArchivedSession(rows[0].id)
    expect((await state.adapter.listSessions()).every(row => !row.archived)).toBe(true)
  })
  it.each(['missing', 'loading', 'error', 'root'] as const)('History_QueuedRestoreDoesNotClearRetiredSource_048 %s', async change => {
    const entry = history('claude', 'root', 'session-1'); entry.sourceRootKey = 'root'
    const entries = [entry], state = setup([], entries)
    const row = (await state.adapter.listSessions())[0]
    await state.adapter.archiveSession(row.id)
    let release!: () => void
    const queued = new Promise<void>(resolve => { release = resolve })
    const restore = state.archive.restoreSession.getMockImplementation()!
    state.archive.restoreSession.mockImplementation(async (path, key, beforeMutation?: () => void, prepareMutation?: () => Promise<void>) => {
      await queued; await restore(path, key, beforeMutation, prepareMutation)
    })
    const restoring = state.adapter.restoreArchivedSession(row.id)
    await vi.waitFor(() => expect(state.archive.restoreSession).toHaveBeenCalledOnce())
    if (change === 'missing') entries.length = 0
    if (change === 'loading') entry.loading = true
    if (change === 'error') entry.error = 'SOURCE_UNAVAILABLE'
    if (change === 'root') entry.sourceRootKey = 'replacement-root'
    release()
    await expect(restoring).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(state.archived.get('/repo')).toEqual([row.id])
  })
  it('History_RootRecheckCannotBorrowAnotherProfileOwner_049', async () => {
    const a = history('claude', 'root', 'session-1'); a.sourceRootKey = 'root'
    const b = { ...a, key: 'other-profile', context: { ...a.context, profileId: 'other-profile' } }
    let release!: (root: string) => void
    const rootCheck = new Promise<string>(resolve => { release = resolve })
    const root = vi.fn<() => Promise<string | undefined>>().mockResolvedValueOnce('root').mockResolvedValueOnce('root').mockReturnValue(rootCheck)
    const entries = [a, b], state = setup([], entries, root); state.archived.set('/repo', ['session-1'])
    const row = (await state.adapter.listSessions())[0]
    const restoring = state.adapter.restoreArchivedSession(row.id)
    await vi.waitFor(() => expect(root).toHaveBeenCalledTimes(3))
    entries.shift(); release('root')
    await expect(restoring).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(state.archived.get('/repo')).toEqual(['session-1'])
  })
  // 容量满时不淘汰旧未读；同一完整来源的新回执仍可替换，并拒绝旧快照清除。
  it('Notice_CacheBoundAndExactClear_033', async () => {
    const map = new Map<string, NativeCliTab>(), entries: NativeHistoryEntry[] = []
    const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive() {}, rename() {}, close: id => { map.delete(id) }, hasOwnedObservationNotice: () => true },
      history: { all: () => entries }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: async () => {}, renameTab: async () => {} },
      archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
    for (let n = 0; n < 1025; n++) {
      const entry = history('claude', 'root', 'session-' + n); entries.push(entry)
      const value = tab({ tabId: 'tab-' + n, cli: 'claude', profileId: 'claude-main', sourceSessionKey: entry.sessions[0].sessionKey, action: { kind: 'resume-id', nativeSessionId: 'session-' + n } })
      const receipt = { kind: 'reply-ended' as const, eventId: 'reply-' + n, receivedAt: n, runId: value.runId, generation: value.generation }
      value.observationNotice = { recent: receipt, unreadReplyEnd: receipt }
      map.set(value.tabId, value)
      await adapter.closeSession('native-tab:' + value.tabId)
    }
    const rows = await adapter.listSessions()
    expect(rows.filter(row => row.observationNotice?.unreadReplyEnd)).toHaveLength(1024)
    expect(rows.find(row => row.nativeSessionId === 'session-0')!.observationNotice?.unreadReplyEnd?.eventId).toBe('reply-0')
    expect(rows.find(row => row.nativeSessionId === 'session-1024')!.observationNotice).toBeUndefined()
    const old = rows.find(row => row.nativeSessionId === 'session-0')!
    const value = tab({ cli: 'claude', profileId: 'claude-main', sourceSessionKey: entries[0].sessions[0].sessionKey, action: { kind: 'resume-id', nativeSessionId: 'session-0' }, runId: 'run-2', generation: 2 })
    const receipt = { kind: 'reply-ended' as const, eventId: 'replacement', receivedAt: 2000, runId: value.runId, generation: 2 }
    value.observationNotice = { recent: receipt, unreadReplyEnd: receipt }; map.set(value.tabId, value)
    await adapter.closeSession('native-tab:' + value.tabId)
    expect(() => adapter.clearUnread!(old.id, old)).toThrow('STALE_SESSION_ATTEMPT')
    const latest = (await adapter.listSessions()).find(row => row.id === old.id)!
    expect(latest.observationNotice?.unreadReplyEnd?.eventId).toBe('replacement')
    await adapter.clearUnread!(latest.id, latest)
    expect((await adapter.listSessions()).filter(row => row.observationNotice?.unreadReplyEnd)).toHaveLength(1023)
  })
  // 旧关闭的历史读取晚到不能覆盖更新代次关闭的未读快照。
  it('Notice_LateCloseKeepsNewReceipt_034', async () => {
    const entry = history('claude', 'root', 'session-1')
    let finish!: () => void, reads = 0
    const map = new Map<string, NativeCliTab>()
    const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive() {}, rename() {}, close: id => { map.delete(id) }, hasOwnedObservationNotice: () => true },
      history: { all: () => [entry], load: async () => { if (++reads === 1) await new Promise<void>(resolve => { finish = resolve }); return entry } },
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: async () => {}, renameTab: async () => {} },
      archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
    const first = tab({ cli: 'claude', profileId: 'claude-main', sourceSessionKey: entry.sessions[0].sessionKey })
    const one = { kind: 'reply-ended' as const, eventId: 'one', receivedAt: 1, runId: first.runId, generation: 1 }
    first.observationNotice = { recent: one, unreadReplyEnd: one }; map.set(first.tabId, first)
    const closing = adapter.closeSession('native-tab:' + first.tabId)
    await Promise.resolve(); await Promise.resolve()
    const second = { ...first, tabId: 'second', runId: 'run-2', generation: 2 }
    const two = { ...one, eventId: 'two', runId: 'run-2', generation: 2, receivedAt: 2 }
    second.observationNotice = { recent: two, unreadReplyEnd: two }; map.set(second.tabId, second)
    await adapter.closeSession('native-tab:' + second.tabId)
    finish(); await closing
    expect((await adapter.listSessions())[0].observationNotice?.unreadReplyEnd?.eventId).toBe('two')
  })
  it('Notice_FirstCloseFindsExactHistoryAfterRefresh_035', async () => {
    const entry = history('claude', 'root', 'session-1')
    const entries: NativeHistoryEntry[] = []
    const value = tab({ cli: 'claude', profileId: 'claude-main', action: { kind: 'new' }, observedSessionId: 'session-1' })
    const receipt = { kind: 'reply-ended' as const, eventId: 'first', receivedAt: 1, runId: value.runId, generation: value.generation }
    value.observationNotice = { recent: receipt, unreadReplyEnd: receipt }
    const map = new Map([[value.tabId, value]])
    const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive() {}, rename() {}, close: id => { map.delete(id) }, hasOwnedObservationNotice: () => true },
      history: { all: () => entries, load: async () => { entries.push(entry); return entry } },
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: async () => {}, renameTab: async () => {} },
      archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
    await adapter.closeSession('native-tab:' + value.tabId)
    expect((await adapter.listSessions())[0].observationNotice?.unreadReplyEnd?.eventId).toBe('first')
  })
  it('Notice_UnresolvedOlderCloseCannotReplaceNewReceipt_036', async () => {
    const entry = history('claude', 'root', 'session-1'), entries: NativeHistoryEntry[] = []
    let release!: () => void, reads = 0
    const map = new Map<string, NativeCliTab>()
    const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive() {}, rename() {}, close: id => { map.delete(id) }, hasOwnedObservationNotice: () => true },
      history: { all: () => entries, load: async () => { if (++reads === 1) await new Promise<void>(resolve => { release = resolve }); return entry } },
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: async () => {}, renameTab: async () => {} },
      archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
    const first = tab({ cli: 'claude', profileId: 'claude-main', action: { kind: 'new' }, observedSessionId: 'session-1' })
    const one = { kind: 'reply-ended' as const, eventId: 'one', receivedAt: 1, runId: first.runId, generation: 1 }
    first.observationNotice = { recent: one, unreadReplyEnd: one }; map.set(first.tabId, first)
    const pending = adapter.closeSession('native-tab:' + first.tabId)
    await Promise.resolve(); await Promise.resolve()
    entries.push(entry)
    const second = tab({ tabId: 'second', cli: 'claude', profileId: 'claude-main', sourceSessionKey: entry.sessions[0].sessionKey,
      action: { kind: 'resume-id', nativeSessionId: 'session-1' }, runId: 'run-2', generation: 2 })
    const two = { ...one, eventId: 'two', runId: 'run-2', generation: 2, receivedAt: 2 }
    second.observationNotice = { recent: two, unreadReplyEnd: two }; map.set(second.tabId, second)
    await adapter.closeSession('native-tab:' + second.tabId)
    release(); await pending
    expect((await adapter.listSessions())[0].observationNotice?.unreadReplyEnd?.eventId).toBe('two')
  })
  it('Notice_AmbiguousFirstCloseDoesNotClaimAnotherSource_037', async () => {
    const entries: NativeHistoryEntry[] = []
    const value = tab({ cli: 'claude', profileId: 'claude-main', action: { kind: 'new' }, observedSessionId: 'session-1' })
    const receipt = { kind: 'reply-ended' as const, eventId: 'ambiguous', receivedAt: 1, runId: value.runId, generation: 1 }
    value.observationNotice = { recent: receipt, unreadReplyEnd: receipt }
    const map = new Map([[value.tabId, value]])
    const adapter = createNativeCliAdapter({ tabs: { tabs: map, activeTabId: null, setActive() {}, rename() {}, close: id => { map.delete(id) }, hasOwnedObservationNotice: () => true },
      history: { all: () => entries, load: async () => { entries.push(history('claude', 'root-one', 'session-1'), history('claude', 'root-two', 'session-1')); return entries[0] } },
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: async () => {}, renameTab: async () => {} },
      archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
    await adapter.closeSession('native-tab:' + value.tabId)
    expect((await adapter.listSessions()).every(row => row.observationNotice === undefined)).toBe(true)
  })
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
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn(), renameTab: vi.fn() },
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
    // 认领后活动行标题跟随 CLI 记录标题，不停留在本地占位名。
    expect(matched[0].title).toBe('claude-/root')
  })
  it('Opencode_PreparedIdentityClaimsTitleAndSurvivesRestart_040', async () => {
    setActivePinia(createPinia())
    const tabs = useNativeTabsStore()
    const active = tabs.create({ cli: 'opencode', projectId: 'project-1', projectPath: '/repo',
      profileId: 'opencode-main', profileRevision: '7',
      preparedIdentity: { tabId: 'tab-prepared', requestId: 'request-prepared', runId: 'run-prepared' },
      action: { kind: 'resume-id', nativeSessionId: 'ses_prepared' } })
    const entry = history('opencode', '/root', 'ses_prepared')
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [entry] },
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn(), renameTab: vi.fn() },
      archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() } })
    expect(active).toMatchObject({ tabId: 'tab-prepared', requestId: 'request-prepared',
      runId: 'run-prepared', action: { kind: 'resume-id', nativeSessionId: 'ses_prepared' } })
    const rows = await adapter.listSessions()
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({ opened: true, resumable: true, title: 'opencode-/root',
      nativeSessionId: 'ses_prepared' })
    tabs.applyLaunchStatus(active.tabId, { instanceId: 'backend', requestId: active.requestId,
      run: { runId: active.runId, generation: active.generation }, phase: 'exited', revision: '1', failure: null })
    expect(tabs.restart(active.tabId, { profileId: 'opencode-main', profileRevision: '7' }).action)
      .toEqual({ kind: 'resume-id', nativeSessionId: 'ses_prepared' })
  })

  // 无观察身份时不猜测，活动行保持本地名（无唯一历史对应可跟随）。
  it('NativeObservedIdentity_WithoutIdentityKeepsLocalTitle_031b', async () => {
    const active = tab({ cli: 'claude', profileId: 'claude-main', action: { kind: 'new' }, title: 'Claude Code' })
    const { adapter } = setup([active], [])
    const rows = await adapter.listSessions('/repo')
    expect(rows[0].title).toBe('Claude Code')
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
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn(), renameTab: vi.fn() },
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
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'unknown', observationState: cli === 'codex' ? 'unavailable' : 'off', attentionState: 'none' })
  })
  it('NativeObservation_RawClaudeHasNoUnavailableBadge_046', async () => {
    const { adapter } = setup([tab({ cli: 'claude', action: { kind: 'raw', argv: [] },
      observerReasonCode: 'OBSERVER_PLUGIN_UNAVAILABLE', observationState: 'unavailable' })], [])
    const session = (await adapter.listSessions())[0]
    expect(session).toMatchObject({ activityState: 'unknown', observationState: 'off', observerReasonCode: 'OBSERVER_UNSUPPORTED' })
    expect(deriveSessionVisualState(session)).toBe('running')
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
    localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify({ claude: true, codex: true }))
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
    expect(sessions.map(value => value.title).sort()).toEqual(['codex-/root-a', 'codex-/root-b'])
    expect(sessions.find(value => value.opened)?.adapterSessionId).toBe('tab-1')
    expect(sessions.find(value => !value.opened)?.adapterSessionId).toBe(second.sessions[0].sessionKey)
  })

  // resume 打开历史会话时，活动行继承 CLI 记录的会话创建时间：激活不改变创建序
  // 位置，不会把排在后面的会话顶到最前。
  it('NativeResume_InheritsCreationTime_013', async () => {
    const entry = history('codex', '/root', 'session-1')
    entry.sessions[0].createdAt = '2026-09-01T00:00:00Z'
    const { adapter, runtime, map } = setup([], [entry])
    runtime.createTab.mockImplementation((input: any) => {
      const created = tab({ tabId: 'created', cli: input.cli, projectPath: input.projectPath, profileId: input.launchConfigId, action: input.action, sourceSessionKey: input.sourceSessionKey, title: input.title ?? 'Created' })
      map.set(created.tabId, created)
      return created
    })
    await adapter.resumeSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', adapterSessionId: entry.sessions[0].sessionKey, nativeSessionId: 'session-1', launchConfigId: 'codex-main' })
    const rows = await adapter.listSessions()
    const open = rows.find(row => row.opened === true)
    expect(open?.createdAt).toBe(Date.parse('2026-09-01T00:00:00Z'))
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
      runtime: { createTab: vi.fn(async () => created), restartTab: vi.fn(), stopTab: vi.fn(), renameTab: vi.fn() },
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
      runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn(), renameTab: vi.fn() },
      archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
    })
    const rows = await adapter.listSessions()
    await expect(adapter.restoreArchivedSession(rows[0].id)).rejects.toThrow('CLI_UNARCHIVE_UNAVAILABLE')
  })

})

// CLI 取消归档成功后，归档状态要靠下一次历史读取按容器位置重投影；
// all() 的缓存仍是归档行，必须强制重读原上下文，调用方的目录刷新才能
// 看到已恢复的行，否则会话滞留归档抽屉直到手动刷新。
it('CodexArchive_UnarchiveForcesHistoryReload_035', async () => {
  setActivePinia(createPinia())
  const entry = history('codex', '/root', 'session-1')
  entry.sessions[0].archived = true
  const { adapter, cliUnarchive, historyLoad, metadata } = setup([], [entry])
  const rows = await adapter.listSessions()
  const record = rows.find(value => value.archived)
  expect(record).toBeDefined()
  await adapter.restoreArchivedSession(record!.id)
  expect(cliUnarchive.unarchive).toHaveBeenCalledWith(entry.context, 'session-1')
  expect(historyLoad).toHaveBeenCalledWith(expect.objectContaining({ ...entry.context, force: true }))
  // 恢复后的行照常写入最近打开时间，不能因 CLI 归档分支提前返回而跳过排序更新。
  expect(metadata.recordSessionOpened).toHaveBeenCalledTimes(1)
})

// 会话同时被 Desk 注册表与 codex CLI 容器归档时，恢复必须先让 CLI 取消归档、
// 再清注册表：CLI unarchive 失败时注册表 key 保持原样，整体失败，不留
// 「注册表已清、CLI 仍归档」的部分状态变更。
it('CodexArchive_CliUnarchiveFailureKeepsAppArchiveKeys_036', async () => {
  setActivePinia(createPinia())
  const entry = history('codex', '/root', 'session-9')
  entry.sessions[0].archived = true
  const { adapter, archive, cliUnarchive, archived } = setup([], [entry])
  const rows = await adapter.listSessions()
  archived.set('/repo', [rows[0].id])
  cliUnarchive.unarchive.mockRejectedValueOnce(new Error('codex unarchive failed'))
  await expect(adapter.restoreArchivedSession(rows[0].id)).rejects.toThrow('codex unarchive failed')
  expect(archive.restoreSession).not.toHaveBeenCalled()
  expect(archived.get('/repo')).toEqual([rows[0].id])
})
// 成功路径同序：CLI unarchive 先于清注册表 key，历史强刷在两者之间，保证清 key
// 失败后的重试不会对已恢复会话重复请求 CLI unarchive。
it('CodexArchive_ConcurrentArchiveUnarchivesBeforeRegistryClear_037', async () => {
  setActivePinia(createPinia())
  const entry = history('codex', '/root', 'session-9')
  entry.sessions[0].archived = true
  const { adapter, archive, cliUnarchive, archived, historyLoad } = setup([], [entry])
  const rows = await adapter.listSessions()
  archived.set('/repo', [rows[0].id])
  await adapter.restoreArchivedSession(rows[0].id)
  expect(vi.mocked(cliUnarchive.unarchive).mock.invocationCallOrder[0])
    .toBeLessThan(vi.mocked(archive.restoreSession).mock.invocationCallOrder[0])
  expect(historyLoad).toHaveBeenCalledTimes(1)
  expect(archive.restoreSession).toHaveBeenCalledTimes(1)
  expect(archived.get('/repo')).toEqual([])
})

// 运行中 Claude 的编辑器状态未知；GUI 改名仅保存显示名。
describe('Native rename preserves the CLI editor', () => {
  it('Rename_ClaudeUnsupportedDoesNotSaveAlias_034', async () => {
    const claudeTab = tab({ tabId: 'cc-1', cli: 'claude', profileId: 'claude-main', status: 'running' })
    const { adapter, runtime, metadata, map } = setup([claudeTab], [])
    await expect(adapter.renameSession('native-tab:cc-1', 'New name')).rejects.toThrow('NATIVE_RENAME_UNSUPPORTED')
    expect(runtime.renameTab).not.toHaveBeenCalled()
    expect(metadata.upsertSessionRecord).not.toHaveBeenCalled()
    expect(map.get('cc-1')?.title).toBe(claudeTab.title)
  })
  // codex 的 /rename 参数形式未证实，激活改名继续由 GUI 显示名兜底，不注入。
  it('Rename_CodexUnverifiedRunDoesNotSaveAlias_035', async () => {
    const codexTab = tab({ tabId: 'cx-1', status: 'running' })
    const { adapter, runtime, metadata, map } = setup([codexTab], [])
    await expect(adapter.renameSession('native-tab:cx-1', 'New name')).rejects.toThrow('SESSION_IDENTITY_UNKNOWN')
    expect(runtime.renameTab).not.toHaveBeenCalled()
    expect(metadata.upsertSessionRecord).not.toHaveBeenCalled()
    expect(map.get('cx-1')?.title).toBe(codexTab.title)
  })
})
