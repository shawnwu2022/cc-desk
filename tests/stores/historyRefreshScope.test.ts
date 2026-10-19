// 回归：codex 审查发现的四条历史发布缺陷（临时审查目录 session-audit.test.ts 的
// 仓库化版本）。四条共同根因是「读取进度污染已发布显示快照」：
// 1. 单项目历史读取把全部历史项目入队刷新（放大为 32 次刷新/24 次发布）
// 2. 来源根在新读取完成前就覆盖 entry，旧根的归档行被重标成新根泄漏进普通列表
// 3. 无读取失败的有界元数据扫描把缺失行永久保留（幽灵行）
// 4. 搜索/验证路径的瞬时读取失败清空历史，运行行丢失来源归属，同名关闭行闪出
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { defineComponent, ref, watch } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useProjectsStateStore } from '@/stores/projectsState'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'

const io = vi.hoisted(() => ({
  scope: vi.fn(), read: vi.fn(), registered: vi.fn(), root: vi.fn(), sessions: vi.fn(),
}))
vi.mock('@/api/tauri', async original => ({
  ...await original<object>(),
  getProjectsState: async () => ({ pinnedProjects: [], archivedSessions: {}, launchPreferences: {} }),
  getProjects: async () => [], getSessions: io.sessions,
  getAppConfig: async () => ({ theme: 'light', language: 'en' }),
  onHookEvent: async () => () => {}, legacyClaudeRootKey: io.root,
  createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }),
}))
vi.mock('@/api/cli', () => ({ cliListProfiles: async () => ({ revision: '1', profiles: [{
  id: 'p', revision: '1', cli: 'claude', name: 'Claude', launcher: { kind: 'native' },
  programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'inherit' },
  skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: true }, env: {},
}] }) }))
vi.mock('@/api/cliAvailability', () => ({ cliGetAvailability: async () => ({
  profileId: 'p', profileRevision: '1', cli: 'claude', state: 'available-unverified', hostStatus: 'available', certified: false,
}) }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: io.registered }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }))

let wrapper: { unmount(): void } | undefined
beforeEach(() => {
  localStorage.clear(); setActivePinia(createPinia()); vi.clearAllMocks()
  useProjectsStateStore().loaded = true
  io.sessions.mockResolvedValue([])
  io.root.mockResolvedValue({ rootKey: '', tabsUseDefaultRoot: false })
  io.scope.mockResolvedValue({ cli: 'claude', sourceRootKey: 'root-a' })
  io.registered.mockResolvedValue({ revision: '1', projects: Array.from({ length: 8 }, (_, i) => ({
    projectId: 'p' + i, hostId: 'host', sourcePathKey: 'source-' + i, selectedPath: '/p' + i,
    canonicalPath: '/p' + i, alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' },
  })) })
  io.read.mockResolvedValue({ state: 'ready', items: Array.from({ length: 8 }, (_, i) => ({
    type: 'session', projectId: 'p' + i, sessionKey: 's' + i, nativeSessionId: 'n' + i,
    title: 'Title ' + i, archived: false, truncated: false,
  })), hasMore: false })
})
afterEach(() => { wrapper?.unmount(); wrapper = undefined })

// 单项目读取只允许刷新该项目：loading/scope 等读取进度不得把其余项目反复入队。
it('History_SingleProjectReadRefreshesOnlyThatProject_067', async () => {
  let runtime: any
  wrapper = { unmount: () => {} }
  const mounted = mount(defineComponent({ setup() {
    runtime = useUnifiedWorkspaceRuntime(ref(null)); return () => null
  } })); wrapper = mounted
  await flushPromises()
  expect(runtime.ready.value).toBe(true)
  const catalog = useUnifiedSessionsStore(), history = useNativeHistoryStore()
  const refresh = vi.spyOn(catalog, 'refresh')
  const published = catalog.sessions
  let publications = 0
  const stop = watch(() => catalog.sessions, () => { publications++ }, { flush: 'sync' })
  let finishScope!: (value: any) => void, finishRead!: (value: any) => void
  io.scope.mockImplementationOnce(() => new Promise(resolve => { finishScope = resolve }))
  io.read.mockImplementationOnce(() => new Promise(resolve => { finishRead = resolve }))
  const pending = history.load({ cli: 'claude', profileId: 'p', profileRevision: '1', projectId: 'p0', projectPath: '/p0', force: true })
  await flushPromises()
  finishScope({ cli: 'claude', sourceRootKey: 'root-a' }); await flushPromises()
  finishRead({ state: 'ready', items: [{ type: 'session', projectId: 'p0', sessionKey: 's0', nativeSessionId: 'n0', title: 'Title 0', archived: false, truncated: false }], hasMore: false })
  await pending; await flushPromises()
  stop()
  const unrelated = refresh.mock.calls.filter(call => call[0] !== '/p0')
  expect(unrelated, '单项目读取不能刷新无关项目').toEqual([])
  expect(publications, '投影不变的读取不能发布新的目录数组').toBe(0)
  expect(catalog.sessions).toBe(published)
})

// 来源根在新读取完成前不得覆盖 entry：旧 root 的归档行重标成新 root 会漏进普通列表。
it('History_RootSwitchDoesNotRetagCachedRows_068', async () => {
  const history = useNativeHistoryStore(), tabs = useNativeTabsStore()
  const context = { cli: 'claude' as const, profileId: 'p', profileRevision: '1', projectId: 'p0', projectPath: '/p0' }
  io.read.mockResolvedValueOnce({ state: 'ready', items: [{ type: 'session', sessionKey: '["local","claude","root-a","n0"]', nativeSessionId: 'n0', title: 'Archived A', archived: false, truncated: false }], hasMore: false })
  await history.load(context)
  const adapter = createNativeCliAdapter({ tabs, history, legacyClaudeRootKey: async () => 'root-a',
    runtime: { createTab: () => { throw Error('unused') }, restartTab: () => { throw Error('unused') }, stopTab: async () => {}, renameTab: async () => {} },
    archive: { getArchivedSessions: () => ['n0'], archiveSession: async () => {}, restoreSession: async () => {} },
  })
  expect((await adapter.listSessions('/p0'))[0].archived).toBe(true)
  io.scope.mockResolvedValueOnce({ cli: 'claude', sourceRootKey: 'root-b' })
  let finish!: (value: any) => void
  io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const pending = history.load({ ...context, force: true })
  await flushPromises()
  const during = await adapter.listSessions('/p0')
  expect(during.filter(row => !row.archived), '旧根归档会话不能借用新根而泄漏进普通列表').toEqual([])
  finish({ state: 'unavailable', reason: 'SOURCE_CHANGED', items: [], hasMore: false })
  await pending
  expect((await adapter.listSessions('/p0')).filter(row => !row.archived)).toEqual([])
})

// 有界元数据读取是正常情况，不等同于目录遗漏；重复正常扫描应能移除已不存在的行。
it('History_BoundedMetadataDoesNotRetainVanishedRows_069', async () => {
  const history = useNativeHistoryStore()
  const context = { cli: 'claude' as const, profileId: 'p', profileRevision: '1', projectId: 'p0', projectPath: '/p0' }
  const a = { type: 'session', sessionKey: 'a', nativeSessionId: 'a', title: 'A', archived: false, truncated: false }
  const b = { ...a, sessionKey: 'b', nativeSessionId: 'b', title: 'B' }
  io.read.mockResolvedValueOnce({ state: 'ready', items: [a, b], hasMore: false })
  await history.load(context)
  io.read.mockResolvedValue({ state: 'ready', items: [{ ...a, truncated: true, metadataIncomplete: true }], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: [] })
  await history.load({ ...context, force: true })
  const second = await history.load({ ...context, force: true })
  expect(second.sessions.map(row => row.nativeSessionId), '无读取失败的重复扫描不应永久保留消失的 B').toEqual(['a'])
})

// 已打开会话的历史读失败，不能撤销既有显示归属而闪出 Legacy 关闭行。
it('History_SearchFailureKeepsDisplayAttribution_070', async () => {
  useProjectsStateStore().pinnedProjects = ['/p0']
  io.root.mockResolvedValue({ rootKey: 'root-a', tabsUseDefaultRoot: false })
  io.sessions.mockResolvedValue([{ sessionId: 'n0', name: 'Title 0', projectPath: '/p0', lastActiveAt: 1 }])
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'claude', profileId: 'p', profileRevision: '1', projectId: 'p0', projectPath: '/p0',
    sourceSessionKey: 's0', action: { kind: 'resume-id', nativeSessionId: 'n0' }, title: 'Title 0' })
  tabs.tab(tab.tabId)!.status = 'running'
  const mounted = mount(defineComponent({ setup() { useUnifiedWorkspaceRuntime(ref(null)); return () => null } })); wrapper = mounted
  await flushPromises()
  const catalog = useUnifiedSessionsStore()
  expect(catalog.sessions.filter(row => row.nativeSessionId === 'n0')).toHaveLength(1)
  io.read.mockResolvedValueOnce({ state: 'unavailable', reason: 'SOURCE_BUSY', items: [], hasMore: false })
  await catalog.searchSessions({ scope: 'current-project', projectPath: '/p0', cli: 'claude' })
  await flushPromises()
  const afterFailure = catalog.sessions.filter(row => row.nativeSessionId === 'n0')
  expect(afterFailure, '读取暂不可用不应把一个运行会话投影为同名打开和关闭两行').toHaveLength(1)
  io.read.mockResolvedValueOnce({ state: 'ready', items: [{ type: 'session', projectId: 'p0', sessionKey: 's0', nativeSessionId: 'n0', title: 'Title 0', archived: false, truncated: false }], hasMore: false })
  await catalog.searchSessions({ scope: 'current-project', projectPath: '/p0', cli: 'claude' })
  await flushPromises()
  expect(catalog.sessions.filter(row => row.nativeSessionId === 'n0')).toHaveLength(1)
})

// 全量刷新或恢复另一条历史会话失败时，现有运行行仍须保持同源折叠。
it.each(['refresh', 'resume'] as const)('History_KeepRowsOnFailure_073 %s', async operation => {
  useProjectsStateStore().pinnedProjects = ['/p0']
  io.root.mockResolvedValue({ rootKey: 'root-a', tabsUseDefaultRoot: false })
  io.sessions.mockResolvedValue([{ sessionId: 'n0', name: 'Title 0', projectPath: '/p0', lastActiveAt: 1 }])
  io.read.mockResolvedValueOnce({ state: 'ready', items: [
    { type: 'session', projectId: 'p0', sessionKey: 's0', nativeSessionId: 'n0', title: 'Title 0', archived: false, truncated: false },
    { type: 'session', projectId: 'p0', sessionKey: 's-new', nativeSessionId: 'n-new', title: 'Other history', archived: false, truncated: false },
  ], hasMore: false })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'claude', profileId: 'p', profileRevision: '1', projectId: 'p0', projectPath: '/p0',
    sourceSessionKey: 's0', action: { kind: 'resume-id', nativeSessionId: 'n0' }, title: 'Title 0' })
  tabs.tab(tab.tabId)!.status = 'running'
  let runtime!: ReturnType<typeof useUnifiedWorkspaceRuntime>
  wrapper = mount(defineComponent({ setup() { runtime = useUnifiedWorkspaceRuntime(ref(null)); return () => null } }))
  await flushPromises()
  const catalog = useUnifiedSessionsStore(), history = useNativeHistoryStore()
  expect(catalog.sessions.filter(row => row.nativeSessionId === 'n0')).toHaveLength(1)
  const target = catalog.sessions.find(row => row.nativeSessionId === 'n-new')!
  expect(target.opened).toBe(false)
  io.read.mockResolvedValueOnce({ state: 'unavailable', reason: 'SOURCE_BUSY', items: [], hasMore: false })
  if (operation === 'refresh') await runtime.refresh()
  else await expect(catalog.resumeCatalogSession(target)).rejects.toThrow('SOURCE_BUSY')
  await flushPromises()
  expect(catalog.sessions.filter(row => row.nativeSessionId === 'n0'), '暂时读失败不能闪出同名关闭行').toHaveLength(1)
  expect(catalog.sessions.find(row => row.nativeSessionId === 'n-new')?.id).toBe(target.id)
  expect(tabs.tabs.size, '失败的验证不能创建新终端').toBe(1)
  const entry = history.get({ cli: 'claude', profileId: 'p', profileRevision: '1', projectId: 'p0', projectPath: '/p0' })!
  expect(entry.error).toBe('SOURCE_BUSY')
  expect(entry.absenceEvidence).toBeUndefined()
})
