import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { defineComponent, h } from 'vue'
import { createI18n } from 'vue-i18n'
import { randomUUID } from 'node:crypto'
import App from '@/App.vue'
import { useShellStore } from '@/stores/shell'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore, matchesNativeAttempt } from '@/stores/nativeTabs'
import { useSessionStore } from '@/stores/session'
import { useProjectManagementStore } from '@/stores/projectManagement'
import en from '@/i18n/locales/en'
const io = vi.hoisted(() => ({ stop: vi.fn(), recover: vi.fn(), archive: vi.fn(), profiles: vi.fn(), patch: vi.fn(), state: vi.fn(), write: vi.fn(), remove: vi.fn(), projects: vi.fn(), registered: vi.fn(), pin: vi.fn(), sessions: vi.fn(), availability: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), getProjectsState: io.state, getProjects: io.projects, getSessions: io.sessions,
  getAppConfig: async () => ({ language: 'en', theme: 'light', terminalTheme: 'cc-box-light' }), updateAppConfig: async () => {}, onHookEvent: async () => () => {}, archiveSession: io.archive, pinProject: io.pin,
  createNativeProjectionClient: () => ({ scope: async () => ({ cli: 'codex' }), read: async () => ({ state: 'ready', items: [{ type: 'session', sessionKey: 'source', nativeSessionId: 'saved', title: 'Saved', cwd: '/repo' }], hasMore: false }) }) }))
vi.mock('@/api/cli', () => ({ cliListProfiles: io.profiles, cliPatchProfile: io.patch }))
vi.mock('@/api/cliAvailability', () => ({ cliGetAvailability: io.availability }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: io.registered, removeProject: io.remove }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: io.write }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMaximized: async () => false }) }))
const wrappers: VueWrapper[] = []
beforeEach(() => {
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID })
  setActivePinia(createPinia()); vi.clearAllMocks(); localStorage.clear()
  io.projects.mockResolvedValue([])
  io.sessions.mockResolvedValue([])
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  io.state.mockResolvedValue({ pinnedProjects: [], archivedSessions: {} })
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cx', revision: '7', cli: 'codex', name: 'Work', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/codex' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
  io.availability.mockImplementation(async (profileId, profileRevision) => ({ profileId, profileRevision, cli: 'codex', state: 'available-unverified', hostStatus: 'available', certified: false }))
  io.stop.mockResolvedValue(undefined); io.recover.mockResolvedValue(undefined)
  io.archive.mockImplementation(async (path, id) => ({ pinnedProjects: [], archivedSessions: { [path]: [id] } }))
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); document.body.innerHTML = ''; vi.restoreAllMocks(); vi.unstubAllGlobals() })
function render(realSettings = false) {
  const terminal = defineComponent({ props: ['tabId', 'active'], setup(props, { expose }) {
    expose({ focus() {}, fitVisible() {}, async stop(attempt: any) { await io.stop(props.tabId, attempt); const tab = useNativeTabsStore().tab(props.tabId); if (matchesNativeAttempt(tab, attempt)) tab!.status = 'stopped' }, async recover(attempt: any) { await io.recover(props.tabId, attempt) } })
    return () => h('div', { 'data-live-terminal': props.tabId, style: { display: props.active ? '' : 'none' } })
  } })
  const legacyTerminal = defineComponent({ props: ['visible'], setup(props, { expose }) {
    expose({ focus() {}, fitVisible() {} })
    return () => h('div', { 'data-legacy-terminal': '', style: { display: props.visible ? '' : 'none' } })
  } })
  const wrapper = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, TerminalView: legacyTerminal, SettingsView: !realSettings } } }); wrappers.push(wrapper); return wrapper
}
async function running(status: 'running' | 'unknown' = 'running') {
  const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', sourceSessionKey: 'source', action: { kind: 'resume-id', nativeSessionId: 'saved' }, title: 'Live work' })
  tabs.tab(tab.tabId)!.status = status; tabs.tab(tab.tabId)!.launchRevision = '7'
  await useUnifiedSessionsStore().refresh(); await useUnifiedSessionsStore().activateSession('native-tab:' + tab.tabId); await flushPromises(); return tab
}

// 关闭最后一个会话后，真实App清除终端、标题和选择，保留可恢复历史与新建引导。
it('WorkspaceClose_LastSession_001', async () => {
  const wrapper = render(); await flushPromises(); const tab = await running()
  const catalog = useUnifiedSessionsStore()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'close' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(catalog.activeSessionId).toBeNull()
  expect(wrapper.find('[data-live-terminal]').exists()).toBe(false)
  expect(wrapper.find('[data-session-title]').exists()).toBe(false)
  expect(wrapper.get('[data-unified-terminal-empty]').text()).toContain(en.workspaceWelcome)
  expect(catalog.sessions.some(row => row.nativeSessionId === 'saved' && row.title === 'Saved')).toBe(true)
})

// 关闭当前会话后选中仍打开的会话，保持其终端实例并同步工作区标题。
it('WorkspaceClose_SelectRemaining_002', async () => {
  const wrapper = render(); await flushPromises(); const remaining = await running()
  useNativeTabsStore().tab(remaining.tabId)!.title = 'Remaining work'; await flushPromises()
  const existingTerminal = wrapper.get(`[data-live-terminal="${remaining.tabId}"]`).element
  const closing = await running()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + closing.tabId, action: 'close' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(useUnifiedSessionsStore().activeSessionId).toBe('native-tab:' + remaining.tabId)
  expect(useNativeTabsStore().activeTabId).toBe(remaining.tabId)
  expect(wrapper.get(`[data-live-terminal="${remaining.tabId}"]`).element).toBe(existingTerminal)
  expect(wrapper.get(`[data-live-terminal="${remaining.tabId}"]`).isVisible()).toBe(true)
  expect(wrapper.get('[data-session-title]').text()).toBe('Remaining work')
  expect(wrapper.find('[data-unified-terminal-empty]').exists()).toBe(false)
})

// 关闭未选中会话不改变当前选择、标题或终端实例。
it('WorkspaceClose_BackgroundSession_003', async () => {
  const wrapper = render(); await flushPromises(); const closing = await running(); const selected = await running()
  const selectedTerminal = wrapper.get(`[data-live-terminal="${selected.tabId}"]`).element
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + closing.tabId, action: 'close' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(useNativeTabsStore().tab(closing.tabId)).toBeUndefined()
  expect(useUnifiedSessionsStore().activeSessionId).toBe('native-tab:' + selected.tabId)
  expect(wrapper.get(`[data-live-terminal="${selected.tabId}"]`).element).toBe(selectedTerminal)
  expect(wrapper.get(`[data-live-terminal="${selected.tabId}"]`).isVisible()).toBe(true)
})

// 停止请求失败时关闭没有完成，保留当前会话和终端并显示确认错误。
it('WorkspaceClose_StopFailure_004', async () => {
  const wrapper = render(); await flushPromises(); await running(); const closing = await running()
  const selectedTerminal = wrapper.get(`[data-live-terminal="${closing.tabId}"]`).element
  io.stop.mockRejectedValueOnce(new Error('NATIVE_STOP_UNCONFIRMED'))
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + closing.tabId, action: 'close' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(useUnifiedSessionsStore().activeSessionId).toBe('native-tab:' + closing.tabId)
  expect(useNativeTabsStore().tab(closing.tabId)?.status).toBe('running')
  expect(wrapper.get(`[data-live-terminal="${closing.tabId}"]`).element).toBe(selectedTerminal)
  expect(useUnifiedSessionsStore().confirmationError).not.toBeNull()
  expect(wrapper.find('[data-unified-terminal-empty]').exists()).toBe(false)
})

// 等待关闭时切换选择会撤销原确认；迟到停止不能清空或覆盖新选择。
it('WorkspaceClose_NewerSelection_005', async () => {
  const wrapper = render(); await flushPromises(); const newer = await running(); const closing = await running()
  let finish!: () => void
  io.stop.mockReturnValueOnce(new Promise<void>(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + closing.tabId, action: 'close' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: 'native-tab:' + newer.tabId }); await flushPromises()
  finish(); await flushPromises()
  expect(useUnifiedSessionsStore().activeSessionId).toBe('native-tab:' + newer.tabId)
  expect(wrapper.get(`[data-live-terminal="${newer.tabId}"]`).isVisible()).toBe(true)
  expect(useNativeTabsStore().tab(closing.tabId)).toBeDefined()
  expect(document.querySelector('[data-session-confirm]')).toBeNull()
})

// 同一行换成新尝试后，旧关闭完成不得移除新尝试或切换工作区。
it('WorkspaceClose_ReplacedAttempt_006', async () => {
  const wrapper = render(); await flushPromises(); await running(); const closing = await running()
  let finish!: () => void
  io.stop.mockReturnValueOnce(new Promise<void>(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + closing.tabId, action: 'close' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  useNativeTabsStore().tab(closing.tabId)!.generation++
  finish(); await flushPromises()
  expect(useNativeTabsStore().tab(closing.tabId)?.generation).toBe(2)
  expect(useUnifiedSessionsStore().activeSessionId).toBe('native-tab:' + closing.tabId)
  expect(wrapper.get(`[data-live-terminal="${closing.tabId}"]`).isVisible()).toBe(true)
  expect(document.querySelector('[data-session-confirm]')).toBeNull()
})

// 显式停止仍保留已结束终端供查看输出，不冒充已关闭。
it('WorkspaceClose_StopKeepsOutput_007', async () => {
  const wrapper = render(); await flushPromises(); const tab = await running()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'stop' }); await flushPromises()
  expect(useNativeTabsStore().tab(tab.tabId)?.status).toBe('stopped')
  expect(useUnifiedSessionsStore().activeSessionId).toBe('native-tab:' + tab.tabId)
  expect(wrapper.get(`[data-live-terminal="${tab.tabId}"]`).isVisible()).toBe(true)
})

// Legacy真实所有权已移除但历史读取未结束时，工作区不能继续显示旧标题或终端。
it('WorkspaceClose_LegacyHistoryWait_008', async () => {
  const wrapper = render(); await flushPromises()
  const legacy = useSessionStore(); const id = legacy.createTab('/repo', { name: 'Legacy closing' })
  const catalog = useUnifiedSessionsStore(); await catalog.refresh(); await catalog.activateSession('legacy-tab:' + id); await flushPromises()
  let finish!: (rows: never[]) => void
  io.sessions.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'legacy-tab:' + id, action: 'close' }); await flushPromises()
  expect(legacy.tabs.has(id)).toBe(false)
  expect(catalog.activeSessionId).toBeNull()
  expect(wrapper.find('[data-session-title]').exists()).toBe(false)
  expect(wrapper.get('[data-legacy-terminal]').isVisible()).toBe(false)
  expect(wrapper.get('[data-unified-terminal-empty]').text()).toContain(en.workspaceWelcome)
  finish([]); await flushPromises()
})

// Legacy关闭等待历史期间的新选择，不能被旧关闭完成时的候选会话覆盖。
it('WorkspaceClose_LegacyLateRead_009', async () => {
  const wrapper = render(); await flushPromises(); await running(); const newer = await running()
  const legacy = useSessionStore(); const id = legacy.createTab('/repo', { name: 'Legacy closing' })
  const catalog = useUnifiedSessionsStore(); await catalog.refresh(); await catalog.activateSession('legacy-tab:' + id); await flushPromises()
  let finish!: (rows: never[]) => void
  io.sessions.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'legacy-tab:' + id, action: 'close' }); await flushPromises()
  await catalog.activateSession('native-tab:' + newer.tabId); await flushPromises()
  const selectedTerminal = wrapper.get(`[data-live-terminal="${newer.tabId}"]`).element
  finish([]); await flushPromises()
  expect(catalog.activeSessionId).toBe('native-tab:' + newer.tabId)
  expect(wrapper.get(`[data-live-terminal="${newer.tabId}"]`).element).toBe(selectedTerminal)
  expect(wrapper.get(`[data-live-terminal="${newer.tabId}"]`).isVisible()).toBe(true)
})

// 剩余会话属于Legacy运行时也必须经其真实适配器激活，不能只改统一选择。
it('WorkspaceClose_SelectLegacy_010', async () => {
  const wrapper = render(); await flushPromises()
  const legacy = useSessionStore(); const id = legacy.createTab('/repo', { name: 'Legacy remaining' })
  legacy.setActiveTab(null)
  const closing = await running()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + closing.tabId, action: 'close' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(useUnifiedSessionsStore().activeSessionId).toBe('legacy-tab:' + id)
  expect(legacy.activeTabId).toBe(id)
  expect(wrapper.get('[data-legacy-terminal]').isVisible()).toBe(true)
  expect(wrapper.get('[data-session-title]').text()).toBe('Legacy remaining')
})

// 新建准备失败已落地为失败行，不能继续显示等待连接的进行中提示。
it('WorkspaceRequest_FailedLaunch_001', async () => {
  const wrapper = render(); await flushPromises()
  const shell = useShellStore()
  shell.requestWorkspaceAction({ kind: 'create-session', input: {
    projectKey: '/repo', projectPath: '/repo', cli: 'codex', launchConfigId: 'missing-profile',
  } }); await flushPromises()
  expect(useUnifiedSessionsStore().activeSession?.safeErrorCode).toBe('NEW_SESSION_PREPARATION_FAILED')
  expect(shell.pendingRequest).toBeNull()
  expect(wrapper.text()).not.toContain(en.workspaceActionPending)
  expect(wrapper.text()).toContain(en.newSessionPreparationFailed)
  expect(useNativeTabsStore().tabs.size).toBe(0)
})

// 旧请求失败时只能清理自己，不能清理仍在执行的新请求。
it('WorkspaceRequest_PreservesNewer_002', async () => {
  const wrapper = render(); await flushPromises(); const tab = await running()
  let rejectFirst!: (reason: unknown) => void; let finishSecond!: () => void
  io.write.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectFirst = reject }))
    .mockReturnValueOnce(new Promise<void>(resolve => { finishSecond = resolve }))
  const shell = useShellStore()
  shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'copy-session-id' }); await flushPromises()
  shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'copy-session-id' }); await flushPromises()
  const sequence = shell.requestSequence
  rejectFirst(new Error('RESOURCE_UNAVAILABLE')); await flushPromises()
  expect(shell.requestSequence).toBe(sequence)
  expect(shell.pendingRequest).toMatchObject({ action: 'copy-session-id' })
  expect(wrapper.text()).toContain(en.workspaceActionPending)
  expect(useUnifiedSessionsStore().actionFeedback).toBeNull()
  finishSecond(); await flushPromises()
  expect(shell.pendingRequest).toBeNull()
})

// 失败清除进行中状态后仍保留显式重试，不能自动重放操作。
it('WorkspaceRequest_ExplicitRetry_003', async () => {
  const wrapper = render(); await flushPromises(); const tab = await running()
  io.write.mockRejectedValueOnce(new Error('RESOURCE_UNAVAILABLE')).mockResolvedValueOnce(undefined)
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'copy-session-id' }); await flushPromises()
  expect(useShellStore().pendingRequest).toBeNull()
  expect(wrapper.text()).not.toContain(en.workspaceActionPending)
  expect(io.write).toHaveBeenCalledOnce()
  ;(document.querySelector('[data-action-feedback] button') as HTMLButtonElement).click(); await flushPromises()
  expect(io.write).toHaveBeenCalledTimes(2)
  expect(useShellStore().pendingRequest).toBeNull()
  expect(document.body.textContent).toContain(en.feedbackCopied)
})
// 运行态关闭先确认，确认前不停止，确认后只关闭原尝试。
it('Feedback_ConfirmsRunningClose_001', async () => {
  render(); await flushPromises(); const tab = await running()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'close' }); await flushPromises()
  expect(document.querySelector('[data-session-confirm]')).not.toBeNull(); expect(io.stop).not.toHaveBeenCalled()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.stop).toHaveBeenCalledTimes(1); expect(useNativeTabsStore().tab(tab.tabId)).toBeUndefined()
})
// 树的停止并归档意图使用同一确认流，确认后保存索引而不删除历史。
it('Feedback_ConfirmsStopArchive_002', async () => {
  render(); await flushPromises(); const tab = await running()
  useShellStore().requestWorkspaceAction({ kind: 'confirmation', request: { kind: 'stop-and-archive', sessionId: 'native-tab:' + tab.tabId, projectKey: '/repo', projectPath: '/repo' } }); await flushPromises()
  expect(document.querySelector('[data-session-confirm]')).not.toBeNull(); expect(io.archive).not.toHaveBeenCalled()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.archive).toHaveBeenCalledTimes(1); expect(useNativeTabsStore().tab(tab.tabId)).toBeUndefined()
})
// 未知状态重启确认只授权检查与停止，检查仍未知时不得生成新尝试。
it('Feedback_UnknownRestartFailsClosed_003', async () => {
  render(); await flushPromises(); const tab = await running('unknown')
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'restart' }); await flushPromises()
  expect(document.querySelector('[data-session-confirm]')).not.toBeNull()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.recover).toHaveBeenCalledTimes(1); expect(io.stop).not.toHaveBeenCalled()
  expect(useNativeTabsStore().tab(tab.tabId)?.generation).toBe(1)
})
// 项目移除保持既有打开会话屏障，并使用共享的类型化项目确认组件。
it('Feedback_ProjectConfirmKeepsBarrier_004', async () => {
  render(); await flushPromises(); const management = useProjectManagementStore()
  management.beginRemove({ projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
  expect(document.querySelector('[data-project-confirm]')).not.toBeNull()
  await running()
  ;(document.querySelector('[data-confirm-project-remove]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.remove).not.toHaveBeenCalled(); expect(useNativeTabsStore().tabs.size).toBe(1)
})

// 失败消息和完成提示均归发起的选择/尝试所有，切换后不能出现在新上下文。
it('Feedback_SuppressesStaleCopyError_005', async () => {
  render(); await flushPromises(); const first = await running()
  let reject!: (error: unknown) => void
  io.write.mockReturnValue(new Promise((_resolve, fail) => { reject = fail }))
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + first.tabId, action: 'copy-session-id' }); await flushPromises()
  const native = useNativeTabsStore(); const current = native.tab(first.tabId)!; current.generation++
  reject({ code: 'REVISION_CONFLICT', message: '/private/SECRET' }); await flushPromises()
  expect(document.querySelector('[data-action-feedback]')).toBeNull()
})
// 确认过程中原尝试被替换时，旧停止结果不能关闭新尝试或显示旧错误。
it('Feedback_StaleConfirmCannotCloseNew_006', async () => {
  render(); await flushPromises(); const tab = await running()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'close' }); await flushPromises()
  let finish!: () => void; io.stop.mockReturnValue(new Promise<void>(resolve => { finish = resolve }))
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  useNativeTabsStore().tab(tab.tabId)!.generation++
  finish(); await flushPromises()
  expect(useNativeTabsStore().tab(tab.tabId)?.generation).toBe(2)
  expect(document.querySelector('[data-session-confirm]')).toBeNull()
})

// 单个 CLI 未登录只显示工具级横幅，不阻断另一 CLI 或提升为全页失败。
it('Feedback_OneCliFailureStaysPartial_007', async () => {
  render(); await flushPromises(); const codex = await running()
  const tabs = useNativeTabsStore(); const claude = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cc', profileRevision: '1', action: { kind: 'new' } }); tabs.markError(claude.tabId, 'AUTH_REQUIRED')
  await flushPromises()
  expect(document.querySelector('[data-cli-banner]')?.textContent).toContain('Claude Code')
  expect(document.querySelector('[data-workspace-fatal]')).toBeNull(); expect(tabs.tab(codex.tabId)?.status).toBe('running')
  const fixed = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cc', profileRevision: '1', action: { kind: 'new' } }); tabs.tab(fixed.tabId)!.status = 'running'; tabs.tab(fixed.tabId)!.lastActivityAt = Date.now() + 1
  await flushPromises(); expect(document.querySelector('[data-cli-banner]')).toBeNull()
})
// 合法未知恢复经确认后，先停止准确的旧尝试，再创建一次新 generation。
it('Feedback_UnknownRestartAfterRecovery_008', async () => {
  render(); await flushPromises(); const tab = await running('unknown')
  io.recover.mockImplementation(async () => { useNativeTabsStore().tab(tab.tabId)!.status = 'running' })
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'restart' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.recover).toHaveBeenCalledTimes(1); expect(io.stop).toHaveBeenCalledTimes(1); expect(useNativeTabsStore().tab(tab.tabId)?.generation).toBe(2)
})
// 延迟复制成功后换选会话，不得给新上下文附上旧成功 toast。
it('Feedback_SuppressesStaleSuccess_009', async () => {
  const { useNotificationsStore } = await import('@/stores/notifications')
  render(); await flushPromises(); const first = await running(); let finish!: () => void
  io.write.mockReturnValue(new Promise<void>(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + first.tabId, action: 'copy-session-id' }); await flushPromises()
  const other = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); await useUnifiedSessionsStore().refresh(); await useUnifiedSessionsStore().activateSession('native-tab:' + other.tabId)
  finish(); await flushPromises(); expect(useNotificationsStore().toasts).toEqual([])
})
// 现有低风险复制完成后给出简短成功反馈，错误详情不暴露原始载荷。
it('Feedback_CopyShowsCompletionToast_010', async () => {
  render(); await flushPromises(); const tab = await running(); io.write.mockResolvedValue(undefined)
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'copy-session-id' }); await flushPromises()
  expect(document.body.textContent).toContain(en.feedbackCopied)
})

// 所有工作区来源均失败且无缓存/打开会话时，才显示全工作区恢复界面。
it('Feedback_TotalSourceFailureShowsPage_011', async () => {
  for (const read of [io.projects, io.registered, io.profiles, io.state]) read.mockRejectedValue(new Error('/private/SECRET'))
  render(); await flushPromises()
  expect(document.querySelector('[data-workspace-fatal]')).not.toBeNull()
  expect((document.querySelector('[data-unified-terminal-host]') as HTMLElement).style.display).toBe('none')
  expect(document.body.textContent).not.toContain('/private/SECRET')
})

// 选择另一个项目即失去原确认的上下文，弹窗不能保留旧动作。
it('Feedback_ProjectSelectionClosesConfirm_012', async () => {
  render(); await flushPromises(); const tab = await running()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'close' }); await flushPromises()
  useUnifiedSessionsStore().selectProjectContext('/other'); await flushPromises()
  expect(document.querySelector('[data-session-confirm]')).toBeNull(); expect(io.stop).not.toHaveBeenCalled()
})
// 导航取消确认后，已经发出的停止可以完成，但不能继续关闭或归档。
it('Feedback_NavigationCancelsRemainingWork_013', async () => {
  render(); await flushPromises(); const tab = await running(); let finish!: () => void
  io.stop.mockReturnValue(new Promise<void>(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'archive' }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  useShellStore().navigate('settings'); await flushPromises(); finish(); await flushPromises()
  expect(io.archive).not.toHaveBeenCalled(); expect(useNativeTabsStore().tab(tab.tabId)?.status).toBe('stopped')
  expect(document.querySelector('[data-session-confirm]')).toBeNull()
})
// 冲突读取最新状态后等待显式重试，成功前无 toast，失败正文不能泄露。
it('Feedback_ConflictNeedsExplicitRetry_014', async () => {
  render(); await flushPromises(); const row = useUnifiedSessionsStore().sessions.find(row => row.runtime === 'native-cli')!
  io.archive.mockRejectedValueOnce({ code: 'REVISION_CONFLICT', message: '/private/SECRET' })
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: row.id, action: 'archive' }); await flushPromises()
  expect(io.archive).toHaveBeenCalledTimes(1); expect(document.querySelector('[data-action-feedback]')?.textContent).toContain(en.errorRevisionConflict)
  expect(document.body.textContent).not.toContain('/private/SECRET')
  ;(document.querySelector('[data-action-feedback] button') as HTMLButtonElement).click(); await flushPromises()
  expect(io.archive).toHaveBeenCalledTimes(2); expect(document.body.textContent).toContain(en.feedbackArchived)
})
// 配置删除复用实际 App 确认组件与真实 store CAS；设置编辑入口由后续任务接入。
it('Feedback_ConfigConfirmationUsesRealStore_015', async () => {
  const { useCliProfilesStore } = await import('@/stores/cliProfiles')
  render(); await flushPromises(); useShellStore().navigate('settings'); await flushPromises()
  const profiles = useCliProfilesStore(); profiles.requestDelete('cx'); await flushPromises()
  expect(document.querySelector('[data-confirm-configuration-delete]')).not.toBeNull(); expect(io.patch).not.toHaveBeenCalled()
  io.patch.mockResolvedValue({ revision: '8', profiles: [] })
  ;(document.querySelector('[data-confirm-configuration-delete]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.patch).toHaveBeenCalledWith('7', { op: 'delete', id: 'cx' }); expect(profiles.profile('cx')).toBeUndefined()
  expect(document.body.textContent).toContain(en.feedbackConfigurationDeleted)
})

// 未完成的其他来源 bootstrap 不应阻塞本地已拥有会话的归档确认。
it('Feedback_PreReadyOwnedConfirmation_016', async () => {
  let release!: (value: unknown[]) => void
  io.projects.mockReturnValue(new Promise(resolve => { release = resolve }))
  render(); await flushPromises(); const tab = await running()
  try {
    useShellStore().requestWorkspaceAction({ kind: 'confirmation', request: { kind: 'stop-and-archive', sessionId: 'native-tab:' + tab.tabId, projectKey: '/repo', projectPath: '/repo' } }); await flushPromises()
    expect(document.querySelector('[data-session-confirm]')).not.toBeNull(); expect(io.stop).not.toHaveBeenCalled()
  } finally { release([]); await flushPromises() }
})

// 同一尝试编号下来源/配置身份改变，也会使原确认失效。
it.each(['profileRevision', 'sourceSessionKey', 'projectId', 'requestId'] as const)('Feedback_ConfirmationPinsSource_%s', async field => {
  render(); await flushPromises(); const tab = await running()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: 'native-tab:' + tab.tabId, action: 'close' }); await flushPromises()
  useNativeTabsStore().tab(tab.tabId)![field] = 'replacement'
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.stop).not.toHaveBeenCalled(); expect(useNativeTabsStore().tab(tab.tabId)).toBeDefined(); expect(document.querySelector('[data-session-confirm]')).toBeNull()
})

// 配置删除的准入屏障发生在启动前，必须保留可取消的准备失败行，不能制造未知启动。
it('Feedback_ConfigDeleteBlocksPreparation_017', async () => {
  const { useCliProfilesStore } = await import('@/stores/cliProfiles')
  render(); await flushPromises(); useShellStore().navigate('settings'); await flushPromises()
  const profiles = useCliProfilesStore(); profiles.requestDelete('cx')
  let finish!: (value: unknown) => void; io.patch.mockReturnValue(new Promise(resolve => { finish = resolve }))
  const deleting = profiles.confirmDelete(); await flushPromises()
  useShellStore().navigate('workspace'); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  try {
    await expect(catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'cx', launchConfigRevision: '7' })).rejects.toThrow('NEW_SESSION_PREPARATION_FAILED')
    expect(catalog.sessions.find(row => catalog.isPreparingSession(row.id))?.processState).toBe('failed')
    expect(useNativeTabsStore().tabs.size).toBe(0)
  } finally { finish({ revision: '8', profiles: [] }); await deleting }
})


// 停止已完成但归档排队时，取消或替换会话来源仍须阻止尚未发出的索引写入。
it.each(['cancel', 'source', 'attempt'] as const)('Feedback_ArchiveQueueGuard_018_%s', async change => {
  const { useProjectsStateStore } = await import('@/stores/projectsState')
  render(); await flushPromises(); const tab = await running()
  const catalog = useUnifiedSessionsStore(); const state = useProjectsStateStore()
  let release!: (value: unknown) => void
  io.pin.mockReturnValue(new Promise(resolve => { release = resolve }))
  const pinning = state.pinProject('/other'); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'confirmation', request: { kind: 'stop-and-archive', sessionId: 'native-tab:' + tab.tabId, projectKey: '/repo', projectPath: '/repo' } }); await flushPromises()
  ;(document.querySelector('[data-session-confirm]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.stop).toHaveBeenCalledOnce(); expect(io.archive).not.toHaveBeenCalled()
  if (change === 'cancel') {
    const cancel = [...document.querySelectorAll('button')].find(button => button.textContent?.trim() === en.cancel)!
    cancel.click(); await flushPromises()
  } else if (change === 'source') useNativeTabsStore().tab(tab.tabId)!.sourceSessionKey = 'replacement'
  else useNativeTabsStore().tab(tab.tabId)!.generation++
  const reads = io.state.mock.calls.length
  release({ pinnedProjects: ['/other'], archivedSessions: {} }); await pinning; await flushPromises()
  expect(io.archive, 'invalidated queued archive must not write metadata').not.toHaveBeenCalled()
  expect(state.archivedSessions.size).toBe(0); expect(state.pinnedProjects).toEqual(['/other'])
  expect(useNativeTabsStore().tab(tab.tabId)).toBeDefined()
  expect(catalog.confirmationError).toBeNull()
  expect(io.state).toHaveBeenCalledTimes(reads); expect(state.lastErrorCode).toBeNull(); expect(state.error).toBe(false)
})


// 真正设置页的删除菜单连接 App 确认，已承认的运行与资源 run authority 均保留。
it('Feedback_SettingsDeleteKeepsRun_019', async () => {
  const { useSidebarStore } = await import('@/stores/sidebar')
  const { useProjectResourcesStore } = await import('@/stores/projectResources')
  const wrapper = render(true); await flushPromises(); const tab = await running()
  const native = useNativeTabsStore()
  native.applyLaunchStatus(tab.tabId, { instanceId: 'instance', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
  const frozen = JSON.stringify(native.tab(tab.tabId)); const resources = useProjectResourcesStore()
  const target = { kind: 'run', runId: tab.runId, generation: tab.generation }
  expect(resources.context).toMatchObject({ target })
  useSidebarStore().activeSettingsSection = 'launch-configurations'; useShellStore().navigate('settings'); await flushPromises()
  await vi.waitFor(() => expect(wrapper.find('[data-launch-row="cx"] [data-launch-menu]').exists()).toBe(true))
  await wrapper.get('[data-launch-row="cx"] [data-launch-menu]').trigger('click'); await flushPromises()
  ;(document.querySelector('[data-item-id="delete"]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.patch).not.toHaveBeenCalled(); expect(document.querySelector('[data-confirm-configuration-delete]')).not.toBeNull()
  io.patch.mockResolvedValue({ revision: '8', profiles: [] })
  ;(document.querySelector('[data-confirm-configuration-delete]') as HTMLButtonElement).click(); await flushPromises()
  expect(io.patch).toHaveBeenCalledOnce(); expect(io.stop).not.toHaveBeenCalled(); expect(io.recover).not.toHaveBeenCalled()
  expect(JSON.stringify(native.tab(tab.tabId))).toBe(frozen); expect(resources.context).toMatchObject({ target })
  expect(document.querySelector('[data-live-terminal]')).not.toBeNull()
})

// 不同预检失败不能都提示用户重新选择程序；仅保留固定错误码。
it.each(['PROGRAM_UNAVAILABLE', 'RUNNER_UNAVAILABLE', 'ENV_SOURCE_MISSING', 'INVALID_REQUEST'])('LaunchPreparation_PreservesActualFailure_002: %s', async code => {
  io.availability.mockImplementation(async (profileId, profileRevision) => ({ profileId, profileRevision, cli: 'codex', state: 'unavailable', hostStatus: 'available', certified: false,
    issue: { code, retryable: false, field: '/private/SECRET', message: 'PRIVATE_TOKEN' } }))
  render(); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'create-session', input: { cli: 'codex', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'cx', launchConfigRevision: '7' } })
  await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  const notice = document.querySelector('[data-launch-preparation]')
  expect(notice?.textContent).not.toContain('Choose the CLI program')
  expect(notice?.textContent).toContain(code)
  expect(notice?.textContent).not.toMatch(/PRIVATE_TOKEN|private\/SECRET/)
  expect(useUnifiedSessionsStore().activeSession).toMatchObject({ preparationIssueCode: code, launchConfigId: 'cx' })
})

// 未选择可信程序时，真实新建入口停在可关闭的准备行，并直达同一配置编辑器。
it('LaunchPreparation_ConfigurationActionDoesNotAdmit_001', async () => {
  io.availability.mockImplementation(async (profileId, profileRevision) => ({ profileId, profileRevision, cli: 'codex', state: 'configuration-required', hostStatus: 'available', certified: false, issue: { code: 'PROGRAM_TRUST_REQUIRED', retryable: false } }))
  const unconfigured = await io.profiles(); unconfigured.profiles[0].programPath = { mode: 'inherit' }; io.profiles.mockResolvedValue(unconfigured)
  render(); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'create-session', input: { cli: 'codex', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'cx', launchConfigRevision: '7' } })
  await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(useUnifiedSessionsStore().activeSession).toMatchObject({ processState: 'failed', safeErrorCode: 'LAUNCH_CONFIGURATION_REQUIRED', launchConfigId: 'cx' })
  const notice = document.querySelector('[data-launch-preparation]')
  expect(notice?.textContent).toContain('Choose the CLI program')
  ;(notice!.querySelector('button') as HTMLButtonElement).click(); await flushPromises()
  expect(document.querySelector<HTMLInputElement>('[data-launch-name]')?.value).toBe('Work')
  expect(document.querySelector('[data-launch-save]')).not.toBeNull()
  expect(io.patch).not.toHaveBeenCalled()
  ;(document.querySelector('[data-launch-cancel]') as HTMLButtonElement).click(); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  const { useCliProfilesStore } = await import('@/stores/cliProfiles')
  const profiles = useCliProfilesStore()
  profiles.profiles[0].name = 'Updated work'; profiles.profiles[0].revision = '8'; profiles.revision = '8'
  ;(document.querySelector('[data-launch-preparation] button') as HTMLButtonElement).click(); await flushPromises()
  expect(document.querySelector<HTMLInputElement>('[data-launch-name]')?.value).toBe('Updated work')
  const pathMode = document.querySelector<HTMLSelectElement>('.launch-editor select')!
  pathMode.value = 'set'; pathMode.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises()
  const program = document.querySelector<HTMLInputElement>('[data-launch-program]')!
  program.value = '/tools/codex'; program.dispatchEvent(new Event('input', { bubbles: true })); await flushPromises()
  io.patch.mockImplementation(async (expected, patch) => {
    expect(expected).toBe('8')
    return { revision: '9', profiles: [{ ...profiles.profiles[0], ...patch.changes, revision: '9' }] }
  })
  ;(document.querySelector('[data-launch-save]') as HTMLButtonElement).click(); await flushPromises()
  expect(profiles.profile('cx')?.programPath).toEqual({ mode: 'set', value: '/tools/codex' })
  expect(document.querySelector('[data-launch-save]')).toBeNull()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  io.availability.mockImplementation(async (profileId, profileRevision) => ({ profileId, profileRevision, cli: 'codex', state: 'available-unverified', hostStatus: 'available', certified: false }))
  useShellStore().requestWorkspaceAction({ kind: 'primary-action', sessionId: useUnifiedSessionsStore().activeSessionId!, action: 'retry' })
  await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(1)
  expect([...useNativeTabsStore().tabs.values()][0]).toMatchObject({ profileId: 'cx', profileRevision: '9' })
  await useUnifiedSessionsStore().closeSession(useUnifiedSessionsStore().activeSessionId!)
  expect(document.querySelector('[data-live-terminal]')).toBeNull()
})
