import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import type { SourceRef } from '@/types/nativeProjection'

const io = vi.hoisted(() => ({ scope: vi.fn(), read: vi.fn() }))
vi.mock('@/api/tauri', () => ({ createNativeProjectionClient: () => io }))
beforeEach(() => { setActivePinia(createPinia()); vi.resetAllMocks() })

describe.each(['restart', 'rename', 'rename-sync', 'unarchive'] as const)('%s history failure', operation => {
  // 真实 reader 和 adapter 共享缓存；校验或写后同步失败只能撤销操作证据，不能清空显示行。
  it.each(['unavailable', 'exception'] as const)('History_KeepActionRows_001 %s', async failure => {
    const context = { cli: 'codex' as const, profileId: 'p', profileRevision: '1', projectId: 'project', projectPath: '/repo' }
    const source: SourceRef = { scopeId: 'scope', instanceId: 'instance', cli: 'codex', sourceRootKey: 'root', identityEpoch: '1',
      profileId: 'p', profileRevision: '1', basis: 'configured-profile',
      target: { kind: 'profile', profileId: 'p', expectedProfileRevision: '1', projectIds: ['project'] } }
    const rows = [
      { type: 'session' as const, sessionKey: '["local","codex","root","n0"]', nativeSessionId: 'n0', title: 'Running', archived: false, truncated: false, cwd: '/repo', updatedAt: null },
      { type: 'session' as const, sessionKey: '["local","codex","root","n1"]', nativeSessionId: 'n1', title: 'History', archived: operation === 'unarchive', truncated: false, cwd: '/repo', updatedAt: null },
    ]
    io.scope.mockResolvedValue(source)
    io.read.mockResolvedValue({ state: 'ready', items: rows, hasMore: false })
    const history = useNativeHistoryStore(), tabs = useNativeTabsStore()
    await history.load(context)
    expect(history.get(context)?.absenceEvidence).toEqual({ cli: 'codex', sourceRootKey: 'root' })
    const tab = tabs.create({ ...context, sourceSessionKey: rows[0].sessionKey,
      action: { kind: 'resume-id', nativeSessionId: 'n0' }, title: 'Running' })
    tabs.markStarting(tab.tabId)
    tabs.tab(tab.tabId)!.status = operation === 'restart' ? 'exited' : 'running'
    tabs.tab(tab.tabId)!.launchRevision = '1'
    const restartTab = vi.fn(), rename = vi.fn(async request => ({ ...request, state: 'renamed' as const }))
    const unarchive = vi.fn(async () => {})
    const adapter = createNativeCliAdapter({ tabs, history,
      runtime: { createTab: vi.fn(), restartTab, stopTab: vi.fn(), renameTab: vi.fn() },
      archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
      nativeRename: { rename }, cliUnarchive: { unarchive },
      runSource: async current => ({ ...source, basis: 'launch-environment',
        target: { kind: 'run', runId: current.runId, generation: current.generation } }),
    })
    const before = await adapter.listSessions('/repo')
    const active = before.find(row => row.opened)!, closed = before.find(row => !row.opened)!
    if (failure === 'unavailable') io.read.mockResolvedValueOnce({ state: 'unavailable', reason: 'SOURCE_BUSY', items: [], hasMore: false })
    else io.read.mockRejectedValueOnce(new Error('SOURCE_READ_FAILED'))
    if (operation === 'restart') {
      await expect(adapter.restartSession(active.id)).rejects.toThrow()
      expect(restartTab).not.toHaveBeenCalled()
    } else if (operation === 'rename') {
      await expect(adapter.renameSession(active.id, 'Renamed')).rejects.toThrow()
      expect(rename).not.toHaveBeenCalled()
    } else if (operation === 'rename-sync') {
      await expect(adapter.renameSession(closed.id, 'Renamed')).rejects.toThrow('RENAME_DISPLAY_SYNC_FAILED')
      expect(rename).toHaveBeenCalledTimes(1)
    } else {
      const pending = adapter.restoreArchivedSession(closed.id)
      if (failure === 'exception') await expect(pending).rejects.toThrow()
      else await pending
      expect(unarchive).toHaveBeenCalledTimes(1)
    }
    const after = await adapter.listSessions('/repo')
    expect(after.map(row => [row.id, row.nativeRootKey, row.archived]), '读失败不能让关闭行消失或撤销运行行的来源根')
      .toEqual(before.map(row => [row.id, row.nativeRootKey, row.archived]))
    expect(history.get(context)?.error).toBeTruthy()
    expect(history.get(context)?.absenceEvidence).toBeUndefined()
    // 后续成功读取恢复真实标题/归档投影，不重放已经完成的外部写入。
    io.read.mockResolvedValueOnce({ state: 'ready', items: [rows[0], { ...rows[1], title: 'Fresh', archived: false }], hasMore: false })
    await history.load({ ...context, force: true, retainOnFailure: true })
    expect((await adapter.listSessions('/repo')).find(row => row.nativeSessionId === 'n1')).toMatchObject({ title: 'Fresh', archived: false })
    expect(history.get(context)?.error).toBeNull()
    expect(rename).toHaveBeenCalledTimes(operation === 'rename-sync' ? 1 : 0)
    expect(unarchive).toHaveBeenCalledTimes(operation === 'unarchive' ? 1 : 0)
  })
})
