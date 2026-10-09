import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useNativeProjectionStore } from '@/stores/nativeProjection'
import { useConfigStore } from '@/stores/config'
import { nativeGetScope } from '@/api/tauri'
const target = (p: string) => ({ kind: 'profile' as const, profileId: p, expectedProfileRevision: '1', projectId: null })
const scope = (p: string) => ({ scopeId: `scope-${p}`, instanceId: 'instance', cli: 'claude', sourceRootKey: p, identityEpoch: '1', profileId: p, profileRevision: '1', target: target(p), basis: 'configured-profile' })
function bridge(invoke: (cmd: string, arg: any) => Promise<unknown>) {
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke } })
}
const reply = (q: any) => ({ source: q.source, resourceKind: q.resourceKind, requestEpoch: q.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: [{ type: 'setting', name: 'model', value: q.source.profileId, origin: 'global' }], hasMore: false })
beforeEach(() => setActivePinia(createPinia()))
afterEach(() => { delete (window as any).__CC_DESK_DOCUMENT__; vi.restoreAllMocks() })
it('uses only the document bridge, never falls back when it is missing', async () => {
  await expect(nativeGetScope(target('p'))).rejects.toMatchObject({ code: 'DOCUMENT_BRIDGE_UNAVAILABLE' })
})
it('late success, failure and finally from a previous selection cannot change the current panel', async () => {
  const pending: { q: any; resolve: (v: unknown) => void; reject: (v: unknown) => void }[] = []
  bridge(async (cmd, q) => cmd === 'native_get_scope' ? scope(q.profileId) : new Promise((resolve, reject) => pending.push({ q, resolve, reject })))
  const s = useNativeProjectionStore()
  const a = s.load(target('a'), 'config'); await vi.waitFor(() => expect(pending).toHaveLength(1))
  const b = s.load(target('b'), 'config'); await vi.waitFor(() => expect(pending).toHaveLength(2))
  pending[1].resolve(reply(pending[1].q)); await b; const current = structuredClone(JSON.parse(JSON.stringify(s.result)))
  pending[0].resolve(reply(pending[0].q)); await a; expect(s.result).toEqual(current); expect(s.source?.profileId).toBe('b')
  const c = s.load(target('c'), 'config'); await vi.waitFor(() => expect(pending).toHaveLength(3))
  const d = s.load(target('d'), 'config'); await vi.waitFor(() => expect(pending).toHaveLength(4))
  pending[2].reject({ code: 'SOURCE_CHANGED', message: 'SECRET' }); await c
  expect(s.isLoading).toBe(true); expect(s.error).toBeNull(); expect(s.result).toBeNull()
  pending[3].resolve(reply(pending[3].q)); await d
  expect(s.source?.profileId).toBe('d'); expect(s.requestEpoch).toBe('4')
})
it('clear invalidates pending work and raw failures are never logged', async () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => undefined)
  let finish!: (v: unknown) => void
  bridge(async () => new Promise(resolve => { finish = resolve }))
  const s = useNativeProjectionStore(); const p = s.load(target('a'), 'config')
  s.clear(); finish(scope('a')); await p
  expect(s.result).toBeNull(); expect(s.source).toBeNull(); expect(s.isLoading).toBe(false)
  bridge(async () => { throw new Error('SECRET native failure') }); await s.load(target('a'), 'config')
  expect(s.error).toBe('SOURCE_UNAVAILABLE'); expect(log).not.toHaveBeenCalled()
})
it('config store exposes scoped resources separately from its legacy config', async () => {
  bridge(async (cmd, q) => cmd === 'native_get_scope' ? scope(q.profileId) : reply(q))
  const s = useConfigStore()
  await s.loadNativeResources(target('a'), 'config')
  expect(s.nativeProjection.source?.profileId).toBe('a'); expect(s.projectConfig).toBeNull()
  s.clearConfig(); expect(s.nativeProjection.result).toBeNull()
})
