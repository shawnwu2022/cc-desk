import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, ref, h } from 'vue'
import { createPinia, setActivePinia, getActivePinia } from 'pinia'
import App from '@/App.vue'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import { createI18n } from 'vue-i18n'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import LaunchConfigurationEditor from '@/components/settings/LaunchConfigurationEditor.vue'
import { useUnifiedWorkspaceRuntime, historyWarningSource } from '@/composables/useUnifiedWorkspaceRuntime'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useSessionStore } from '@/stores/session'
import type { LaunchAction } from '@/types/cli'
import type { ProjectsState } from '@/types/app'
import { useAppStore } from '@/stores/app'
import { useProjectManagementStore } from '@/stores/projectManagement'
import { useShellStore } from '@/stores/shell'
import { useAttentionStore } from '@/stores/attention'

const io = vi.hoisted(() => ({ projects: vi.fn(), sessions: vi.fn(), profiles: vi.fn(), registered: vi.fn(), register: vi.fn(), patchProfile: vi.fn(), getState: vi.fn(), upsertRecord: vi.fn(), setPreference: vi.fn(), scope: vi.fn(), read: vi.fn(), prepare: vi.fn(), writeText: vi.fn(), archive: vi.fn(), restore: vi.fn(), open: vi.fn(), remove: vi.fn(), runChecks: vi.fn(), ptySpawn: vi.fn(), ptyInput: vi.fn(), ptyKill: vi.fn(), legacyRoot: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), runChecks: io.runChecks, ptySpawn: io.ptySpawn, ptyInput: io.ptyInput, ptyKill: io.ptyKill, getProjectsState: io.getState, upsertSessionUiRecord: io.upsertRecord, setProjectLaunchPreference: io.setPreference, updateAppConfig: vi.fn().mockResolvedValue(undefined), getAppConfig: vi.fn().mockResolvedValue({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' }), archiveSession: io.archive, restoreSession: io.restore, getProjects: io.projects, getSessions: io.sessions, openInFileManager: io.open, nativePrepareOpencodeSession: io.prepare, createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }), legacyClaudeRootKey: io.legacyRoot, onHookEvent: async () => () => {} }))
vi.mock('@/api/cli', () => ({ cliListProfiles: io.profiles, cliPatchProfile: io.patchProfile }))
vi.mock('@/api/cliAvailability', () => ({ cliGetAvailability: async (profileId: string, profileRevision: string) => {
  const profile = useCliProfilesStore().profile(profileId)!
  const selected = profile.programPath.mode === 'set'
  return { profileId, profileRevision, cli: profile.cli, state: selected ? 'available-unverified' : 'configuration-required', hostStatus: 'available', certified: false,
    ...(selected ? {} : { issue: { code: 'PROGRAM_TRUST_REQUIRED', retryable: false } }) }
} }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: io.registered, registerProject: io.register, removeProject: io.remove }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: io.writeText }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMaximized: async () => false, isFocused: async () => true, onFocusChanged: async () => () => {}, requestUserAttention: async () => {} }) }))
let persisted: ProjectsState
const wrappers: VueWrapper[] = []
beforeEach(() => {
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => 'legacy-tab-id' })
  localStorage.clear(); setActivePinia(createPinia()); vi.clearAllMocks(); useProjectsStateStore().loaded = true
  persisted = { pinnedProjects: [], archivedSessions: {}, launchPreferences: {} }
  io.getState.mockImplementation(async () => structuredClone(persisted))
  io.upsertRecord.mockImplementation(async (key, record) => { persisted.sessionRecords ??= {}; persisted.sessionRecords[key] = structuredClone(record); return structuredClone(persisted) })
  io.setPreference.mockImplementation(async (path, preference) => { persisted.launchPreferences![path] = structuredClone(preference); return structuredClone(persisted) })
  io.projects.mockResolvedValue([{ path: '/legacy', name: 'Legacy' }]); io.sessions.mockResolvedValue([])
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/codex' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  io.register.mockImplementation(async path => ({ revision: '2', projectId: 'registered-new', projects: [{ projectId: 'registered-new', hostId: 'host', sourcePathKey: 'source-new', selectedPath: path, canonicalPath: path, alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }))
  io.legacyRoot.mockResolvedValue({ rootKey: '', tabsUseDefaultRoot: false })
  io.prepare.mockResolvedValue('ses_prepared')
  io.scope.mockResolvedValue({ cli: 'codex' }); io.read.mockReset().mockResolvedValue({ state: 'ready', items: [{ type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }] })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); vi.useRealTimers(); vi.unstubAllGlobals() })
function render() {
  const port = { startLegacy: vi.fn(), stopLegacy: vi.fn(), restartLegacy: vi.fn(), renameLegacy: vi.fn(), stopNative: vi.fn().mockResolvedValue(undefined), recoverNative: vi.fn().mockResolvedValue(undefined), renameNative: vi.fn().mockResolvedValue(undefined), focus: vi.fn() }
  let runtime!: ReturnType<typeof useUnifiedWorkspaceRuntime>
  const w = mount(defineComponent({ setup() { runtime = useUnifiedWorkspaceRuntime(ref(port)); return () => null } })); wrappers.push(w)
  return { runtime, port }
}

it('IndexRebuild_IsolatedReaderKeepsAppAndOldListOnPartial_066', async () => {
  const { useSessionIndexMaintenanceStore } = await import('@/stores/sessionIndexMaintenance')
  const api = await import('@/api/tauri')
  const calls = vi.spyOn(api, 'sessionIndexMaintenance').mockImplementation(async(id,action)=>({id,state:action==='begin'?'staging':action==='commit'?'committed':'cancelled'}))
  render(); await flushPromises()
  const pinia=getActivePinia(), catalog=useUnifiedSessionsStore(), rows=catalog.sessions
  const maintenance=useSessionIndexMaintenanceStore()
  io.read.mockResolvedValue({state:'ready',items:[],hasMore:false,historyMetadataIncomplete:true,historyReadFailures:['SOURCE_UNSUPPORTED']})
  await maintenance.run();await flushPromises()
  expect(getActivePinia() === pinia).toBe(true)
  expect(maintenance.state).toBe('failed')
  expect(calls.mock.calls.map(call=>call[1])).toEqual(['begin','cancel'])
  expect(catalog.sessions).toBe(rows)
  calls.mockRestore()
})

it('IndexRebuild_BoundedMetadataCanPublish_067', async () => {
  const { useSessionIndexMaintenanceStore } = await import('@/stores/sessionIndexMaintenance')
  const api = await import('@/api/tauri')
  const calls=vi.spyOn(api,'sessionIndexMaintenance').mockImplementation(async(id,action)=>({id,state:action==='begin'?'staging':action==='commit'?'committed':'cancelled'}))
  render();await flushPromises()
  io.read.mockResolvedValue({state:'ready',items:[],hasMore:false,historyMetadataIncomplete:true,historyReadFailures:[]})
  const maintenance=useSessionIndexMaintenanceStore();await maintenance.run();await flushPromises()
  expect(maintenance.state).toBe('completed')
  expect(calls.mock.calls.map(call=>call[1])).toEqual(['begin','commit'])
  calls.mockRestore()
})

it('Restart_NewObservedConversationKeepsNativeIdAcrossGeneration_068', async()=>{
  const api=await import('@/api/tauri')
  const scope=vi.spyOn(api,'nativeGetScope')
  render();await flushPromises()
  const tabs=useNativeTabsStore(),created=tabs.create({cli:'codex',projectId:'project',projectPath:'/repo',profileId:'cx',profileRevision:'7',action:{kind:'new'}})
  tabs.markStarting(created.tabId)
  const tab=tabs.tab(created.tabId)!;tab.status='exited';tab.observedSessionId='history-id'
  scope.mockResolvedValue({scopeId:'run-scope',instanceId:'instance',cli:'codex',sourceRootKey:'root',identityEpoch:'1',profileId:'cx',profileRevision:'7',target:{kind:'run',runId:tab.runId,generation:tab.generation},basis:'launch-environment'})
  io.scope.mockResolvedValue({cli:'codex',sourceRootKey:'root'})
  io.read.mockResolvedValue({state:'ready',items:[{type:'session',projectId:'project',sessionKey:JSON.stringify(['local','codex','root','history-id']),nativeSessionId:'history-id',title:'History',cwd:'/repo'}],hasMore:false})
  const catalog=useUnifiedSessionsStore();await catalog.refresh()
  await catalog.restartSession(`native-tab:${created.tabId}`)
  expect(tabs.tab(created.tabId)).toMatchObject({generation:2,action:{kind:'resume-id',nativeSessionId:'history-id'},sourceSessionKey:JSON.stringify(['local','codex','root','history-id'])})
  scope.mockRestore()
})
describe('Workspace source warning diagnostics', () => {
  // 无身份的来源错误不能冒认会话；明确会话错误则精确定位，忽略后新问题再次提示。
  it('HistoryRecovery_ExactAndUnknown_080', async () => {
    const item = { type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'Known', cwd: '/repo', readFailure: 'SOURCE_INVALID' }
    io.read.mockResolvedValue({ state: 'ready', items: [item], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_INVALID'] })
    const { runtime } = render(); await flushPromises()
    expect(runtime.historyIssues.value.some(issue => issue.sessionId === 'history-id' && issue.code === 'SOURCE_INVALID')).toBe(true)
    expect(runtime.historyIssues.value.some(issue => issue.sessionId === undefined && issue.code === 'SOURCE_INVALID')).toBe(true)
    expect(useUnifiedSessionsStore().sessions.find(row => row.nativeSessionId === 'history-id')?.historyReadFailure).toBe('SOURCE_INVALID')
    runtime.dismissSourceNotice()
    await runtime.refresh()
    expect(runtime.historyNoticeDismissed.value).toBe(true)
    io.read.mockResolvedValue({ state: 'ready', items: [{ ...item, sessionKey: 'new-key', nativeSessionId: 'new-id' }], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_INVALID'] })
    await runtime.refresh()
    expect(runtime.historyNoticeDismissed.value).toBe(false)
  })
  // 针对单一已授权上下文重扫，成功清除问题，旧身份动作不再有效。
  it('HistoryRecovery_TargetedRetry_081', async () => {
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED'] })
    const { runtime } = render(); await flushPromises()
    const issue = runtime.historyIssues.value[0]
    io.scope.mockClear(); io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
    await runtime.retryHistoryIssue(issue.key)
    expect(io.scope).toHaveBeenCalledTimes(1)
    expect(io.scope.mock.calls[0][0]).toMatchObject({ profileId: 'cx', expectedProfileRevision: '7', projectIds: ['project'] })
    expect(runtime.historyIssues.value).toEqual([])
    expect(runtime.historyMetadataPartial.value).toBe(false)
    io.scope.mockClear(); await runtime.retryHistoryIssue(issue.key)
    expect(io.scope).not.toHaveBeenCalled()
  })
  // 来源级错误重试成功后撤销旧汇总和确认；同一错误重新发生也应再次提示。
  it('HistoryRecovery_SourceRetryClears_082', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_UNSUPPORTED', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    const issue = runtime.historyIssues.value[0]
    expect(issue.sessionId).toBeUndefined()
    runtime.dismissSourceNotice()
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
    await runtime.retryHistoryIssue(issue.key)
    expect(runtime.sourceDiagnosticsWarnings.value).toEqual([])
    expect(runtime.sourceWarnings.value).toEqual([])
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_UNSUPPORTED', items: [], hasMore: false })
    await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })
  // 修复部分已确认的问题，不得让其它完全未变化的已忽略问题重新亮横幅。
  it('HistoryRecovery_PartialRepairKeepsAck_083', async () => {
    const item = { type: 'session', projectId: 'project', sessionKey: 'one', nativeSessionId: 'one', title: 'Known', cwd: '/repo', metadataIncomplete: true, readFailure: 'SOURCE_INVALID' }
    const response = { state: 'ready', items: [item, { ...item, sessionKey: 'two', nativeSessionId: 'two' }], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_INVALID'] }
    io.read.mockResolvedValue(response)
    const { runtime } = render(); await flushPromises()
    runtime.dismissSourceNotice()
    io.read.mockResolvedValue({ ...response, items: [{ ...response.items[0], readFailure: undefined }, response.items[1]] })
    await runtime.refresh()
    expect(runtime.historyIssues.value.filter(row => row.sessionId).map(row => row.sessionId)).toEqual(['two'])
    expect(runtime.historyNoticeDismissed.value).toBe(true)
  })
  // 后台只改变诊断字段仍须发布受影响项目；恢复时同一列表标记随之撤销。
  it('HistoryRecovery_DiagnosticProjection_084', async () => {
    const { runtime } = render(); await flushPromises()
    const entry = useNativeHistoryStore().all()[0]
    entry.sessions[0].readFailure = 'SOURCE_INVALID'
    await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(row => row.nativeSessionId === 'history-id')?.historyReadFailure).toBe('SOURCE_INVALID')
    delete entry.sessions[0].readFailure
    await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(row => row.nativeSessionId === 'history-id')?.historyReadFailure).toBeUndefined()
    expect(runtime.historyIssues.value.filter(row => row.sessionId)).toEqual([])
  })
  // 配置修订撤销后，旧详情上的路径和重试按钮不能作为读取授权。
  it('HistoryRecovery_StaleOwner_085', async () => {
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_INVALID'] })
    const { runtime } = render(); await flushPromises()
    const issue = runtime.historyIssues.value[0]
    useCliProfilesStore().profiles[0].revision = '8'
    io.scope.mockClear(); io.open.mockClear()
    await runtime.retryHistoryIssue(issue.key)
    await runtime.openHistoryIssueProject(issue.key)
    expect(io.scope).not.toHaveBeenCalled()
    expect(io.open).not.toHaveBeenCalled()
  })
  // 单条历史读取失败保留已识别会话，仅显示可收起的部分历史诊断。
  it('HistoryPartial_KeepsPositiveRows_001', async () => {
    io.read.mockResolvedValue({ state: 'ready', items: [{ type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }], hasMore: false,
      historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED'] })
    const { runtime } = render(); await flushPromises()
    expect(useUnifiedSessionsStore().sessions.some(row => row.nativeSessionId === 'history-id')).toBe(true)
    expect(runtime.sourceWarnings.value).toEqual([])
    expect(runtime.error.value).toBeNull()
    expect(runtime.historyReadWarnings?.value).toEqual([{ source: 'codex-history', code: 'SOURCE_UNSUPPORTED', stage: 'read-source-enumeration' }])
    expect(useNativeHistoryStore().all()[0].absenceEvidence).toBeUndefined()
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
    await runtime.refresh()
    expect(runtime.historyReadWarnings?.value).toEqual([])
    expect(runtime.historyMetadataPartial.value).toBe(false)
  })
  // 部分历史诊断默认收起，不展示来源整体失败横幅；原始错误不进入UI。
  it('HistoryPartial_AppCompactDetails_002', async () => {
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_INVALID'] })
    const i18n = createI18n({ legacy: false, locale: 'en', messages: { en, zh } })
    const w = mount(App, { global: { plugins: [i18n], stubs: { NativeCliTerminal: true, SettingsView: true } } })
    wrappers.push(w); await flushPromises()
    expect(useNativeHistoryStore().all()[0].readFailures).toEqual(['SOURCE_INVALID'])
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(false)
    const details = w.find('[data-history-read-details]')
    expect(details.exists()).toBe(true)
    expect((details.element as HTMLDetailsElement).open).toBe(false)
    expect(details.text()).toContain('SOURCE_INVALID')
    expect(details.text()).not.toContain('still unavailable')
    i18n.global.locale.value = 'zh'; await flushPromises()
    expect(details.find('summary').text()).toBe('历史读取详情（部分条目已略过）')
  })
  // 真关闭提示；加载中的临时状态与相同重试结果不能重置确认状态。
  it('Warnings_DismissRetry_001', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_TOO_LARGE', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    expect(runtime.sourceNoticeDismissed?.value).toBe(false)
    runtime.dismissSourceNotice()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    expect(runtime.error.value).toBe('workspaceRuntimePartial')
    expect(runtime.sourceWarnings.value[0].code).toBe('SOURCE_TOO_LARGE')
    let finish!: (value: unknown) => void
    io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    const refresh = runtime.refresh(); await flushPromises()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    finish({ state: 'unavailable', reason: 'SOURCE_TOO_LARGE', items: [], hasMore: false }); await refresh
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
  })

  // 新代码必须重新提示，成功检查之后相同失败复发也必须重新提示。
  it('Warnings_NewAndRecur_002', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_TOO_LARGE', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_UNSUPPORTED', items: [], hasMore: false })
    await runtime.refresh(); expect(runtime.sourceNoticeDismissed.value).toBe(false)
    runtime.dismissSourceNotice()
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
    await runtime.refresh(); expect(runtime.sourceWarnings.value).toEqual([])
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_UNSUPPORTED', items: [], hasMore: false })
    await runtime.refresh(); expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  // canonical配置名称不参与身份；同码新增配置不能继承另一个配置的关闭记录。
  it('Warnings_ProfileIdentity_003', async () => {
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation', profileId: 'forged', name: 'raw-secret' })
    const { runtime } = render(); await flushPromises()
    expect(runtime.sourceWarningConfigurations.value.map(row => row.name)).toEqual(['CX'])
    expect(JSON.stringify(runtime.sourceWarningConfigurations.value)).not.toContain('raw-secret')
    runtime.dismissSourceNotice()
    const original = structuredClone(io.profiles.mock.results[0].value instanceof Promise ? await io.profiles.mock.results[0].value : {})
    original.profiles[0].name = 'Renamed CX'
    io.profiles.mockResolvedValue(original); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    io.profiles.mockResolvedValue({ ...original, profiles: [...original.profiles, { ...original.profiles[0], id: 'cx-second', name: 'Second CX' }] })
    await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
    expect(runtime.sourceWarningConfigurations.value.map(row => row.name)).toEqual(['Renamed CX', 'Second CX'])
    expect(runtime.sourceWarnings.value).toEqual([{ source: 'codex-history', code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation' }])
  })

  // 两个来源中一个恢复不重新提示未变化的另一个，前者再次失败则重新提示。
  it('Warnings_SubsetRecur_006', async () => {
    io.projects.mockRejectedValue({ code: 'SOURCE_BUSY' })
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation' })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    io.projects.mockResolvedValue([{ path: '/legacy', name: 'Legacy' }]); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    expect(runtime.sourceWarnings.value).toHaveLength(1)
    io.projects.mockRejectedValue({ code: 'SOURCE_BUSY' }); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  // 同一个后台catalog失败保持收起；后台出现新的固定代码则重新提示。
  it('Warnings_CatalogRepeat_007', async () => {
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation' })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    vi.spyOn(useUnifiedSessionsStore(), 'refresh').mockRejectedValue({ code: 'SOURCE_BUSY' })
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    await flushPromises(); expect(runtime.sourceNoticeDismissed.value).toBe(false)
    runtime.dismissSourceNotice()
    tab.title = 'Updated'; await flushPromises()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    expect(runtime.sourceWarnings.value).toHaveLength(2)
  })

  // 同源同码在两个配置间交换仍是新的受影响配置，不能把警告键和配置键拆成两个集合。
  it('Warnings_ProfileSwap_008', async () => {
    const original = await io.profiles()
    io.profiles.mockResolvedValue({ ...original, profiles: [...original.profiles, { ...original.profiles[0], id: 'cx-second', name: 'Second CX' }] })
    let swap = false
    io.scope.mockImplementation(async (target: { profileId: string }) => { throw { code: (target.profileId === 'cx') !== swap ? 'SCOPE_UNKNOWN' : 'SCOPE_STALE', stage: 'scope-profile-validation' } })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    swap = true; await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  // 十二条显示上限之外的新代码和新配置同样重新提示，显示集合保持不变。
  it('Warnings_OmittedIdentity_009', async () => {
    const codes = ['SCOPE_UNKNOWN', 'SCOPE_STALE', 'SCOPE_REVOKED', 'SCOPE_CAPACITY', 'SCOPE_EPOCH_EXHAUSTED', 'SCOPE_UNAVAILABLE', 'SOURCE_UNSUPPORTED', 'SOURCE_INVALID', 'SOURCE_INVALID_TEXT', 'SOURCE_PATH_REJECTED', 'SOURCE_CHANGED', 'SOURCE_NOT_REGULAR', 'SOURCE_TOO_LARGE', 'SOURCE_TOO_MANY_ENTRIES', 'SOURCE_UNSUPPORTED']
    const original = await io.profiles()
    const profiles = codes.map((_code, index) => ({ ...original.profiles[0], id: `cx-${index}`, name: `Configuration ${index}` }))
    io.profiles.mockResolvedValue({ ...original, profiles: profiles.slice(0, 13) })
    io.scope.mockImplementation(async (target: { profileId: string }) => { throw { code: codes[Number(target.profileId.slice(3))] ?? 'SOURCE_UNSUPPORTED', stage: 'scope-profile-validation' } })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    const visible = structuredClone(runtime.sourceWarnings.value.map(row => ({ ...row })))
    expect(visible).toHaveLength(12)
    io.profiles.mockResolvedValue({ ...original, profiles: profiles.slice(0, 14) }); await runtime.refresh()
    expect(runtime.sourceWarnings.value).toEqual(visible)
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
    runtime.dismissSourceNotice()
    io.profiles.mockResolvedValue({ ...original, profiles }); await runtime.refresh()
    expect(runtime.sourceWarnings.value).toEqual(visible)
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
    runtime.dismissSourceNotice()
    io.profiles.mockResolvedValue({ ...original, profiles: [...profiles].reverse() }); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    expect(runtime.sourceWarnings.value).toEqual(visible)
    io.profiles.mockResolvedValue({ ...original, profiles: [...profiles.slice(0, 14), { ...profiles[14], id: 'cx-16' }] }); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
    expect(runtime.sourceWarnings.value).toEqual(visible)
  })

  // Dismiss removes terminal notices, retaining an explicit source entry and dialog.
  it('Warnings_AppDismissLocale_004', async () => {
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation', field: '/private/path', name: 'raw-secret' })
    const i18n = createI18n({ legacy: false, locale: 'en', messages: { en, zh } })
    const w = mount(App, { attachTo: document.body, global: { plugins: [i18n], stubs: { NativeCliTerminal: true, SettingsView: true, LaunchConfigurationEditor: true } } })
    wrappers.push(w); await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(true)
    expect(w.find('[data-workspace-source-details]').text()).toContain('CX')
    ;(w.find('[data-dismiss-source-notice]').element as HTMLElement).focus()
    await w.find('[data-dismiss-source-notice]').trigger('click'); await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(false)
    expect(w.find('[data-workspace-source-compact]').exists()).toBe(false)
    expect(w.find('[data-workspace-source-details]').exists()).toBe(false)
    const entry = w.find('[data-history-source-entry]')
    expect(entry.exists()).toBe(true)
    expect(document.activeElement).toBe(entry.element)
    await entry.trigger('click'); await flushPromises()
    const dialog = () => document.querySelector('[data-history-source-dialog]')!
    expect(dialog().textContent).toContain('SCOPE_UNKNOWN')
    expect(dialog().textContent).not.toMatch(/raw-secret|private\/path/)
    i18n.global.locale.value = 'zh'; await flushPromises()
    expect(dialog().textContent).toContain(zh.sourceWarningScopeUnknown)
    ;(dialog().querySelector('[data-source-warning-retry]') as HTMLElement).click(); await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(false)
    ;(dialog().querySelector('[data-source-warning-configuration]') as HTMLElement).click(); await flushPromises()
    expect(w.findComponent(LaunchConfigurationEditor).props('request')).toEqual({ kind: 'edit', profileId: 'cx' })
    useShellStore().navigate('projects'); await flushPromises()
    expect(document.querySelector('[data-history-source-dialog]')).toBeNull()
    useShellStore().navigate('workspace'); await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(false)
    await w.find('[data-history-source-entry]').trigger('click'); await flushPromises()
    expect(dialog().textContent).toContain('SCOPE_UNKNOWN')
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); await flushPromises()
    expect(document.querySelector('[data-history-source-dialog]')).toBeNull()
  })

  it('HistoryPartial_DismissRetryNavigation_003', async () => {
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED'] })
    const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } })
    wrappers.push(w); await flushPromises()
    expect(w.find('[data-history-metadata-partial]').exists()).toBe(true)
    await w.find('[data-dismiss-history-notice]').trigger('click'); await flushPromises()
    expect(w.find('[data-history-metadata-partial]').exists()).toBe(false)
    expect(w.find('[data-history-read-details]').exists()).toBe(false)
    expect(useNativeHistoryStore().all()[0].metadataIncomplete).toBe(true)
    useShellStore().requestWorkspaceAction({ kind: 'refresh' }); await flushPromises()
    expect(w.find('[data-history-metadata-partial]').exists()).toBe(false)
    useShellStore().navigate('projects'); useShellStore().navigate('workspace'); await flushPromises()
    expect(w.find('[data-history-metadata-partial]').exists()).toBe(false)
    await w.find('[data-history-source-entry]').trigger('click'); await flushPromises()
    expect(document.querySelector('[data-history-source-dialog]')?.textContent).toContain('SOURCE_UNSUPPORTED')
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_INVALID'] })
    useShellStore().requestWorkspaceAction({ kind: 'refresh' }); await flushPromises()
    expect(w.find('[data-history-metadata-partial]').exists()).toBe(true)
  })

  it('HistoryPartial_PendingStaleRecovery_004', async () => {
    const partial = { state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED'] }
    io.read.mockResolvedValue(partial)
    const { runtime } = render(); await flushPromises()
    runtime.dismissSourceNotice()
    expect(runtime.historyNoticeDismissed.value).toBe(true)
    let finish!: (value: unknown) => void
    io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    const old = runtime.refresh(); await flushPromises()
    // A temporary unloaded entry is not a successful source observation.
    expect(runtime.historyMetadataPartial.value).toBe(false)
    runtime.dismissSourceNotice() // An unrelated notice may be dismissed while reads are pending.
    await runtime.refresh()
    expect(runtime.historyNoticeDismissed.value).toBe(true)
    finish({ ...partial, historyReadFailures: ['SOURCE_INVALID'] }); await old; await flushPromises()
    expect(runtime.historyNoticeDismissed.value).toBe(true)
    expect(runtime.historyReadWarnings.value[0].code).toBe('SOURCE_UNSUPPORTED')
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
    await runtime.refresh()
    expect(runtime.sourceIssuesAvailable.value).toBe(false)
    io.read.mockResolvedValue(partial); await runtime.refresh()
    expect(runtime.historyNoticeDismissed.value).toBe(false)
  })

  it('Warnings_AppDismissesBothKinds_012', async () => {
    const configured = await io.profiles()
    io.profiles.mockResolvedValue({ ...configured, profiles: [...configured.profiles, { ...configured.profiles[0], id: 'claude-profile', cli: 'claude' }] })
    io.scope.mockImplementation(async ({ profileId }) => ({ cli: profileId === 'cx' ? 'codex' : 'claude' }))
    io.read.mockImplementation(async ({ source }) => source.cli === 'codex'
      ? { state: 'unavailable', reason: 'SOURCE_BUDGET_EXCEEDED', items: [], hasMore: false }
      : { state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED'] })
    const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } })
    wrappers.push(w); await flushPromises()
    expect(w.find('[data-workspace-source-details]').text()).toContain('five-second time budget')
    expect(w.find('[data-history-metadata-partial]').exists()).toBe(true)
    await w.find('[data-dismiss-source-notice]').trigger('click'); await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(false)
    expect(w.find('[data-history-metadata-partial]').exists()).toBe(false)
    expect(w.find('[data-history-read-details]').exists()).toBe(false)
    await w.find('[data-history-source-entry]').trigger('click'); await flushPromises()
    expect(document.querySelector('[data-history-source-dialog]')?.textContent).toContain('SOURCE_BUDGET_EXCEEDED')
    expect(document.querySelector('[data-history-source-dialog]')?.textContent).toContain('SOURCE_UNSUPPORTED')
  })

  it('HistoryPartial_LastKnownActivity_006', async () => {
    // Display fallback requires a positively admitted exact root, never absent identity.
    io.scope.mockResolvedValue({ cli: 'codex', sourceRootKey: 'admitted-root-key' })
    const item = { type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'Known task', cwd: '/repo', updatedAt: '2026-10-10T11:00:00Z' }
    io.read.mockResolvedValue({ state: 'ready', items: [item], hasMore: false })
    const { runtime } = render(); await flushPromises()
    const known = Date.parse(item.updatedAt)
    expect(useUnifiedSessionsStore().sessions[0].lastActivityAt).toBe(known)
    const { updatedAt: _updated, ...summary } = item
    io.read.mockResolvedValue({ state: 'ready', items: [{ ...summary, truncated: true }], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_TOO_LARGE'] })
    await runtime.refresh()
    expect(useNativeHistoryStore().all()[0].sessions[0].updatedAt).toBeUndefined()
    expect(useNativeHistoryStore().all()[0].absenceEvidence).toBeUndefined()
    expect(runtime.historyMetadataPartial.value).toBe(true)
    expect(useUnifiedSessionsStore().sessions[0]).toMatchObject({ lastActivityAt: known, activityState: 'unknown' })
    io.read.mockResolvedValue({ state: 'ready', items: [{ ...summary, sessionKey: 'different-root-key', truncated: true }], hasMore: false, historyMetadataIncomplete: true })
    await runtime.refresh()
    expect(useUnifiedSessionsStore().sessions[0].lastActivityAt).toBe(0)
  })

  it('Warnings_BackgroundDiagnosticsTruncated_015', async () => {
    const configured = await io.profiles()
    io.profiles.mockResolvedValue({ ...configured, profiles: Array.from({ length: 14 }, (_, index) => ({ ...configured.profiles[0], id: `p-${index}`, cli: index < 7 ? 'codex' : 'claude' })) })
    io.scope.mockImplementation(async ({ profileId }) => ({ cli: Number(profileId.slice(2)) < 7 ? 'codex' : 'claude' }))
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    const codes = ['SOURCE_UNAVAILABLE', 'SOURCE_BUSY', 'SOURCE_UNSUPPORTED', 'SOURCE_TOO_LARGE', 'SOURCE_BUDGET_EXCEEDED', 'SOURCE_CHANGED', 'SOURCE_AMBIGUOUS']
    for (const entry of useNativeHistoryStore().all()) {
      entry.error = codes[Number(entry.context.profileId.slice(2)) % codes.length]
      entry.diagnosticStage = 'read-source-enumeration'
    }
    await flushPromises()
    expect(runtime.sourceWarningsTruncated.value).toBe(false)
    expect(runtime.sourceDiagnosticsWarnings.value).toHaveLength(12)
    expect(runtime.sourceDiagnosticsTruncated?.value).toBe(true)
  })

  it('Warnings_SourceRootIdentity_011', async () => {
    io.scope.mockResolvedValue({ cli: 'codex', sourceRootKey: 'root-one' })
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_BUDGET_EXCEEDED', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    expect(useNativeHistoryStore().all()[0]).toHaveProperty('sourceRootKey', 'root-one')
    runtime.dismissSourceNotice()
    io.scope.mockResolvedValue({ cli: 'codex', sourceRootKey: 'root-two' })
    await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  it('HistoryPartial_DrawerPendingIsNotHealthy_005', async () => {
    const partial = { state: 'ready', items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_UNSUPPORTED'] }
    io.read.mockResolvedValue(partial)
    const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } })
    wrappers.push(w); await flushPromises()
    await w.find('[data-dismiss-history-notice]').trigger('click'); await flushPromises()
    await w.find('[data-history-source-entry]').trigger('click'); await flushPromises()
    let finish!: (value: unknown) => void
    io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    ;(document.querySelector('[data-source-warning-retry]') as HTMLElement).click(); await flushPromises()
    expect(document.querySelector('[data-history-source-dialog]')?.textContent).not.toContain('History sources are up to date.')
    expect(document.querySelector('[data-history-source-dialog]')?.textContent).toContain(en.loading)
    finish(partial); await flushPromises()
    expect(w.find('[data-history-metadata-partial]').exists()).toBe(false)
    // Close/restore adapters can force this exact context without a runtime refresh.
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_BUDGET_EXCEEDED', items: [], hasMore: false })
    await useNativeHistoryStore().load({ ...useNativeHistoryStore().all()[0].context, force: true }); await flushPromises()
    expect(w.find('[data-history-source-entry]').exists()).toBe(true)
    expect(document.querySelector('[data-history-source-dialog]')?.textContent).toContain('SOURCE_BUDGET_EXCEEDED')
    expect(document.querySelector('[data-history-source-dialog]')?.textContent).not.toContain('History sources are up to date.')
  })

  it('Warnings_DismissHiddenSidebarFocus_014', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_BUDGET_EXCEEDED', items: [], hasMore: false })
    const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } })
    wrappers.push(w); await flushPromises()
    useShellStore().sidebarVisible = false; await flushPromises()
    ;(w.find('[data-dismiss-source-notice]').element as HTMLElement).focus()
    await w.find('[data-dismiss-source-notice]').trigger('click'); await flushPromises()
    expect(document.activeElement).toBe(w.find('[data-primary-section="workspace"]').element)
    useShellStore().sidebarVisible = true; await flushPromises()
    await w.find('[data-history-source-entry]').trigger('click'); await flushPromises()
    expect(document.querySelector('[data-history-source-dialog]')).not.toBeNull()
    useShellStore().sidebarVisible = false; await flushPromises()
    expect(document.querySelector('[data-history-source-dialog]')).toBeNull()
  })

  it('Warnings_ThrownReadRootIdentity_013', async () => {
    io.scope.mockResolvedValue({ cli: 'codex', sourceRootKey: 'root-one' })
    io.read.mockRejectedValue({ code: 'SOURCE_READ_FAILED' })
    const { runtime } = render(); await flushPromises()
    runtime.dismissSourceNotice()
    io.scope.mockResolvedValue({ cli: 'codex', sourceRootKey: 'root-two' })
    await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  it('Warnings_ProfileRevision_010', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_BUDGET_EXCEEDED', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    runtime.dismissSourceNotice()
    const current = await io.profiles()
    io.profiles.mockResolvedValue({ ...current, profiles: current.profiles.map((profile: object) => ({ ...profile, revision: '8' })) })
    await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  it('Runtime_RefreshDropsStaleRevisionHistory_050', async () => {
    const { runtime } = render(); await flushPromises()
    const history = useNativeHistoryStore()
    expect(history.all().map(entry => entry.context.profileRevision)).toEqual(['7'])
    const current = await io.profiles()
    io.profiles.mockResolvedValue({ ...current, profiles: current.profiles.map((profile: object) => ({ ...profile, revision: '8' })) })
    await runtime.refresh(); await flushPromises()
    // 旧 revision 的幽灵 entry 必须被清理，否则同一会话会同时出现两行。
    expect(history.all().map(entry => entry.context.profileRevision)).toEqual(['8'])
  })

  it('Runtime_GroupsStartupHistoryByProfile_051', async () => {
    io.registered.mockResolvedValue({ revision: '1', projects: [
      { projectId: 'project-1', hostId: 'host', sourcePathKey: 's1', selectedPath: '/repo-a', canonicalPath: '/repo-a', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } },
      { projectId: 'project-2', hostId: 'host', sourcePathKey: 's2', selectedPath: '/repo-b', canonicalPath: '/repo-b', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } },
    ] })
    io.read.mockResolvedValue({ state: 'ready', reason: null, items: [], hasMore: false })
    render(); await flushPromises()
    // 同一 profile 的多个项目必须合并为一次后端观察：每个项目各扫一次全库是启动巨慢的根因。
    expect(io.scope).toHaveBeenCalledExactlyOnceWith({ kind: 'profile', profileId: 'cx', expectedProfileRevision: '7', projectIds: ['project-1', 'project-2'] })
  })

  it.each(['profile-revision', 'profile-removal', 'project-removal'] as const)('Runtime_InvalidatesPendingContext_052_%s', async change => {
    let finishOld!: (value: unknown) => void
    io.read.mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve }))
    const { runtime } = render(); await flushPromises()
    const history = useNativeHistoryStore()
    expect(history.all().map(entry => [entry.context.profileRevision, entry.loading])).toEqual([['7', true]])
    const current = await io.profiles()
    if (change === 'profile-revision') {
      io.profiles.mockResolvedValue({ ...current, profiles: current.profiles.map((profile: object) => ({ ...profile, revision: '8' })) })
    } else if (change === 'profile-removal') {
      io.profiles.mockResolvedValue({ ...current, profiles: [] })
    } else {
      io.registered.mockResolvedValue({ revision: '2', projects: [] })
    }
    await runtime.refresh(); await flushPromises()
    // 配置撤销立即失效旧 owner，不能等旧 read 结束才删除其 entry。
    expect(history.all().some(entry => entry.context.profileRevision === '7')).toBe(false)
    finishOld({ state: 'ready', items: [{ type: 'session', projectId: 'project', sessionKey: 'late-root', nativeSessionId: 'late-history', title: 'Obsolete', cwd: '/repo' }] })
    await flushPromises()
    expect(history.all().map(entry => entry.context.profileRevision)).toEqual(change === 'profile-revision' ? ['8'] : [])
    expect(useUnifiedSessionsStore().sessions.some(row => row.nativeSessionId === 'late-history')).toBe(false)
  })

  it('reprojects the catalog when the legacy root proof arrives late', async () => {
    // 根证明 IPC 晚于首次历史加载完成时，仅更新非响应式状态不会改动已投影的行；
    // 必须在证明到达后重投影，否则重复行会一直保留到下一次无关刷新。
    let resolveProof: ((value: unknown) => void) | undefined
    io.legacyRoot.mockImplementation(() => new Promise(resolve => { resolveProof = resolve }))
    const { runtime } = render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const refresh = vi.spyOn(catalog, 'refresh')
    expect(resolveProof).toBeTypeOf('function')
    expect(refresh).not.toHaveBeenCalled()
    resolveProof!({ rootKey: 'admitted-root-key', tabsUseDefaultRoot: true })
    await flushPromises(); await flushPromises()
    expect(runtime.error.value).toBeNull()
    expect(refresh).toHaveBeenCalled()
  })

  // 根替换或验证失败后，本 runtime 不能把旧终端/缓存的证明重新绑定到新目录。
  it.each([{ failure: 'replaced', opened: false }, { failure: 'unavailable', opened: false }, { failure: 'replaced', opened: true }])('Runtime_LegacyRootLeaseRevoked_066: $failure/$opened', async ({ failure, opened }) => {
    io.projects.mockResolvedValue([{ path: '/repo', name: 'Repo' }])
    io.sessions.mockResolvedValue([{ sessionId: 'history-id', name: 'Legacy', projectPath: '/repo', lastActiveAt: 10 }])
    io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cl', revision: '7', cli: 'claude', name: 'CL', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
    io.scope.mockResolvedValue({ cli: 'claude', sourceRootKey: 'root-a' })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root-a' })
    useProjectsStateStore().archivedSessions.set('/repo', ['history-id'])
    const { runtime } = render(); await flushPromises(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    expect(runtime.ready.value).toBe(true)
    expect(catalog.sessions.filter(row => row.nativeSessionId === 'history-id')).toHaveLength(1)
    expect(catalog.sessions.find(row => row.nativeSessionId === 'history-id')).toMatchObject({ runtime: 'native-cli', archived: true })
    if (opened) {
      const legacy = useSessionStore(), tabId = legacy.createTab('/repo', { sessionId: 'history-id' })
      legacy.setTabPty(tabId, 'legacy-pty')
      legacy.setTabRootProof(tabId, 'legacy-pty', true)
      await catalog.refresh()
      expect(catalog.sessions.filter(row => row.nativeSessionId === 'history-id')).toHaveLength(1)
      expect(catalog.sessions.find(row => row.nativeSessionId === 'history-id')).toMatchObject({ runtime: 'legacy-claude', opened: true, archived: false })
    }
    if (failure === 'replaced') io.legacyRoot.mockResolvedValue({ rootKey: 'root-b' })
    else io.legacyRoot.mockRejectedValue(new Error('SOURCE_UNAVAILABLE'))
    await catalog.refresh()
    expect(catalog.sessions.filter(row => row.nativeSessionId === 'history-id')).toHaveLength(2)
    expect(catalog.sessions.find(row => row.runtime === 'legacy-claude')).toMatchObject({ archived: !opened })
    expect(catalog.sessions.find(row => row.runtime === 'native-cli')).toMatchObject({ archived: false })
    expect(io.restore).not.toHaveBeenCalled()
    io.legacyRoot.mockResolvedValue({ rootKey: 'root-a' })
    await catalog.refresh()
    expect(catalog.sessions.filter(row => row.nativeSessionId === 'history-id')).toHaveLength(2)
  })

  it('keeps incomplete empty metadata visible without turning positive history into a failed source', async () => {
    // 仅截断观察（无省略）是 Claude 有界读取的常态：缺席证据仍被抑制，但不再
    // 常驻横幅；确有省略（读失败）时横幅才亮起并在完整刷新后熄灭。
    io.read.mockResolvedValue({ state: 'ready', reason: null, items: [], hasMore: false, historyMetadataIncomplete: true })
    const { runtime } = render(); await flushPromises()
    expect(runtime.error.value).toBeNull()
    expect(runtime.historyMetadataPartial.value).toBe(false)
    io.read.mockResolvedValue({ state: 'ready', reason: null, items: [], hasMore: false, historyMetadataIncomplete: true, historyReadFailures: ['SOURCE_TOO_LARGE'] })
    await runtime.refresh()
    expect(runtime.historyMetadataPartial.value).toBe(true)
    io.read.mockResolvedValue({ state: 'ready', reason: null, items: [], hasMore: false })
    await runtime.refresh()
    expect(runtime.historyMetadataPartial.value).toBe(false)
  })
  it('retains authenticated safe error codes while hiding private exception fields and preserving open sessions', async () => {
    io.projects.mockRejectedValue(new Error('private C:\\users\\secret token=hidden'))
    io.scope.mockRejectedValue({ code: 'FORBIDDEN', field: 'private-path', message: 'token=hidden', retryable: false })
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' }, title: 'Private session title' })
    const { runtime } = render(); await flushPromises()
    expect(runtime.error.value).toBe('workspaceRuntimePartial')
    expect(runtime.sourceWarnings?.value).toEqual([
      { source: 'project-discovery', code: 'SOURCE_UNAVAILABLE' },
      { source: 'codex-history', code: 'FORBIDDEN', stage: 'scope-invoke' },
    ])
    expect(runtime.openSessions.value.some(row => row.adapterSessionId === tab.tabId)).toBe(true)
    expect(JSON.stringify(runtime.sourceWarnings?.value)).not.toMatch(/private|token|secret|title/i)
  })

  it('retains an unavailable history reason and clears its warning only after a successful refresh', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_TOO_LARGE', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    expect(runtime.sourceWarnings?.value).toEqual([{ source: 'codex-history', code: 'SOURCE_TOO_LARGE', stage: 'read-source-enumeration' }])
    expect(runtime.error.value).toBe('workspaceRuntimePartial')
    io.read.mockResolvedValue({ state: 'ready', reason: null, items: [], hasMore: false })
    await runtime.refresh()
    expect(runtime.sourceWarnings?.value).toEqual([])
    expect(runtime.error.value).toBeNull()
  })

  it('does not let an old refresh publish source failures after a newer successful refresh', async () => {
    const { runtime } = render(); await flushPromises()
    let rejectOld!: (reason: unknown) => void
    io.scope.mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectOld = reject }))
    const old = runtime.refresh(); await flushPromises()
    await runtime.refresh()
    rejectOld({ code: 'FORBIDDEN', retryable: false }); await old
    expect(runtime.sourceWarnings?.value).toEqual([])
    expect(runtime.error.value).toBeNull()
  })

  it('shows the failed source and safe code in collapsed details in the actual App warning', async () => {
    io.scope.mockRejectedValue({ code: 'SOURCE_READ_FORBIDDEN', stage: 'scope-source-root', field: '/private/path', message: 'raw-secret' })
    const w = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } })
    wrappers.push(w); await flushPromises()
    const details = w.find('[data-workspace-source-details]')
    expect(details.exists()).toBe(true)
    expect(details.attributes('open')).toBeUndefined()
    expect(details.text()).toContain('Codex CLI history')
    expect(details.text()).toContain('SOURCE_READ_FORBIDDEN')
    expect(details.text()).toContain('scope-source-root')
    expect(details.text()).not.toMatch(/private|raw-secret/)
    expect(w.text()).toContain(en.workspaceRuntimePartial)
  })

  it('attributes later catalog failures without allowing an older catalog read to overwrite a refreshed result', async () => {
    const { runtime } = render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const original = catalog.refresh.bind(catalog)
    const refresh = vi.spyOn(catalog, 'refresh').mockRejectedValueOnce({ code: 'SOURCE_BUSY' })
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    await flushPromises()
    expect(runtime.sourceWarnings?.value).toContainEqual({ source: 'catalog', code: 'SOURCE_BUSY' })
    let rejectOld!: (reason: unknown) => void
    refresh.mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectOld = reject })).mockImplementation(original)
    useNativeTabsStore().tab(tab.tabId)!.title = 'Changed'
    await flushPromises()
    await runtime.refresh()
    rejectOld({ code: 'FORBIDDEN' }); await flushPromises()
    expect(runtime.sourceWarnings?.value).toEqual([])
    expect(runtime.error.value).toBeNull()
  })
})

describe('Unified production runtime', () => {
  it('Runtime_OpencodeNewBindsOfficialIdBeforeLaunch_086', async () => {
    io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'oc', revision: '7', cli: 'opencode',
      name: 'OpenCode', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/opencode' },
      defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
    io.scope.mockResolvedValue({ cli: 'opencode', sourceRootKey: 'root' })
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const created = await catalog.createSession({ cli: 'opencode', projectKey: '/repo', projectPath: '/repo', action: { kind: 'new' } })
    const active = useNativeTabsStore().tab(created.adapterSessionId)!
    expect(io.prepare).toHaveBeenCalledWith(expect.objectContaining({ tabId: active.tabId, requestId: active.requestId,
      runId: active.runId, profileId: 'oc', expectedProfileRevision: '7', action: { kind: 'new' } }))
    expect(active.action).toEqual({ kind: 'resume-id', nativeSessionId: 'ses_prepared' })
    io.read.mockResolvedValue({ state: 'ready', items: [{ type: 'session', projectId: 'project',
      sessionKey: 'prepared-key', nativeSessionId: 'ses_prepared', title: 'CLI title', cwd: '/repo' }], hasMore: false })
    await runtime.refresh()
    const rows = catalog.sessions.filter(row => row.nativeSessionId === 'ses_prepared')
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({ opened: true, title: 'CLI title', resumable: true })
    const tabs = useNativeTabsStore()
    tabs.markStarting(active.tabId)
    tabs.applyLaunchStatus(active.tabId, { instanceId: 'backend', requestId: active.requestId,
      run: { runId: active.runId, generation: active.generation }, phase: 'running', revision: '1', failure: null })
    io.read.mockResolvedValue({ state: 'ready', items: [{ type: 'session', projectId: 'project',
      sessionKey: 'prepared-key', nativeSessionId: 'ses_prepared', title: 'Updated CLI title', cwd: '/repo' }], hasMore: false })
    tabs.tab(active.tabId)!.titleChangeRevision = 1
    await new Promise(resolve => setTimeout(resolve, 400))
    await flushPromises()
    expect(catalog.sessions.find(row => row.nativeSessionId === 'ses_prepared')?.title).toBe('Updated CLI title')
    io.read.mockResolvedValue({ state: 'ready', items: [{ type: 'session', projectId: 'project',
      sessionKey: 'prepared-key', nativeSessionId: 'ses_prepared', title: 'Final CLI title', cwd: '/repo' }], hasMore: false })
    tabs.applyLaunchStatus(active.tabId, { instanceId: 'backend', requestId: active.requestId,
      run: { runId: active.runId, generation: active.generation }, phase: 'exited', revision: '2', failure: null })
    await new Promise(resolve => setTimeout(resolve, 400))
    await flushPromises()
    expect(catalog.sessions.find(row => row.nativeSessionId === 'ses_prepared')?.title).toBe('Final CLI title')
  })
  it('Runtime_OpencodePrepareFailureLeavesNoAnonymousTab_087', async () => {
    io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'oc', revision: '7', cli: 'opencode',
      name: 'OpenCode', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/opencode' },
      defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
    io.prepare.mockRejectedValue({ code: 'OPENCODE_SESSION_PREPARE_FAILED' })
    render(); await flushPromises()
    await expect(useUnifiedSessionsStore().createSession({ cli: 'opencode', projectKey: '/repo', projectPath: '/repo' })).rejects.toBeDefined()
    expect(useNativeTabsStore().tabs.size).toBe(0)
  })
  it('Runtime_OpencodePreparationDoesNotPublishAnonymousTab_088', async () => {
    io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'oc', revision: '7', cli: 'opencode',
      name: 'OpenCode', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/opencode' },
      defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
    let finish!: (id: string) => void
    io.prepare.mockReturnValue(new Promise(resolve => { finish = resolve }))
    render(); await flushPromises()
    const creating = useUnifiedSessionsStore().createSession({ cli: 'opencode', projectKey: '/repo', projectPath: '/repo' })
    await flushPromises()
    expect(io.prepare).toHaveBeenCalledOnce()
    expect(useNativeTabsStore().tabs.size).toBe(0)
    finish('ses_prepared')
    const created = await creating
    expect(useNativeTabsStore().tab(created.adapterSessionId)?.action)
      .toEqual({ kind: 'resume-id', nativeSessionId: 'ses_prepared' })
  })
  // 配置与项目共用后端 workspace revision；注册项目后创建另一CLI配置须先读取当前CAS。
  it.each([['claude', 'codex'], ['codex', 'claude']] as const)('Runtime_SequentialCliDefaults_045: %s then %s', async (firstCli, secondCli) => {
    let revision = 0
    const profiles: any[] = [], projects: any[] = []
    io.profiles.mockImplementation(async () => ({ revision: String(revision), profiles: structuredClone(profiles) }))
    io.registered.mockImplementation(async () => ({ revision: String(revision), projects: structuredClone(projects) }))
    io.patchProfile.mockImplementation(async (expectedRevision, patch) => {
      if (expectedRevision !== String(revision)) throw { code: 'REVISION_CONFLICT', retryable: false }
      ++revision
      if (patch.op === 'create') profiles.push({ ...structuredClone(patch.profile), revision: String(revision) })
      else Object.assign(profiles.find(profile => profile.id === patch.id), structuredClone(patch.changes), { revision: String(revision) })
      return { revision: String(revision), profiles: structuredClone(profiles) }
    })
    io.register.mockImplementation(async selectedPath => {
      ++revision
      projects.push({ projectId: 'registered', hostId: 'host', sourcePathKey: 'source', selectedPath, canonicalPath: selectedPath, alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } })
      return { revision: String(revision), projectId: 'registered', projects: structuredClone(projects) }
    })
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    await expect(catalog.createSession({ cli: firstCli, projectKey: '/new', projectPath: '/new' })).rejects.toThrow('LAUNCH_CONFIGURATION_REQUIRED')
    const firstId = catalog.activeSessionId!
    await useCliProfilesStore().patch('1', { op: 'update', id: `desk-safe-${firstCli}`, changes: { programPath: { mode: 'set', value: `/tools/${firstCli}` } } })
    const first = await catalog.restartSession(firstId)
    expect(useNativeTabsStore().tab(first.adapterSessionId)?.cli).toBe(firstCli)
    expect(revision).toBe(3)
    await expect(catalog.createSession({ cli: secondCli, projectKey: '/new', projectPath: '/new' })).rejects.toThrow('LAUNCH_CONFIGURATION_REQUIRED')
    const secondId = catalog.activeSessionId!
    await useCliProfilesStore().patch('4', { op: 'update', id: `desk-safe-${secondCli}`, changes: { programPath: { mode: 'set', value: `/tools/${secondCli}` } } })
    const second = await catalog.restartSession(secondId)
    expect(useNativeTabsStore().tab(second.adapterSessionId)).toMatchObject({ cli: secondCli, profileId: `desk-safe-${secondCli}`, profileRevision: '5', projectId: 'registered' })
    expect(io.patchProfile.mock.calls.filter(([, patch]) => patch.op === 'create').map(([expectedRevision]) => expectedRevision)).toEqual(['0', '3'])
    expect(io.register).toHaveBeenCalledOnce()
    expect(useNativeTabsStore().tabs.size).toBe(2)
  })
  // 一个来源失败不能隐藏另一个来源的已注册项目和历史；初始化不启动CLI。
  it('Runtime_BootsPartialSources_001', async () => {
    io.projects.mockRejectedValue(new Error('private /path secret'))
    const { runtime, port } = render(); await flushPromises()
    const unified = useUnifiedSessionsStore()
    expect(unified.initialized).toBe(true)
    expect(unified.sessions.map(s => s.title)).toContain('History')
    expect(port.startLegacy).not.toHaveBeenCalled(); expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(runtime.error.value).not.toContain('/path')
    expect(runtime.cliAvailability.value.codex).toBe('unknown')
    expect(io.runChecks).not.toHaveBeenCalled(); expect(io.ptySpawn).not.toHaveBeenCalled()
    expect(io.ptyInput).not.toHaveBeenCalled(); expect(io.ptyKill).not.toHaveBeenCalled()
  })
  // 恢复使用历史来源的完整身份，当前选项变化不能覆盖其修订号和项目。
  it('Runtime_PreservesResumeOrigin_002', async () => {
    render(); await flushPromises()
    const unified = useUnifiedSessionsStore(); const history = unified.sessions.find(s => s.title === 'History')!
    const opened = await unified.resumeSession({ runtime: 'native-cli', cli: history.cli, projectKey: history.projectKey, projectPath: history.projectPath, adapterSessionId: history.adapterSessionId, nativeSessionId: history.nativeSessionId })
    expect(useNativeTabsStore().tab(opened.adapterSessionId)).toMatchObject({ projectId: 'project', profileId: 'cx', profileRevision: '7', sourceSessionKey: 'root-key', action: { kind: 'resume-id', nativeSessionId: 'history-id' } })
    useNativeTabsStore().close(opened.adapterSessionId)
    useCliProfilesStore().profiles[0].revision = '8'
    await expect(unified.resumeSession({ runtime: 'native-cli', cli: history.cli, projectKey: history.projectKey, projectPath: history.projectPath, adapterSessionId: history.adapterSessionId, nativeSessionId: history.nativeSessionId })).rejects.toThrow('PROFILE_SELECTION_CHANGED')
    expect(useNativeTabsStore().tabs.size).toBe(0)
  })
  // 显式创建通过鉴权项目注册，原始argv保留字符串边界；初始化仍然只读。
  it('Runtime_RequiresRegisteredProject_003', async () => {
    render(); await flushPromises(); const unified = useUnifiedSessionsStore()
    expect(io.register).not.toHaveBeenCalled()
    const registered = await unified.createSession({ projectKey: '/unknown', projectPath: '/unknown', cli: 'codex', action: { kind: 'new' } })
    expect(useNativeTabsStore().tab(registered.adapterSessionId)?.projectId).toBe('registered-new')
    expect(io.register).toHaveBeenCalledWith('/unknown')
    const opened = await unified.createSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', action: { kind: 'raw', argv: ['two words', '', '--literal= x'] } })
    expect(useNativeTabsStore().tab(opened.adapterSessionId)!.action).toEqual({ kind: 'raw', argv: ['two words', '', '--literal= x'] })
  })
  // 非 UI 来源的激活请求也不能把历史行恢复，防止陈旧请求越过按钮边界。
  it('Runtime_HistoryActivateDoesNotLaunch_057', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(), shell = useShellStore()
    const history = catalog.sessions.find(session => !session.opened)!
    shell.requestWorkspaceAction({ kind: 'activate', sessionId: history.id }); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(useSessionStore().tabs.size).toBe(0)
    expect(catalog.activeSessionId).toBeNull()
  })
  it('Runtime_ClearRequestKeepsClickedReceipt_058', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(), shell = useShellStore()
    const row = catalog.sessions.find(session => !session.opened)!
    const first = { kind: 'reply-ended' as const, eventId: 'first', receivedAt: 1, runId: 'run-1', generation: 1 }
    const second = { ...first, eventId: 'second', runId: 'run-2', generation: 2 }
    row.observationNotice = { recent: first, unreadReplyEnd: first }
    const clear = vi.spyOn(catalog, 'clearUnread').mockResolvedValue()
    shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: row.id, action: 'clear-unread', expectedNotice: { ...first } })
    row.observationNotice = { recent: second, unreadReplyEnd: second }
    await flushPromises()
    expect(clear).toHaveBeenCalledWith(row.id, first)
  })
  // 每个请求序号仅执行一次，较早完成不能清掉较新待确认请求。
  it('Runtime_OwnsRequestSequence_004', async () => {
    render(); await flushPromises(); const shell = useShellStore(); const unified = useUnifiedSessionsStore()
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); await unified.refresh()
    let finish!: () => void; const activate = vi.spyOn(unified, 'activateSession').mockImplementation(() => new Promise<void>(r => { finish = r }))
    shell.requestWorkspaceAction({ kind: 'activate', sessionId: `native-tab:${tab.tabId}` }); await flushPromises()
    shell.requestWorkspaceAction({ kind: 'confirmation', request: { kind: 'stop-and-archive', sessionId: `native-tab:${tab.tabId}`, projectKey: '/repo', projectPath: '/repo' } }); await flushPromises()
    finish(); await flushPromises()
    expect(activate).toHaveBeenCalledTimes(1); expect(shell.pendingRequest).toBeNull(); expect(unified.sessionConfirmation?.kind).toBe('stop-and-archive')
    shell.section = 'settings'; await flushPromises(); expect(activate).toHaveBeenCalledTimes(1)
  })
  // 运行中归档仍须确认；关闭是用户直接操作，不再弹第二层确认。
  it('Runtime_OpensOwnedConfirmations_005', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    const unified = useUnifiedSessionsStore(); await unified.refresh(); const shell = useShellStore()
    for (const action of ['archive'] as const) {
      shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action }); await flushPromises()
      expect(shell.pendingRequest).toBeNull(); expect(unified.sessionConfirmation?.kind).toBe('stop-and-archive'); expect(port.stopNative).not.toHaveBeenCalled(); expect(tabs.tab(tab.tabId)).toBeDefined()
    }
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/repo', projectPath: '/repo' } }); await flushPromises()
    expect(shell.pendingRequest).toBeNull(); expect(useNewSessionDraftStore().chooserVisible).toBe(true); expect(useNewSessionDraftStore().visible).toBe(false); expect(tabs.tabs.size).toBe(1)
  })
  // 后台store变化发布统一目录，不要求用户刷新；历史记录不创建终端。
  it('Runtime_ProjectsLiveStores_006', async () => {
    const { runtime } = render(); await flushPromises()
    const id = useSessionStore().createTab('/legacy', { name: 'Legacy open' }); await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(s => s.adapterSessionId === id)?.title).toBe('Legacy open')
    expect(runtime.openSessions.value.map(s => s.id)).toEqual([`legacy-tab:${id}`])
    useSessionStore().tabs.get(id)!.status = 'running'; useSessionStore().tabs.get(id)!.pending = true; await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(s => s.adapterSessionId === id)?.attentionState).toBe('needs-user')
    expect(useNativeHistoryStore().all()).toHaveLength(1); expect(useWorkspaceStore().projects).toHaveLength(1)
  })
  // 即使目录仍显示停止，直接关闭也必须停止真实store中的当前尝试。
  it('Runtime_CloseChecksLiveState_007', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    const unified = useUnifiedSessionsStore(); await unified.refresh(); await flushPromises()
    tabs.tab(tab.tabId)!.status = 'running'
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action: 'close' })
    await flushPromises()
    expect(port.stopNative).toHaveBeenCalledOnce()
    expect(port.stopNative.mock.calls[0][0]).toBe(tab.tabId)
    expect(useShellStore().pendingRequest).toBeNull(); expect(unified.sessionConfirmation).toBeNull()
    expect(tabs.tab(tab.tabId)).toBeUndefined()
  })
  // 重启等待停止时出现新代次，旧完成不能覆盖新的代次或启动配置。
  it('Runtime_StaleRestartIsRejected_008', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    const unified = useUnifiedSessionsStore(); await unified.refresh()
    let finish!: () => void; port.stopNative.mockReturnValue(new Promise<void>(r => { finish = r }))
    const restarting = unified.restartSession(`native-tab:${tab.tabId}`); const rejected = expect(restarting).rejects.toThrow('STALE_NATIVE_ATTEMPT'); await flushPromises()
    tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); finish(); await rejected
    expect(tabs.tab(tab.tabId)?.generation).toBe(2)
  })
  // 完整App使用真实初始化/适配器/宿主，历史不自动启动，创建后导航不重建。
  it('Runtime_NormalAppWiresRealHost_009', async () => {
    const mounted: string[] = []; const unmounted: string[] = []
    const child = defineComponent({ props: ['tabId', 'active'], setup(props, { expose }) {
      mounted.push(props.tabId)
      expose({ focus() {}, fitVisible() {}, async recover() {}, async stop() {} })
      return () => h('div', { 'data-runtime-tab': props.tabId, 'data-visible': String(props.active) })
    }, unmounted() { unmounted.push(String(this.tabId)) } })
    const w = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: child, SettingsView: true } } }); wrappers.push(w); await flushPromises()
    const unified = useUnifiedSessionsStore(); expect(unified.sessions.map(s => s.title)).toEqual(['History']); expect(mounted).toEqual([])
    const opened = await unified.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', action: { kind: 'new' } }); await flushPromises()
    expect(mounted).toEqual([opened.adapterSessionId]); const element = w.get('[data-runtime-tab]').element
    useShellStore().navigate('projects'); await flushPromises(); expect(w.get('[data-runtime-tab]').attributes('data-visible')).toBe('false')
    useShellStore().navigate('workspace'); await flushPromises(); expect(w.get('[data-runtime-tab]').element).toBe(element); expect(unmounted).toEqual([])
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: opened.id, action: 'copy-session-id' }); await flushPromises()
    expect(io.writeText).not.toHaveBeenCalled()
    useUnifiedSessionsStore().beginRename(opened.id)
    useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: opened.id, title: 'Unified name' }); await flushPromises()
    expect(tabsTitle()).toBe('Codex CLI'); expect(useShellStore().pendingRequest).toBeNull()
    expect(unified.actionFeedback?.detailCode).toBe('SESSION_IDENTITY_UNKNOWN')
    function tabsTitle() { return useNativeTabsStore().tab(opened.adapterSessionId)?.title }
  })

  // 恢复条目的启动配置被替换时，不能丢失其原始来源并启动另一配置。
  it('Runtime_ResumeRejectsWrongOrigin_010', async () => {
    render(); await flushPromises()
    const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'other' })
    const unified = useUnifiedSessionsStore(); const history = unified.sessions.find(s => s.title === 'History')!
    await expect(unified.resumeSession({ runtime: 'native-cli', cli: 'codex', projectKey: '/repo', projectPath: '/repo', adapterSessionId: history.adapterSessionId, nativeSessionId: history.nativeSessionId, launchConfigId: 'other' })).rejects.toThrow('PROFILE_SELECTION_CHANGED')
    expect(useNativeTabsStore().tabs.size).toBe(0)
  })
  // 重启等待停止期间项目被移除，完成后不能使用前端缓存路径启动新代次。
  it('Runtime_RestartRechecksProject_011', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    const unified = useUnifiedSessionsStore(); await unified.refresh()
    let finish!: () => void; port.stopNative.mockReturnValue(new Promise<void>(r => { finish = r }))
    const restarting = unified.restartSession(`native-tab:${tab.tabId}`); const rejected = expect(restarting).rejects.toThrow('PROJECT_NOT_FOUND'); await flushPromises()
    useWorkspaceStore().projects = []; tabs.tab(tab.tabId)!.status = 'exited'; finish(); await rejected
    expect(tabs.tab(tab.tabId)?.generation).toBe(1)
  })

  // 真实Legacy store归档后仍保留统一目录记录，普通列表隐藏而抽屉可恢复。
  it('Runtime_LegacyArchiveRemainsVisible_012', async () => {
    io.sessions.mockResolvedValue([{ sessionId: 'old-session', name: 'Old Legacy', projectPath: '/legacy', lastActiveAt: 10 }])
    io.archive.mockResolvedValue({ pinnedProjects: [], archivedSessions: { '/legacy': ['old-session'] } })
    io.restore.mockResolvedValue({ pinnedProjects: [], archivedSessions: {} })
    const { runtime, port } = render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(); const original = catalog.sessions.find(session => session.nativeSessionId === 'old-session')!
    await catalog.archiveSession(original.id); await runtime.refresh(); await flushPromises()
    expect(useSessionStore().getHistoryFor('/legacy')).toEqual([])
    expect(catalog.projectGroups.flatMap(group => group.sessions).some(session => session.id === original.id)).toBe(false)
    expect(catalog.sessions.find(session => session.id === original.id)).toMatchObject({ archived: true, title: 'Old Legacy' })
    const drawer = mount(ArchivedSessionsDrawer, { props: { open: true, sessions: catalog.sessions }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { teleport: true } } }); wrappers.push(drawer)
    expect(drawer.text()).toContain('Old Legacy')
    await catalog.restoreArchivedSession(original.id); await flushPromises()
    expect(catalog.projectGroups.flatMap(group => group.sessions).find(session => session.id === original.id)).toMatchObject({ archived: false })
    expect(useSessionStore().getHistoryFor('/legacy').map(session => session.sessionId)).toEqual(['old-session'])
    expect(port.startLegacy).not.toHaveBeenCalled()
  })

  it('Runtime_RealAppTwoClickAndFrozenSuccess_013', async () => {
    const child = defineComponent({ props: ['tabId', 'active'], setup(props, { expose }) {
      expose({ focus() {}, fitVisible() {}, async recover() {}, async stop() {} })
      return () => h('div', { 'data-runtime-tab': props.tabId })
    } })
    const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: child, SettingsView: true } } }); wrappers.push(w); await flushPromises()
    const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'custom', name: 'Custom' })
    const draft = useNewSessionDraftStore(); draft.setDefault('codex', 'cx'); profiles.select('codex', 'custom')
    await w.get('.project-node[aria-label="repo"] [data-project-quick-action]').trigger('click'); await flushPromises()
    ;(document.querySelector('[data-item-id="codex"]') as HTMLButtonElement).click(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = [...tabs.tabs.values()][0]
    expect(tab).toMatchObject({ cli: 'codex', profileId: 'cx', profileRevision: '7', projectId: 'project', action: { kind: 'new' } })
    expect(w.find('[data-runtime-tab]').exists()).toBe(true)
    expect(useUnifiedSessionsStore().sessions.find(row => row.id === `native-tab:${tab.tabId}`)?.processState).toBe('starting')
    expect(useUnifiedSessionsStore().activeSessionId).toBe(`native-tab:${tab.tabId}`)
    draft.setDefault('codex', 'custom')
    expect(draft.preferred({ projectPath: '/repo' }, 'codex')?.id).toBe('custom')
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    await flushPromises()
    expect(draft.preferred({ projectPath: '/repo' }, 'codex')?.id).toBe('cx')
    expect(persisted.launchPreferences?.['/repo']?.codexLaunchConfigId).toBe('cx')
    expect(io.setPreference).toHaveBeenCalledOnce()
    expect(useShellStore().pendingRequest).toBeNull()
  })
  it('Runtime_PreReadyCreateHasImmediatePlaceholder_014', async () => {
    let release!: (value: unknown[]) => void
    io.projects.mockReturnValue(new Promise(resolve => { release = resolve }))
    render(); await flushPromises()
    useShellStore().requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/repo', projectPath: '/repo', intent: 'codex' } })
    await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(1)
    release([]); await flushPromises()
  })
  it('Runtime_RetryPreparationDoesNotReplayUnknownLaunch_015', async () => {
    render(); await flushPromises()
    io.register.mockRejectedValueOnce(new Error('secret registration failure'))
    const shell = useShellStore(); const catalog = useUnifiedSessionsStore()
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/new', projectPath: '/new', intent: 'codex' } }); await flushPromises()
    const failed = catalog.sessions.find(row => row.projectPath === '/new')!
    expect(failed).toMatchObject({ processState: 'failed', safeErrorCode: 'NEW_SESSION_PREPARATION_FAILED' }); expect(useNativeTabsStore().tabs.size).toBe(0)
    shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: failed.id, action: 'retry' }); await flushPromises()
    expect(io.register).toHaveBeenCalledTimes(2); const tab = [...useNativeTabsStore().tabs.values()][0]
    expect(catalog.sessions.some(row => row.id === failed.id)).toBe(false)
    useNativeTabsStore().markUnknown(tab.tabId); await flushPromises()
    shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action: 'retry' }); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(1); expect(useNativeTabsStore().tab(tab.tabId)?.generation).toBe(1)
    expect(io.register).toHaveBeenCalledTimes(2)
  })
  it('Runtime_ReplacedAttemptCannotRecordSuccess_016', async () => {
    render(); await flushPromises()
    const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'other' })
    const catalog = useUnifiedSessionsStore(); const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'other' })
    const tabs = useNativeTabsStore(); const old = { ...tabs.tab(opened.adapterSessionId)! }
    tabs.markError(old.tabId, 'LAUNCH_FAILED'); tabs.restart(old.tabId, { profileId: 'other', profileRevision: '7' })
    expect(tabs.applyLaunchStatus(old.tabId, { instanceId: 'i', requestId: old.requestId, run: { runId: old.runId, generation: old.generation }, revision: '1', phase: 'running', failure: null })).toBe(false)
    expect(useNewSessionDraftStore().preferred({ projectPath: '/repo' }, 'codex')?.id).toBe('cx')
  })

  it('Runtime_RetriedLaunchRecordsOnlyNewSuccess_017', async () => {
    render(); await flushPromises()
    const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'other' })
    const catalog = useUnifiedSessionsStore(); const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'other' })
    const tabs = useNativeTabsStore(); tabs.markError(opened.adapterSessionId, 'LAUNCH_FAILED'); await catalog.restartSession(opened.id)
    const tab = tabs.tab(opened.adapterSessionId)!
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    await flushPromises()
    expect(useNewSessionDraftStore().preferred({ projectPath: '/repo' }, 'codex')?.id).toBe('other')
  })
  it('Runtime_ProfileChangeDuringRegistrationFailsSafely_018', async () => {
    render(); await flushPromises(); let finish!: (value: unknown) => void
    io.register.mockReturnValue(new Promise(resolve => { finish = resolve }))
    const creating = useUnifiedSessionsStore().createSession({ cli: 'codex', projectKey: '/new', projectPath: '/new' })
    const failed = expect(creating).rejects.toThrow('NEW_SESSION_PREPARATION_FAILED'); await flushPromises()
    useCliProfilesStore().profiles[0].revision = '8'
    finish({ revision: '2', projectId: 'new', projects: [{ projectId: 'new', hostId: 'h', sourcePathKey: 's', selectedPath: '/new', canonicalPath: '/new', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }); await failed
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(useUnifiedSessionsStore().sessions.find(s => s.projectPath === '/new')?.processState).toBe('failed')
  })

  it('Runtime_RestoreRequestOpensDialog_019', async () => {
    render(); await flushPromises(); const shell = useShellStore()
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectPath: '/repo', projectKey: '/repo', intent: 'restore' } }); await flushPromises()
    expect(shell.pendingRequest).toBeNull()
    expect(useUnifiedSessionsStore().resumeDialog).toMatchObject({ project: { projectPath: '/repo', projectKey: '/repo' }, mode: 'history' })
    expect(useNativeTabsStore().tabs.size).toBe(0)
  })

  it('Runtime_UnstartedAdmissionCanBeCancelled_020', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(); const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    expect(useNativeTabsStore().tab(opened.adapterSessionId)?.status).toBe('stopped')
    useShellStore().requestWorkspaceAction({ kind: 'primary-action', sessionId: opened.id, action: 'cancel-start' }); await flushPromises()
    expect(useNativeTabsStore().tab(opened.adapterSessionId)).toBeUndefined()
    expect(catalog.sessions.some(s => s.id === opened.id)).toBe(false)
  })

  it('Runtime_HeaderUsesQuickChooserBeforeAdvanced_021', async () => {
    const terminal = defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-runtime-tab': props.tabId }) } })
    const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
    await w.get('button[data-new-session]').trigger('click'); await flushPromises()
    expect(document.querySelector('[role=dialog]')).toBeNull()
    const codex = document.querySelector('[data-item-id=codex]') as HTMLButtonElement
    expect(codex).not.toBeNull(); codex.click(); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(1); expect(w.find('[data-runtime-tab]').exists()).toBe(true)
    await w.get('button[data-new-session]').trigger('click'); await flushPromises()
    ;(document.querySelector('[data-item-id=options]') as HTMLButtonElement).click(); await flushPromises()
    expect(document.querySelector('[role=dialog]')).not.toBeNull(); expect(useNativeTabsStore().tabs.size).toBe(1)
  })
  it('Runtime_PreferenceSaveFailureNeverRetriesLaunch_022', async () => {
    const { runtime } = render(); await flushPromises()
    io.setPreference.mockRejectedValue(new Error('private storage path'))
    const catalog = useUnifiedSessionsStore(); const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    const tabs = useNativeTabsStore(); const tab = { ...tabs.tab(opened.adapterSessionId)! }
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    await flushPromises()
    expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'running', generation: 1, requestId: tab.requestId, runId: tab.runId })
    expect(runtime.error.value).toBe('newSessionPreferenceSaveFailed')
    expect(io.setPreference).toHaveBeenCalledOnce(); expect(io.getState).toHaveBeenCalledOnce()
    await runtime.refresh(); await flushPromises()
    expect(io.setPreference).toHaveBeenCalledOnce(); expect(tabs.tabs.size).toBe(1)
    expect(catalog.sessions.find(row => row.id === opened.id)?.processState).toBe('running')
  })

  it('Runtime_PreReadyCancelPreventsLateAdmission_023', async () => {
    let releaseBootstrap!: (value: unknown[]) => void
    io.projects.mockReturnValue(new Promise(resolve => { releaseBootstrap = resolve }))
    const { runtime } = render(); await flushPromises()
    let releaseRegistration!: (value: unknown) => void
    io.register.mockReturnValue(new Promise(resolve => { releaseRegistration = resolve }))
    const shell = useShellStore(); const catalog = useUnifiedSessionsStore()
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/new', projectPath: '/new', intent: 'codex' } }); await flushPromises()
    expect(runtime.ready.value).toBe(false)
    const placeholder = catalog.sessions.find(row => row.projectPath === '/new')!
    shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: placeholder.id, action: 'cancel-start' }); await flushPromises()
    const afterCancel = catalog.sessions.find(row => row.id === placeholder.id)?.processState
    releaseRegistration({ revision: '2', projectId: 'new', projects: [{ projectId: 'new', hostId: 'h', sourcePathKey: 's', selectedPath: '/new', canonicalPath: '/new', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(afterCancel).toBe('failed'); expect(shell.pendingRequest).toBeNull()
    releaseBootstrap([]); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(catalog.sessions.find(row => row.id === placeholder.id)?.safeErrorCode).toBe('NEW_SESSION_CANCELLED')
  })
  it('Runtime_ReselectedPlaceholderTransfersSelection_024', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(); const draft = useNewSessionDraftStore()
    let release!: () => void; const prepare = draft.prepareInput
    vi.spyOn(draft, 'prepareInput').mockImplementation(async input => { await new Promise<void>(resolve => { release = resolve }); return prepare(input) })
    const creating = catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
    const placeholder = catalog.activeSessionId!
    useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: placeholder }); await flushPromises()
    release(); const created = await creating; await flushPromises()
    expect(catalog.activeSessionId).toBe(created.id)
    expect(catalog.sessions.some(row => row.id === placeholder)).toBe(false)
  })
  it('Runtime_PlaceholderCannotStealNewerSessionSelection_025', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const existing = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    const draft = useNewSessionDraftStore(); let release!: () => void; const prepare = draft.prepareInput
    vi.spyOn(draft, 'prepareInput').mockImplementation(async input => { await new Promise<void>(resolve => { release = resolve }); return prepare(input) })
    const creating = catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
    await catalog.activateSession(catalog.activeSessionId!)
    await catalog.activateSession(existing.id)
    release(); await creating; await flushPromises()
    expect(catalog.activeSessionId).toBe(existing.id)
  })

  it('Runtime_UnrelatedClosePreservesPlaceholderSelection_026', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const existing = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    useNativeTabsStore().markError(existing.adapterSessionId, 'LAUNCH_FAILED'); await flushPromises()
    const draft = useNewSessionDraftStore(); const prepare = draft.prepareInput; let release!: () => void
    vi.spyOn(draft, 'prepareInput').mockImplementation(async input => { await new Promise<void>(resolve => { release = resolve }); return prepare(input) })
    const creating = catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
    const placeholder = catalog.activeSessionId!
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: existing.id, action: 'close' }); await flushPromises()
    expect(catalog.activeSessionId).toBe(placeholder)
    expect(catalog.sessions.some(row => row.id === existing.id)).toBe(false)
    release(); const created = await creating; await flushPromises()
    expect(catalog.activeSessionId).toBe(created.id)
  })

})

// 旧归档键映射多个来源时显示明确、安全的歧义提示，保留所有记录。
it('Runtime_ReportsArchiveAmbiguity_027', async () => {
  const { runtime } = render(); await flushPromises()
  const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'other' })
  const history = useNativeHistoryStore(); await history.load({ cli: 'codex', profileId: 'other', profileRevision: '7', projectId: 'project', projectPath: '/repo' })
  const { makeSessionCatalogKey } = await import('@/utils/sessionPresentation')
  const key = 'native-history:' + makeSessionCatalogKey({ runtime: 'native-cli', cli: 'codex', projectPath: '/repo', adapterSessionId: 'root-key', nativeSessionId: 'history-id' })
  useProjectsStateStore().archivedSessions.set('/repo', [key]); await useUnifiedSessionsStore().refresh(); await flushPromises()
  const row = useUnifiedSessionsStore().sessions.find(row => row.archived)!
  useShellStore().requestWorkspaceAction({ kind: 'restore-archive', sessionId: row.id }); await flushPromises()
  expect(runtime.error.value).toBe('resumeAmbiguous'); expect(io.restore).not.toHaveBeenCalled()
})

// 移除等待原生取消注册时，Legacy恢复不能创建或启动新的终端所有者。
it('Runtime_RemoveBlocksLegacyRestore_028', async () => {
  io.sessions.mockResolvedValue([{ sessionId: 'legacy-history', name: 'Legacy history', projectPath: '/legacy', lastActiveAt: 10 }])
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'p', hostId: 'h', sourcePathKey: 's', selectedPath: '/legacy', canonicalPath: '/legacy', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  const { port } = render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(); const row = catalog.sessions.find(session => session.nativeSessionId === 'legacy-history')!
  const management = useProjectManagementStore(); management.beginRemove({ projectKey: '/legacy', projectPath: '/legacy' })
  let release!: (value: unknown) => void
  io.remove.mockImplementation(() => new Promise(resolve => { release = resolve }))
  const removing = management.remove(); await flushPromises()
  expect(useAppStore().isProjectRemoving('/legacy')).toBe(true); expect(io.remove).toHaveBeenCalledOnce()
  const restoring = catalog.resumeSession({ runtime: 'legacy-claude', cli: 'claude', projectKey: '/legacy', projectPath: '/legacy', adapterSessionId: row.adapterSessionId, nativeSessionId: row.nativeSessionId }).catch(() => undefined)
  await flushPromises(); release({ revision: '2', projects: [] }); await removing; await restoring
  expect(port.startLegacy).not.toHaveBeenCalled(); expect(useSessionStore().tabs.size).toBe(0)
})
// 较早恢复的历史读取在移除期间或完成后返回，也不能取得新的终端所有权。
it.each(['during', 'after'])('Runtime_RemoveRevokesDeferredLegacy_%s', async when => {
  io.sessions.mockResolvedValue([{ sessionId: 'legacy-history', name: 'Legacy history', projectPath: '/legacy', lastActiveAt: 10 }])
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'p', hostId: 'h', sourcePathKey: 's', selectedPath: '/legacy', canonicalPath: '/legacy', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  const { port } = render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(); const row = catalog.sessions.find(session => session.nativeSessionId === 'legacy-history')!
  let finishHistory!: (value: unknown) => void
  io.sessions.mockImplementation(() => new Promise(resolve => { finishHistory = resolve }))
  const restoring = catalog.resumeSession({ runtime: 'legacy-claude', cli: 'claude', projectKey: '/legacy', projectPath: '/legacy', adapterSessionId: row.adapterSessionId, nativeSessionId: row.nativeSessionId }).catch(() => undefined)
  await flushPromises()
  const management = useProjectManagementStore(); management.beginRemove({ projectKey: '/legacy', projectPath: '/legacy' })
  let release!: (value: unknown) => void
  io.remove.mockImplementation(() => new Promise(resolve => { release = resolve }))
  const removing = management.remove(); await flushPromises()
  if (when === 'after') { release({ revision: '2', projects: [] }); await removing }
  finishHistory([{ sessionId: 'legacy-history', name: 'Legacy history', projectPath: '/legacy', lastActiveAt: 10 }]); await restoring
  if (when === 'during') { release({ revision: '2', projects: [] }); await removing }
  expect(port.startLegacy).not.toHaveBeenCalled(); expect(useSessionStore().tabs.size).toBe(0)
})

// 旧工作台身份测试迁移到真实统一runtime；新建/恢复/原始参数均固定所选CLI、配置修订和注册项目。
describe.each(['claude', 'codex'] as const)('Unified action identity: %s', cli => {
  it.each<LaunchAction>([
    { kind: 'new' }, { kind: 'raw', argv: ['', '中文', 'two words', '--future'] },
    { kind: 'resume-picker', scope: 'current-project' }, { kind: 'resume-id', nativeSessionId: 'session-123' },
  ])('Runtime_FreezesActionIdentity_029: $kind', async action => {
    io.profiles.mockResolvedValue({ revision: '9', profiles: ['claude', 'codex'].map(kind => ({
      id: kind, revision: kind === 'claude' ? '3' : '8', cli: kind, name: kind,
      launcher: { kind: 'native' }, programPath: { mode: 'set', value: `/tools/${kind}` }, defaultArgs: { mode: 'inherit' },
      skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {},
    })) })
    render(); await flushPromises()
    const unified = useUnifiedSessionsStore()
    const input = { cli, projectKey: '/repo', projectPath: '/repo', registeredProjectId: 'project',
      launchConfigId: cli, launchConfigRevision: cli === 'claude' ? '3' : '8', action }
    const opened = action.kind === 'resume-id' || action.kind === 'resume-picker'
      ? await unified.launchResume(input) : await unified.createSession(input)
    expect(useNativeTabsStore().tab(opened.adapterSessionId)).toMatchObject({
      cli, projectId: 'project', projectPath: '/repo', profileId: cli,
      profileRevision: input.launchConfigRevision, action,
    })
    expect(io.patchProfile).not.toHaveBeenCalled(); expect(io.register).not.toHaveBeenCalled()
    expect(io.runChecks).not.toHaveBeenCalled(); expect(io.ptySpawn).not.toHaveBeenCalled()
    expect(io.ptyInput).not.toHaveBeenCalled(); expect(io.ptyKill).not.toHaveBeenCalled()
  })
})

// 正常App根据真实适配器的打开/历史投影显示菜单，激活后的第二次双击才编辑。
it('Runtime_RowDoubleClickSelection_028', async () => {
  const terminal = defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-runtime-tab': props.tabId }) } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(), tabs = useNativeTabsStore()
  const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
  const tab = tabs.tab(opened.adapterSessionId)!
  tab.status = 'exited'; tab.launchRevision = '1'; await catalog.refresh(); await flushPromises()
  catalog.selectProjectContext('/legacy'); await flushPromises()
  await w.get('.search-input').setValue('/repo'); await flushPromises()
  const row = w.get(`[data-session-row="${opened.id}"]`)
  await row.trigger('contextmenu'); await flushPromises()
  expect(document.querySelector('[data-item-id="rename"]')).toBeNull()
  expect(document.querySelector('[data-item-id="restart"]')).not.toBeNull()
  expect(document.querySelector('[data-item-id="close"]')).toBeNull()
  expect(row.get('.session-primary-action button').attributes('aria-label')).toBe('Close')
  document.querySelector('[role="menu"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); await flushPromises()
  await row.trigger('keydown', { key: 'F2', code: 'F2' }); await flushPromises()
  expect(catalog.sessions.find(session => session.id === opened.id)?.renameState).toBe('idle')
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await flushPromises(); row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await flushPromises()
  expect(catalog.activeSessionId).toBe(opened.id)
  expect(row.find('input').exists()).toBe(false)
  expect(w.emitted('workspace-request')?.filter(([request]) => (request as { kind: string }).kind === 'activate')).toHaveLength(1)
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await flushPromises(); row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await flushPromises()
  expect(row.find('input').exists()).toBe(true)
  await row.get('input').setValue('Renamed by double click')
  await row.get('input').trigger('keydown', { key: 'Enter' }); await flushPromises()
  expect(tabs.tab(opened.adapterSessionId)?.title).toBe('Codex CLI')
  expect(row.get('input').element.value).toBe('Renamed by double click')
  expect(catalog.actionFeedback?.detailCode).toBe('SESSION_IDENTITY_UNKNOWN')
  expect(tabs.tab(opened.adapterSessionId)?.status).toBe('exited')
  expect(tabs.tab(opened.adapterSessionId)?.generation).toBe(1)
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'F2', code: 'F2', bubbles: true })); await flushPromises()
  expect(row.find('input').exists()).toBe(true)
  expect(tabs.tabs.size).toBe(1)
  expect(io.ptySpawn).not.toHaveBeenCalled()
})

// 历史行首次点击恢复精确来源，双击的第二次点击不重复，也不弹出重命名。
it.each(['legacy-claude', 'native-cli'] as const)('Runtime_HistoryMenuAndDoubleClick_029: %s', async runtime => {
  io.sessions.mockResolvedValue([{ sessionId: 'legacy-history', name: 'Legacy history', projectPath: '/legacy', lastActiveAt: 10 }])
  const startLegacy = vi.fn().mockResolvedValue({ ok: true })
  const nativeTerminal = defineComponent({ setup(_props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div') } })
  const legacyTerminal = defineComponent({ setup(_props, { expose }) { expose({ startTab: startLegacy, focus() {}, fitVisible() {} }); return () => h('div') } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: nativeTerminal, XTermTerminal: legacyTerminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const history = catalog.sessions.find(session => session.runtime === runtime)!
  await w.get('.search-input').setValue(history.projectPath); await flushPromises()
  const row = w.findAll('[data-session-row]').find(item => item.attributes('data-session-row') === history.id)!
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await flushPromises(); row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await flushPromises()
  expect(catalog.resumeDialog).toBeNull()
  expect(document.querySelector('[data-resume-target]')).toBeNull()
  expect(document.querySelector('.rename-input')).toBeNull()
  expect(w.emitted('workspace-request')?.filter(([request]) => (request as { kind: string }).kind === 'activate') ?? []).toHaveLength(0)
  expect(row.find('[data-session-launch]').exists()).toBe(false)
  expect(useNativeTabsStore().tabs.size).toBe(runtime === 'native-cli' ? 1 : 0)
  expect(useSessionStore().tabs.size).toBe(runtime === 'legacy-claude' ? 1 : 0)
  if (runtime === 'legacy-claude') expect(startLegacy).toHaveBeenCalledOnce()
  else expect(useNativeTabsStore().tabs.values().next().value?.action).toEqual({ kind: 'resume-id', nativeSessionId: 'history-id' })
})

// 快速恢复的异步来源检查不得越过导航、替代请求或配置变更，且不能重放副作用。
it.each(['navigation', 'project', 'replacement', 'profile'] as const)('Runtime_DirectResumeAdmission_046: %s', async change => {
  render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const history = catalog.sessions.find(session => session.title === 'History')!
  let finish!: (value: unknown) => void
  io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: history.id, action: 'resume' }); await flushPromises()
  expect(catalog.resumeDialog).toBeNull()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  if (change === 'navigation') shell.navigate('settings')
  else if (change === 'project') { catalog.selectProjectContext('/other'); shell.navigate('workspace') }
  else if (change === 'replacement') shell.requestWorkspaceAction({ kind: 'restore-session', project: history, mode: 'history' })
  else useCliProfilesStore().profiles[0].revision = '8'
  finish({ state: 'ready', items: [{ type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }] }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  if (change === 'replacement') expect(catalog.resumeDialog?.mode).toBe('history')
})

// 实际 App 的 projectManagement 投影发布行待定态，重复点击不再恢复。
it('Runtime_AppResumePendingRow_058', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, XTermTerminal: true, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const history = catalog.sessions.find(session => session.title === 'History')!
  await w.get('.search-input').setValue('/repo'); await flushPromises()
  let finish!: (value: unknown) => void
  io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const row = w.findAll('[data-session-row]').find(item => item.attributes('data-session-row') === history.id)!
  expect(row.find('[data-session-launch]').exists()).toBe(false)
  await row.trigger('click'); await flushPromises()
  expect(row.attributes('aria-busy')).toBe('true')
  await row.trigger('click'); await flushPromises()
  expect(w.emitted('workspace-request')?.filter(([request]) => (request as { kind: string }).kind === 'primary-action')).toHaveLength(1)
  finish({ state: 'unavailable', reason: 'SOURCE_BUSY', items: [], hasMore: false }); await flushPromises()
  expect(catalog.isResumePending(history)).toBe(false)
  const retained = w.findAll('[data-session-row]').find(item => item.attributes('data-session-row') === history.id)!
  expect(retained, '读失败后保留可重试的历史行').toBeDefined()
  expect(retained.attributes('aria-busy')).not.toBe('true')
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(catalog.actionFeedback).toMatchObject({ detailCode: 'SOURCE_BUSY', retryable: true })
})

// 同一历史的两次显式请求共享核实；较早请求失效不能取消仍有效的新请求或重复启动。
it('Runtime_DirectResumeCoalesces_047', async () => {
  render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const history = catalog.sessions.find(session => session.title === 'History')!
  let finish!: (value: unknown) => void
  io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: history.id, action: 'resume' }); await flushPromises()
  shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: history.id, action: 'resume' }); await flushPromises()
  finish({ state: 'ready', items: [{ type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }] }); await flushPromises()
  expect(catalog.resumeDialog).toBeNull()
  expect(useNativeTabsStore().tabs.size).toBe(1)
  expect(catalog.activeSessionId).toMatch(/^native-tab:/)
})

// 等待停止期间被更新的尝试不能被原关闭操作删除。
it('Runtime_DirectCloseRechecksAttempt_048', async () => {
  const { port } = render(); await flushPromises()
  const tabs = useNativeTabsStore(), catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  tabs.tab(tab.tabId)!.status = 'running'; await catalog.refresh()
  let finish!: () => void
  port.stopNative.mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve }))
  shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: `native-tab:${tab.tabId}`, action: 'close' }); await flushPromises()
  expect(port.stopNative).toHaveBeenCalledOnce()
  tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
  finish(); await flushPromises()
  expect(tabs.tab(tab.tabId)?.generation).toBe(2)
  expect(catalog.sessionConfirmation).toBeNull()
})

// 已打开的结束Legacy恢复在异步来源核实后才选择；失败提示与显式Retry仍属于该请求。
it('Runtime_EndedLegacyResumeFailureKeepsRetry_049', async () => {
  const { port, runtime } = render(); await flushPromises()
  const legacy = useSessionStore(), catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const tabId = legacy.createTab('/legacy', { sessionId: 'owned-history', name: 'Ended owner' })
  await catalog.refresh()
  port.restartLegacy.mockRejectedValueOnce({ code: 'RESOURCE_UNAVAILABLE', message: '/private/secret TOKEN' })
  shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: `legacy-tab:${tabId}`, action: 'resume' }); await flushPromises()
  expect(port.restartLegacy).toHaveBeenCalledOnce()
  expect(catalog.actionFeedback).toMatchObject({ detailCode: 'RESOURCE_UNAVAILABLE', retryable: true })
  expect(JSON.stringify(catalog.actionFeedback)).not.toContain('/private/secret')
  expect(catalog.resumeDialog).toBeNull()
  runtime.retryAction(); await flushPromises()
  expect(port.restartLegacy).toHaveBeenCalledTimes(2)
  expect(port.restartLegacy.mock.calls.every(([id]) => id === tabId)).toBe(true)
  expect(legacy.tabs.size).toBe(1)
  expect(catalog.activeSessionId).toBe(`legacy-tab:${tabId}`)
})

// 已结束的 Legacy 终端在异步核实之后才选择；重启错误仍属于本次请求，并支持明确重试。
it('Runtime_EndedLegacyResumeRetainsFailureAndRetry_049', async () => {
  const { runtime, port } = render(); await flushPromises()
  const legacy = useSessionStore(), catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const id = legacy.createTab('/legacy', { sessionId: 'legacy-ended', name: 'Ended Legacy' })
  await catalog.refresh(); await flushPromises()
  port.restartLegacy.mockRejectedValueOnce({ code: 'RESOURCE_UNAVAILABLE', message: '/private/secret TOKEN' })
  shell.requestWorkspaceAction({ kind: 'menu-action', action: 'resume', sessionId: `legacy-tab:${id}` }); await flushPromises()
  expect(port.restartLegacy).toHaveBeenCalledOnce()
  expect(catalog.actionFeedback).toMatchObject({ messageKey: 'errorResourceUnavailable', retryable: true })
  expect(JSON.stringify(catalog.actionFeedback)).not.toContain('/private/secret')
  expect(catalog.resumeDialog).toBeNull()
  port.restartLegacy.mockResolvedValueOnce(undefined)
  runtime.retryAction(); await flushPromises()
  expect(port.restartLegacy).toHaveBeenCalledTimes(2)
  expect(port.restartLegacy.mock.calls.map(call => call[0])).toEqual([id, id])
  expect(legacy.tabs.size).toBe(1)
  expect(catalog.activeSessionId).toBe(`legacy-tab:${id}`)
})

// 默认配置缺少程序路径时保留失败占位的明确取消入口，不把历史或未知尝试当作可丢弃新建。
it('Runtime_DiscardFailedPreparation_030', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useCliProfilesStore().profiles[0].programPath = { mode: 'inherit' }
  await w.get('button[data-new-session]').trigger('click'); await flushPromises()
  document.querySelector<HTMLButtonElement>('[data-item-id="codex"]')!.click(); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const failed = catalog.sessions.find(row => row.safeErrorCode === 'LAUNCH_CONFIGURATION_REQUIRED')!
  expect(failed).toBeDefined()
  await w.get('.search-input').setValue(failed.projectPath); await flushPromises()
  const row = w.findAll('[data-session-row]').find(item => item.attributes('data-session-row') === failed.id)!
  await row.trigger('contextmenu'); await flushPromises()
  expect(document.querySelector('[data-item-id="close"]')).toBeNull()
  expect(document.querySelector('[data-item-id="restart"]')).toBeNull()
  expect(document.querySelector('[data-item-id="rename"]')).toBeNull()
  expect(document.querySelector('[data-item-id="archive"]')).toBeNull()
  const discard = document.querySelector<HTMLButtonElement>('[data-item-id="discard-creation"]')
  expect(discard?.textContent).toBe('Cancel creation')
  discard!.click(); await flushPromises()
  expect(catalog.sessions.some(session => session.id === failed.id)).toBe(false)
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(useSessionStore().tabs.size).toBe(0)
  expect(io.ptySpawn).not.toHaveBeenCalled()
})

// owning store先重启而catalog尚未刷新时，双击不能把旧尝试的点击授权给新generation。
it('Runtime_DoubleClickOwnsAttempt_031', async () => {
  const terminal = defineComponent({ props: ['tabId'], setup(_props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div') } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(), tabs = useNativeTabsStore()
  const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
  await w.get('.search-input').setValue('/repo'); await flushPromises()
  const row = w.get(`[data-session-row="${opened.id}"]`)
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  const previous = catalog.sessions.find(session => session.id === opened.id)
  tabs.restart(opened.adapterSessionId, { profileId: 'cx', profileRevision: '7' })
  expect(catalog.sessions.find(session => session.id === opened.id)).toBe(previous)
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true }))
  row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true }))
  await flushPromises()
  expect(w.emitted('workspace-request')?.filter(([request]) => (request as { action?: string }).action === 'rename') ?? []).toHaveLength(0)
  expect(row.find('input').exists()).toBe(false)
  expect(tabs.tab(opened.adapterSessionId)?.generation).toBe(2)
})

// 一个项目的运行状态变化不替换另一项目的行或改变打开顺序。
it('Catalog_RuntimeProjectScope_001', async () => {
  const tabs = useNativeTabsStore()
  const first = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  const other = tabs.create({ cli: 'codex', projectId: 'other', projectPath: '/other', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  Object.assign(first, { status: 'running', launchRevision: '1' })
  Object.assign(other, { status: 'running', launchRevision: '1' })
  const { runtime, port } = render(); await flushPromises()
  expect(runtime.ready.value).toBe(true)
  const catalog = useUnifiedSessionsStore()
  const retained = catalog.sessions.find(row => row.adapterSessionId === other.tabId)
  expect(retained).toBeDefined()
  const order = catalog.sessions.map(row => row.id)
  tabs.markUnknown(first.tabId); await flushPromises()
  expect(catalog.sessions.find(row => row.adapterSessionId === first.tabId)?.processState).toBe('unknown')
  expect(catalog.sessions.find(row => row.adapterSessionId === other.tabId)).toBe(retained)
  expect(catalog.sessions.map(row => row.id)).toEqual(order)
  expect(port.stopNative).not.toHaveBeenCalled()
  expect(io.ptySpawn).not.toHaveBeenCalled()
})

it('Catalog_OutputTouchDoesNotInvalidateProjection_005', async () => {
  const tabs = useNativeTabsStore()
  const created = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  Object.assign(tabs.tab(created.tabId)!, { status: 'running', launchRevision: '1' })
  render(); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const refresh = vi.spyOn(catalog, 'refresh')
  tabs.tab(created.tabId)!.lastActivityAt += 1
  await flushPromises()
  expect(refresh).not.toHaveBeenCalled()
  tabs.markUnknown(created.tabId); await flushPromises()
  expect(refresh).toHaveBeenCalledWith('/repo', { background: true })
})

// owner 跨项目、替换、移除与新增均更新目录，已替换对象的迟到变化不能发布。
it('Catalog_OwnerChanges_002', async () => {
  const tabs = useNativeTabsStore()
  const first = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', sourceSessionKey: 'root-key', action: { kind: 'resume-id', nativeSessionId: 'history-id' } })
  const other = tabs.create({ cli: 'codex', projectId: 'other', projectPath: '/other', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  Object.assign(first, { status: 'running', launchRevision: '1' })
  const { port } = render(); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  expect(catalog.sessions.filter(row => row.projectPath === '/repo')).toHaveLength(1)
  const retained = catalog.sessions.find(row => row.adapterSessionId === other.tabId)
  const owner = tabs.tab(first.tabId)!
  Object.assign(owner, { projectPath: '/moved', projectId: 'moved' }); await flushPromises()
  expect(catalog.sessions.find(row => row.adapterSessionId === first.tabId)?.projectPath).toBe('/moved')
  expect(catalog.sessions.filter(row => row.projectPath === '/repo')).toEqual([expect.objectContaining({ nativeSessionId: 'history-id', opened: false, processState: 'stopped' })])
  tabs.tabs.set(first.tabId, { ...owner, title: 'Replacement' }); await flushPromises()
  const replacement = catalog.sessions.find(row => row.adapterSessionId === first.tabId)
  expect(replacement?.title).toBe('Replacement')
  owner.title = 'Late old owner'; await flushPromises()
  expect(catalog.sessions.find(row => row.adapterSessionId === first.tabId)).toBe(replacement)
  tabs.tabs.delete(first.tabId); await flushPromises()
  expect(catalog.sessions.some(row => row.adapterSessionId === first.tabId)).toBe(false)
  expect(catalog.projectGroups.some(group => group.projectPath === '/moved')).toBe(false)
  const added = tabs.create({ cli: 'codex', projectId: 'added', projectPath: '/added', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); await flushPromises()
  expect(catalog.sessions.some(row => row.adapterSessionId === added.tabId)).toBe(true)
  expect(catalog.sessions.find(row => row.adapterSessionId === other.tabId)).toBe(retained)
  expect(port.stopNative).not.toHaveBeenCalled()
  expect(io.ptySpawn).not.toHaveBeenCalled()
})

// 关注项进入和确认清除只刷新精确 PTY 的项目，不能让其他项目丢失行身份。
it('Catalog_AttentionProjectScope_003', async () => {
  const legacy = useSessionStore()
  const id = legacy.createTab('/legacy', { name: 'Legacy' })
  Object.assign(legacy.tabs.get(id)!, { status: 'running', ptyId: 'legacy-pty', ptyGeneration: 1 })
  const other = useNativeTabsStore().create({ cli: 'codex', projectId: 'other', projectPath: '/other', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(), attention = useAttentionStore()
  const retained = catalog.sessions.find(row => row.adapterSessionId === other.tabId)
  attention.ingestEvent({ ptyId: 'legacy-pty', sessionId: null, eventName: 'Notification', timestamp: 1, state: 'idle', detail: { type: 'notification', data: { notificationType: 'permission_prompt' } } })
  await flushPromises()
  expect(catalog.sessions.find(row => row.adapterSessionId === id)?.attentionKind).toBe('permission')
  expect(catalog.sessions.find(row => row.adapterSessionId === id)?.attentionState).toBe('needs-user')
  attention.ackPty('legacy-pty'); await flushPromises()
  expect(catalog.sessions.find(row => row.adapterSessionId === id)?.attentionState).toBe('none')
  expect(catalog.sessions.find(row => row.adapterSessionId === other.tabId)).toBe(retained)
})

// 卸载取消排队的目录发布并释放 owner 监听，后台状态变化不再改写目录。
it('Catalog_UnmountRevokesQueue_004', async () => {
  const tabs = useNativeTabsStore()
  const first = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  Object.assign(first, { status: 'running', launchRevision: '1' })
  render(); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const retained = catalog.sessions.find(row => row.adapterSessionId === first.tabId)
  tabs.markUnknown(first.tabId)
  wrappers.pop()!.unmount(); await flushPromises()
  expect(catalog.sessions.find(row => row.adapterSessionId === first.tabId)).toBe(retained)
  tabs.tab(first.tabId)!.title = 'After unmount'; await flushPromises()
  expect(catalog.sessions.find(row => row.adapterSessionId === first.tabId)).toBe(retained)
})

// 同一真实尝试的只读目录刷新不能打断当前行双击重命名。
it('Runtime_DoubleClickSurvivesRefresh_032', async () => {
  const terminal = defineComponent({ props: ['tabId'], setup(_props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div') } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
  await w.get('.search-input').setValue('/repo'); await flushPromises()
  const row = w.get(`[data-session-row="${opened.id}"]`)
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  await catalog.refresh(); await flushPromises()
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true }))
  row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await flushPromises()
  expect(row.find('input').exists()).toBe(true)
})

// 实际App切换到另一会话立即移走旧编辑器，迟到的旧提交不写metadata也不抢回选择。
it('Runtime_SwitchRevokesRowEditor_033', async () => {
  const terminal = defineComponent({ props: ['tabId'], setup(_props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div') } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const first = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', title: 'First' })
  const second = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', title: 'Second' }); await flushPromises()
  // Opening-order persistence is not a manual rename; source display titles remain intact.
  expect(catalog.sessions.find(row => row.id === first.id)?.title).toBe('First')
  expect(catalog.sessions.find(row => row.id === second.id)?.title).toBe('Second')
  expect(io.upsertRecord).toHaveBeenCalledTimes(2)
  expect(io.upsertRecord).toHaveBeenNthCalledWith(1, first.id, expect.objectContaining({ title: '', lastOpenedAt: expect.any(Number) }), true)
  expect(io.upsertRecord).toHaveBeenNthCalledWith(2, second.id, expect.objectContaining({ title: '', lastOpenedAt: expect.any(Number) }), true)
  io.upsertRecord.mockClear()
  await catalog.activateSession(first.id)
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'F2', code: 'F2', bubbles: true })); await flushPromises()
  const firstRow = w.get(`[data-session-row="${first.id}"]`)
  await firstRow.get('input').setValue('Old draft')
  await w.get(`[data-session-row="${second.id}"]`).trigger('click'); await flushPromises()
  expect(catalog.activeSessionId).toBe(second.id)
  expect(catalog.sessions.find(row => row.id === first.id)?.renameState).toBe('idle')
  expect(w.find(`[data-session-row="${first.id}"] input`).exists()).toBe(false)
  useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: first.id, title: 'Stale submit' }); await flushPromises()
  expect(io.upsertRecord).not.toHaveBeenCalled()
  expect(catalog.activeSessionId).toBe(second.id)
  expect(catalog.sessions.find(row => row.id === first.id)?.title).toBe('First')
})

// 启动时已停用的 CLI 在启用后必须重新加载历史：catalog.refresh 只读缓存，
// 组合根对支持开关的 watch 必须触发完整刷新（含历史观察组重装载）。
it('HistorySupport_ColdEnableReloadsHistory_059', async () => {
  localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify({ claude: true, codex: false }))
  const { runtime } = render()
  await runtime.refresh()
  const catalog = useUnifiedSessionsStore()
  expect(catalog.sessions.filter(row => row.cli === 'codex')).toHaveLength(0)
  expect(io.read).not.toHaveBeenCalled()
  const { useCliSupportStore } = await import('@/stores/cliSupport')
  useCliSupportStore().setCliEnabled('codex', true)
  await vi.waitFor(() => expect(catalog.sessions.some(row => row.cli === 'codex' && row.nativeSessionId === 'history-id')).toBe(true))
})

// 模板 ID 不能证明 false 是旧默认值：用户关闭后刷新不得覆写，也不应增加 revision。
it('Observer_ExplicitOffSurvivesRefresh_060', async () => {
  const claudeTemplate = { id: 'desk-safe-claude', revision: '2', cli: 'claude', name: 'Claude Code', launcher: { kind: 'native' },
    programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'set', value: [] },
    skipPermissions: { mode: 'set', value: false }, observer: { mode: 'set', value: false }, env: {} }
  const codexTemplate = { ...claudeTemplate, id: 'desk-safe-codex', cli: 'codex', observer: { mode: 'set', value: false } }
  const custom = { ...claudeTemplate, id: 'my-claude', observer: { mode: 'set', value: false } }
  let revision = 2
  io.profiles.mockImplementation(async () => ({ revision: String(revision), profiles: [structuredClone(claudeTemplate), structuredClone(codexTemplate), structuredClone(custom)] }))
  const patches: any[] = []
  io.patchProfile.mockImplementation(async (expectedRevision, patch) => {
    patches.push({ expectedRevision, patch })
    revision += 1
    return { revision: String(revision), profiles: [structuredClone(claudeTemplate), structuredClone(codexTemplate), structuredClone(custom)] }
  })
  const { runtime } = render(); await runtime.refresh()
  await runtime.refresh()
  const observerPatches = patches.filter(entry => entry.patch.op === 'update' && entry.patch.changes?.observer)
  expect(observerPatches).toEqual([])
  expect(useCliProfilesStore().profile('desk-safe-claude')?.observer).toEqual({ mode: 'set', value: false })
  expect(useCliProfilesStore().revision).toBe('2')
})

// 同 profile 两个项目的回复结束合并为一次读取；普通 owner 状态变化不得扫描历史。
it('History_ReplyRefreshCoalesces_061', async () => {
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cl', revision: '7', cli: 'claude', name: 'Claude', launcher: { kind: 'native' },
    programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: true }, env: {} }] })
  const project = { projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }
  io.registered.mockResolvedValue({ revision: '1', projects: [project, { ...project, projectId: 'other', selectedPath: '/other', canonicalPath: '/other' }] })
  io.scope.mockResolvedValue({ cli: 'claude' })
  let title = 'Initial CLI title'
  io.read.mockImplementation(async () => ({ state: 'ready', items: [
    { type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title, cwd: '/repo' },
    { type: 'session', projectId: 'other', sessionKey: 'other-key', nativeSessionId: 'other-id', title, cwd: '/other' },
  ], hasMore: false }))
  const { runtime } = render(); await flushPromises(); await runtime.refresh()
  const tabs = useNativeTabsStore()
  const one = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', sourceSessionKey: 'root-key', action: { kind: 'resume-id', nativeSessionId: 'history-id' } })
  const two = tabs.create({ cli: 'claude', projectId: 'other', projectPath: '/other', profileId: 'cl', profileRevision: '7', sourceSessionKey: 'other-key', action: { kind: 'resume-id', nativeSessionId: 'other-id' } })
  for (const tab of [one, two]) tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
  await flushPromises()
  vi.useFakeTimers()
  const reads = io.read.mock.calls.length
  title = 'Fresh CLI title'
  for (let index = 0; index < 10; index++) {
    for (const tab of [one, two]) {
      const attempt = { requestId: tab.requestId, runId: tab.runId, generation: tab.generation }
      tabs.applyObservation(tab.tabId, attempt, { observation: 'active', activity: index % 2 ? 'idle' : 'working' })
      tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'reply-ended', eventId: 'reply-' + index, receivedAt: index, runId: tab.runId, generation: tab.generation })
    }
    await flushPromises()
  }
  await vi.advanceTimersByTimeAsync(249)
  expect(io.read).toHaveBeenCalledTimes(reads)
  await vi.advanceTimersByTimeAsync(1); await flushPromises()
  expect(useUnifiedSessionsStore().sessions.find(row => row.adapterSessionId === one.tabId)?.title).toBe(title)
  expect(io.read.mock.calls.length - reads).toBe(1)
  expect(io.scope.mock.calls[io.scope.mock.calls.length - 1]?.[0].projectIds.sort()).toEqual(['other', 'project'])
  const afterReply = io.read.mock.calls.length
  for (let index = 0; index < 10; index++) {
    tabs.applyObservation(one.tabId, { requestId: one.requestId, runId: one.runId, generation: one.generation }, { observation: 'active', activity: index % 2 ? 'idle' : 'working' })
    await flushPromises()
  }
  await vi.advanceTimersByTimeAsync(350)
  expect(io.read).toHaveBeenCalledTimes(afterReply)
  // 身份确认可以重读一次；确认回执、选择和焦点随后不能再触发 IO。
  tabs.applyObservationIdentity(one.tabId, one, 'history-id')
  await flushPromises(); await vi.advanceTimersByTimeAsync(250); await flushPromises()
  expect(io.read).toHaveBeenCalledTimes(afterReply + 1)
  tabs.ackReplyEndNotice(one.tabId, one, 'reply-9')
  await flushPromises(); await vi.advanceTimersByTimeAsync(350)
  expect(io.read).toHaveBeenCalledTimes(afterReply + 1)
  const afterIdentity = io.read.mock.calls.length
  title = 'First prompt is already in native history'
  tabs.applyObservationNotice(one.tabId, one, { kind: 'prompt-submitted', eventId: 'first-prompt', receivedAt: 100, runId: one.runId, generation: one.generation })
  tabs.applyObservationNotice(one.tabId, one, { kind: 'tool-started', eventId: 'following-tool', receivedAt: 101, runId: one.runId, generation: one.generation })
  await flushPromises(); await vi.advanceTimersByTimeAsync(250); await flushPromises()
  expect(io.read).toHaveBeenCalledTimes(afterIdentity + 1)
  expect(useUnifiedSessionsStore().sessions.find(row => row.adapterSessionId === one.tabId)?.title).toBe(title)
  tabs.applyObservationNotice(one.tabId, one, { kind: 'prompt-submitted', eventId: 'first-prompt', receivedAt: 100, runId: one.runId, generation: one.generation })
  await flushPromises(); await vi.advanceTimersByTimeAsync(350)
  expect(io.read).toHaveBeenCalledTimes(afterIdentity + 1)
})

// 去抖期间失去原 attempt、配置或组件 owner 的事件，不能发起新的历史读取。
it.each(['restart', 'close', 'profile', 'unmount'] as const)('History_QueuedOwnerRevoked_062 %s', async invalidation => {
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cl', revision: '7', cli: 'claude', name: 'Claude', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: true }, env: {} }] })
  io.scope.mockResolvedValue({ cli: 'claude' })
  const { runtime } = render(); await flushPromises(); await runtime.refresh()
  vi.useFakeTimers()
  const tabs = useNativeTabsStore(), tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
  tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
  tabs.applyObservation(tab.tabId, tab, { observation: 'active', activity: 'idle' })
  tabs.applyObservationNotice(tab.tabId, tab, { kind: 'reply-ended', eventId: 'queued', receivedAt: 1, runId: tab.runId, generation: tab.generation })
  await flushPromises()
  const reads = io.read.mock.calls.length
  if (invalidation === 'restart' || invalidation === 'close') tabs.applyLaunchStatus(tab.tabId,
    { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '2', phase: 'exited', failure: null })
  if (invalidation === 'restart') tabs.restart(tab.tabId, { profileId: 'cl', profileRevision: '7' })
  if (invalidation === 'close') tabs.close(tab.tabId)
  if (invalidation === 'profile') useCliProfilesStore().profiles[0].revision = '8'
  if (invalidation === 'unmount') wrappers.pop()!.unmount()
  await flushPromises(); await vi.advanceTimersByTimeAsync(350)
  expect(io.read).toHaveBeenCalledTimes(reads)
})

// 读取未结束时的新事件只保留一次后继读取，完成后显示最新 CLI 标题。
it('History_InFlightKeepsLatest_063', async () => {
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cl', revision: '7', cli: 'claude', name: 'Claude', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: true }, env: {} }] })
  io.scope.mockResolvedValue({ cli: 'claude' })
  const result = (title: string) => ({ state: 'ready', items: [{ type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title, cwd: '/repo' }], hasMore: false })
  io.read.mockResolvedValue(result('Initial'))
  const { runtime } = render(); await flushPromises(); await runtime.refresh()
  vi.useFakeTimers()
  const tabs = useNativeTabsStore(), tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
  tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
  tabs.applyObservation(tab.tabId, tab, { observation: 'active', activity: 'idle' })
  let finish!: (value: unknown) => void
  io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const reads = io.read.mock.calls.length
  tabs.applyObservationIdentity(tab.tabId, tab, 'history-id')
  await flushPromises(); await vi.advanceTimersByTimeAsync(250); await flushPromises()
  expect(io.read).toHaveBeenCalledTimes(reads + 1)
  for (let index = 0; index < 10; index++) {
    tabs.applyObservationNotice(tab.tabId, tab, { kind: 'reply-ended', eventId: 'later-' + index, receivedAt: index, runId: tab.runId, generation: tab.generation })
    await flushPromises()
  }
  await vi.advanceTimersByTimeAsync(350)
  expect(io.read).toHaveBeenCalledTimes(reads + 1)
  io.read.mockResolvedValue(result('Latest')); finish(result('Earlier'))
  await flushPromises(); await vi.advanceTimersByTimeAsync(250); await flushPromises()
  expect(useUnifiedSessionsStore().sessions.find(row => row.adapterSessionId === tab.tabId)?.title).toBe('Latest')
  expect(io.read).toHaveBeenCalledTimes(reads + 2)
})

// 第三组等待前两组结束时被关闭，下一批不得扫描已经撤销的 owner。
it('History_BatchRechecksOwner_064', async () => {
  const profile = { revision: '7', cli: 'claude', name: 'Claude', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: true }, env: {} }
  io.profiles.mockResolvedValue({ revision: '7', profiles: ['a', 'b', 'c'].map(id => ({ ...profile, id })) })
  io.scope.mockImplementation(async input => ({ cli: 'claude', scopeId: input.profileId }))
  io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
  const { runtime } = render(); await flushPromises(); await runtime.refresh()
  vi.useFakeTimers()
  const tabs = useNativeTabsStore()
  const owners = ['a', 'b', 'c'].map(profileId => tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId, profileRevision: '7', action: { kind: 'new' } }))
  const finish: (() => void)[] = []
  io.read.mockImplementation(() => new Promise(resolve => { finish.push(() => resolve({ state: 'ready', items: [], hasMore: false })) }))
  const reads = io.read.mock.calls.length
  for (const tab of owners) {
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    tabs.applyObservation(tab.tabId, tab, { observation: 'active', activity: 'idle' })
    tabs.applyObservationIdentity(tab.tabId, tab, 'history-id')
  }
  await flushPromises(); await vi.advanceTimersByTimeAsync(250); await flushPromises()
  expect(io.read).toHaveBeenCalledTimes(reads + 2)
  const third = owners[2]
  tabs.applyLaunchStatus(third.tabId, { instanceId: 'i', requestId: third.requestId, run: { runId: third.runId, generation: third.generation }, revision: '2', phase: 'exited', failure: null })
  tabs.close(third.tabId); await flushPromises()
  io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
  finish.forEach(resolve => resolve()); await flushPromises()
  await vi.advanceTimersByTimeAsync(350)
  expect(io.read).toHaveBeenCalledTimes(reads + 2)
})

// 自动刷新遇到临时读取失败时保留已知显示行，但保持错误与缺失证明失效。
it.each(['SOURCE_BUSY', 'SOURCE_CHANGED'])('History_AutoFailureKeepsRows_065 %s', async code => {
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cl', revision: '7', cli: 'claude', name: 'Claude', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: true }, env: {} }] })
  io.scope.mockResolvedValue({ cli: 'claude', sourceRootKey: 'held-root' })
  io.read.mockResolvedValue({ state: 'ready', items: [{ type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'Known title', cwd: '/repo' }], hasMore: false })
  const { runtime } = render(); await flushPromises(); await runtime.refresh()
  vi.useFakeTimers()
  const tabs = useNativeTabsStore(), tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
  tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
  tabs.applyObservation(tab.tabId, tab, { observation: 'active', activity: 'idle' })
  io.read.mockResolvedValue({ state: 'unavailable', reason: code, items: [], hasMore: false })
  tabs.applyObservationIdentity(tab.tabId, tab, 'history-id')
  await flushPromises(); await vi.advanceTimersByTimeAsync(250); await flushPromises()
  expect(useNativeHistoryStore().all()[0].error).toBe(code)
  expect(useNativeHistoryStore().all()[0].absenceEvidence).toBeUndefined()
  expect(runtime.sourceDiagnosticsWarnings.value.some(warning => warning.code === code)).toBe(true)
  expect(useUnifiedSessionsStore().sessions.find(row => row.adapterSessionId === tab.tabId)?.title).toBe('Known title')
})

// 来源问题入口紧贴面板标题（标题右侧），不占列表行也不与右侧动作组混排：引导期
// 来源（项目元数据/配置）失败时列表为空，没有行可以挂载该信号。
it('Warnings_SourceEntryFollowsPanelTitle_069', async () => {
  io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation' })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } })
  wrappers.push(w); await flushPromises()
  const group = w.get('.panel-title-group')
  expect(group.get('.panel-title').text()).toBe(en.sessions)
  const entry = group.get('[data-history-source-entry]')
  // 排在标题之后，且不属于右侧动作组。
  expect(group.element.lastElementChild?.contains(entry.element)).toBe(true)
  expect(w.get('.header-actions').find('[data-history-source-entry]').exists()).toBe(false)
  // 图标按钮：可访问名称留在 aria-label/tooltip，没有可见文案。
  expect(entry.classes()).toContain('ui-icon-button')
  expect(entry.attributes('aria-label')).toBe(en.historySourceIssues)
  // 入口是通往诊断抽屉的唯一路径，必须仍能打开抽屉。
  await entry.trigger('click'); await flushPromises()
  expect(document.querySelector('[data-history-source-dialog]')).not.toBeNull()
})

describe('history warning source mapping', () => {
  it('Runtime_OpencodeHistoryWarningSource_01', () => {
    expect(historyWarningSource('claude')).toBe('claude-history')
    expect(historyWarningSource('codex')).toBe('codex-history')
    expect(historyWarningSource('opencode')).toBe('opencode-history')
  })
})
