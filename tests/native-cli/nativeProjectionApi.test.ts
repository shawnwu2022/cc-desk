import { afterEach, describe, it, expect, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { createProjectionClient } from '@/api/nativeProjection'
import type { ReadRequest, SourceRef, ScopeTarget } from '@/types/nativeProjection'
const target: ScopeTarget = { kind: 'profile', profileId: 'p', expectedProfileRevision: '1', projectId: null }
const source: SourceRef = { scopeId: 'scope-1', instanceId: 'instance', cli: 'claude', sourceRootKey: 'root', identityEpoch: '1', profileId: 'p', profileRevision: '1', target, basis: 'configured-profile' }
const request = (): ReadRequest => ({ source: structuredClone(source), resourceKind: 'history', requestEpoch: '1', limit: 100, offset: 0 })
const response = () => ({ source: structuredClone(source), resourceKind: 'history', requestEpoch: '1', observedAt: '2', state: 'ready', reason: null, items: [], hasMore: false })
const deferredReads: Array<Array<() => void>> = []
afterEach(async () => {
  for (const pending of deferredReads.splice(0)) {
    while (pending.length) { pending.shift()!(); await flushPromises() }
  }
})
describe('D12 authenticated projection API', () => {
  // 传输完成与客户端发布之间的微任务撤销仍不得发布旧文档结果。
  it('Projection_FinalAdmissionRace_007', async () => {
    for (const operation of ['read', 'scope'] as const) {
      let current = true
      const client = createProjectionClient({ instanceId: 'instance', invoke: () => {
        queueMicrotask(() => queueMicrotask(() => { current = false }))
        return Promise.resolve(operation === 'read' ? response() : structuredClone(source))
      } }, () => current)
      await expect(operation === 'read' ? client.read(request()) : client.scope(target)).rejects.toMatchObject({ code: 'BACKEND_INSTANCE_CHANGED' })
      expect(current).toBe(false)
    }
  })
  // 历史与资源客户端共享两个扫描槽，等待请求冻结且按提交顺序进入原桥。
  it('Projection_SharedReaderBudget_001', async () => {
    const pending: Array<() => void> = [], admitted: string[] = []
    deferredReads.push(pending)
    let active = 0, maximum = 0
    const bridge = { instanceId: 'instance', invoke: async (_command: string, payload: unknown) => {
      const query = payload as ReadRequest
      admitted.push(query.requestEpoch); maximum = Math.max(maximum, ++active)
      await new Promise<void>(resolve => pending.push(resolve))
      --active
      return { ...response(), source: query.source, resourceKind: query.resourceKind, requestEpoch: query.requestEpoch }
    } }
    const queued = { ...request(), requestEpoch: '3', resourceKind: 'instructions' as const }
    const reads = [createProjectionClient(bridge).read(request()),
      createProjectionClient(bridge).read({ ...request(), requestEpoch: '2', resourceKind: 'config' }),
      createProjectionClient(bridge).read(queued),
      createProjectionClient(bridge).read({ ...request(), requestEpoch: '4' })]
    queued.source.sourceRootKey = 'changed'; queued.requestEpoch = '99'
    expect(admitted).toEqual(['1', '2'])
    pending.shift()!(); await flushPromises()
    expect(admitted).toEqual(['1', '2', '3'])
    while (pending.length) { pending.shift()!(); await flushPromises() }
    const results = await Promise.all(reads)
    expect(maximum).toBe(2)
    expect(results[2].source.sourceRootKey).toBe('root')
    expect(results.map(row => row.requestEpoch)).toEqual(['1', '2', '3', '4'])
  })

  // 两个扫描和三十二个等待请求之外的读取立即返回可再次请求的固定忙碌码。
  it('Projection_BoundedReadQueue_002', async () => {
    const releases: Array<() => void> = []
    deferredReads.push(releases)
    let invoked = 0
    const bridge = { instanceId: 'instance', invoke: async (_command: string, payload: unknown) => {
      ++invoked
      await new Promise<void>(resolve => releases.push(resolve))
      return { ...response(), requestEpoch: (payload as ReadRequest).requestEpoch }
    } }
    const reads = Array.from({ length: 35 }, (_, index) => createProjectionClient(bridge).read({ ...request(), requestEpoch: String(index + 1) })
      .then(value => ({ value }), error => ({ error })))
    expect(invoked).toBe(2)
    const overflow = await reads[34]
    expect(overflow).toMatchObject({ error: { code: 'SOURCE_BUSY', retryable: true } })
    while (releases.length) { releases.shift()!(); await flushPromises() }
    expect((await Promise.all(reads)).filter(row => 'value' in row)).toHaveLength(34)
    expect(invoked).toBe(34)
  })

  // 等待超过五秒的请求移出队列，后续空闲不得偷偷执行或重试它。
  it('Projection_QueueWaitExpires_003', async () => {
    vi.useFakeTimers()
    try {
      const releases: Array<() => void> = []
      deferredReads.push(releases)
      const invoke = vi.fn(async (_command: string, payload: unknown) => {
        await new Promise<void>(resolve => releases.push(resolve))
        return { ...response(), requestEpoch: (payload as ReadRequest).requestEpoch }
      })
      const bridge = { instanceId: 'instance', invoke }
      const first = createProjectionClient(bridge).read(request()), second = createProjectionClient(bridge).read({ ...request(), requestEpoch: '2' })
      let waitingError: unknown
      const waiting = createProjectionClient(bridge).read({ ...request(), requestEpoch: '3' }).catch(error => { waitingError = error })
      await vi.advanceTimersByTimeAsync(5000)
      expect(waitingError).toMatchObject({ code: 'SOURCE_BUSY', retryable: true })
      releases.forEach(release => release()); await Promise.all([first, second])
      await waiting
      expect(invoke).toHaveBeenCalledTimes(2)
    } finally { vi.useRealTimers() }
  })

  // 文档替换后等待请求和旧响应都拒绝，不能借新桥执行旧请求。
  it('Projection_QueuedDocumentChange_004', async () => {
    const releases: Array<() => void> = []
    deferredReads.push(releases)
    let current = true
    const invoke = vi.fn(async (_command: string, payload: unknown) => {
      await new Promise<void>(resolve => releases.push(resolve))
      return { ...response(), requestEpoch: (payload as ReadRequest).requestEpoch }
    })
    const bridge = { instanceId: 'instance', invoke }
    const reads = [1, 2, 3].map(epoch => createProjectionClient(bridge, () => current).read({ ...request(), requestEpoch: String(epoch) }).catch(error => error))
    current = false
    releases.forEach(release => release())
    expect((await Promise.all(reads)).map(error => error.code)).toEqual(['BACKEND_INSTANCE_CHANGED', 'BACKEND_INSTANCE_CHANGED', 'BACKEND_INSTANCE_CHANGED'])
    expect(invoke).toHaveBeenCalledTimes(2)
  })

  // 同一个后端实例的新文档必须等旧文档的两个扫描结束，不能另开扫描预算。
  it('Projection_ReplacedBridgeBudget_005', async () => {
    const releases: Array<() => void> = []
    deferredReads.push(releases)
    let oldCurrent = true, active = 0, maximum = 0
    const perform = async (_command: string, payload: unknown) => {
      maximum = Math.max(maximum, ++active)
      await new Promise<void>(resolve => releases.push(resolve))
      --active
      return { ...response(), requestEpoch: (payload as ReadRequest).requestEpoch }
    }
    const oldBridge = { instanceId: 'instance', invoke: vi.fn(perform) }
    const old = [1, 2].map(epoch => createProjectionClient(oldBridge, () => oldCurrent).read({ ...request(), requestEpoch: String(epoch) }).catch(error => error))
    oldCurrent = false
    const nextBridge = { instanceId: 'instance', invoke: vi.fn(perform) }
    const next = createProjectionClient(nextBridge).read({ ...request(), requestEpoch: '3' })
    expect(nextBridge.invoke).not.toHaveBeenCalled()
    releases.shift()!(); await flushPromises()
    expect(nextBridge.invoke).toHaveBeenCalledOnce()
    while (releases.length) { releases.shift()!(); await flushPromises() }
    expect(maximum).toBe(2)
    expect((await Promise.all(old)).map(error => error.code)).toEqual(['BACKEND_INSTANCE_CHANGED', 'BACKEND_INSTANCE_CHANGED'])
    expect((await next).requestEpoch).toBe('3')
  })

  // 完成后旧客户端再次读取仍加入实例当前队列，后端重启的新实例有独立预算。
  it('Projection_IdleAndRestartBudget_006', async () => {
    const releases: Array<() => void> = []
    deferredReads.push(releases)
    let block = false
    const invoke = vi.fn(async (_command: string, payload: unknown) => {
      const query = payload as ReadRequest
      if (block) await new Promise<void>(resolve => releases.push(resolve))
      return { ...response(), source: query.source, requestEpoch: query.requestEpoch }
    })
    const bridge = { instanceId: 'instance', invoke }
    const original = createProjectionClient(bridge)
    await original.read(request())
    block = true
    const reads = [original.read({ ...request(), requestEpoch: '2' }),
      createProjectionClient(bridge).read({ ...request(), requestEpoch: '3' }),
      createProjectionClient(bridge).read({ ...request(), requestEpoch: '4' })]
    expect(invoke).toHaveBeenCalledTimes(3)
    const restarted = { instanceId: 'restart', invoke }
    reads.push(createProjectionClient(restarted).read({ ...request(), source: { ...source, instanceId: 'restart' }, requestEpoch: '5' }))
    expect(invoke).toHaveBeenCalledTimes(4)
    while (releases.length) { releases.shift()!(); await flushPromises() }
    expect((await Promise.all(reads)).map(row => row.requestEpoch)).toEqual(['2', '3', '4', '5'])
  })
  it('keeps fixed partial history failures and rejects unsafe or incomplete diagnostics', async () => {
    const partial = { ...response(), historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED'] }
    const client = createProjectionClient({ instanceId: 'instance', invoke: async () => partial })
    expect((await client.read(request()))).toHaveProperty('historyReadFailures', ['SOURCE_UNSUPPORTED'])
    for (const bad of [
      { ...partial, historyReadFailures: ['SECRET file'] },
      { ...partial, historyReadFailures: ['SOURCE_READ_FORBIDDEN'] },
      { ...partial, historyReadFailures: [] },
      { ...partial, historyReadFailures: ['SOURCE_UNSUPPORTED', 'SOURCE_UNSUPPORTED'] },
      { ...partial, historyMetadataIncomplete: false },
      { ...partial, state: 'unavailable', reason: 'SOURCE_INVALID' },
    ]) await expect(createProjectionClient({ instanceId: 'instance', invoke: async () => bad }).read(request())).rejects.toThrow('INVALID_PROJECTION')
    await expect(createProjectionClient({ instanceId: 'instance', invoke: async () => ({ ...partial, resourceKind: 'config' }) })
      .read({ ...request(), resourceKind: 'config' })).rejects.toThrow('INVALID_PROJECTION')
  })
  it('retains the bounded-history completeness flag and rejects malformed or misplaced flags', async () => {
    const r = { ...response(), historyMetadataIncomplete: true }
    const client = createProjectionClient({ instanceId: 'instance', invoke: async () => r })
    expect((await client.read(request())).historyMetadataIncomplete).toBe(true)
    for (const value of ['true', 1, null]) {
      const bad = createProjectionClient({ instanceId: 'instance', invoke: async () => ({ ...r, historyMetadataIncomplete: value }) })
      await expect(bad.read(request())).rejects.toThrow('INVALID_PROJECTION')
    }
    const misplaced = createProjectionClient({ instanceId: 'instance', invoke: async () => ({ ...r, resourceKind: 'config' }) })
    await expect(misplaced.read({ ...request(), resourceKind: 'config' })).rejects.toThrow('INVALID_PROJECTION')
  })
  it('uses exact native commands and retains the document bridge', async () => {
    const invoke = vi.fn(async (cmd: string) => cmd === 'native_get_scope' ? structuredClone(source) : response())
    const client = createProjectionClient({ instanceId: 'instance', invoke })
    expect(await client.scope(target)).toEqual(source)
    expect(await client.read(request())).toEqual(response())
    expect(invoke.mock.calls.map(c => c[0])).toEqual(['native_get_scope', 'native_list_resources'])
  })
  it('rejects mismatched instance, target, revision, root or epoch without retries', async () => {
    for (const changed of [{ instanceId: 'other' }, { identityEpoch: '2' }, { profileRevision: '3' }, { sourceRootKey: 'other' }, { scopeId: 'other' }]) {
      const r = response(); Object.assign(r.source, changed)
      const invoke = vi.fn(async () => r)
      await expect(createProjectionClient({ instanceId: 'instance', invoke }).read(request())).rejects.toThrow()
      expect(invoke).toHaveBeenCalledTimes(1)
    }
  })
  it('rejects invalid request fields before calling the bridge', async () => {
    for (const change of [{ requestEpoch: '01' }, { requestEpoch: 4 }, { requestEpoch: '18446744073709551616' }, { limit: 0 }, { root: '/etc' }, { query: 'invalid for history' }]) {
      const invoke = vi.fn(async () => response())
      await expect(createProjectionClient({ instanceId: 'instance', invoke }).read({ ...request(), ...change } as ReadRequest)).rejects.toThrow()
      expect(invoke).not.toHaveBeenCalled()
    }
  })
  it('rejects unavailable responses with partial data and raw native fields', async () => {
    const bad = [
      { ...response(), state: 'unavailable', reason: 'SOURCE_CHANGED', items: [{ type: 'setting', name: 'model', value: 'x', origin: 'global' }] },
      { ...response(), items: [{ type: 'mcp', name: 'x', transport: 'stdio', origin: 'global', env: { SECRET: 'secret' } }] },
      { ...response(), requestEpoch: '2' }, { ...response(), items: [{ type: 'unknown' }] },
      { ...response(), observedAt: 2 }, { ...response(), state: 'unavailable', reason: 'RAW SECRET MESSAGE' },
    ]
    for (const r of bad) await expect(createProjectionClient({ instanceId: 'instance', invoke: async () => r }).read(request())).rejects.toThrow()
  })
  it('freezes request before awaiting the transport and rejects changed scope replies', async () => {
    const r = request(); let resolve!: (v: unknown) => void
    const invoke = vi.fn(() => new Promise<unknown>(done => { resolve = done }))
    const promise = createProjectionClient({ instanceId: 'instance', invoke }).read(r)
    r.source.sourceRootKey = 'other'; r.requestEpoch = '7'
    resolve(response()); expect((await promise).source.sourceRootKey).toBe('root')
    expect((invoke.mock.calls as unknown as [string, ReadRequest][])[0][1].source.sourceRootKey).toBe('root')
  })
  it('accepts explicit unavailable and canonical u64 beyond JS precision', async () => {
    const r = response(); r.requestEpoch = '9007199254740993'; r.state = 'unavailable'; r.reason = 'SOURCE_CHANGED' as any
    const q = request(); q.requestEpoch = r.requestEpoch
    expect((await createProjectionClient({ instanceId: 'instance', invoke: async () => r }).read(q)).state).toBe('unavailable')
  })
})
