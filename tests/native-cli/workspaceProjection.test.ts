import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { useWorkspaceStore } from '@/stores/workspace'
const project = (id: string) => ({ projectId: id, hostId: 'local', sourcePathKey: id, selectedPath: `/work/${id}`, canonicalPath: null, alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } })
const source = (target: any) => ({ scopeId: `scope-${target.profileId}-${target.projectId}`, instanceId: 'instance', cli: 'codex', sourceRootKey: target.profileId, identityEpoch: '1', profileId: target.profileId, profileRevision: '1', target, basis: 'configured-profile' })
function bridge(invoke: (c: string, v: any) => Promise<unknown>) { Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke } }) }
beforeEach(() => { setActivePinia(createPinia()); mockIPC(c => { if (c !== 'cli_list_projects') throw new Error('native default call'); return { revision: '1', projects: [project('a'), project('b')] } }) })
afterEach(() => { clearMocks(); delete (window as any).__CC_DESK_DOCUMENT__ })
it('retains registered projects when every native source is unavailable', async () => {
  bridge(async () => { throw { code: 'SCOPE_UNKNOWN' } })
  const w = useWorkspaceStore(); await w.loadNativeHome({ profileId: 'profile', revision: '1' })
  expect(w.projects).toHaveLength(2); expect(w.status).toBe('loaded'); expect(w.enrichment.a.state).toBe('unavailable')
})
it('registry-only home does not perform native reads', async () => {
  const invoke = vi.fn(async () => { throw new Error('must not scan') }); bridge(invoke)
  const w = useWorkspaceStore(); await w.loadNativeHome(null)
  expect(w.projects).toHaveLength(2); expect(invoke).not.toHaveBeenCalled()
})
it('late old-profile enrichment never replaces the next profile or project registry', async () => {
  const pending: { resolve: (v: unknown) => void; q: any }[] = []
  bridge(async (c, q) => c === 'native_get_scope' ? source(q) : new Promise(resolve => pending.push({ resolve, q })))
  const w = useWorkspaceStore(); await w.load()
  const old = w.enrich({ profileId: 'old', revision: '1' }); await vi.waitFor(() => expect(pending).toHaveLength(2))
  const next = w.enrich({ profileId: 'next', revision: '1' }); await vi.waitFor(() => expect(pending).toHaveLength(4))
  for (const i of [2, 3, 0, 1]) { const { q, resolve } = pending[i]; resolve({ source: q.source, resourceKind: 'history', requestEpoch: q.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: [], hasMore: false }) }
  await Promise.all([old, next]); expect(w.projects).toHaveLength(2)
  expect(w.enrichment.a.result?.source.profileId).toBe('next')
  await w.load(); expect(w.enrichment).toEqual({})
})
it('a delayed older home selection cannot restart enrichment after a newer home', async () => {
  const lists: ((x: unknown) => void)[] = []
  mockIPC(() => new Promise(resolve => lists.push(resolve)))
  const calls: string[] = []
  bridge(async (c, q) => {
    if (c === 'native_get_scope') { calls.push(q.profileId); return source(q) }
    return { source: q.source, resourceKind: 'history', requestEpoch: q.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: [], hasMore: false }
  })
  const w = useWorkspaceStore()
  const old = w.loadNativeHome({ profileId: 'old', revision: '1' })
  const next = w.loadNativeHome({ profileId: 'next', revision: '1' })
  await vi.waitFor(() => expect(lists).toHaveLength(2))
  lists[1]({ revision: '1', projects: [project('a')] }); await next
  lists[0]({ revision: '1', projects: [project('a')] }); await old
  expect(calls).toEqual(['next'])
  expect(w.enrichment.a.result?.source.profileId).toBe('next')
})
