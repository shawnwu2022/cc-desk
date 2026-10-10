import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import App from '@/App.vue'
import { useSessionStore } from '@/stores/session'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useShellStore } from '@/stores/shell'
import en from '@/i18n/locales/en'

const io = vi.hoisted(() => ({ terms: [] as any[], spawn: vi.fn(), kill: vi.fn(), history: vi.fn(), output: null as any, exit: null as any }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {
  options: any; textarea!: HTMLTextAreaElement; element!: HTMLElement; cols = 80; rows = 24; output = ''; modes = { bracketedPasteMode: false }; unicode = { activeVersion: '6' }; buffer = { active: { length: 0 } }
  _core = { coreService: { onUserInput: () => ({ dispose() {} }) } }
  focus = vi.fn(); refresh = vi.fn(); dispose = vi.fn()
  constructor(options: any) { this.options = options; io.terms.push(this) }
  open(el: HTMLElement) { this.element = el; this.textarea = document.createElement('textarea'); el.append(this.textarea) }
  loadAddon() {} onData() { return { dispose() {} } } onResize() {} attachCustomKeyEventHandler() {} getSelection() { return '' } write(data: string) { this.output += data }
} }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit = vi.fn() } }))
vi.mock('@xterm/addon-unicode11', () => ({ Unicode11Addon: class {} }))
vi.mock('@tauri-apps/api/window', () => ({ UserAttentionType: { Informational: 2 }, getCurrentWindow: () => ({
  onResized: async () => () => {}, isMaximized: async () => false, isMinimized: async () => false, isFocused: async () => true,
  onFocusChanged: async () => () => {}, requestUserAttention: async () => {}, setTitle: async () => {},
}) }))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }))
vi.mock('@/api/cli', () => ({ cliListProfiles: async () => ({ revision: '0', profiles: [] }) }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: async () => ({ revision: '0', projects: [] }) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  ptySpawn: io.spawn, ptyKill: io.kill, ptyResize: async () => {}, logMessage: vi.fn(),
  getAppConfig: async () => ({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' }),
  getProjectsState: async () => ({ pinnedProjects: [], archivedSessions: {} }),
  getProjects: async () => [{ path: '/legacy', name: 'Legacy' }], getSessions: io.history,
  onPtyOutput: async (fn: any) => { io.output = fn; return () => {} },
  onPtyExit: async (fn: any) => { io.exit = fn; return () => {} },
  onHookEvent: async () => () => {},
} ))
let wrapper: VueWrapper | null = null
beforeEach(() => {
  setActivePinia(createPinia()); localStorage.clear(); vi.clearAllMocks(); clearMocks(); io.terms.length = 0
  mockIPC(() => undefined)
  let identity = 0
  vi.stubGlobal('crypto', { randomUUID: () => `legacy-${++identity}` })
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { fn(0); return 1 })
  io.history.mockResolvedValue([{ sessionId: 'saved-session', name: 'Saved conversation', projectPath: '/legacy', lastActiveAt: 1 }])
  io.spawn.mockImplementation(async ({ id }: any) => ({ id })); io.kill.mockResolvedValue(undefined)
})
afterEach(() => { wrapper?.unmount(); wrapper = null; document.body.innerHTML = ''; vi.unstubAllGlobals(); clearMocks() })

describe('Legacy session resume in App', () => {
  // 停止保留回看内容，显式恢复通过真实宿主重新启动原会话，而点击行只选择。
  it('LegacyApp_ResumeEndedOpen_001', async () => {
    wrapper = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } })
    await flushPromises()
    const catalog = useUnifiedSessionsStore(), legacy = useSessionStore(), shell = useShellStore()
    const history = catalog.sessions.find(row => row.nativeSessionId === 'saved-session')!
    const opened = await catalog.resumeCatalogSession(history.id); await flushPromises()
    const first = legacy.tabs.get(opened.adapterSessionId)!, pty = first.ptyId!
    io.output({ id: pty, data: 'retained conversation' })
    shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: opened.id, action: 'stop' }); await flushPromises()
    expect(legacy.tabs.get(opened.adapterSessionId)).toMatchObject({ status: 'stopped', sessionId: 'saved-session', ptyId: null })
    expect(io.terms[0].output).toBe('retained conversation'); expect(io.terms[0].dispose).not.toHaveBeenCalled()
    shell.requestWorkspaceAction({ kind: 'activate', sessionId: opened.id }); await flushPromises()
    expect(io.spawn).toHaveBeenCalledOnce()
    shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: opened.id, action: 'resume' }); await flushPromises()
    expect(io.spawn, 'Resume must start a second PTY for the original session').toHaveBeenCalledTimes(2)
    expect(io.spawn.mock.calls[1][0]).toMatchObject({ cwd: '/legacy', args: ['--resume', 'saved-session'] })
    expect(legacy.tabs.get(opened.adapterSessionId)).toMatchObject({ status: 'running', sessionId: 'saved-session', ptyGeneration: 2 })
    expect(catalog.activeSessionId).toBe(opened.id); expect(legacy.tabs.size).toBe(1)
    expect(io.terms[0].dispose).toHaveBeenCalledOnce()
  })

  // 显式关闭删除终端但保留历史，随后恢复仍使用原生会话ID和项目。
  it('LegacyApp_CloseHistoryResume_002', async () => {
    wrapper = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } })
    await flushPromises()
    const catalog = useUnifiedSessionsStore(), legacy = useSessionStore(), shell = useShellStore()
    const history = catalog.sessions.find(row => row.nativeSessionId === 'saved-session')!
    const opened = await catalog.resumeCatalogSession(history.id); await flushPromises()
    shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: opened.id, action: 'close' }); await flushPromises()
    expect(catalog.sessionConfirmation).toBeNull()
    expect(io.kill).toHaveBeenCalledOnce()
    expect(legacy.tabs.size).toBe(0); expect(io.terms[0].dispose).toHaveBeenCalledOnce()
    const retained = catalog.sessions.find(row => row.nativeSessionId === 'saved-session')!
    expect(retained).toMatchObject({ id: history.id, runtime: 'legacy-claude', resumable: true })
    const restored = await catalog.resumeCatalogSession(retained.id); await flushPromises()
    expect(restored.id).not.toBe(opened.id)
    expect(legacy.tabs.get(restored.adapterSessionId)).toMatchObject({ status: 'running', sessionId: 'saved-session', projectPath: '/legacy' })
    expect(io.spawn.mock.calls[1][0]).toMatchObject({ cwd: '/legacy', args: ['--resume', 'saved-session'] })
  })

  // 恢复的启动回执延迟时，重复恢复不能重复启动，完成不能抢回较新的会话选择。
  it('LegacyApp_ResumeKeepsSelection_003', async () => {
    wrapper = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } })
    await flushPromises()
    const catalog = useUnifiedSessionsStore(), legacy = useSessionStore(), shell = useShellStore()
    const history = catalog.sessions.find(row => row.nativeSessionId === 'saved-session')!
    const opened = await catalog.resumeCatalogSession(history.id); await flushPromises()
    io.exit({ id: legacy.tabs.get(opened.adapterSessionId)!.ptyId }); await flushPromises()
    let complete!: () => void
    io.spawn.mockImplementationOnce(({ id }: any) => new Promise(resolve => { complete = () => resolve({ id }) }))
    shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: opened.id, action: 'resume' }); await flushPromises()
    expect(io.spawn).toHaveBeenCalledTimes(2)
    shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: opened.id, action: 'resume' }); await flushPromises()
    const otherId = legacy.createTab('/legacy', { sessionId: 'another-session', name: 'Another conversation' }); await flushPromises()
    await catalog.activateSession(`legacy-tab:${otherId}`)
    complete(); await flushPromises()
    expect(io.spawn).toHaveBeenCalledTimes(2)
    expect(catalog.activeSessionId).toBe(`legacy-tab:${otherId}`); expect(legacy.activeTabId).toBe(otherId)
    expect(legacy.tabs.get(opened.adapterSessionId)).toMatchObject({ sessionId: 'saved-session', ptyGeneration: 2, status: 'running' })
  })

  // 历史选择器的明确恢复也必须重启仍打开的已结束会话，保留原生ID和当前终端所有权。
  it('LegacyApp_ChooserResumesEnded_004', async () => {
    wrapper = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } })
    await flushPromises()
    const catalog = useUnifiedSessionsStore(), legacy = useSessionStore()
    const history = catalog.sessions.find(row => row.nativeSessionId === 'saved-session')!
    const opened = await catalog.resumeCatalogSession(history.id); await flushPromises()
    io.exit({ id: legacy.tabs.get(opened.adapterSessionId)!.ptyId }); await flushPromises()
    catalog.openResumeDialog({ project: { projectKey: '/legacy', projectPath: '/legacy' }, mode: 'history', cli: 'claude', sessionId: opened.id })
    await flushPromises()
    const confirm = document.querySelector<HTMLButtonElement>('[data-confirm-resume]')!
    expect(confirm).not.toBeNull(); confirm.click(); await flushPromises()
    expect(io.spawn, 'History chooser Resume must restart the ended open session').toHaveBeenCalledTimes(2)
    expect(io.spawn.mock.calls[1][0]).toMatchObject({ cwd: '/legacy', args: ['--resume', 'saved-session'] })
    expect(legacy.tabs.get(opened.adapterSessionId)).toMatchObject({ status: 'running', sessionId: 'saved-session', ptyGeneration: 2 })
    expect(catalog.resumeDialog).toBeNull(); expect(legacy.tabs.size).toBe(1)
  })

  // 恢复读取当前所有权期间的新选择撤销旧恢复，不能启动后台旧会话或抢回选择。
  it('LegacyApp_ResumeLosesSelection_005', async () => {
    wrapper = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } })
    await flushPromises()
    const catalog = useUnifiedSessionsStore(), legacy = useSessionStore()
    const history = catalog.sessions.find(row => row.nativeSessionId === 'saved-session')!
    const opened = await catalog.resumeCatalogSession(history.id); await flushPromises()
    io.exit({ id: legacy.tabs.get(opened.adapterSessionId)!.ptyId }); await flushPromises()
    const otherId = legacy.createTab('/legacy', { sessionId: 'another-session', name: 'Another conversation' }); await flushPromises()
    const resuming = catalog.resumeCatalogSession(opened.id)
    const rejected = expect(resuming).rejects.toThrow('STALE_SESSION_ATTEMPT')
    await catalog.activateSession(`legacy-tab:${otherId}`); await rejected; await flushPromises()
    expect(io.spawn).toHaveBeenCalledOnce()
    expect(catalog.activeSessionId).toBe(`legacy-tab:${otherId}`); expect(legacy.activeTabId).toBe(otherId)
    expect(legacy.tabs.get(opened.adapterSessionId)?.status).toBe('stopped')
  })
})
