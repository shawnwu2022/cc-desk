import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

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
