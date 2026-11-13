import { beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises } from '@vue/test-utils'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { useNativeHistoryStore } from '@/stores/nativeHistory'

const io = vi.hoisted(() => ({ scope: vi.fn(), read: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }),
}))
beforeEach(() => { setActivePinia(createPinia()); vi.clearAllMocks() })

async function fixture(profiles = 1, verifiedReceipt = false) {
  const history = useNativeHistoryStore()
  const projects = Array.from({ length: 8 }, (_, index) => ({ projectId: 'p' + index, projectPath: '/repo' + index }))
  let archived = false, active = 0, peak = 0
  io.scope.mockImplementation(async target => ({ scopeId: 'scope-' + target.profileId + '-' + target.projectIds.join('-'), instanceId: 'instance', cli: 'codex',
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
  const nativeArchive = { archive: vi.fn(async request => { archived = true; return verifiedReceipt ? { ...request, state: 'archived', rootArchived: true, descendants: 'unverified' } : undefined }) }
  const adapter = createNativeCliAdapter({ history,
    tabs: { tabs: new Map(), activeTabId: null, setActive: vi.fn(), close: vi.fn(), rename: vi.fn() },
    runtime: {} as any,
    archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() },
    nativeArchive,
  })
  return { history, adapter, nativeArchive, peak: () => peak, projects }
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

// 后端已证明准确根归档后，目标立即进入归档；同根其他项目仍等真实回读，不假定后代结果。
it('CodexArchive_VerifiedReceiptCompletesBeforeBackgroundRead_004', async () => {
  const f = await fixture(1, true)
  const target = (await f.adapter.listSessions()).find(row => row.projectPath === '/repo0' && row.nativeOrigin?.profileId === 'profile0')!
  const read = io.read.getMockImplementation()!
  let finish!: () => Promise<void>
  io.read.mockImplementationOnce(input => new Promise(resolve => { finish = async () => { resolve(await read(input)) } }))
  let completed = false
  const operation = f.adapter.archiveSession(target.id).then(() => { completed = true })
  await flushPromises()
  expect(completed).toBe(true)
  const rows = await f.adapter.listSessions()
  expect(rows.find(row => row.id === target.id)?.archived).toBe(true)
  expect(rows.filter(row => row.id !== target.id).every(row => !row.archived)).toBe(true)
  expect(f.nativeArchive.archive).toHaveBeenCalledOnce()
  await finish(); await operation; await flushPromises()
  expect(f.history.all().filter(entry => entry.sourceRootKey === 'root').every(entry => entry.sessions[0].archived)).toBe(true)
})

// 后台扫描失败仍保留经原生回执证明的目标状态和真实错误，不能重发归档。
it('CodexArchive_BackgroundFailureKeepsVerifiedTarget_005', async () => {
  const f = await fixture(1, true)
  const target = (await f.adapter.listSessions()).find(row => row.projectPath === '/repo0' && row.nativeOrigin?.profileId === 'profile0')!
  io.read.mockRejectedValueOnce(new Error('SOURCE_UNAVAILABLE'))
  await expect(f.adapter.archiveSession(target.id)).resolves.toBeUndefined()
  await flushPromises()
  const entry = f.history.all().find(entry => entry.context.profileId === 'profile0' && entry.context.projectId === 'p0')!
  expect(entry.error).toBe('SOURCE_UNAVAILABLE')
  expect(entry.sessions[0].archived).toBe(true)
  expect(f.nativeArchive.archive).toHaveBeenCalledOnce()
})

// 回执只能更新当前准确 entry；旧 owner、失效来源、失败行不能借成功回执升级权限。
it.each(['replaced', 'loading', 'error', 'source', 'duplicate', 'readFailure', 'nativeId'] as const)(
  'CodexArchive_ReceiptRejectsUnprovenTarget_006 %s', async boundary => {
    const f = await fixture(1, true)
    const entry = f.history.all().find(entry => entry.context.profileId === 'profile0' && entry.context.projectId === 'p0')!
    const request = { operationId: 'receipt-fixture', source: structuredClone(JSON.parse(JSON.stringify(entry.source))), sessionKey: entry.sessions[0].sessionKey }
    const receipt = { ...request, state: 'archived' as const, rootArchived: true as const, descendants: 'unverified' as const }
    if (boundary === 'replaced') await f.history.load({ ...entry.context, force: true })
    if (boundary === 'loading') entry.loading = true
    if (boundary === 'error') entry.error = 'SOURCE_BUSY'
    if (boundary === 'source') entry.source = { ...entry.source!, identityEpoch: '2' }
    if (boundary === 'duplicate') entry.sessions.push({ ...entry.sessions[0] })
    if (boundary === 'readFailure') entry.sessions[0].readFailure = 'SOURCE_INVALID_TEXT'
    expect(f.history.applyArchiveReceipt(entry, request, receipt, boundary === 'nativeId' ? 'foreign-id' : entry.sessions[0].nativeSessionId)).toBe(false)
    expect(f.history.all().flatMap(entry => entry.sessions).every(row => !row.archived)).toBe(true)
  },
)

// 错误 operation、来源或根证明一律 Unknown；只读失败后再次点击仍不可重放。
it.each(['operationId', 'source', 'rootArchived', 'descendants'] as const)(
  'CodexArchive_MalformedReceiptKeepsUnknownBarrier_007 %s', async field => {
    const f = await fixture(1, true)
    const target = (await f.adapter.listSessions()).find(row => row.projectPath === '/repo0' && row.nativeOrigin?.profileId === 'profile0')!
    f.nativeArchive.archive.mockImplementationOnce(async request => ({ ...request, state: 'archived', rootArchived: true, descendants: 'unverified',
      ...(field === 'operationId' ? { operationId: 'foreign-operation' } : {}),
      ...(field === 'source' ? { source: { ...request.source, identityEpoch: '2' } } : {}),
      ...(field === 'rootArchived' ? { rootArchived: false } : {}),
      ...(field === 'descendants' ? { descendants: 'verified' } : {}),
    }) as any)
    await expect(f.adapter.archiveSession(target.id)).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
    await expect(f.adapter.archiveSession(target.id)).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
    expect((await f.adapter.listSessions()).find(row => row.id === target.id)?.archived).toBe(false)
    expect(f.nativeArchive.archive).toHaveBeenCalledOnce()
  },
)

// 新观察在 RPC 等待期间接管时，迟到成功不得覆盖它，仍须等新的同源读回。
it('CodexArchive_ReceiptCannotBypassNewReadOwner_008', async () => {
  const f = await fixture(1, true)
  const target = (await f.adapter.listSessions()).find(row => row.projectPath === '/repo0' && row.nativeOrigin?.profileId === 'profile0')!
  const entry = f.history.all().find(entry => entry.context.profileId === 'profile0' && entry.context.projectId === 'p0')!
  const archive = f.nativeArchive.archive.getMockImplementation()!
  let resolveArchive!: () => Promise<void>
  f.nativeArchive.archive.mockImplementationOnce(request => new Promise(resolve => { resolveArchive = async () => resolve(await archive(request)) }))
  let completed = false
  const operation = f.adapter.archiveSession(target.id).then(() => { completed = true })
  await flushPromises()
  await f.history.load({ ...entry.context, force: true })
  const read = io.read.getMockImplementation()!
  let finish!: () => Promise<void>
  io.read.mockImplementationOnce(input => new Promise(resolve => { finish = async () => resolve(await read(input)) }))
  await resolveArchive(); await flushPromises()
  expect(completed).toBe(false)
  expect(f.history.get(entry.context)?.sessions[0].archived).toBe(false)
  await finish(); await operation
  expect(f.history.get(entry.context)?.sessions[0].archived).toBe(true)
})