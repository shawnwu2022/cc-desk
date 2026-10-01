import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, ref, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import App from '@/App.vue'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import { createI18n } from 'vue-i18n'
import en from '@/i18n/locales/en'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
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

const io = vi.hoisted(() => ({ projects: vi.fn(), sessions: vi.fn(), profiles: vi.fn(), registered: vi.fn(), register: vi.fn(), patchProfile: vi.fn(), getState: vi.fn(), setPreference: vi.fn(), scope: vi.fn(), read: vi.fn(), writeText: vi.fn(), archive: vi.fn(), restore: vi.fn(), open: vi.fn(), remove: vi.fn(), runChecks: vi.fn(), ptySpawn: vi.fn(), ptyInput: vi.fn(), ptyKill: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), runChecks: io.runChecks, ptySpawn: io.ptySpawn, ptyInput: io.ptyInput, ptyKill: io.ptyKill, getProjectsState: io.getState, setProjectLaunchPreference: io.setPreference, updateAppConfig: vi.fn().mockResolvedValue(undefined), getAppConfig: vi.fn().mockResolvedValue({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' }), archiveSession: io.archive, restoreSession: io.restore, getProjects: io.projects, getSessions: io.sessions, openInFileManager: io.open, createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }), onHookEvent: async () => () => {} }))
vi.mock('@/api/cli', () => ({ cliListProfiles: io.profiles, cliPatchProfile: io.patchProfile }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: io.registered, registerProject: io.register, removeProject: io.remove }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: io.writeText }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMaximized: async () => false }) }))
let persisted: ProjectsState
const wrappers: VueWrapper[] = []
beforeEach(() => {
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => 'legacy-tab-id' })
  localStorage.clear(); setActivePinia(createPinia()); vi.clearAllMocks(); useProjectsStateStore().loaded = true
  persisted = { pinnedProjects: [], archivedSessions: {}, launchPreferences: {} }
  io.getState.mockImplementation(async () => structuredClone(persisted))
  io.setPreference.mockImplementation(async (path, preference) => { persisted.launchPreferences![path] = structuredClone(preference); return structuredClone(persisted) })
  io.projects.mockResolvedValue([{ path: '/legacy', name: 'Legacy' }]); io.sessions.mockResolvedValue([])
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  io.register.mockImplementation(async path => ({ revision: '2', projectId: 'registered-new', projects: [{ projectId: 'registered-new', hostId: 'host', sourcePathKey: 'source-new', selectedPath: path, canonicalPath: path, alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }))
  io.scope.mockResolvedValue({ cli: 'codex' }); io.read.mockResolvedValue({ state: 'ready', items: [{ type: 'session', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }] })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); vi.unstubAllGlobals() })
function render() {
  const port = { startLegacy: vi.fn(), stopLegacy: vi.fn(), restartLegacy: vi.fn(), renameLegacy: vi.fn(), stopNative: vi.fn().mockResolvedValue(undefined), recoverNative: vi.fn().mockResolvedValue(undefined), focus: vi.fn() }
  let runtime!: ReturnType<typeof useUnifiedWorkspaceRuntime>
  const w = mount(defineComponent({ setup() { runtime = useUnifiedWorkspaceRuntime(ref(port)); return () => null } })); wrappers.push(w)
  return { runtime, port }
}
describe('Unified production runtime', () => {
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
  // 新建/恢复/运行中关闭与归档保持待处理，不能假装已完成或自动执行。
  it('Runtime_OpensOwnedConfirmations_005', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    const unified = useUnifiedSessionsStore(); await unified.refresh(); const shell = useShellStore()
    for (const action of ['close', 'archive'] as const) {
      shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action }); await flushPromises()
      expect(shell.pendingRequest).toBeNull(); expect(unified.sessionConfirmation?.kind).toBe(action === 'close' ? 'close-running' : 'stop-and-archive'); expect(port.stopNative).not.toHaveBeenCalled(); expect(tabs.tab(tab.tabId)).toBeDefined()
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
    useSessionStore().tabs.get(id)!.pending = true; await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(s => s.adapterSessionId === id)?.attentionState).toBe('needs-user')
    expect(useNativeHistoryStore().all()).toHaveLength(1); expect(useWorkspaceStore().projects).toHaveLength(1)
  })
  // 目录快照尚未发布运行态时，关闭动作必须检查真实store，保持待确认。
  it('Runtime_CloseChecksLiveState_007', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    const unified = useUnifiedSessionsStore(); await unified.refresh(); await flushPromises()
    tabs.tab(tab.tabId)!.status = 'running'
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action: 'close' })
    await flushPromises()
    expect(port.stopNative).not.toHaveBeenCalled()
    expect(useShellStore().pendingRequest).toBeNull(); expect(unified.sessionConfirmation?.kind).toBe('close-running')
    expect(tabs.tab(tab.tabId)).toBeDefined()
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
    useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: opened.id, title: 'Unified name' }); await flushPromises()
    expect(tabsTitle()).toBe('Unified name'); expect(useShellStore().pendingRequest).toBeNull()
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
      launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' },
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
