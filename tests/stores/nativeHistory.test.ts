import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises } from '@vue/test-utils'

const clients: any[] = []
vi.mock('@/api/tauri', () => ({
  createNativeProjectionClient: () => clients.shift(),
}))

import { nativeHistoryContextKey, useNativeHistoryStore } from '@/stores/nativeHistory'

function source(cli: 'claude' | 'codex', profileId: string, revision: string, projectId: string, root = '/root') {
  return {
    scopeId: 'scope_1', instanceId: 'instance_1', cli, sourceRootKey: root, identityEpoch: '1',
    profileId, profileRevision: revision,
    target: { kind: 'profile' as const, profileId, expectedProfileRevision: revision, projectId },
    basis: 'configured-profile' as const,
  }
}

beforeEach(() => { setActivePinia(createPinia()); clients.length = 0 })

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
    const item = { type: 'session', sessionKey: 'key', nativeSessionId: 'id', title: 'Known' }
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
      items: [{ type: 'session', sessionKey: '["local","codex","/root","new"]', nativeSessionId: 'new', title: 'New', truncated: false, cwd: '/repo', updatedAt: '2026-09-29T00:00:00Z' }], hasMore: false,
    }) })
    const store = useNativeHistoryStore()
    const input = { cli: 'codex' as const, profileId: 'codex-main', profileRevision: '7', projectId: 'project-1', projectPath: '/repo' }
    const p1 = store.load(input)
    const p2 = store.load({ ...input, force: true })
    await p2
    firstResolve({
      source: s, resourceKind: 'history', requestEpoch: '1', observedAt: '1', state: 'ready', reason: null,
      items: [{ type: 'session', sessionKey: '["local","codex","/root","old"]', nativeSessionId: 'old', title: 'Old', truncated: false, cwd: '/repo', updatedAt: '2026-09-28T00:00:00Z' }], hasMore: false,
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
