import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import XTermTerminal from '@/components/XTermTerminal.vue'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useSessionStore } from '@/stores/session'
import { useCliProfilesStore } from '@/stores/cliProfiles'

const io = vi.hoisted(() => ({
  terminals: [] as import('@xterm/xterm').Terminal[], channels: [] as any[],
  user: vi.fn(), protocol: vi.fn(), ack: vi.fn(), legacyInput: vi.fn(), output: null as any,
}))
// Keep the installed xterm 5.5 parser, CoreService, input(), onUserInput and onData.
// Only DOM open/focus/render geometry and addon lifecycle are omitted; jsdom
// cannot lay out a terminal. Parser/CoreService/input paths are not mocked.
vi.mock('@xterm/xterm', async original => {
  const canvas = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  const actual = await original<typeof import('@xterm/xterm')>()
  canvas.mockRestore()
  return { ...actual, Terminal: class extends actual.Terminal {
    constructor(options: any) { super(options); this.dispose = this.dispose.bind(this); io.terminals.push(this) }
    open(element: HTMLElement) {
      const core = (this as any)._core
      core.element = element
      core.textarea = document.createElement('textarea')
      element.append(core.textarea)
    }
    loadAddon(addon: import('@xterm/xterm').ITerminalAddon) { addon.activate(this) }
    focus() {}
    refresh() {}
  } }
})
vi.mock('@tauri-apps/api/core', async original => ({ ...await original<object>(), Channel: class { onmessage: any } }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { activate() {} dispose() {} fit() {} } }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readText: vi.fn(), readImage: vi.fn(), writeText: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMinimized: async () => false }) }))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  cliResize: vi.fn().mockResolvedValue(undefined), cliStop: vi.fn().mockResolvedValue(undefined),
  cliWriteInput: io.user, cliWriteProtocol: io.protocol, cliAckOutput: io.ack,
  ptyInput: io.legacyInput, ptyKill: vi.fn().mockResolvedValue(undefined), ptyResize: vi.fn().mockResolvedValue(undefined),
  ptySpawn: async ({ id }: any) => ({ id }), logMessage: vi.fn().mockResolvedValue(undefined),
  onPtyOutput: async (callback: any) => { io.output = callback; return () => {} }, onPtyExit: async () => () => {},
}))
let wrapper: VueWrapper | null = null
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); io.terminals.length = 0; io.channels.length = 0
  // Keep the real component -> launch entry -> attempt chain; stop at authenticated IPC.
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any, channel: any) {
      if (command === 'cli_start') {
        io.channels.push({ input, channel })
        return { instanceId: 'test-backend', requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, phase: 'running', revision: '1', failure: null }
      }
      if (command === 'cli_get_launch_status') {
        const original = io.channels.find(value => value.input.requestId === input.requestId).input
        return { instanceId: 'test-backend', requestId: input.requestId, run: { runId: original.runId, generation: original.generation }, phase: 'running', revision: '2', failure: null }
      }
      throw new Error('UNEXPECTED_BRIDGE_COMMAND')
    },
  } })
  let id = 0
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => `legacy-${++id}` })
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { fn(0); return 1 })
  io.user.mockImplementation(async input => ({ ...input, state: 'host-written', confirmedBytes: String(input.bytes.length) }))
  io.protocol.mockImplementation(async (_run, bytes) => ({ state: 'host-written', confirmedBytes: String(bytes.length) }))
  io.ack.mockResolvedValue(undefined); io.legacyInput.mockResolvedValue(undefined)
  useCliProfilesStore().profiles = [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }]
})
afterEach(() => { wrapper?.unmount(); wrapper = null; Reflect.deleteProperty(window, '__CC_DESK_DOCUMENT__'); vi.unstubAllGlobals(); document.body.innerHTML = ''; if (vi.isMockFunction(Date.now)) vi.mocked(Date.now).mockRestore() })

describe('Unified host with pinned real xterm parser', () => {
  // 真实组件、Pinia、启动入口和请求冻结共同运行；同一尝试再次start不得重发。
  it.each(['claude', 'codex'] as const)('Native_RealLaunchComposition_006: %s', async cli => {
    useCliProfilesStore().profiles = [{ ...useCliProfilesStore().profiles[0], id: `${cli}-main`, cli }]
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli, projectId: 'project', projectPath: '/repo', profileId: `${cli}-main`, profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
    expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'running', errorCode: null, generation: 1 })
    expect(io.channels).toHaveLength(1)
    expect(io.channels[0].input).toMatchObject({ cli, profileId: `${cli}-main`, expectedProfileRevision: '7', requestId: tab.requestId, runId: tab.runId, action: { kind: 'new' } })
    await (wrapper.vm as any).start(); await flushPromises()
    expect(io.channels).toHaveLength(1)
  })
  // 真实 Native host/parser 输入输出推进活动；轮询、隐藏输入与旧 owner 不推进。
  it('Native_MeaningfulActivity_005', async () => {
    const now = vi.spyOn(Date, 'now').mockReturnValue(1000)
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
    now.mockReturnValue(61000)
    await (wrapper.vm as any).recover(); await flushPromises()
    expect(tabs.tab(tab.tabId)!.lastActivityAt, 'unchanged poll must age').toBe(1000)
    now.mockReturnValue(62000)
    io.channels[0].channel.onmessage({ runId: tab.runId, generation: 1, streamEpoch: '1', offset: '0', bytes: [65] })
    await vi.waitFor(() => expect(io.ack).toHaveBeenCalled())
    expect(tabs.tab(tab.tabId)!.lastActivityAt).toBe(62000)
    // One hundred admitted output chunks in the same second do not publish one hundred catalog updates.
    for (let n = 1; n <= 100; n++) {
      now.mockReturnValue(62000 + n)
      io.channels[0].channel.onmessage({ runId: tab.runId, generation: 1, streamEpoch: '1', offset: String(n), bytes: [65] })
    }
    expect(tabs.tab(tab.tabId)!.lastActivityAt).toBe(62000)
    now.mockReturnValue(65000)
    io.terminals[0].input('real input', true)
    await vi.waitFor(() => expect(io.user).toHaveBeenCalledTimes(1))
    await flushPromises()
    expect(tabs.tab(tab.tabId)!.lastActivityAt).toBe(65000)
    await wrapper.setProps({ active: false }); now.mockReturnValue(70000)
    io.terminals[0].input('hidden input', true); await flushPromises()
    expect(tabs.tab(tab.tabId)!.lastActivityAt).toBe(65000)
    now.mockReturnValue(75000)
    tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
    now.mockReturnValue(80000)
    io.channels[0].channel.onmessage({ runId: tab.runId, generation: 1, streamEpoch: '1', offset: '101', bytes: [65] })
    expect(tabs.tab(tab.tabId)!.lastActivityAt).toBe(75000)
  })

  // 真实xterm解析DSR时隐藏Native仍输出协议应答和ACK，同时拒绝用户输入。
  it.each([true, false])('Native_HiddenParserReplies_001 %s', async initiallyVisible => {
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: initiallyVisible } }); await flushPromises()
    await wrapper.setProps({ active: false })
    io.terminals[0].input('hidden keyboard', true)
    io.channels[0].channel.onmessage({ runId: tab.runId, generation: 1, streamEpoch: '1', offset: '0', bytes: [27, 91, 54, 110] })
    await vi.waitFor(() => expect(io.ack).toHaveBeenCalled())
    await vi.waitFor(() => expect(io.protocol).toHaveBeenCalledTimes(1))
    expect(new TextDecoder().decode(io.protocol.mock.calls[0][1])).toBe('\x1b[1;1R')
    expect(io.protocol.mock.calls[0][0]).toEqual({ runId: tab.runId, generation: 1 })
    expect(io.user).not.toHaveBeenCalled(); expect(io.legacyInput).not.toHaveBeenCalled()
    await wrapper.setProps({ active: true })
    io.terminals[0].input('visible keyboard', true)
    await vi.waitFor(() => expect(io.user).toHaveBeenCalledTimes(1))
    expect(new TextDecoder().decode(io.user.mock.calls[0][0].bytes)).toBe('visible keyboard')
  })
  // 真实Legacy解析DSR不被disableStdin吞掉，隐藏用户信号不能进入Legacy PTY。
  it.each([true, false])('Legacy_HiddenParserReplies_002 %s', async initiallyVisible => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    wrapper = mount(XTermTerminal, { props: { visible: initiallyVisible } }); await flushPromises()
    await (wrapper.vm as any).startTab(id); await flushPromises()
    const ptyId = sessions.tabs.get(id)!.ptyId!
    await wrapper.setProps({ visible: false })
    io.terminals[0].input('hidden keyboard', true)
    io.output({ id: ptyId, data: '\x1b[6n' })
    await vi.waitFor(() => expect(io.legacyInput).toHaveBeenCalledTimes(1))
    expect(io.legacyInput).toHaveBeenCalledWith(ptyId, '\x1b[1;1R', 'terminal-ondata')
    expect(io.user).not.toHaveBeenCalled(); expect(io.protocol).not.toHaveBeenCalled()
    await wrapper.setProps({ visible: true })
    io.terminals[0].input('visible keyboard', true)
    expect(io.legacyInput).toHaveBeenCalledTimes(2)
    expect(io.legacyInput.mock.calls[1][1]).toBe('visible keyboard')
  })
})
