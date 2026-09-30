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
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useSessionStore } from '@/stores/session'
import { useShellStore } from '@/stores/shell'

const io = vi.hoisted(() => ({ projects: vi.fn(), sessions: vi.fn(), profiles: vi.fn(), registered: vi.fn(), scope: vi.fn(), read: vi.fn(), writeText: vi.fn(), archive: vi.fn(), restore: vi.fn(), open: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), updateAppConfig: vi.fn().mockResolvedValue(undefined), getAppConfig: vi.fn().mockResolvedValue({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' }), archiveSession: io.archive, restoreSession: io.restore, getProjects: io.projects, getSessions: io.sessions, openInFileManager: io.open, createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }), onHookEvent: async () => () => {} }))
vi.mock('@/api/cli', () => ({ cliListProfiles: io.profiles, cliPatchProfile: vi.fn() }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: io.registered }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: io.writeText }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMaximized: async () => false }) }))
const wrappers: VueWrapper[] = []
beforeEach(() => {
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => 'legacy-tab-id' })
  setActivePinia(createPinia()); vi.clearAllMocks(); useProjectsStateStore().loaded = true
  io.projects.mockResolvedValue([{ path: '/legacy', name: 'Legacy' }]); io.sessions.mockResolvedValue([])
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
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
  // 前端路径不能自动注册或授权Native项目，原始argv保留字符串边界。
  it('Runtime_RequiresRegisteredProject_003', async () => {
    render(); await flushPromises(); const unified = useUnifiedSessionsStore()
    await expect(unified.createSession({ projectKey: '/unknown', projectPath: '/unknown', cli: 'codex', action: { kind: 'new' } })).rejects.toThrow('PROJECT_NOT_FOUND')
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
    expect(activate).toHaveBeenCalledTimes(1); expect(shell.pendingRequest?.kind).toBe('confirmation')
    shell.section = 'settings'; await flushPromises(); expect(activate).toHaveBeenCalledTimes(1)
  })
  // 新建/恢复/运行中关闭与归档保持待处理，不能假装已完成或自动执行。
  it('Runtime_LeavesUnconfirmedPending_005', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    const unified = useUnifiedSessionsStore(); await unified.refresh(); const shell = useShellStore()
    for (const action of ['close', 'archive', 'resume'] as const) {
      shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action }); await flushPromises()
      expect(shell.pendingRequest).toMatchObject({ action }); expect(port.stopNative).not.toHaveBeenCalled(); expect(tabs.tab(tab.tabId)).toBeDefined()
    }
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/repo', projectPath: '/repo' } }); await flushPromises()
    expect(shell.pendingRequest?.kind).toBe('new-session'); expect(tabs.tabs.size).toBe(1)
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
    expect(useShellStore().pendingRequest).toMatchObject({ action: 'close' })
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

})
