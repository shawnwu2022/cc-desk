import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import App from '@/App.vue'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import { createI18n } from 'vue-i18n'
import en from '@/i18n/locales/en'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { captureNativeAttempt, useNativeTabsStore } from '@/stores/nativeTabs'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useSessionStore } from '@/stores/session'
import type { ProjectsState } from '@/types/app'
import { useAppStore } from '@/stores/app'
import { useShellStore } from '@/stores/shell'

const io = vi.hoisted(() => ({ projects: vi.fn(), sessions: vi.fn(), profiles: vi.fn(), registered: vi.fn(), register: vi.fn(), patchProfile: vi.fn(), getState: vi.fn(), upsertRecord: vi.fn(), setPreference: vi.fn(), scope: vi.fn(), read: vi.fn(), writeText: vi.fn(), archive: vi.fn(), restore: vi.fn(), open: vi.fn(), remove: vi.fn(), runChecks: vi.fn(), ptySpawn: vi.fn(), ptyInput: vi.fn(), ptyKill: vi.fn(), nativeStop: vi.fn(), getConfig: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), runChecks: io.runChecks, ptySpawn: io.ptySpawn, ptyInput: io.ptyInput, ptyKill: io.ptyKill, getProjectsState: io.getState, upsertSessionUiRecord: io.upsertRecord, setProjectLaunchPreference: io.setPreference, updateAppConfig: vi.fn().mockResolvedValue(undefined), getAppConfig: io.getConfig, archiveSession: io.archive, restoreSession: io.restore, getProjects: io.projects, getSessions: io.sessions, openInFileManager: io.open, createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }), onHookEvent: async () => () => {} }))
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
  io.getConfig.mockResolvedValue({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' })
  persisted = { pinnedProjects: [], archivedSessions: {}, launchPreferences: {} }
  io.getState.mockImplementation(async () => structuredClone(persisted))
  io.upsertRecord.mockImplementation(async (key, record) => { persisted.sessionRecords ??= {}; persisted.sessionRecords[key] = structuredClone(record); return structuredClone(persisted) })
  io.setPreference.mockImplementation(async (path, preference) => { persisted.launchPreferences![path] = structuredClone(preference); return structuredClone(persisted) })
  io.archive.mockImplementation(async (path, id) => { persisted.archivedSessions[path] = [...(persisted.archivedSessions[path] ?? []), id]; return structuredClone(persisted) })
  io.restore.mockImplementation(async (path, id) => { persisted.archivedSessions[path] = (persisted.archivedSessions[path] ?? []).filter(value => value !== id); return structuredClone(persisted) })
  io.projects.mockResolvedValue([{ path: '/legacy', name: 'Legacy' }]); io.sessions.mockResolvedValue([])
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  io.register.mockImplementation(async path => ({ revision: '2', projectId: 'registered-new', projects: [{ projectId: 'registered-new', hostId: 'host', sourcePathKey: 'source-new', selectedPath: path, canonicalPath: path, alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }))
  io.scope.mockResolvedValue({ cli: 'codex' }); io.read.mockResolvedValue({ state: 'ready', items: [{ type: 'session', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }] })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); vi.unstubAllGlobals(); vi.restoreAllMocks() })

function renderApp() {
  const child = defineComponent({ props: ['tabId', 'active'], setup(props, { expose }) {
    expose({ focus() {}, fitVisible() {}, async recover() {}, async stop() { io.nativeStop(props.tabId); useNativeTabsStore().tab(props.tabId)!.status = 'exited' } })
    return () => h('div', { 'data-runtime-tab': props.tabId })
  } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: child, TerminalView: true, SettingsView: true } } });
  wrappers.push(w); return w
}
async function openNativeRow(w: VueWrapper) {
  await flushPromises()
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  tabs.tab(tab.tabId)!.status = 'running'; tabs.tab(tab.tabId)!.launchRevision = '1'
  await flushPromises()
  await useUnifiedSessionsStore().activateSession(`native-tab:${tab.tabId}`); await flushPromises()
  const project = w.findAll('.project-node').find(row => row.text().includes('repo'))!
  await project.get('.project-main').trigger('click'); await flushPromises()
  return { tabs, tab, row: w.get(`[data-session-row="native-tab:${tab.tabId}"]`) }
}
describe('Final workspace review regressions', () => {
  it('DIAGNOSTICS: visible session menu action must be handled', async () => {
    const w = renderApp(); const { row } = await openNativeRow(w)
    await row.get('.session-overflow-trigger button').trigger('click'); await flushPromises()
    document.querySelector<HTMLElement>('[data-item-id="view-diagnostics"]')!.click(); await flushPromises()
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: useUnifiedSessionsStore().activeSessionId!, action: 'view-diagnostics' }); await flushPromises()
    expect(document.querySelector('[role="dialog"]'), 'diagnostics detail surface').not.toBeNull()
  })
  it('RENAME: row F2 draft must be invalidated when exact Native attempt changes', async () => {
    const w = renderApp(); const { row, tabs, tab } = await openNativeRow(w)
    await row.trigger('keydown', { key: 'F2', code: 'F2' }); await flushPromises()
    await row.get('input').setValue('Old attempt draft')
    tabs.tab(tab.tabId)!.status = 'exited'
    tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
    const input = w.find(`[data-session-row="native-tab:${tab.tabId}"] input`)
    expect(input.exists(), 'replacement attempt discards the old editor').toBe(false)
    useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: `native-tab:${tab.tabId}`, title: 'Old attempt draft' }); await flushPromises()
    expect(io.upsertRecord, 'old draft must not write metadata after replacement attempt').not.toHaveBeenCalled()
  })
  it('RENAME: menu entry must capture canonical edit ownership before Save', async () => {
    const w = renderApp(); const { row, tab } = await openNativeRow(w)
    await row.get('.session-overflow-trigger button').trigger('click'); await flushPromises()
    document.querySelector<HTMLElement>('[data-item-id="rename"]')!.click(); await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(s => s.id === `native-tab:${tab.tabId}`)?.renameState).toBe('editing')
  })
  it('POINTER: a mouse menu click must dispatch stop, without a following row activation', async () => {
    const w = renderApp(); const { row } = await openNativeRow(w)
    const stop = vi.spyOn(useUnifiedSessionsStore(), 'stopSession').mockResolvedValue(undefined)
    await row.get('.session-overflow-trigger button').trigger('click'); await flushPromises()
    document.querySelector<HTMLElement>('[data-item-id="stop"]')!.click(); await flushPromises()
    expect(stop).toHaveBeenCalledTimes(1)
    expect((w.emitted('workspace-request') ?? []).map(event => (event[0] as any).kind)).toEqual(['menu-action'])
  })
  it('KEYBOARD: ArrowDown from an expanded session row moves within the tree', async () => {
    const w = renderApp(); const { row } = await openNativeRow(w)
    expect(w.findAll('[data-session-row]').length).toBeGreaterThan(1)
    ;(row.element as HTMLElement).focus(); await row.trigger('keydown', { key: 'ArrowDown', code: 'ArrowDown' }); await flushPromises()
    expect(document.activeElement, 'there is a following historical row, but no navigation handler').not.toBe(row.element)
  })
  it('QUICK SWITCH: Ctrl+P must expose searchable project and session choices', async () => {
    const w = renderApp(); await openNativeRow(w)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', code: 'KeyP', ctrlKey: true, bubbles: true })); await flushPromises()
    expect(useShellStore().section, 'current handler opens only project maintenance, with no session switcher').toBe('workspace')
    expect(document.activeElement).toBe(w.get('.search-input').element)
  })
  it('ACTIVITY: identical running status polls do not count as meaningful activity', () => {
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    const poll = { instanceId: 'backend', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running' as const, failure: null }
    const now = vi.spyOn(Date, 'now').mockReturnValue(1000)
    tabs.applyLaunchStatus(tab.tabId, poll)
    now.mockReturnValue(61000)
    tabs.applyLaunchStatus(tab.tabId, poll)
    expect(tabs.tab(tab.tabId)!.lastActivityAt).toBe(1000)
    now.mockRestore()
  })
  // 鼠标关闭/归档进入真实确认，取消无副作用，明确确认只影响准确拥有的会话。
  it.each(['close', 'archive'] as const)('Pointer_ConfirmOwner_001 %s', async action => {
    const w = renderApp(); const { tabs, tab, row } = await openNativeRow(w)
    if (action === 'archive') {
      tabs.tab(tab.tabId)!.action = { kind: 'resume-id', nativeSessionId: 'history-id' }
      tabs.tab(tab.tabId)!.sourceSessionKey = 'root-key'; await flushPromises()
    }
    await row.get('.session-overflow-trigger button').trigger('click'); await flushPromises()
    const item = document.querySelector<HTMLElement>(`[data-item-id="${action}"]`)!
    expect(row.element.contains(item), 'normal menus teleport outside the clickable row').toBe(false)
    item.click(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    expect(catalog.sessionConfirmation?.kind).toBe(action === 'close' ? 'close-running' : 'stop-and-archive')
    expect(io.nativeStop).not.toHaveBeenCalled(); expect(io.archive).not.toHaveBeenCalled()
    await catalog.confirmSessionAction(); await flushPromises()
    expect(io.nativeStop).toHaveBeenCalledTimes(1)
    expect(tabs.tab(tab.tabId)).toBeUndefined()
    expect(io.archive).toHaveBeenCalledTimes(action === 'archive' ? 1 : 0)
  })
  // 抽屉明确不 teleport，菜单点击也不能冒泡为 activate。
  it('Pointer_ArchiveRestore_002', async () => {
    const w = renderApp(); await openNativeRow(w)
    const catalog = useUnifiedSessionsStore()
    const history = catalog.sessions.find(value => value.id.startsWith('native-history:'))!
    await catalog.archiveSession(history.id); await flushPromises()
    await w.get('[data-view-archived]').trigger('click'); await flushPromises()
    const drawer = w.getComponent(ArchivedSessionsDrawer)
    const list = drawer.findComponent({ name: 'SessionList' })
    const row = list.get('[data-session-row]')
    await row.get('.session-overflow-trigger button').trigger('click'); await flushPromises()
    const item = row.get('[data-item-id="restore-archive"]')
    await item.trigger('click'); await flushPromises()
    expect(io.restore).toHaveBeenCalledTimes(1)
    expect(catalog.resumeDialog).toBeNull()
    expect(drawer.emitted('restore-request')).toEqual([[history.id]])
    expect(list.emitted('activate')).toBeUndefined()
    expect(catalog.sessions.find(value => value.id === history.id)?.archived).toBe(false)
  })
  // 已取消的编辑不能通过迟到 Save 被解释为新的尝试。
  it('Rename_CancelRejectsLateSave_003', async () => {
    const w = renderApp(); const { row, tab } = await openNativeRow(w)
    await row.trigger('keydown', { key: 'F2', code: 'F2' }); await flushPromises()
    await row.get('input').setValue('Discarded')
    await row.get('input').trigger('keydown', { key: 'Escape' }); await flushPromises()
    useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: `native-tab:${tab.tabId}`, title: 'Discarded' }); await flushPromises()
    expect(row.find('input').exists()).toBe(false)
    expect(io.upsertRecord).not.toHaveBeenCalled()
  })
  // 诊断仅展示当前准确 owner 的白名单快照，任意内部字段不得渲染。
  it.each(['restart', 'source', 'close', 'selection', 'navigation'] as const)('Diagnostics_Invalidate_004 %s', async change => {
    const w = renderApp(); const { tabs, tab } = await openNativeRow(w)
    const live = tabs.tab(tab.tabId)!
    live.title = 'SECRET TITLE /private/user'; live.errorCode = 'TOKEN=secret /private/config'
    live.action = { kind: 'raw', argv: ['--secret=redacted'] }; await flushPromises()
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action: 'view-diagnostics' }); await flushPromises()
    const dialog = document.querySelector<HTMLElement>('[data-session-diagnostics]')!
    expect(dialog).not.toBeNull()
    expect(dialog.textContent).toContain('Codex CLI')
    expect(dialog.textContent).not.toMatch(/SECRET|TOKEN|private|--secret|cx|root-key/)
    expect(useShellStore().pendingRequest).toBeNull()
    expect(io.ptySpawn).not.toHaveBeenCalled(); expect(io.ptyInput).not.toHaveBeenCalled(); expect(io.nativeStop).not.toHaveBeenCalled()
    if (change === 'restart') { live.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }) }
    else if (change === 'source') live.profileRevision = '8'
    else if (change === 'close') tabs.close(tab.tabId)
    else if (change === 'selection') useUnifiedSessionsStore().selectProjectContext('/legacy')
    else useShellStore().navigate('projects')
    await flushPromises()
    expect(document.querySelector('[data-session-diagnostics]')).toBeNull()
  })
  // 方向键只遍历当前可见结果；历史行必须 Enter 后才显示明确恢复流程。
  it('Keyboard_FilteredHistory_005', async () => {
    const w = renderApp(); await openNativeRow(w)
    const search = w.get('.search-input')
    await search.setValue('History'); await search.trigger('keydown', { key: 'ArrowDown' }); await flushPromises()
    const first = document.activeElement as HTMLElement
    expect(first.classList.contains('project-row')).toBe(true)
    first.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })); await flushPromises()
    const historical = document.activeElement as HTMLElement
    expect(historical.dataset.sessionRow).toMatch(/^native-history:/)
    expect(useUnifiedSessionsStore().resumeDialog).toBeNull()
    historical.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); await flushPromises()
    expect(useUnifiedSessionsStore().resumeDialog?.sessionId).toBe(historical.dataset.sessionRow)
    expect(io.ptySpawn).not.toHaveBeenCalled()
  })
  // 自定义快速切换从其他页面恢复树与焦点；IME 与弹窗仍拥有自己的键盘。
  it('QuickSwitch_CustomMapping_006', async () => {
    const w = renderApp(); const { tab } = await openNativeRow(w)
    const app = useAppStore(); const shell = useShellStore()
    app.shortcutBindings = { ...app.shortcutBindings, projects: 'Mod+KeyK' }
    shell.navigate('settings'); shell.sidebarVisible = false
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', code: 'KeyK', ctrlKey: true, bubbles: true, isComposing: true })); await flushPromises()
    expect(shell.section).toBe('settings')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', code: 'KeyK', ctrlKey: true, bubbles: true })); await flushPromises()
    expect(shell.section).toBe('workspace'); expect(shell.sidebarVisible).toBe(true)
    expect(document.activeElement).toBe(w.get('.search-input').element)
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action: 'close' }); await flushPromises()
    const active = document.activeElement
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', code: 'KeyK', ctrlKey: true, bubbles: true })); await flushPromises()
    expect(document.activeElement).toBe(active)
    expect(useUnifiedSessionsStore().sessionConfirmation?.kind).toBe('close-running')
  })

  // Ctrl+P 的项目结果可直接选择项目，不创建或恢复会话。
  it('QuickSwitch_SelectProject_007', async () => {
    const w = renderApp(); await openNativeRow(w)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', code: 'KeyP', ctrlKey: true, bubbles: true })); await flushPromises()
    const search = w.get('.search-input'); await search.setValue('Legacy')
    await search.trigger('keydown', { key: 'ArrowDown' }); await flushPromises()
    const result = document.activeElement as HTMLElement
    expect(result.dataset.projectKey).toBe('/legacy')
    result.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); await flushPromises()
    expect(useUnifiedSessionsStore().activeSessionId).toBeNull()
    expect(w.getComponent({ name: 'WorkspaceView' }).props('project')?.projectPath).toBe('/legacy')
    expect(useUnifiedSessionsStore().resumeDialog).toBeNull(); expect(io.ptySpawn).not.toHaveBeenCalled()
  })
  // 同一项目中 Legacy/Native 排序由实际活动决定；100次确认轮询不得抢到首位。
  it('Activity_MixedOrderAndBurst_008', async () => {
    const w = renderApp(); const { tabs, tab } = await openNativeRow(w)
    const now = vi.spyOn(Date, 'now').mockReturnValue(1000)
    const poll = { instanceId: 'backend', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running' as const, failure: null }
    tabs.tab(tab.tabId)!.lastActivityAt = 1000
    const legacy = useSessionStore(); const legacyId = legacy.createTab('/repo')
    legacy.tabs.get(legacyId)!.lastActiveAt = 2000
    await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const order = () => catalog.projectGroups.find(group => group.projectPath === '/repo')!.sessions.filter(row => row.id.includes('tab:')).map(row => row.id)
    expect(order()).toEqual([`legacy-tab:${legacyId}`, `native-tab:${tab.tabId}`])
    const refresh = vi.spyOn(catalog, 'refresh')
    now.mockReturnValue(61000)
    for (let n = 0; n < 100; n++) { tabs.applyLaunchStatus(tab.tabId, poll); await flushPromises() }
    expect(order()).toEqual([`legacy-tab:${legacyId}`, `native-tab:${tab.tabId}`])
    expect(refresh).not.toHaveBeenCalled()
    for (let n = 0; n < 100; n++) { now.mockReturnValue(62000 + n); tabs.touch(tab.tabId, captureNativeAttempt(tab)); await flushPromises() }
    expect(order()).toEqual([`native-tab:${tab.tabId}`, `legacy-tab:${legacyId}`])
    expect(refresh).toHaveBeenCalledTimes(1)
  })

  // 准备中的占位记录也有诊断入口，取消 owner 后立即关闭而不启动 CLI。
  it('Diagnostics_PreparingOwner_009', async () => {
    renderApp(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    let finish!: () => void
    catalog.configureCreationPreparer(input => new Promise(resolve => { finish = () => resolve(input) }))
    const creating = catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    const cancelled = expect(creating).rejects.toThrow('NEW_SESSION_CANCELLED')
    const id = catalog.activeSessionId!
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: id, action: 'view-diagnostics' }); await flushPromises()
    expect(document.querySelector('[data-session-diagnostics]')?.textContent).toContain('Preparing session')
    await catalog.stopSession(id); finish(); await cancelled; await flushPromises()
    expect(document.querySelector('[data-session-diagnostics]')).toBeNull()
    expect(io.ptySpawn).not.toHaveBeenCalled(); expect(useNativeTabsStore().tabs.size).toBe(0)
  })

  // 退出快捷切换或离开侧栏后，普通项目 Enter 恢复展开行为，不切换当前会话。
  it.each(['escape', 'navigation'] as const)('QuickSwitch_DismissRestoresEnter_010 %s', async dismissal => {
    const w = renderApp(); await openNativeRow(w)
    const shell = useShellStore(); const selected = useUnifiedSessionsStore().activeSessionId
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', code: 'KeyP', ctrlKey: true, bubbles: true })); await flushPromises()
    if (dismissal === 'escape') {
      await w.get('.search-input').trigger('keydown', { key: 'Escape' }); await flushPromises()
      expect(shell.sidebarVisible).toBe(false)
      shell.sidebarVisible = true
    } else { shell.navigate('projects'); await flushPromises(); shell.navigate('workspace') }
    await flushPromises()
    const project = w.get('[data-project-key="/legacy"]')
    expect(project.attributes('aria-expanded')).toBe('false')
    await project.trigger('keydown', { key: 'Enter' }); await flushPromises()
    expect(project.attributes('aria-expanded')).toBe('true')
    expect(useUnifiedSessionsStore().activeSessionId).toBe(selected)
  })

})
