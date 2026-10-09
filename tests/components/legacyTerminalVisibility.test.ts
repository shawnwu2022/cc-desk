import { beforeEach, afterEach, describe, it, expect, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import XTermTerminal from '@/components/XTermTerminal.vue'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { sendTerminalCommand } from '@/composables/useTerminalCommand'
import { platform } from '@/utils/platform'
const io = vi.hoisted(() => ({ terms: [] as any[], fits: [] as any[], input: vi.fn(), kill: vi.fn(), spawn: vi.fn(), output: null as any, exit: null as any, outputReady: vi.fn(), exitReady: vi.fn(), dragReady: vi.fn(), copy: vi.fn(), clip: vi.fn() }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {
  options: any; textarea!: HTMLTextAreaElement; element!: HTMLElement; cols = 80; rows = 24; output = ''; modes = { bracketedPasteMode: false }; unicode = { activeVersion: '6' }; buffer = { active: { length: 0 } }
  userSignal: (() => void) | null = null
  _core = { coreService: { onUserInput: (fn: () => void) => { this.userSignal = fn; return { dispose() {} } } } }
  userData(data: string) { this.userSignal?.(); this.data(data) }
  focus = vi.fn(); refresh = vi.fn(); dispose = vi.fn(); selection = 'old selection'; data: any; key: any
  constructor(options: any) { this.options = options; io.terms.push(this) }
  open(el: HTMLElement) { this.element = el; this.textarea = document.createElement('textarea'); el.append(this.textarea) }
  loadAddon() {} onData(fn: any) { if (!this.data) this.data = fn; return { dispose() {} } } onResize() {} attachCustomKeyEventHandler(fn: any) { this.key = fn } getSelection() { return this.selection } write(data: string) { this.output += data }
} }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit = vi.fn(); constructor() { io.fits.push(this) } } }))
vi.mock('@xterm/addon-unicode11', () => ({ Unicode11Addon: class {} }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readText: io.clip, readImage: vi.fn(), writeText: io.copy }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMinimized: async () => false }) }))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: io.dragReady }) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), ptySpawn: io.spawn, ptyInput: io.input, ptyKill: io.kill, ptyResize: vi.fn().mockResolvedValue(undefined), logMessage: vi.fn(), getSessions: vi.fn().mockResolvedValue([]), onPtyOutput: async (fn: any) => { io.output = fn; return io.outputReady() }, onPtyExit: async (fn: any) => { io.exit = fn; return io.exitReady() } }))
let wrapper: VueWrapper | null = null
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); io.terms.length = 0; io.fits.length = 0
  let id = 0; vi.stubGlobal('crypto', { randomUUID: () => `pty-${++id}` }); vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} }); vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { fn(0); return 1 })
  io.outputReady.mockResolvedValue(() => {}); io.exitReady.mockResolvedValue(() => {}); io.dragReady.mockResolvedValue(() => {})
  io.copy.mockResolvedValue(undefined); io.spawn.mockImplementation(async ({ id }: any) => ({ id })); io.kill.mockResolvedValue(undefined); io.input.mockResolvedValue(undefined)
})
afterEach(() => { wrapper?.unmount(); wrapper = null; vi.unstubAllGlobals() })
describe('Legacy unified ownership', () => {
  it('Legacy_PasteKeyboardUsesDomMimeWithoutExtraPermissions_014', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: true } }); await flushPromises(); await (wrapper.vm as any).startTab(id); await flushPromises()
    const chord = new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, cancelable: true })
    expect(io.terms[0].key(chord)).toBe(true)
    expect(chord.defaultPrevented).toBe(false)
    const event = new Event('paste', { bubbles: true, cancelable: true })
    Object.defineProperty(event, 'clipboardData', { value: { getData: () => '', types: ['Files'], items: [{ kind: 'file', type: 'image/png' }] } })
    io.terms[0].textarea.dispatchEvent(event); await flushPromises()
    expect(io.input).toHaveBeenCalledExactlyOnceWith(sessions.tabs.get(id)!.ptyId, platform === 'windows' ? '\x1bv' : '\x16', 'clipboard-dom')
    expect(io.clip).not.toHaveBeenCalled()
    // Ctrl+L stays an ordinary CLI input chord.
    expect(io.terms[0].key(new KeyboardEvent('keydown', { key: 'l', ctrlKey: true }))).toBe(true)
  })
  // 隐藏Legacy时，窗口复制和旧命令通道不能截获Native输入。
  it('Legacy_HiddenInputIsIsolated_001', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: true, fontSize: 12 } }); await flushPromises(); await (wrapper.vm as any).startTab(id); await flushPromises()
    await wrapper.setProps({ visible: false }); io.input.mockClear(); io.copy.mockClear()
    io.terms[0].userData('must not send'); expect(sendTerminalCommand('wrong runtime')).toBe(false)
    const copy = new Event('copy', { bubbles: true, cancelable: true }); window.dispatchEvent(copy)
    expect(io.input).not.toHaveBeenCalled(); expect(io.copy).not.toHaveBeenCalled(); expect(copy.defaultPrevented).toBe(false)
  })
  // DOM粘贴读取后失去可见性，微任务不得写入原先可见的Legacy PTY。
  it('Legacy_PasteLosesVisibility_002', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: true } }); await flushPromises(); await (wrapper.vm as any).startTab(id); await flushPromises()
    const event = new Event('paste', { bubbles: true, cancelable: true })
    Object.defineProperty(event, 'clipboardData', { value: { getData: () => 'old paste', types: ['text/plain'] } })
    io.terms[0].textarea.dispatchEvent(event)
    void wrapper.setProps({ visible: false }); await flushPromises()
    expect(io.input).not.toHaveBeenCalled()
  })
  // 字号变化不测量后台实例；退出后保留内容直到真正关闭会话。
  it('Legacy_RetainsEndedScrollback_003', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: true, fontSize: 12 } }); await flushPromises(); await (wrapper.vm as any).startTab(id); await flushPromises()
    const ptyId = sessions.tabs.get(id)!.ptyId!; await wrapper.setProps({ visible: false }); io.fits.forEach(f => f.fit.mockClear())
    io.output({ id: ptyId, data: 'hidden output' }); useAppStore().fontSize = 18; await flushPromises()
    expect(io.fits.every(f => f.fit.mock.calls.length === 0)).toBe(true)
    io.exit({ id: ptyId }); await flushPromises()
    expect(io.terms[0].output).toBe('hidden output'); expect(io.terms[0].dispose).not.toHaveBeenCalled()
    await wrapper.setProps({ visible: true }); expect(wrapper.find(`[data-tab="${id}"]`).exists()).toBe(true)
    sessions.removeTab(id); await flushPromises(); expect(io.terms[0].dispose).toHaveBeenCalled()
  })
  // 重启等待停止期间关闭tab，完成后不能启动孤儿PTY。
  it('Legacy_ClosedRestartCannotSpawn_004', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: true } }); await flushPromises(); await (wrapper.vm as any).startTab(id); await flushPromises()
    let finish!: () => void; io.kill.mockReturnValue(new Promise<void>(r => { finish = r }))
    const restarting = (wrapper.vm as any).restartTab(id)
    const rejected = expect(restarting).rejects.toThrow('STALE_LEGACY_ATTEMPT')
    sessions.removeTab(id); await flushPromises(); finish(); await rejected
    expect(io.spawn).toHaveBeenCalledTimes(1)
  })
  // 旧启动失败只能清理旧PTY，不能误杀同tab的新PTY。
  it('Legacy_OldStartCannotKillNew_005', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: true } }); await flushPromises()
    let fail!: (reason: Error) => void; io.spawn.mockReturnValue(new Promise((_resolve, reject) => { fail = reject }))
    const starting = (wrapper.vm as any).startTab(id); await flushPromises()
    const original = sessions.tabs.get(id)!.ptyId
    sessions.setTabPty(id, 'newer-pty')
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {})
    fail(new Error('old failure')); await starting; await flushPromises()
    expect(sessions.tabs.get(id)?.ptyId).toBe('newer-pty')
    expect(io.kill).not.toHaveBeenCalledWith('newer-pty')
    expect(io.kill).toHaveBeenCalledWith(original)
    quiet.mockRestore()
  })

  // Legacy切换两次后新可见终端恢复输入，旧终端始终禁用用户输入。
  it('Legacy_SwitchRestoresInput_006', async () => {
    const sessions = useSessionStore(); const first = sessions.createTab('/first')
    wrapper = mount(XTermTerminal, { props: { visible: true } }); await flushPromises(); await (wrapper.vm as any).startTab(first)
    const second = sessions.createTab('/second'); await (wrapper.vm as any).startTab(second); await flushPromises()
    await wrapper.setProps({ visible: false }); await wrapper.setProps({ visible: true }); await flushPromises()
    sessions.setActiveTab(first); await flushPromises()
    expect(io.terms[0].key(new KeyboardEvent('keydown', { key: 'x' }))).toBe(true)
    expect(io.terms[1].key(new KeyboardEvent('keydown', { key: 'x' }))).toBe(false)
    io.input.mockClear(); io.terms[1].userData('wrong'); io.terms[0].userData('right')
    expect(io.input).toHaveBeenCalledTimes(1); expect(io.input.mock.calls[0][1]).toBe('right')
  })

  // 隐藏Legacy不能阻断由xterm协议解析产生的应答，避免后台CLI等待DSR卡住。
  it('Legacy_HiddenProtocolRemainsLive_007', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: true } }); await flushPromises(); await (wrapper.vm as any).startTab(id); await flushPromises()
    await wrapper.setProps({ visible: false }); io.input.mockClear()
    io.terms[0].data('parser response')
    expect(io.input).toHaveBeenCalledWith(sessions.tabs.get(id)!.ptyId, 'parser response', 'terminal-ondata')
    io.input.mockClear(); io.terms[0].userData('hidden user')
    expect(io.input).not.toHaveBeenCalled()
  })

  // 首次启动等待核心输出/退出监听就绪，拖放监听延迟不能成为启动门禁。
  it('Legacy_CoreListenersBeforeSpawn_008', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    let outputReady!: (unlisten: () => void) => void; let exitReady!: (unlisten: () => void) => void
    io.outputReady.mockReturnValue(new Promise(r => { outputReady = r })); io.exitReady.mockReturnValue(new Promise(r => { exitReady = r }))
    io.dragReady.mockReturnValue(new Promise(() => {}))
    wrapper = mount(XTermTerminal, { props: { visible: true } })
    const starting = (wrapper.vm as any).startTab(id); await flushPromises()
    expect(io.spawn).not.toHaveBeenCalled()
    expect(io.outputReady).toHaveBeenCalledTimes(1); expect(io.exitReady).toHaveBeenCalledTimes(1)
    outputReady(() => {}); await flushPromises(); expect(io.spawn).not.toHaveBeenCalled()
    exitReady(() => {})
    io.spawn.mockImplementation(async ({ id }: any) => { io.output({ id, data: 'first byte' }); return { id } })
    expect(await starting).toEqual({ ok: true })
    expect(io.terms[0].output).toBe('first byte')
  })
  // 等待监听注册期间关闭目标后，旧启动不得生成孤儿进程。
  it('Legacy_ReadinessRechecksOwner_009', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    let outputReady!: (unlisten: () => void) => void
    io.outputReady.mockReturnValue(new Promise(r => { outputReady = r }))
    wrapper = mount(XTermTerminal, { props: { visible: true } })
    const starting = (wrapper.vm as any).startTab(id); await flushPromises()
    expect(io.spawn).not.toHaveBeenCalled()
    sessions.removeTab(id); outputReady(() => {})
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {})
    expect((await starting).ok).toBe(false); expect(io.spawn).not.toHaveBeenCalled(); quiet.mockRestore()
  })

})
