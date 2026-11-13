import { beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { useNativeHistoryStore } from '@/stores/nativeHistory'

const io = vi.hoisted(() => ({ scope: vi.fn(), read: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }),
}))
beforeEach(() => { setActivePinia(createPinia()); vi.clearAllMocks() })

async function fixture(profiles = 1) {
  const history = useNativeHistoryStore()
  const projects = Array.from({ length: 8 }, (_, index) => ({ projectId: 'p' + index, projectPath: '/repo' + index }))
  let archived = false, active = 0, peak = 0
  io.scope.mockImplementation(async target => ({ scopeId: JSON.stringify(target), instanceId: 'instance', cli: 'codex',
    sourceRootKey: target.profileId === 'foreign' ? 'foreign-root' : 'root', identityEpoch: '1',
    profileId: target.profileId, profileRevision: target.expectedProfileRevision, basis: 'configured-profile', target }))
  io.read.mockImplementation(async ({ source }) => {
    // 与后端两扫描预算一致；延迟只隔离文件系统，不替换真实 history 发布/归属逻辑。
    if (active === 2) return { state: 'unavailable', reason: 'SOURCE_BUSY', items: [], hasMore: false }
    peak = Math.max(peak, ++active)
    await new Promise(resolve => setTimeout(resolve, 10))
    active--
    return { state: 'ready', hasMore: false, items: projects.filter(project => source.target.projectIds.includes(project.projectId)).map((project, index) => ({
      type: 'session', projectId: project.projectId, sessionKey: project.projectId, nativeSessionId: project.projectId,
      title: 'Fixture ' + index, cwd: project.projectPath, archived, truncated: false,
    })) }
  })
  for (const profileId of [...Array.from({ length: profiles }, (_, index) => 'profile' + index), 'foreign']) {
    await history.loadGrouped({ cli: 'codex', profileId, profileRevision: '1', projects })
  }
  io.scope.mockClear(); io.read.mockClear(); peak = 0
  const adapter = createNativeCliAdapter({ history,
    tabs: { tabs: new Map(), activeTabId: null, setActive: vi.fn(), close: vi.fn(), rename: vi.fn() },
    runtime: {} as any,
    archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
    nativeArchive: { archive: async () => { archived = true } },
  })
  return { history, adapter, peak: () => peak, projects }
}

// 八个同配置项目只作一次同根观察，不重复全树扫描或制造 SOURCE_BUSY。
it('CodexArchive_GroupedReadbackAvoidsSourceFailures_001', async () => {
  const f = await fixture()
  const target = (await f.adapter.listSessions()).find(row => row.projectPath === '/repo0' && row.nativeOrigin?.profileId === 'profile0')!
  await f.adapter.archiveSession(target.id)
  const own = f.history.all().filter(entry => entry.sourceRootKey === 'root')
  expect(own.map(entry => entry.error)).toEqual(Array(8).fill(null))
  expect(own.every(entry => entry.loaded && entry.sessions[0].archived)).toBe(true)
  expect(io.read).toHaveBeenCalledTimes(1)
  expect(io.scope).toHaveBeenCalledWith(expect.objectContaining({ projectIds: f.projects.map(project => project.projectId) }))
  expect(f.history.all().filter(entry => entry.sourceRootKey === 'foreign-root').every(entry => !entry.sessions[0].archived)).toBe(true)
})

// 同根多配置仍独立授权，但每批最多两次观察，所有项目完成后才结束回读。
it('CodexArchive_MultipleProfilesRespectReaderBudget_002', async () => {
  const f = await fixture(3)
  const target = (await f.adapter.listSessions()).find(row => row.projectPath === '/repo0' && row.nativeOrigin?.profileId === 'profile0')!
  await f.adapter.archiveSession(target.id)
  expect(f.history.all().filter(entry => entry.sourceRootKey === 'root').every(entry => entry.error === null && entry.sessions[0].archived)).toBe(true)
  expect(io.read).toHaveBeenCalledTimes(3)
  expect(f.peak()).toBeLessThanOrEqual(2)
})

// 无关项目在合并观察中撤销时，仍应核实准确目标；只重读，不重放已发归档。
it('CodexArchive_UnrelatedInvalidationKeepsTargetReadback_003', async () => {
  const f = await fixture()
  const target = (await f.adapter.listSessions()).find(row => row.projectPath === '/repo0' && row.nativeOrigin?.profileId === 'profile0')!
  const peer = f.history.all().find(entry => entry.context.profileId === 'profile0' && entry.context.projectId === 'p1')!
  const read = io.read.getMockImplementation()!
  io.read.mockImplementationOnce(async input => {
    const result = await read(input)
    f.history.invalidate(peer.context)
    return result
  })
  await expect(f.adapter.archiveSession(target.id)).resolves.toBeUndefined()
  expect(f.history.all().find(entry => entry.context.profileId === 'profile0' && entry.context.projectId === 'p0')?.sessions[0].archived).toBe(true)
  expect(f.history.get(peer.context)).toBeUndefined()
  expect(io.read).toHaveBeenCalledTimes(2)
})
