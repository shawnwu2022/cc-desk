import { describe, it, expect, vi } from 'vitest'
import { createProjectionClient } from '@/api/nativeProjection'
import type { ReadRequest, SourceRef, ScopeTarget } from '@/types/nativeProjection'
const target: ScopeTarget = { kind: 'profile', profileId: 'p', expectedProfileRevision: '1', projectId: null }
const source: SourceRef = { scopeId: 'scope-1', instanceId: 'instance', cli: 'claude', sourceRootKey: 'root', identityEpoch: '1', profileId: 'p', profileRevision: '1', target, basis: 'configured-profile' }
const request = (): ReadRequest => ({ source: structuredClone(source), resourceKind: 'history', requestEpoch: '1', limit: 100, offset: 0 })
const response = () => ({ source: structuredClone(source), resourceKind: 'history', requestEpoch: '1', observedAt: '2', state: 'ready', reason: null, items: [], hasMore: false })
describe('D12 authenticated projection API', () => {
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
