import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises } from '@vue/test-utils'

const clients: any[] = []
vi.mock('@/api/tauri', () => ({
  createNativeProjectionClient: () => clients.shift(),
}))

import { createProjectionClient } from '@/api/nativeProjection'
import { nativeHistoryContextKey, useNativeHistoryStore } from '@/stores/nativeHistory'

function source(cli: 'claude' | 'codex', profileId: string, revision: string, projectId: string, root = '/root') {
  return {
    scopeId: 'scope_1', instanceId: 'instance_1', cli, sourceRootKey: root, identityEpoch: '1',
    profileId, profileRevision: revision,
    target: { kind: 'profile' as const, profileId, expectedProfileRevision: revision, projectIds: [projectId] },
    basis: 'configured-profile' as const,
  }
}

beforeEach(() => { setActivePinia(createPinia()); clients.length = 0 })

// 部分时间是下界；保留同一来源的最新显示时间，完整观察仍可纠正。
it('History_PartialTimestampKeepsLatestKnown_025', async () => {
  const context = { cli: 'claude' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
  const item = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Task', archived: false, truncated: false }
  for (const [updatedAt, metadataIncomplete] of [
    ['2026-10-15T12:00:00Z', false], ['2026-10-15T09:00:00Z', true],
    [null, true], ['2026-10-15T08:00:00Z', false],
  ]) clients.push({ scope: vi.fn().mockResolvedValue(source('claude', 'p', '1', 'x')),
    read: vi.fn().mockResolvedValue({ state: 'ready', items: [{ ...item, updatedAt, metadataIncomplete }], hasMore: false, historyMetadataIncomplete: metadataIncomplete }) })
  const store = useNativeHistoryStore()
  await store.load(context)
  const partial = (await store.load({ ...context, force: true })).sessions[0]
  expect(partial.updatedAt).toBe('2026-10-15T09:00:00Z')
  expect(partial.lastKnownActivityAt).toBe(Date.parse('2026-10-15T12:00:00Z'))
  expect((await store.load({ ...context, force: true })).sessions[0].lastKnownActivityAt).toBe(Date.parse('2026-10-15T12:00:00Z'))
  expect((await store.load({ ...context, force: true })).sessions[0].lastKnownActivityAt).toBeUndefined()
})

it.each(['root', 'session'])('History_PartialTimestampKeepsExactIdentity_026 %s', async changed => {
  const context = { cli: 'claude' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
  const original = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Task', archived: false, updatedAt: '2026-10-15T12:00:00Z' }
  clients.push({ scope: vi.fn().mockResolvedValue(source('claude', 'p', '1', 'x')), read: vi.fn().mockResolvedValue({ state: 'ready', items: [original], hasMore: false }) })
  clients.push({ scope: vi.fn().mockResolvedValue(source('claude', 'p', '1', 'x', changed === 'root' ? '/other' : '/root')),
    read: vi.fn().mockResolvedValue({ state: 'ready', items: [{ ...original, nativeSessionId: changed === 'session' ? 'other' : 'id', updatedAt: '2026-10-15T09:00:00Z', metadataIncomplete: true }], hasMore: false, historyMetadataIncomplete: true }) })
  const store = useNativeHistoryStore()
  await store.load(context)
  expect((await store.load({ ...context, force: true })).sessions[0].lastKnownActivityAt).toBeUndefined()
})

describe('native history cache', () => {
  // 同一上下文加载未完成时普通读取共享它，不再重复提交扫描。
  it('History_SharePendingContext_001', async () => {
    const finish: Array<(value: unknown) => void> = []
    const s = source('codex', 'p', '1', 'x')
    const read = vi.fn(() => new Promise(resolve => { finish.push(resolve) }))
    clients.push({ scope: vi.fn().mockResolvedValue(s), read })
    clients.push({ scope: vi.fn().mockResolvedValue(s), read })
    const context = { cli: 'codex' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
    const history = useNativeHistoryStore()
    const first = history.load(context), second = history.load(context)
    await flushPromises()
    finish.forEach(resolve => resolve({ state: 'ready', items: [], hasMore: false }))
    const [a, b] = await Promise.all([first, second])
    expect(a).toBe(b)
    expect(read).toHaveBeenCalledOnce()
  })

  // 忙碌失败不形成永久已加载缓存，下一次显式读取仍可检查来源。
  it.each(['rejection', 'unavailable', 'scope-throw'] as const)('History_BusyCanReload_002: %s', async mode => {
    const s = source('codex', 'p', '1', 'x')
    clients.push({ scope: mode === 'scope-throw' ? vi.fn(() => { throw { code: 'SOURCE_BUSY' } }) : vi.fn().mockResolvedValue(s), read: mode === 'rejection'
      ? vi.fn().mockRejectedValue({ code: 'SOURCE_BUSY', stage: 'read-source-enumeration' })
      : vi.fn().mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_BUSY', items: [], hasMore: false }) })
    clients.push({ scope: vi.fn().mockResolvedValue(s), read: vi.fn().mockResolvedValue({ state: 'ready', items: [], hasMore: false }) })
    const context = { cli: 'codex' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
    const history = useNativeHistoryStore()
    await history.load(context).catch(() => {})
    expect(history.get(context)?.loaded).toBe(false)
    expect((await history.load(context)).error).toBeNull()
  })

  // 旧读取先结束时等待较新强制刷新完成，不把加载中的条目当作验证结果。
  it('History_AwaitReplacementOwner_003', async () => {
    let finishOld!: (value: unknown) => void, finishNew!: (value: unknown) => void
    const s = source('codex', 'p', '1', 'x')
    clients.push({ scope: vi.fn().mockResolvedValue(s), read: vi.fn(() => new Promise(resolve => { finishOld = resolve })) })
    clients.push({ scope: vi.fn().mockResolvedValue(s), read: vi.fn(() => new Promise(resolve => { finishNew = resolve })) })
    const context = { cli: 'codex' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
    const history = useNativeHistoryStore()
    let oldSettled = false
    const old = history.load(context).then(value => { oldSettled = true; return value })
    await Promise.resolve()
    const newer = history.load({ ...context, force: true })
    await Promise.resolve()
    finishOld({ state: 'ready', items: [], hasMore: false })
    await flushPromises()
    expect(oldSettled).toBe(false)
    finishNew({ state: 'ready', items: [], hasMore: false })
    const [a, b] = await Promise.all([old, newer])
    expect(a).toBe(b)
    expect(a.loading).toBe(false)
  })

  // 分页保留去重后的固定失败码，部分观察不能证明历史不存在。
  it('History_RetainPageReadFailures_004', async () => {
    const s = source('codex', 'p', '1', 'x')
    const item = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Known' , archived: false }
    clients.push({ scope: vi.fn().mockResolvedValue(s), read: vi.fn()
      .mockResolvedValueOnce({ state: 'ready', items: [item], hasMore: true, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED'] })
      .mockResolvedValueOnce({ state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED', 'SOURCE_INVALID'] }) })
    const entry = await useNativeHistoryStore().load({ cli: 'codex', profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' })
    expect(entry.readFailures).toEqual(['SOURCE_UNSUPPORTED', 'SOURCE_INVALID'])
    expect(entry.sessions).toHaveLength(1)
    expect(entry.metadataIncomplete).toBe(true)
    expect(entry.absenceEvidence).toBeUndefined()
    expect(entry.error).toBeNull()
  })
  it('keys cache by cli, exact profile revision, project id and normalized path', () => {
    const a = nativeHistoryContextKey({ cli: 'claude', profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: 'C:\\Work\\Repo\\' })
    const b = nativeHistoryContextKey({ cli: 'claude', profileId: 'p', profileRevision: '2', projectId: 'x', projectPath: 'c:/work/repo' })
    const c = nativeHistoryContextKey({ cli: 'codex', profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: 'c:/work/repo' })
    expect(a).not.toBe(b)
    expect(a).not.toBe(c)
    expect(a).toContain('c:/work/repo')
  })

  it('discards stale completion after a force reload of the same context', async () => {
    let firstResolve!: (value: any) => void
    const firstRead = new Promise(resolve => { firstResolve = resolve })
    const s = source('codex', 'codex-main', '7', 'project-1')
    clients.push({ scope: vi.fn().mockResolvedValue(s), read: vi.fn().mockReturnValue(firstRead) })
    clients.push({ scope: vi.fn().mockResolvedValue(s), read: vi.fn().mockResolvedValue({
      source: s, resourceKind: 'history', requestEpoch: '2', observedAt: '2', state: 'ready', reason: null,
      items: [{ type: 'session', sessionKey: '["local","codex","/root","new"]', nativeSessionId: 'new', title: 'New', truncated: false, cwd: '/repo', updatedAt: '2026-09-29T00:00:00Z' , archived: false }], hasMore: false,
    }) })
    const store = useNativeHistoryStore()
    const input = { cli: 'codex' as const, profileId: 'codex-main', profileRevision: '7', projectId: 'project-1', projectPath: '/repo' }
    const p1 = store.load(input)
    const p2 = store.load({ ...input, force: true })
    await p2
    firstResolve({
      source: s, resourceKind: 'history', requestEpoch: '1', observedAt: '1', state: 'ready', reason: null,
      items: [{ type: 'session', sessionKey: '["local","codex","/root","old"]', nativeSessionId: 'old', title: 'Old', truncated: false, cwd: '/repo', updatedAt: '2026-09-28T00:00:00Z' , archived: false }], hasMore: false,
    })
    await p1
    expect(store.get(input)?.sessions.map(x => x.nativeSessionId)).toEqual(['new'])
  })

  it('fails closed on a client projection error', async () => {
    clients.push({ scope: vi.fn().mockRejectedValue(new Error('INVALID_PROJECTION')), read: vi.fn() })
    const store = useNativeHistoryStore()
    await expect(store.load({ cli: 'claude', profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' })).rejects.toThrow('INVALID_PROJECTION')
    expect(store.all()[0]?.sessions).toEqual([])
    expect(store.all()[0]?.loaded).toBe(true)
  })

  it('does not infer absence from incomplete metadata even when every row was filtered out', async () => {
    const s = source('codex', 'p', '1', 'x')
    clients.push({ scope: vi.fn().mockResolvedValue(s), read: vi.fn().mockResolvedValue({
      source: s, resourceKind: 'history', requestEpoch: '1', observedAt: '1', state: 'ready', reason: null,
      items: [], hasMore: false, historyMetadataIncomplete: true,
    }) })
    const result = await useNativeHistoryStore().load({ cli: 'codex', profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' })
    expect(result.error).toBeNull()
    expect(result.sessions).toEqual([])
    expect(result.absenceEvidence).toBeUndefined()
    expect(result.metadataIncomplete).toBe(true)
  })
})

it('History_CompletePaginationRemainsPositiveOnly_011', async () => {
  const s = source('codex', 'p', '1', 'x')
  const item = (id: string) => ({ type: 'session', sessionKey: `key-${id}`, nativeSessionId: id, title: id, truncated: false, archived: false })
  const read = vi.fn()
    .mockResolvedValueOnce({ state: 'ready', observedAt: '100', items: [item('a')], hasMore: true })
    .mockResolvedValueOnce({ state: 'ready', observedAt: '100', items: [item('b')], hasMore: false })
  clients.push({ scope: vi.fn().mockResolvedValue(s), read })
  const entry = await useNativeHistoryStore().load({ cli: 'codex', profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' })
  expect(entry.sessions.map(row => row.nativeSessionId)).toEqual(['a', 'b'])
  expect(entry.metadataIncomplete).toBe(false)
  expect(entry.absenceEvidence).toBeUndefined()
  expect(read.mock.calls[0][0].requestEpoch).toBe(read.mock.calls[1][0].requestEpoch)
  expect(read.mock.calls[1][0].offset).toBe(1)
})

it('History_ExpiredContinuationCannotAdmitCachedPresence_012', async () => {
  const s = source('codex', 'p', '1', 'x')
  const context = { cli: 'codex' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
  const known = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Known' , archived: false }
  clients.push({ scope: vi.fn().mockResolvedValue(s), read: vi.fn().mockResolvedValue({ state: 'ready', items: [known], hasMore: false }) })
  const read = vi.fn()
    .mockResolvedValueOnce({ state: 'ready', items: [known], hasMore: true })
    .mockResolvedValueOnce({ state: 'unavailable', reason: 'SOURCE_CHANGED', items: [], hasMore: false })
  clients.push({ scope: vi.fn().mockResolvedValue(s), read })
  const history = useNativeHistoryStore()
  await history.load(context)
  const fresh = await history.load({ ...context, force: true })
  expect(fresh.error).toBe('SOURCE_CHANGED')
  expect(fresh.loading).toBe(false)
  expect(fresh.sessions).toEqual([])
  expect(fresh.absenceEvidence).toBeUndefined()
})

it('History_SnapshotExpiryRestartsWholeObservationOnce_013', async () => {
  const s = source('codex', 'p', '1', 'x')
  const item = (id: string) => ({ type: 'session', sessionKey: `key-${id}`, nativeSessionId: id, title: id, truncated: false, archived: false })
  const read = vi.fn()
    .mockResolvedValueOnce({ state: 'ready', observedAt: '10', items: [item('old-page')], hasMore: true })
    .mockResolvedValueOnce({ state: 'unavailable', reason: 'SOURCE_SNAPSHOT_EXPIRED', items: [], hasMore: false })
    .mockResolvedValueOnce({ state: 'ready', observedAt: '20', items: [item('fresh-a')], hasMore: true })
    .mockResolvedValueOnce({ state: 'ready', observedAt: '20', items: [item('fresh-b')], hasMore: false })
  clients.push({ scope: vi.fn().mockResolvedValue(s), read })
  const entry = await useNativeHistoryStore().load({ cli: 'codex', profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' })
  expect(entry.error).toBeNull()
  expect(entry.sessions.map(row => row.nativeSessionId)).toEqual(['fresh-a', 'fresh-b'])
  expect(entry.absenceEvidence).toBeUndefined()
  expect(read.mock.calls.map(call => call[0].offset)).toEqual([0, 1, 0, 1])
  expect(read.mock.calls[2][0].requestEpoch).not.toBe(read.mock.calls[0][0].requestEpoch)
})

it('History_RepeatedSnapshotExpiryIsBoundedAndFailsClosed_014', async () => {
  const s = source('codex', 'p', '1', 'x')
  const item = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Known' , archived: false }
  const read = vi.fn()
    .mockResolvedValueOnce({ state: 'ready', items: [item], hasMore: true })
    .mockResolvedValueOnce({ state: 'unavailable', reason: 'SOURCE_SNAPSHOT_EXPIRED', items: [], hasMore: false })
    .mockResolvedValueOnce({ state: 'ready', items: [item], hasMore: true })
    .mockResolvedValueOnce({ state: 'unavailable', reason: 'SOURCE_SNAPSHOT_EXPIRED', items: [], hasMore: false })
  clients.push({ scope: vi.fn().mockResolvedValue(s), read })
  const entry = await useNativeHistoryStore().load({ cli: 'codex', profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' })
  expect(read).toHaveBeenCalledTimes(4)
  expect(entry.error).toBe('SOURCE_SNAPSHOT_EXPIRED')
  expect(entry.sessions).toEqual([])
  expect(entry.absenceEvidence).toBeUndefined()
})

it('History_UnknownTitleKeepsExactLastObservation_015', async () => {
  const context = { cli: 'claude' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
  const original = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Observed native title', titleUnknown: false , archived: false }
  const uncertain = { ...original, title: 'Untitled', titleUnknown: true }
  for (const item of [original, uncertain, uncertain, { ...uncertain, title: 'Untitled', titleUnknown: false }]) clients.push({ scope: vi.fn().mockResolvedValue(source('claude', 'p', '1', 'x')), read: vi.fn().mockResolvedValue({ state: 'ready', items: [item], hasMore: false, historyMetadataIncomplete: true }) })
  const history = useNativeHistoryStore()
  await history.load(context)
  expect((await history.load({ ...context, force: true })).sessions[0]).toMatchObject({ title: 'Untitled', titleUnknown: true, lastKnownTitle: 'Observed native title' })
  expect((await history.load({ ...context, force: true })).sessions[0].lastKnownTitle).toBe('Observed native title')
  expect((await history.load({ ...context, force: true })).sessions[0].lastKnownTitle).toBeUndefined()
})

it('History_UnknownTitleNeverCrossesRootOrSessionIdentity_016', async () => {
  const context = { cli: 'claude' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
  const original = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Observed native title' , archived: false }
  for (const [root, item] of [['/root', original], ['/root', { ...original, nativeSessionId: 'other', title: 'Untitled', titleUnknown: true }], ['/different', { ...original, title: 'Untitled', titleUnknown: true }]] as const) clients.push({ scope: vi.fn().mockResolvedValue(source('claude', 'p', '1', 'x', root)), read: vi.fn().mockResolvedValue({ state: 'ready', items: [item], hasMore: false, historyMetadataIncomplete: true }) })
  const history = useNativeHistoryStore()
  await history.load(context)
  expect((await history.load({ ...context, force: true })).sessions[0].lastKnownTitle).toBeUndefined()
  expect((await history.load({ ...context, force: true })).sessions[0].lastKnownTitle).toBeUndefined()
})

// Real store and strict bridge decoder, three distinct admitted project scopes.
// A released receipt restarts only that whole observation and never mixes rows.
it('History_ThreeScopesStrictBridgePagination_017', async () => {
  const calls: Array<{ project: string; epoch: string; offset: number }> = []
  let expired = false
  const invoke = vi.fn(async (command: string, payload: any) => {
    if (command === 'native_get_scope') return { ...source('codex', 'p', '1', payload.projectIds[0]), scopeId: `scope_${payload.projectIds[0]}` }
    const project = payload.source.target.projectIds[0]
    calls.push({ project, epoch: payload.requestEpoch, offset: payload.offset })
    await new Promise(resolve => setTimeout(resolve, 0))
    const response = { source: payload.source, resourceKind: 'history', requestEpoch: payload.requestEpoch, observedAt: payload.requestEpoch, state: 'ready', reason: null, items: [], hasMore: false }
    if (project === 'x' && payload.offset === 1 && !expired) { expired = true; return { ...response, state: 'unavailable', reason: 'SOURCE_SNAPSHOT_EXPIRED' } }
    const id = `${project}-${payload.offset === 0 ? 'a' : 'b'}-${payload.requestEpoch}`
    return { ...response, items: [{ type: 'session', sessionKey: JSON.stringify(['local', 'codex', '/root', id]), nativeSessionId: id, title: id, truncated: false, archived: false, cwd: `/repo/${project}`, updatedAt: null }], hasMore: payload.offset === 0 }
  })
  const contexts = ['x', 'y', 'z'].map(projectId => ({ cli: 'codex' as const, profileId: 'p', profileRevision: '1', projectId, projectPath: `/repo/${projectId}` }))
  for (const _context of contexts) clients.push(createProjectionClient({ instanceId: 'instance_1', invoke }))
  const entries = await Promise.all(contexts.map(context => useNativeHistoryStore().load(context)))
  for (const entry of entries) {
    expect(entry.error).toBeNull()
    expect(entry.sessions).toHaveLength(2)
    expect(entry.sessions.every(item => item.nativeSessionId.startsWith(`${entry.context.projectId}-`) && item.nativeSessionId.endsWith(`-${entry.requestEpoch}`))).toBe(true)
    expect(entry.absenceEvidence).toBeUndefined()
  }
  expect(calls.filter(call => call.project === 'x').map(call => call.offset)).toEqual([0, 1, 0, 1])
  expect(calls.filter(call => call.project === 'y').map(call => call.offset)).toEqual([0, 1])
  expect(calls.filter(call => call.project === 'z').map(call => call.offset)).toEqual([0, 1])
})

it('History_IncompletePromptFallbackKeepsHigherNativeTitle_018', async () => {
  const context = { cli: 'claude' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
  const base = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Known AI title', titleSource: 'ai', truncated: true, metadataIncomplete: true, updatedAt: null , archived: false }
  for (const [item, partial] of [[base, true], [{ ...base, title: 'First user fallback', titleSource: 'prompt' }, true], [{ ...base, title: 'New AI title' }, true], [{ ...base, title: 'Complete prompt title', titleSource: 'prompt', truncated: false, metadataIncomplete: false }, false]] as const) clients.push({ scope: vi.fn().mockResolvedValue(source('claude', 'p', '1', 'x')), read: vi.fn().mockResolvedValue({ state: 'ready', items: [item], hasMore: false, historyMetadataIncomplete: partial }) })
  const history = useNativeHistoryStore()
  await history.load(context)
  expect((await history.load({ ...context, force: true })).sessions[0].lastKnownTitle).toBe('Known AI title')
  expect((await history.load({ ...context, force: true })).sessions[0].lastKnownTitle).toBeUndefined()
  expect((await history.load({ ...context, force: true })).sessions[0].lastKnownTitle).toBeUndefined()
})

// A display-length cap is not incomplete source metadata. Another row's partial
// source status cannot turn this EOF-complete observation into a title fallback.
it('History_CompleteLongPromptReplacesPriorTitleDespiteOtherPartialRows_019', async () => {
  const context = { cli: 'claude' as const, profileId: 'p', profileRevision: '1', projectId: 'x', projectPath: '/repo' }
  const base = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Old AI title', titleSource: 'ai', truncated: true , archived: false }
  for (const item of [base, { ...base, title: 'Complete long prompt', titleSource: 'prompt', metadataIncomplete: false }]) clients.push({ scope: vi.fn().mockResolvedValue(source('claude', 'p', '1', 'x')), read: vi.fn().mockResolvedValue({ state: 'ready', items: [item], hasMore: false, historyMetadataIncomplete: true }) })
  const history = useNativeHistoryStore()
  await history.load(context)
  const row = (await history.load({ ...context, force: true })).sessions[0]
  expect(row.title).toBe('Complete long prompt')
  expect(row.lastKnownTitle).toBeUndefined()
})

// 目录离线只影响该登记项目，健康项目仍经真实 projection 校验发布历史。
it('History_IsolateOfflineProject_020', async () => {
  const invoke = vi.fn(async (command: string, payload: any) => {
    if (command === 'native_get_scope') {
      if (payload.projectIds.includes('offline')) throw { code: 'SOURCE_CHANGED', stage: 'scope-project-registration' }
      return { scopeId: 'scope-healthy', instanceId: 'instance_1', cli: 'codex', sourceRootKey: '/root', identityEpoch: '1',
        profileId: 'p', profileRevision: '1', target: payload, basis: 'configured-profile' }
    }
    return { source: payload.source, resourceKind: 'history', requestEpoch: payload.requestEpoch, observedAt: '1',
      state: 'ready', reason: null, hasMore: false,
      items: [{ type: 'session', projectId: 'healthy', sessionKey: '["local","codex","/root","session-healthy"]',
        nativeSessionId: 'session-healthy', title: 'Healthy', truncated: false, cwd: '/repo/healthy', updatedAt: null , archived: false }] }
  })
  const client = createProjectionClient({ instanceId: 'instance_1', invoke })
  clients.push(client, client, client)
  const result = await useNativeHistoryStore().loadGrouped({ cli: 'codex', profileId: 'p', profileRevision: '1',
    projects: [{ projectId: 'healthy', projectPath: '/repo/healthy' }, { projectId: 'offline', projectPath: '/repo/offline' }] })
  expect(result.get('healthy')?.sessions.map(row => row.nativeSessionId)).toEqual(['session-healthy'])
  expect(result.get('healthy')?.error).toBeNull()
  expect(result.get('offline')?.error).toBe('SOURCE_CHANGED')
  expect(result.get('offline')?.absenceEvidence).toBeUndefined()
  expect(invoke.mock.calls.filter(([command]) => command === 'native_list_resources')).toHaveLength(1)
})

// 65 个登记项目分批准入，不能因为单 scope 的 64 项预算丢失全部历史。
it('History_Batch65Projects_021', async () => {
  const invoke = vi.fn(async (command: string, payload: any) => {
    if (command === 'native_get_scope') return {
      scopeId: `scope-${payload.projectIds.length}`, instanceId: 'instance_1', cli: 'codex', sourceRootKey: '/root', identityEpoch: '1',
      profileId: 'p', profileRevision: '1', target: payload, basis: 'configured-profile',
    }
    return { source: payload.source, resourceKind: 'history', requestEpoch: payload.requestEpoch, observedAt: '1',
      state: 'ready', reason: null, hasMore: false,
      items: payload.source.target.projectIds.map((projectId: string) => ({ type: 'session', projectId,
        sessionKey: JSON.stringify(['local', 'codex', '/root', `session-${projectId}`]), nativeSessionId: `session-${projectId}`,
        title: projectId, truncated: false, archived: false, cwd: `/repo/${projectId}`, updatedAt: null })) }
  })
  const client = createProjectionClient({ instanceId: 'instance_1', invoke })
  clients.push(client, client)
  const projects = Array.from({ length: 65 }, (_, i) => ({ projectId: `project-${i}`, projectPath: `/repo/project-${i}` }))
  const result = await useNativeHistoryStore().loadGrouped({ cli: 'codex', profileId: 'p', profileRevision: '1', projects })
  expect([...result.values()].flatMap(entry => entry.sessions)).toHaveLength(65)
  expect([...result.values()].every(entry => entry.error === null)).toBe(true)
  expect(invoke.mock.calls.filter(([command]) => command === 'native_get_scope').map(([, payload]) => payload.projectIds.length)).toEqual([64, 1])
})

// 最后一批单项目的失败不能把已完成的前 64 个项目变成整组失败。
it('History_LastBatchFailure_022', async () => {
  const invoke = vi.fn(async (command: string, payload: any) => {
    if (command === 'native_get_scope') {
      if (payload.projectIds.includes('project-64')) throw { code: 'PROJECT_IDENTITY_CHANGED', stage: 'scope-project-registration' }
      return { scopeId: 'scope-many', instanceId: 'instance_1', cli: 'codex', sourceRootKey: '/root', identityEpoch: '1',
        profileId: 'p', profileRevision: '1', target: payload, basis: 'configured-profile' }
    }
    return { source: payload.source, resourceKind: 'history', requestEpoch: payload.requestEpoch, observedAt: '1',
      state: 'ready', reason: null, hasMore: false,
      items: payload.source.target.projectIds.map((projectId: string) => ({ type: 'session', projectId,
        sessionKey: JSON.stringify(['local', 'codex', '/root', projectId]), nativeSessionId: projectId,
        title: projectId, truncated: false, archived: false, cwd: `/repo/${projectId}`, updatedAt: null })) }
  })
  const client = createProjectionClient({ instanceId: 'instance_1', invoke })
  clients.push(client, client)
  const projects = Array.from({ length: 65 }, (_, i) => ({ projectId: `project-${i}`, projectPath: `/repo/project-${i}` }))
  const result = await useNativeHistoryStore().loadGrouped({ cli: 'codex', profileId: 'p', profileRevision: '1', projects })
  expect([...result.values()].flatMap(entry => entry.sessions)).toHaveLength(64)
  expect(result.get('project-64')).toMatchObject({ error: 'PROJECT_IDENTITY_CHANGED', loaded: true, loading: false })
  expect(result.get('project-64')?.absenceEvidence).toBeUndefined()
  expect([...result.values()].slice(0, 64).every(entry => entry.error === null && !entry.loading)).toBe(true)
})

// root/profile 错误仍失败关闭，不得按项目拆分重试或沿用旧观察来取得授权。
it('History_RootFailureNoRetry_023', async () => {
  const invoke = vi.fn(async () => { throw { code: 'SOURCE_CHANGED', stage: 'scope-source-root' } })
  clients.push(createProjectionClient({ instanceId: 'instance_1', invoke }))
  const result = await useNativeHistoryStore().loadGrouped({ cli: 'codex', profileId: 'p', profileRevision: '1',
    projects: [{ projectId: 'a', projectPath: '/repo/a' }, { projectId: 'b', projectPath: '/repo/b' }] })
  expect(invoke).toHaveBeenCalledOnce()
  expect([...result.values()].every(entry => entry.error === 'SOURCE_CHANGED' && entry.absenceEvidence === undefined)).toBe(true)
})

// 拆分失败批次时，被新 owner 接管的项目必须等待新观察，不能发布加载中的中间状态。
it('History_IsolationOwnerFence_024', async () => {
  let rejectGroup!: (value: unknown) => void, finishNew!: (value: unknown) => void
  const invoke = vi.fn(async (command: string, payload: any) => {
    if (command === 'native_get_scope') {
      if (payload.projectIds.length === 2) return new Promise((_resolve, reject) => { rejectGroup = reject })
      if (payload.projectIds.includes('offline')) throw { code: 'SOURCE_CHANGED', stage: 'scope-project-registration' }
      return { scopeId: 'scope-new', instanceId: 'instance_1', cli: 'codex', sourceRootKey: '/root', identityEpoch: '1',
        profileId: 'p', profileRevision: '1', target: payload, basis: 'configured-profile' }
    }
    return new Promise(resolve => { finishNew = value => resolve({ source: payload.source, resourceKind: 'history',
      requestEpoch: payload.requestEpoch, observedAt: '1', state: 'ready', reason: null, hasMore: false, items: value }) })
  })
  const client = createProjectionClient({ instanceId: 'instance_1', invoke })
  clients.push(client, client, client)
  const store = useNativeHistoryStore()
  const healthy = { cli: 'codex' as const, profileId: 'p', profileRevision: '1', projectId: 'healthy', projectPath: '/repo/healthy' }
  let oldSettled = false
  const old = store.loadGrouped({ cli: 'codex', profileId: 'p', profileRevision: '1',
    projects: [healthy, { projectId: 'offline', projectPath: '/repo/offline' }] }).then(value => { oldSettled = true; return value })
  await flushPromises()
  const newer = store.load({ ...healthy, force: true })
  await flushPromises()
  rejectGroup({ code: 'SOURCE_CHANGED', stage: 'scope-project-registration' })
  await flushPromises()
  expect(oldSettled).toBe(false)
  finishNew([])
  const [result, replacement] = await Promise.all([old, newer])
  expect(result.get('healthy')).toBe(replacement)
  expect(replacement.loading).toBe(false)
  expect(result.get('offline')?.error).toBe('SOURCE_CHANGED')
})
