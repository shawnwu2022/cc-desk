import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises } from '@vue/test-utils'
import { createLaunchAttempt } from '@/api/cliLaunchAttempt'
import { publicNativeErrorCode } from '@/utils/nativeErrorCode'
import { createProjectionClient } from '@/api/nativeProjection'
import { useProjectResourcesStore } from '@/stores/projectResources'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useSessionStore } from '@/stores/session'
import { useNativeProjectionStore } from '@/stores/nativeProjection'
import { useWorkspaceStore } from '@/stores/workspace'
import type { ReadRequest, ResourceItem, ScopeTarget } from '@/types/nativeProjection'

const host = vi.hoisted(() => ({ invoke: vi.fn(), config: vi.fn(), skills: vi.fn(), agents: vi.fn(), mcp: vi.fn(), plugins: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  createNativeProjectionClient: () => createProjectionClient({ instanceId: 'instance', invoke: host.invoke }),
  getProjectConfig: host.config, getAllSkills: host.skills, getAllAgents: host.agents, getAllMcpServers: host.mcp, getAllPlugins: host.plugins,
}))
let items: ResourceItem[]
let tabId: string
function source(target: ScopeTarget) {
  const tab = [...useNativeTabsStore().tabs.values()].find(tab => target.kind === 'run' && tab.runId === target.runId)
  return { scopeId: 'scope', instanceId: 'instance', cli: tab?.cli ?? 'claude', sourceRootKey: 'private-root', identityEpoch: '1',
    profileId: tab?.profileId ?? (target.kind === 'profile' ? target.profileId : 'config'),
    profileRevision: tab?.profileRevision ?? (target.kind === 'profile' ? target.expectedProfileRevision : '1'), target,
    basis: target.kind === 'run' ? 'launch-environment' : 'configured-profile' }
}
function response(request: ReadRequest, rows = items, hasMore = false) {
  return { source: request.source, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: rows, hasMore }
}
function selectNative(id = 'a', cli: 'claude' | 'codex' = 'claude') {
  const tab = useNativeTabsStore().create({ cli, projectId: 'project', projectPath: '/work/project', profileId: 'config', profileRevision: '1', action: { kind: 'new' } })
  useNativeTabsStore().tab(tab.tabId)!.status = 'running'
  useNativeTabsStore().tab(tab.tabId)!.launchRevision = '1'
  const sessions = useUnifiedSessionsStore()
  sessions.sessions.push({ id, runtime: 'native-cli', cli, projectKey: '/work/project', projectPath: '/work/project', adapterSessionId: tab.tabId,
    title: id, processState: 'running', attentionState: 'none', lastActivityAt: 1, archived: false, resumable: false, launchConfigId: 'config',
    nativeOrigin: { cli, projectId: 'project', projectPath: '/work/project', profileId: 'config', profileRevision: '1' } })
  sessions.activeSessionId = id
  return tab.tabId
}
function selectLegacy(path = '/work/project') {
  const sessions = useUnifiedSessionsStore()
  sessions.sessions.push({ id: 'legacy-history:a', runtime: 'legacy-claude', cli: 'claude', projectKey: path, projectPath: path, adapterSessionId: 'legacy-a',
    title: 'Legacy', processState: 'stopped', attentionState: 'none', lastActivityAt: 1, archived: false, resumable: true })
  sessions.activeSessionId = 'legacy-history:a'
}
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (value: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b }); return { promise, resolve, reject } }
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks()
  items = [{ type: 'document', name: 'CLAUDE.md', text: 'Run the unit tests before proposing a change.', truncated: false, origin: 'project' }]
  host.invoke.mockImplementation(async (command: string, request: any) => command === 'native_get_scope' ? source(request) : response(request))
  host.config.mockResolvedValue({ basic: [], mcp: [], skills: [], agents: [], hooks: [] })
  host.skills.mockResolvedValue([]); host.agents.mockResolvedValue([]); host.mcp.mockResolvedValue([]); host.plugins.mockResolvedValue([])
  useCliProfilesStore().profiles = [{ id: 'config', revision: '1', cli: 'claude', name: 'Standard', launcher: { kind: 'native' }, programPath: { mode: 'inherit' },
    defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }]
  useWorkspaceStore().projects = [{ projectId: 'project', hostId: 'host', sourcePathKey: 'path', selectedPath: '/work/project', canonicalPath: null,
    alias: { mode: 'inherit' }, hidden: { mode: 'inherit' }, pinned: { mode: 'inherit' } }]
  tabId = selectNative()
})

describe('Scoped structured project resources', () => {
  // 当前 Native 终端优先读取冻结的运行身份，不使用配置选择器。
  it('Resources_RunAuthority_001', async () => {
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    const tab = useNativeTabsStore().tab(tabId)!
    expect(host.invoke.mock.calls[0]).toEqual(['native_get_scope', { kind: 'run', runId: tab.runId, generation: 1 }])
    expect(store.items).toEqual([{ type: 'document', name: 'CLAUDE.md', text: items[0].type === 'document' ? items[0].text : '', truncated: false, origin: 'project', withheld: false }])
    expect(store.unavailable).toBe(false)
  })
  // 已明确拒绝的运行 scope 不能退回当前默认配置。
  it('Resources_NoRunFallback_002', async () => {
    host.invoke.mockRejectedValue({ code: 'SCOPE_REVOKED', message: 'secret-path' })
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.unavailable).toBe(true); expect(store.error).toBe('SCOPE_REVOKED'); expect(store.items).toEqual([])
    expect(host.invoke.mock.calls).toHaveLength(1)
  })
  // 尚未提交的终端按自己的配置修订和项目读取，不用其他默认配置。
  it('Resources_ExactProfile_003', async () => {
    const tab = useNativeTabsStore().tab(tabId)!; tab.status = 'stopped'; tab.launchRevision = null
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(host.invoke.mock.calls[0]).toEqual(['native_get_scope', { kind: 'profile', profileId: 'config', expectedProfileRevision: '1', projectId: 'project' }])
    expect(store.items).toHaveLength(1)
  })
  // 配置修订不一致时不猜测新的配置身份。
  it('Resources_StaleProfile_004', async () => {
    const tab = useNativeTabsStore().tab(tabId)!; tab.status = 'stopped'; tab.launchRevision = null
    useCliProfilesStore().profiles[0].revision = '2'
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.unavailable).toBe(true); expect(host.invoke).not.toHaveBeenCalled()
  })
  // 同一所有者刷新保留旧结构化内容并标记过期。
  it('Resources_RetainRefresh_005', async () => {
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    const pending = deferred<any>(); host.invoke.mockImplementation((command, request) => command === 'native_get_scope' ? source(request) : pending.promise)
    const refresh = store.refresh(); await flushPromises()
    expect(store.loading).toBe(true); expect(store.stale).toBe(true); expect(store.items).toHaveLength(1)
    pending.resolve(response(host.invoke.mock.calls[host.invoke.mock.calls.length - 1][1], [])); await refresh
    expect(store.items).toEqual([]); expect(store.stale).toBe(false); expect(store.unavailable).toBe(false)
  })
  // 会话切换立即清空旧内容，较晚回复不能覆盖新所有者。
  it('Resources_SessionFence_006', async () => {
    const old = deferred<any>(); let oldRequest: ReadRequest
    host.invoke.mockImplementation((command, request) => command === 'native_get_scope' ? source(request) : (oldRequest = request, old.promise))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    host.invoke.mockImplementation((command, request) => command === 'native_get_scope' ? source(request) : response(request, []))
    selectNative('b'); expect(store.items).toEqual([]); await flushPromises()
    old.resolve(response(oldRequest!)); await flushPromises()
    expect(store.items).toEqual([]); expect(store.loading).toBe(false); expect(store.context?.sessionId).toBe('b')
  })
  // 同一会话重启产生的新 request/run/generation 拒绝旧回复。
  it('Resources_AttemptFence_007', async () => {
    const old = deferred<any>(); let oldRequest: ReadRequest
    host.invoke.mockImplementation((command, request) => command === 'native_get_scope' ? source(request) : (oldRequest = request, old.promise))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    host.invoke.mockImplementation((command, request) => command === 'native_get_scope' ? source(request) : response(request, []))
    useNativeTabsStore().tab(tabId)!.requestId = 'new-request'; await flushPromises()
    old.resolve(response(oldRequest!)); await flushPromises()
    expect(store.items).toEqual([]); expect(store.loading).toBe(false)
  })
  // 当前 Native scope 返回别的 CLI 时在读取前拒绝。
  it('Resources_CliMismatch_008', async () => {
    host.invoke.mockImplementation((_command, target) => ({ ...source(target), cli: 'codex' }))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.unavailable).toBe(true); expect(store.items).toEqual([]); expect(host.invoke).toHaveBeenCalledTimes(1)
  })
  // 配置字段严格 allowlist，凭证字段导致整个认证 DTO 无效。
  it('Resources_ConfigAllowlist_009', async () => {
    items = [{ type: 'setting', name: 'env', value: 'DO_NOT_RENDER', origin: 'project' }]
    const store = useProjectResourcesStore(); store.kind = 'config'; store.setActive(true); await flushPromises()
    expect(store.items).toEqual([]); expect(store.unavailable).toBe(true)
  })
  // allowlist 字段中的凭证、原始路径与 header 外观内容也必须隐藏。
  it('Resources_UnsafeValues_010', async () => {
    items = [
      { type: 'setting', name: 'model', value: 'sk-proj-DO_NOT_RENDER', origin: '/home/private' },
      { type: 'setting', name: 'language', value: 'en', origin: 'project' },
      { type: 'setting', name: 'outputStyle', value: 'Authorization: Bearer DO_NOT_RENDER', origin: 'global' },
    ]
    const store = useProjectResourcesStore(); store.kind = 'config'; store.setActive(true); await flushPromises()
    expect(store.items).toMatchObject([{ value: null, origin: 'unknown', withheld: true }, { value: 'en', origin: 'project' }, { value: null, withheld: true }])
    expect(JSON.stringify(store.items)).not.toContain('DO_NOT_RENDER'); expect(JSON.stringify(store.items)).not.toContain('/home/private')
  })
  // instructions 仅使用惯性文本插值，疑似密钥和环境赋值不会进入 DTO。
  it('Resources_InstructionPrivacy_011', async () => {
    items = [{ type: 'document', name: '/private/DO_NOT_RENDER', text: 'TOKEN=DO_NOT_RENDER', truncated: true, origin: 'plugin:sk-proj-DO_NOT_RENDER' }]
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.items).toMatchObject([{ name: null, text: null, truncated: true, origin: 'plugin', withheld: true }])
    expect(JSON.stringify(store.items)).not.toContain('DO_NOT_RENDER')
  })
  // 预算以内的单页有剩余数据时明确显示 partial，不自行翻页。
  it('Resources_BoundedPartial_012', async () => {
    host.invoke.mockImplementation((command, request) => command === 'native_get_scope' ? source(request) : response(request, items, true))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.partial).toBe(true); expect(host.invoke).toHaveBeenCalledTimes(2)
    expect(host.invoke.mock.calls[1][1]).toMatchObject({ limit: 200, offset: 0 })
  })
  // Legacy 仅使用显式项目源并剔除同名用户源和未知源。
  it('Resources_LegacyExactProject_013', async () => {
    selectLegacy(); host.skills.mockResolvedValue([
      { name: 'review', displayName: 'Project review', description: 'Read project tests', sourceType: 'project', sourceLabel: '/private/path' },
      { name: 'review', displayName: 'DO_NOT_RENDER', sourceType: 'user' },
      { name: 'DO_NOT_RENDER', sourceType: 'plugin' },
    ])
    const store = useProjectResourcesStore(); store.kind = 'skills'; store.setActive(true); await flushPromises()
    expect(host.skills).toHaveBeenCalledWith('/work/project'); expect(host.invoke).not.toHaveBeenCalled()
    expect(store.items).toMatchObject([{ type: 'skill', name: 'Project review', description: 'Read project tests', origin: 'project' }]); expect(store.items).toHaveLength(1)
    expect(store.partial).toBe(true); expect(store.legacyProjectOnly).toBe(true)
  })
  // Legacy Plugin 必须有当前项目路径，不接受缺失路径或其他项目。
  it('Resources_LegacyPluginPath_014', async () => {
    selectLegacy(); host.plugins.mockResolvedValue([
      { name: 'project-plugin', version: '1.2.3', enabled: true, scope: 'project', projectPath: '/work/project' },
      { name: 'DO_NOT_RENDER', scope: 'project', projectPath: '/other/project' },
      { name: 'DO_NOT_RENDER', scope: 'project' },
      { name: 'DO_NOT_RENDER', scope: 'user' },
    ])
    const store = useProjectResourcesStore(); store.kind = 'plugins'; store.setActive(true); await flushPromises()
    expect(store.items).toHaveLength(1); expect(store.items[0]).toMatchObject({ name: 'project-plugin', installed: true }); expect(store.partial).toBe(true)
  })
  // Legacy 缺少 instruction 契约时保持 unavailable 而非伪造空文档。
  it('Resources_LegacyInstructions_015', async () => {
    selectLegacy(); const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.unavailable).toBe(true); expect(store.error).toBe('SOURCE_UNSUPPORTED'); expect(host.config).not.toHaveBeenCalled(); expect(host.invoke).not.toHaveBeenCalled()
  })
  // 没有活动会话不借用配置选择器或默认 home。
  it('Resources_NoSessionAuthority_016', async () => {
    useUnifiedSessionsStore().activeSessionId = null
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.context).toBeNull(); expect(store.items).toEqual([]); expect(host.invoke).not.toHaveBeenCalled()
  })
  // 关闭实际终端后 catalog 旧投影不得退回配置读取。
  it('Resources_CloseRevokesAuthority_017', async () => {
    const sessions = useUnifiedSessionsStore(); sessions.sessions[0].id = `native-tab:${tabId}`; sessions.activeSessionId = `native-tab:${tabId}`
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    useNativeTabsStore().close(tabId); await flushPromises()
    expect(store.items).toEqual([]); expect(store.unavailable).toBe(true); expect(host.invoke).toHaveBeenCalledTimes(2)
  })
  // 传输异常只保留固定安全 code，不反射 message。
  it('Resources_SafeFailure_018', async () => {
    host.invoke.mockRejectedValue(new Error('/private/secret TOKEN=DO_NOT_RENDER'))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.error).toBe('SOURCE_UNAVAILABLE'); expect(store.items).toEqual([])
  })
  // 刷新失败保留同一个所有者的旧数据但明确过期和不可用。
  it('Resources_RefreshFailure_019', async () => {
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    host.invoke.mockRejectedValue({ code: 'SOURCE_BUSY' }); await store.refresh()
    expect(store.items).toHaveLength(1); expect(store.stale).toBe(true); expect(store.unavailable).toBe(true)
  })
  // Legacy 配置仅投影允许字段，源标签、env、permissions 不进入资源。
  it('Resources_LegacyConfigPrivacy_020', async () => {
    selectLegacy(); host.config.mockResolvedValue({ basic: [
      { model: 'sonnet', theme: 'DO_NOT_RENDER', env: { TOKEN: 'DO_NOT_RENDER' }, source: { type: 'project', label: '/private/DO_NOT_RENDER', path: '/work/project/.claude/settings.json' } },
      { model: 'DO_NOT_RENDER', source: { type: 'user', label: 'User' } },
    ], mcp: [], skills: [], agents: [], hooks: [] })
    const store = useProjectResourcesStore(); store.kind = 'config'; store.setActive(true); await flushPromises()
    expect(store.items).toEqual([{ type: 'setting', name: 'model', value: 'sonnet', origin: 'project', withheld: false }])
  })
  // Scalar 标签内的相对路径、token 字段和私钥文本不会泄漏。
  it('Resources_LabelPrivacy_021', async () => {
    items = [
      { type: 'skill', name: 'relative/private/path', description: 'review', origin: 'project' },
      { type: 'skill', name: 'my_token_value', description: '-----BEGIN PRIVATE KEY-----', origin: 'project' },
    ]
    const store = useProjectResourcesStore(); store.kind = 'skills'; store.setActive(true); await flushPromises()
    expect(store.items).toMatchObject([{ name: null, withheld: true }, { name: null, description: null, withheld: true }])
  })
  // Profile 读取途中修订变化立刻清除旧数据并拒绝旧读取。
  it('Resources_ConfigFence_022', async () => {
    const tab = useNativeTabsStore().tab(tabId)!; tab.status = 'stopped'; tab.launchRevision = null
    const pending = deferred<any>(); let request: ReadRequest
    host.invoke.mockImplementation((command, value) => command === 'native_get_scope' ? source(value) : (request = value, pending.promise))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    useCliProfilesStore().profiles[0].revision = '2'
    pending.resolve(response(request!)); await flushPromises()
    expect(store.items).toEqual([]); expect(store.unavailable).toBe(true)
  })
  // 同会话 CLI 身份改变不会接纳原 CLI 的完成结果。
  it('Resources_CliFence_023', async () => {
    const pending = deferred<any>(); let request: ReadRequest
    host.invoke.mockImplementation((command, value) => command === 'native_get_scope' ? source(value) : (request = value, pending.promise))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    useUnifiedSessionsStore().sessions[0].cli = 'codex'
    pending.resolve(response(request!)); await flushPromises()
    expect(store.items).toEqual([]); expect(store.unavailable).toBe(true)
  })
  // 旧请求 reject/finally 不能改变新请求 loading 或 error。
  it('Resources_FailureFence_024', async () => {
    const old = deferred<any>(), current = deferred<any>()
    host.invoke.mockImplementation((command, value) => command === 'native_get_scope' ? source(value) : old.promise)
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    host.invoke.mockImplementation((command, value) => command === 'native_get_scope' ? source(value) : current.promise)
    selectNative('b'); await flushPromises()
    old.reject({ code: 'SCOPE_REVOKED' }); await flushPromises()
    expect(store.loading).toBe(true); expect(store.error).toBeNull()
    current.resolve(response(host.invoke.mock.calls[host.invoke.mock.calls.length - 1][1], [])); await flushPromises()
    expect(store.loading).toBe(false); expect(store.unavailable).toBe(false)
  })
  // 已运行会话不借用新配置修订，仍读取自己的 launch scope。
  it('Resources_RunKeepsSnapshot_025', async () => {
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    useCliProfilesStore().profiles[0].revision = '2'; await flushPromises(); await store.refresh()
    expect(store.items).toHaveLength(1); expect(store.unavailable).toBe(false)
    expect(host.invoke.mock.calls.filter(call => call[0] === 'native_get_scope').every(call => call[1].kind === 'run')).toBe(true)
  })
  // Legacy 终端 PTY 代次改变也拒绝旧请求。
  it('Resources_LegacyAttemptFence_026', async () => {
    const legacy = useSessionStore(); const id = 'legacy-tab'
    legacy.tabs.set(id, { tabId: id, projectPath: '/work/project', ptyId: 'pty', ptyGeneration: 1, sessionId: 'session', name: 'Legacy', status: 'running', createdAt: 1, lastActiveAt: 1, working: false, pending: false, isResume: false })
    selectLegacy(); const row = useUnifiedSessionsStore().sessions[1]; row.id = `legacy-tab:${id}`; row.adapterSessionId = id
    useUnifiedSessionsStore().activeSessionId = row.id
    const pending = deferred<any>(); host.skills.mockReturnValueOnce(pending.promise).mockResolvedValue([])
    const store = useProjectResourcesStore(); store.kind = 'skills'; store.setActive(true); await flushPromises()
    legacy.tabs.get(id)!.ptyGeneration = 2; await flushPromises()
    pending.resolve([{ name: 'old', displayName: 'old', sourceType: 'project', description: 'Old result' }]); await flushPromises()
    expect(store.items).toEqual([]); expect(store.loading).toBe(false)
  })
  // 切换分类不能把同一会话的旧说明放入设置分类。
  it('Resources_CategoryFence_027', async () => {
    const pending = deferred<any>(); let request: ReadRequest
    host.invoke.mockImplementation((command, value) => command === 'native_get_scope' ? source(value) : (request = value, pending.promise))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    host.invoke.mockImplementation((command, value) => command === 'native_get_scope' ? source(value) : response(value, []))
    store.kind = 'config'; await flushPromises(); pending.resolve(response(request!)); await flushPromises()
    expect(store.items).toEqual([]); expect(store.kind).toBe('config')
  })
  // Compatibility 面板 clear 不撤销当前资源读取的独立所有权。
  it('Resources_ProjectionIsolation_028', async () => {
    const pending = deferred<any>(); let request: ReadRequest
    host.invoke.mockImplementation((command, value) => command === 'native_get_scope' ? source(value) : (request = value, pending.promise))
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    useNativeProjectionStore().clear(); pending.resolve(response(request!)); await flushPromises()
    expect(store.items).toHaveLength(1); expect(useNativeProjectionStore().result).toBeNull()
  })
  // 切换会话发生在 scope 回复前时不能再读取已经失去选择的资源。
  it('Resources_ScopeContinuationFence_029', async () => {
    const pending = deferred<any>(); const oldSource = source({ kind: 'run', runId: useNativeTabsStore().tab(tabId)!.runId, generation: 1 })
    host.invoke.mockReturnValueOnce(pending.promise)
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    useUnifiedSessionsStore().activeSessionId = null
    pending.resolve(oldSource); await flushPromises()
    expect(host.invoke).toHaveBeenCalledTimes(1); expect(store.items).toEqual([])
  })
  // 相对路径不构成 Legacy 项目 authority，不能触发默认 home 读取。
  it('Resources_RejectRelativeProject_030', async () => {
    selectLegacy('relative'); const store = useProjectResourcesStore(); store.kind = 'config'; store.setActive(true); await flushPromises()
    expect(host.config).not.toHaveBeenCalled(); expect(store.unavailable).toBe(true)
  })
  // 非历史会话的 owning tab 消失后不能借用残留 origin。
  it('Resources_MissingOwnerNoFallback_031', async () => {
    useNativeTabsStore().close(tabId)
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(host.invoke).not.toHaveBeenCalled(); expect(store.unavailable).toBe(true)
  })
  // 历史会话只使用已保存且当前仍匹配的 profile/project origin。
  it('Resources_HistoryExactOrigin_032', async () => {
    const sessions = useUnifiedSessionsStore(); sessions.sessions[0].id = 'native-history:exact'; sessions.sessions[0].processState = 'stopped'; sessions.activeSessionId = 'native-history:exact'
    useNativeTabsStore().close(tabId)
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(host.invoke.mock.calls[0]).toEqual(['native_get_scope', { kind: 'profile', profileId: 'config', expectedProfileRevision: '1', projectId: 'project' }])
    expect(store.items).toHaveLength(1)
  })
  // Codex 运行 scope 不依赖 Claude 兼容读取或默认配置。
  it('Resources_CodexRun_033', async () => {
    selectNative('codex', 'codex')
    items = [{ type: 'document', name: 'AGENTS.md', text: 'Use focused tests.', truncated: false, origin: 'project' }]
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.items).toMatchObject([{ name: 'AGENTS.md' }]); expect(store.context?.cli).toBe('codex'); expect(host.config).not.toHaveBeenCalled()
  })
  // 正确 profile 但项目登记映射错误时，不能按传入路径自行授予 authority。
  it('Resources_ProjectBinding_034', async () => {
    const tab = useNativeTabsStore().tab(tabId)!; tab.status = 'stopped'; tab.launchRevision = null
    useWorkspaceStore().projects[0].selectedPath = '/other/project'
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(host.invoke).not.toHaveBeenCalled(); expect(store.unavailable).toBe(true)
  })
  // Windows 显式项目路径可使用原有读取契约，不进行平台环境模拟声明。
  it('Resources_WindowsProjectPath_035', async () => {
    selectLegacy('C:\\work\\project')
    const store = useProjectResourcesStore(); store.kind = 'config'; store.setActive(true); await flushPromises()
    expect(host.config).toHaveBeenCalledWith('C:\\work\\project'); expect(store.unavailable).toBe(false)
  })
  // 兼容只读路径失败不记录原始异常或污染共享 sidebar 内容。
  it('Resources_LegacySafeFailure_036', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    try {
      selectLegacy(); host.skills.mockRejectedValue(new Error('/private/TOKEN=DO_NOT_RENDER'))
      const store = useProjectResourcesStore(); store.kind = 'skills'; store.setActive(true); await flushPromises()
      expect(store.error).toBe('SOURCE_UNAVAILABLE'); expect(store.items).toEqual([]); expect(spy).not.toHaveBeenCalled()
    } finally { spy.mockRestore() }
  })
  // 实际提交后的无效回执不能被误认成未提交的 profile authority。
  it('Resources_MalformedReceiptRun_037', async () => {
    const tabs = useNativeTabsStore(); const tab = tabs.tab(tabId)!
    tab.status = 'stopped'; tab.launchRevision = null; tabs.markStarting(tabId)
    const sent = vi.fn(async () => ({}))
    const attempt = createLaunchAttempt({ requestId: tab.requestId, tabId, runId: tab.runId, generation: tab.generation, cli: tab.cli,
      profileId: tab.profileId, expectedProfileRevision: tab.profileRevision, launchCwd: tab.projectPath, action: { kind: 'new' }, extraArgs: [], cols: 80, rows: 24 },
      'instance', { start: sent, status: vi.fn() })
    try { await attempt.start() } catch (failure) { tabs.markError(tabId, publicNativeErrorCode(failure, 'NATIVE_LAUNCH_FAILED')) }
    expect(sent).toHaveBeenCalledOnce(); expect(tab.status).toBe('failed'); expect(tab.launchRevision).toBeNull()
    host.invoke.mockRejectedValue({ code: 'SCOPE_UNKNOWN' })
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(host.invoke.mock.calls).toEqual([['native_get_scope', { kind: 'run', runId: tab.runId, generation: tab.generation }]])
    expect(store.unavailable).toBe(true)
  })
  // 缺失正向未提交证明的 stopped 记录也不能退回 profile。
  it('Resources_MissingAttemptProof_038', async () => {
    const tab = useNativeTabsStore().tab(tabId)!; tab.status = 'stopped'; tab.launchRevision = null
    tab.requestId = 'replacement-without-proof'
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.context?.runtime === 'native-cli' && store.context.target.kind).toBe('run')
  })
  // 初次准备证据保留到 failed/stopped，即使没有拿到修订回执。
  it('Resources_PreparedAttemptProof_039', async () => {
    const tabs = useNativeTabsStore(); const tab = tabs.tab(tabId)!
    tab.launchRevision = null; tabs.markStarting(tabId); tabs.markError(tabId, 'INVALID_LAUNCH_RESPONSE'); tab.status = 'stopped'
    const store = useProjectResourcesStore(); store.setActive(true); await flushPromises()
    expect(store.context?.runtime === 'native-cli' && store.context.target.kind).toBe('run')
  })
  // get_all_mcp_servers 的 project 标签可能来自父目录，不能形成精确项目证明。
  it('Resources_RejectAncestorMcp_040', async () => {
    selectLegacy(); host.mcp.mockResolvedValue([{ name: 'ancestor-server', displayName: 'ancestor-server', sourceType: 'project', sourceLabel: 'Project', serverType: 'stdio' }])
    const store = useProjectResourcesStore(); store.kind = 'mcp'; store.setActive(true); await flushPromises()
    expect(store.items).toEqual([]); expect(host.mcp).not.toHaveBeenCalled()
  })
  // 已有 MCP config DTO 的 source.path 必须精确属于当前项目 .mcp.json。
  it('Resources_McpExactSourcePath_041', async () => {
    selectLegacy(); host.config.mockResolvedValue({ basic: [], skills: [], agents: [], hooks: [], mcp: [
      { name: 'exact-server', type: 'stdio', command: '/private/DO_NOT_RENDER', env: { TOKEN: 'DO_NOT_RENDER' }, source: { type: 'project', label: 'private', path: '/work/project/.mcp.json' } },
      { name: 'ancestor', source: { type: 'project', path: '/work/.mcp.json' } },
      { name: 'other', source: { type: 'project', path: '/other/.mcp.json' } },
      { name: 'missing', source: { type: 'project' } },
      { name: 'user', source: { type: 'user', path: '/work/project/.mcp.json' } },
    ] })
    const store = useProjectResourcesStore(); store.kind = 'mcp'; store.setActive(true); await flushPromises()
    expect(store.items).toEqual([{ type: 'mcp', name: 'exact-server', transport: 'stdio', origin: 'project', withheld: false }])
    expect(JSON.stringify(store.items)).not.toContain('DO_NOT_RENDER'); expect(store.partial).toBe(true)
  })
  // settings DTO 也以精确文件来源拒绝缺失/父目录/别的项目记录。
  it('Resources_ConfigExactSourcePath_042', async () => {
    selectLegacy(); host.config.mockResolvedValue({ mcp: [], skills: [], agents: [], hooks: [], basic: [
      { model: 'sonnet', source: { type: 'project', path: '/work/project/.claude/settings.json' } },
      { model: 'ancestor', source: { type: 'project', path: '/work/.claude/settings.json' } },
      { model: 'other', source: { type: 'local', path: '/other/.claude/settings.local.json' } },
      { model: 'missing', source: { type: 'project' } },
    ] })
    const store = useProjectResourcesStore(); store.kind = 'config'; store.setActive(true); await flushPromises()
    expect(store.items).toEqual([{ type: 'setting', name: 'model', value: 'sonnet', origin: 'project', withheld: false }])
  })
  // Windows 分隔符与尾斜杠仍使用统一规范化，父目录和缺失路径不通过。
  it('Resources_McpWindowsSource_043', async () => {
    selectLegacy('C:\\work\\project\\')
    host.config.mockResolvedValue({ basic: [], skills: [], agents: [], hooks: [], mcp: [
      { name: 'exact', type: 'http', source: { type: 'project', path: 'C:/work/project/.mcp.json' } },
      { name: 'ancestor', source: { type: 'project', path: 'C:/work/.mcp.json' } },
      { name: 'relative', source: { type: 'project', path: '.mcp.json' } },
    ] })
    const store = useProjectResourcesStore(); store.kind = 'mcp'; store.setActive(true); await flushPromises()
    expect(store.items).toEqual([{ type: 'mcp', name: 'exact', transport: 'http', origin: 'project', withheld: false }])
  })
})
