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
  user: vi.fn(), protocol: vi.fn(), ack: vi.fn(), stop: vi.fn(), legacyInput: vi.fn(), output: null as any,
  observation: null as ((payload: import('@/types/hook').HookEventPayload) => void) | null,
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
vi.mock('@/api/observer', () => ({ onNativeObservation: async (handler: typeof io.observation) => { io.observation = handler; return () => {} } }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { activate() {} dispose() {} fit() {} } }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readText: vi.fn(), readImage: vi.fn(), writeText: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMinimized: async () => false }) }))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  cliResize: vi.fn().mockResolvedValue(undefined), cliStop: io.stop,
  cliWriteInput: io.user, cliWriteProtocol: io.protocol, cliAckOutput: io.ack,
  ptyInput: io.legacyInput, ptyKill: vi.fn().mockResolvedValue(undefined), ptyResize: vi.fn().mockResolvedValue(undefined),
  ptySpawn: async ({ id }: any) => ({ id }), logMessage: vi.fn().mockResolvedValue(undefined),
  onPtyOutput: async (callback: any) => { io.output = callback; return () => {} }, onPtyExit: async () => () => {},
}))
let wrapper: VueWrapper | null = null
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); io.terminals.length = 0; io.channels.length = 0
  io.observation = null
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
  io.stop.mockResolvedValue(undefined); io.ack.mockResolvedValue(undefined); io.legacyInput.mockResolvedValue(undefined)
  useCliProfilesStore().profiles = [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }]
})
afterEach(() => { wrapper?.unmount(); wrapper = null; Reflect.deleteProperty(window, '__CC_DESK_DOCUMENT__'); vi.unstubAllGlobals(); vi.useRealTimers(); document.body.innerHTML = ''; if (vi.isMockFunction(Date.now)) vi.mocked(Date.now).mockRestore() })

describe('Unified host with pinned real xterm parser', () => {
  // 真实host→现有Native bus→mapper→store；通知不把无序activity变成completed。
  it('Native_ReceiptCompositionOwnsExactAttempt_018', async () => {
    useCliProfilesStore().profiles = [{ ...useCliProfilesStore().profiles[0], id: 'claude-main', cli: 'claude' }]
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'claude-main', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
    const payload = { ptyId: null, sessionId: 'provider', eventName: 'Stop', state: 'unknown', timestamp: 1,
      runId: tab.runId, generation: tab.generation, eventId: 'receipt-old', observerSource: 'claude-hook',
      detail: { type: 'stop', data: { lastAssistantMessage: 'SECRET' } } } as import('@/types/hook').HookEventPayload
    expect(io.observation).not.toBeNull()
    io.observation!(payload)
    expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'running', activityState: 'unknown', attentionState: 'none',
      observationNotice: { recent: { kind: 'reply-ended', eventId: 'receipt-old' }, unreadReplyEnd: { eventId: 'receipt-old' } } })
    expect(JSON.stringify(tabs.tab(tab.tabId)?.observationNotice)).not.toContain('SECRET')
    expect(tabs.applyLaunchStatus(tab.tabId, { instanceId: 'test-backend', requestId: tab.requestId,
      run: { runId: tab.runId, generation: tab.generation }, phase: 'exited', revision: '3', failure: null })).toBe(true)
    const next = tabs.restart(tab.tabId, { profileId: 'claude-main', profileRevision: '7' }); await flushPromises()
    // The real parser fence must settle before the new launch/subscription owns events.
    await vi.waitFor(() => {
      expect(io.channels).toHaveLength(2)
      expect(tabs.tab(next.tabId)?.status).toBe('running')
    })
    io.observation!(payload)
    expect(tabs.tab(next.tabId)?.observationNotice?.unreadReplyEnd ?? null).toBeNull()
    io.observation!({ ...payload, eventId: 'receipt-new', runId: next.runId, generation: next.generation })
    expect(tabs.tab(next.tabId)?.observationNotice?.unreadReplyEnd?.eventId).toBe('receipt-new')
    wrapper.unmount(); wrapper = null
    io.observation!({ ...payload, eventId: 'late-unmounted', runId: next.runId, generation: next.generation })
    expect(tabs.tab(next.tabId)?.observationNotice?.unreadReplyEnd?.eventId).toBe('receipt-new')
  })
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

// 真实组件/入口/attempt 组合保留安全拒绝码并凭取消回执结束未知启动。
it.each(['claude', 'codex'] as const)('Native_DeniedLaunchCancellation_007 %s', async cli => {
  const profiles = useCliProfilesStore()
  profiles.profiles = [{ ...profiles.profiles[0], id: `${cli}-main`, cli }]
  let original: any
  const calls: string[] = []
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      calls.push(command)
      if (command === 'cli_start') { original = input; throw { code: 'PROGRAM_TRUST_REQUIRED', retryable: false } }
      if (command === 'cli_cancel_launch') {
        expect(input).toEqual(original)
        return { instanceId: 'test-backend', requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, revision: '1', phase: 'cancelled', failure: null }
      }
      throw new Error('unexpected command')
    },
  } })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli, projectId: 'project', projectPath: '/repo', profileId: `${cli}-main`, profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'unknown', errorCode: 'PROGRAM_TRUST_REQUIRED', launchRevision: null })
  profiles.profiles = [] // Cleanup must not rebuild from the current selection.
  await expect((wrapper.vm as any).stop()).resolves.toBeUndefined()
  expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'failed', launchRevision: '1' })
  expect(calls).toEqual(['cli_start', 'cli_cancel_launch'])
  expect(io.stop).not.toHaveBeenCalled()
})

// 丢失开始回执的正在执行进程必须先检查准确状态，再停止并等待退出回执。
it('Native_LostReceiptStopsRealRun_008', async () => {
  vi.useFakeTimers()
  let original: any
  let reads = 0
  const calls: string[] = []
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      calls.push(command)
      if (command === 'cli_start') { original = input; throw new Error('transport lost') }
      const phase = command === 'cli_get_launch_status' && ++reads > 1 ? 'exited' : 'running'
      return { instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: phase === 'exited' ? '3' : '2', phase, failure: null }
    },
  } })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const stopped = (wrapper.vm as any).stop()
  const checked = expect(stopped).resolves.toBeUndefined()
  void checked.catch(() => undefined)
  await vi.advanceTimersByTimeAsync(300)
  await checked
  expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'exited', launchRevision: '3' })
  expect(io.stop).toHaveBeenCalledExactlyOnceWith({ runId: tab.runId, generation: tab.generation })
  expect(calls.filter(command => command === 'cli_start')).toHaveLength(1)
  vi.useRealTimers()
})

// 取消终态早于启动拒绝时，迟到错误不得把已取消会话重新标成未知。
it('Native_LateStartPreservesCancel_009', async () => {
  let rejectStart!: (error: unknown) => void
  let original: any
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') {
        original = input
        return new Promise((_resolve, reject) => { rejectStart = reject })
      }
      return { instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: '1', phase: 'cancelled', failure: null }
    },
  } })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  await (wrapper.vm as any).stop()
  rejectStart({ code: 'PROGRAM_TRUST_REQUIRED', retryable: false }); await flushPromises()
  expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'failed', launchRevision: '1', errorCode: 'LAUNCH_CANCELLED' })
})

// 取消回执早于旧状态检查失败时，检查失败不能撤销已取消的终态证明。
it('Native_LateStatusPreservesCancel_010', async () => {
  let rejectStatus!: (error: unknown) => void
  let original: any
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') { original = input; throw new Error('lost') }
      if (command === 'cli_get_launch_status') return new Promise((_resolve, reject) => { rejectStatus = reject })
      return { instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: '1', phase: 'cancelled', failure: null }
    },
  } })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const recovering = (wrapper.vm as any).recover(); await flushPromises()
  await (wrapper.vm as any).stop()
  rejectStatus(new Error('lost status')); await recovering
  expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'failed', launchRevision: '1', errorCode: 'LAUNCH_CANCELLED' })
})

// 延迟成功启动回执在停止期间到达时不能重新打开用户输入或后台轮询。
it('Native_LateStartKeepsInputPaused_011', async () => {
  vi.useFakeTimers()
  let resolveStart!: (receipt: unknown) => void
  let finishStop!: () => void
  let original: any
  let statusReads = 0
  const receipt = (phase: string, revision: string) => ({ instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision, phase, failure: null })
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') { original = input; return new Promise(resolve => { resolveStart = resolve }) }
      if (command === 'cli_cancel_launch') return receipt('running', '2')
      if (command === 'cli_get_launch_status') { statusReads++; return receipt('exited', '3') }
      throw new Error('unexpected command')
    },
  } })
  io.stop.mockImplementation(() => new Promise<void>(resolve => { finishStop = resolve }))
  const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const stopped = (wrapper.vm as any).stop(); await flushPromises()
  resolveStart(receipt('running', '2')); await flushPromises()
  io.terminals[0].input('must not send while stopping', true); await flushPromises()
  expect(io.user).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(1500)
  expect(statusReads).toBe(0)
  finishStop(); await vi.advanceTimersByTimeAsync(100); await stopped
  expect(useNativeTabsStore().tab(tab.tabId)?.status).toBe('exited')
})

// 没有文档桥接时本地工厂尚未提交启动，可以报告失败并关闭，不能变成不可取消的未知。
it('Native_LocalBridgeFailureIsUnsent_012', async () => {
  Reflect.deleteProperty(window, '__CC_DESK_DOCUMENT__')
  const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  expect(useNativeTabsStore().tab(tab.tabId)).toMatchObject({ status: 'failed', launchRevision: null, errorCode: 'DOCUMENT_BRIDGE_UNAVAILABLE' })
  expect(io.channels).toHaveLength(0)
})

// 本地选择配置已消失时没有提交启动，必须保留可关闭的确定失败状态。
it('Native_LocalProfileFailureIsUnsent_013', async () => {
  useCliProfilesStore().profiles = []
  const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  expect(useNativeTabsStore().tab(tab.tabId)).toMatchObject({ status: 'failed', launchRevision: null, errorCode: 'CLI_PROFILE_REQUIRED' })
  expect(io.channels).toHaveLength(0)
})

// spawn 已开始但监督器尚未接管时，RUN_NOT_FOUND 不代表结束，仍需停止后检查真实退出。
it('Native_StopWaitsForAdoption_014', async () => {
  vi.useFakeTimers()
  let original: any
  let reads = 0
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') { original = input; throw new Error('lost') }
      const phase = command === 'cli_cancel_launch' ? 'starting' : ++reads < 2 ? 'running' : 'exited'
      const revision = phase === 'starting' ? '1' : phase === 'running' ? '2' : '3'
      return { instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision, phase, failure: null }
    },
  } })
  io.stop.mockRejectedValueOnce({ code: 'RUN_NOT_FOUND', retryable: false }).mockResolvedValue(undefined)
  const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const stopped = (wrapper.vm as any).stop()
  await vi.advanceTimersByTimeAsync(300); await stopped
  expect(useNativeTabsStore().tab(tab.tabId)).toMatchObject({ status: 'exited', launchRevision: '3' })
  expect(io.stop).toHaveBeenCalledTimes(2)
  expect(io.stop.mock.calls.every(([run]) => run.runId === tab.runId && run.generation === tab.generation)).toBe(true)
})

// 取消传输失败不能关闭未知会话，也不能调用停止不存在的run或重新启动。
it('Native_CancelLossRetainsUnknown_015', async () => {
  const calls: string[] = []
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string) { calls.push(command); throw new Error('transport lost') },
  } })
  const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  vi.useFakeTimers()
  const stopping = expect((wrapper.vm as any).stop()).rejects.toThrow('NATIVE_STOP_UNCONFIRMED')
  await vi.advanceTimersByTimeAsync(5100); await stopping
  expect(useNativeTabsStore().tab(tab.tabId)).toMatchObject({ status: 'unknown', launchRevision: null })
  expect(calls.slice(0, 2)).toEqual(['cli_start', 'cli_cancel_launch'])
  expect(calls.slice(2).length).toBeGreaterThan(0)
  expect(calls.slice(2).every(command => command === 'cli_get_launch_status')).toBe(true)
  expect(io.stop).not.toHaveBeenCalled()
})

// 取消、停止或状态 IPC 永不返回时，关闭等待仍须在同一个期限结束并保留未结束会话。
it.each(['cancel', 'stop', 'status'] as const)('Native_StopBoundsPendingIpc_016 %s', async pendingStep => {
  vi.useFakeTimers()
  let original: any
  let ended = false
  let finishLate!: (value?: unknown) => void
  const pending = new Promise(resolve => { finishLate = resolve })
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') { original = input; throw new Error('lost') }
      if (ended) return { instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: '3', phase: pendingStep === 'cancel' ? 'cancelled' : 'exited', failure: null }
      if (command === 'cli_cancel_launch' && pendingStep === 'cancel'
        || command === 'cli_get_launch_status' && pendingStep === 'status') return pending
      return { instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: '2', phase: 'running', failure: null }
    },
  } })
  if (pendingStep === 'stop') io.stop.mockReturnValue(pending)
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  let outcome = 'pending'
  void (wrapper.vm as any).stop().then(() => { outcome = 'resolved' }, (error: Error) => { outcome = error.message })
  await vi.advanceTimersByTimeAsync(5100)
  expect(outcome).toBe('NATIVE_STOP_UNCONFIRMED')
  expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'unknown', errorCode: 'NATIVE_STOP_UNCONFIRMED' })
  const beforeLate = { ...tabs.tab(tab.tabId)! }
  finishLate(pendingStep === 'stop' ? undefined : { instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: '3', phase: pendingStep === 'cancel' ? 'cancelled' : 'exited', failure: null })
  await flushPromises()
  expect(tabs.tab(tab.tabId)).toEqual(beforeLate)
  io.terminals[0].input('paused after timeout', true); await flushPromises()
  expect(io.user).not.toHaveBeenCalled()
  ended = true
  await expect((wrapper.vm as any).stop()).resolves.toBeUndefined()
  expect(tabs.tab(tab.tabId)?.status).toBe(pendingStep === 'cancel' ? 'failed' : 'exited')
})

// 关闭超时后迟到的原始启动回执只进入 attempt，不得重新发布运行态或打开输入。
it('Native_StopTimeoutFencesLateStart_017', async () => {
  vi.useFakeTimers()
  let original: any
  let finishStart!: (value: unknown) => void
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') { original = input; return new Promise(resolve => { finishStart = resolve }) }
      return new Promise(() => {})
    },
  } })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const outcome = (wrapper.vm as any).stop().catch((error: Error) => error.message)
  await vi.advanceTimersByTimeAsync(5100)
  expect(await outcome).toBe('NATIVE_STOP_UNCONFIRMED')
  finishStart({ instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: '2', phase: 'running', failure: null })
  await flushPromises()
  expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'unknown', errorCode: 'NATIVE_STOP_UNCONFIRMED' })
  io.terminals[0].input('late start cannot unpause', true); await flushPromises()
  expect(io.user).not.toHaveBeenCalled()
})

// 停止前已在等待的检查回执晚于超时到达时，不能重新打开用户输入。
it('Native_StopTimeoutFencesOldStatus_018', async () => {
  vi.useFakeTimers()
  let original: any
  let finishStatus!: (value: unknown) => void
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') {
        original = input
        return { instanceId: 'test-backend', requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, revision: '2', phase: 'running', failure: null }
      }
      if (command === 'cli_get_launch_status') return new Promise(resolve => { finishStatus = resolve })
      return new Promise(() => {})
    },
  } })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const recovery = (wrapper.vm as any).recover(); await flushPromises()
  const outcome = (wrapper.vm as any).stop().catch((error: Error) => error.message)
  await vi.advanceTimersByTimeAsync(5100)
  expect(await outcome).toBe('NATIVE_STOP_UNCONFIRMED')
  finishStatus({ instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: '3', phase: 'running', failure: null })
  await recovery
  expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'unknown', errorCode: 'NATIVE_STOP_UNCONFIRMED' })
  io.terminals[0].input('late status cannot unpause', true); await flushPromises()
  expect(io.user).not.toHaveBeenCalled()
})

// 旧后台检查永不返回时，超时释放检查所有权，用户明确恢复后仍能启动新的周期检查。
it('Native_RecoverRestartsAfterTimeout_019', async () => {
  vi.useFakeTimers()
  let original: any
  let reads = 0
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') original = input
      else if (command === 'cli_cancel_launch' || command === 'cli_get_launch_status' && ++reads === 1) return new Promise(() => {})
      const phase = reads >= 3 ? 'exited' : 'running'
      return { instanceId: 'test-backend', requestId: original.requestId, run: { runId: original.runId, generation: original.generation }, revision: phase === 'running' ? '2' : '3', phase, failure: null }
    },
  } })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  await vi.advanceTimersByTimeAsync(1500)
  expect(reads).toBe(1)
  const stopped = (wrapper.vm as any).stop().catch((error: Error) => error.message)
  await vi.advanceTimersByTimeAsync(5100)
  expect(await stopped).toBe('NATIVE_STOP_UNCONFIRMED')
  await (wrapper.vm as any).recover()
  expect(tabs.tab(tab.tabId)?.status).toBe('running')
  await vi.advanceTimersByTimeAsync(1500)
  expect(reads).toBe(3)
  expect(tabs.tab(tab.tabId)?.status).toBe('exited')
})

// 第一条 writer 挂起时排队第二条；真实 parser 跨批往返后必须暂停，不能因终值相同继续发送。
it.each([
  ['applicationCursorKeysMode', '\x1b[?1h', '\x1b[?1l'],
  ['sendFocusMode', '\x1b[?1004h', '\x1b[?1004l'],
  ['bracketedPasteMode', '\x1b[?2004h', '\x1b[?2004l'],
] as const)('Native_ParsedModeFencesQueue_020 %s', async (mode, enable, disable) => {
  let releaseFirst!: () => void
  const blocked = new Promise<void>(resolve => { releaseFirst = resolve })
  io.user.mockImplementation(async input => {
    if (input.generation === 1 && input.inputSeq === '1') await blocked
    return { ...input, state: 'host-written', confirmedBytes: String(input.bytes.length) }
  })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const term = io.terminals[0]
  const originalRun = { runId: tab.runId, generation: tab.generation }
  term.input('first', true)
  await vi.waitFor(() => expect(io.user).toHaveBeenCalledTimes(1))
  term.input('second', true)
  let offset = 0
  for (const output of [enable, disable]) {
    const parsed = new Promise<void>(resolve => {
      const subscription = term.onWriteParsed(() => { subscription.dispose(); resolve() })
    })
    const bytes = [...new TextEncoder().encode(output)]
    io.channels[0].channel.onmessage({ ...originalRun, streamEpoch: '1', offset: String(offset), bytes })
    offset += bytes.length
    await parsed
  }
  expect(term.modes[mode]).toBe(false)
  await vi.waitFor(() => expect(io.ack).toHaveBeenCalledTimes(2))
  const activity = vi.spyOn(tabs, 'touch')
  const diagnostics = vi.spyOn(tabs, 'setDiagnostic')
  releaseFirst(); await flushPromises()
  expect(io.user).toHaveBeenCalledTimes(1)
  expect(tabs.tab(tab.tabId)?.errorCode).toBe('NATIVE_INPUT_PAUSED')
  expect(activity).not.toHaveBeenCalled()
  expect(diagnostics.mock.calls).toEqual([[tab.tabId, 'NATIVE_INPUT_PAUSED']])
  term.input('third', true); await flushPromises()
  expect(io.user).toHaveBeenCalledTimes(1)
  expect(diagnostics).toHaveBeenCalledTimes(1)

  // 新 generation 使用独立 tracker/queue；旧输出不能改变新 run 的 mode 身份。
  tabs.tab(tab.tabId)!.status = 'exited'
  tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
  expect(io.terminals).toHaveLength(1)
  expect(io.channels).toHaveLength(2)
  io.channels[0].channel.onmessage({ ...originalRun, streamEpoch: '1', offset: String(offset), bytes: [...new TextEncoder().encode(enable)] })
  term.input('new generation', true); await flushPromises()
  expect(io.user).toHaveBeenCalledTimes(2)
  expect(io.user.mock.calls[1][0]).toMatchObject({ generation: 2, inputSeq: '1', modeEpoch: '1' })
  expect(term.modes[mode]).toBe(false)
})

// 旧 writer 仍挂起时重启，新 generation 独立发送；旧完成不能放行排队输入或覆盖新诊断。
it('Native_PendingInputGeneration_021', async () => {
  let releaseFirst!: () => void
  const blocked = new Promise<void>(resolve => { releaseFirst = resolve })
  io.user.mockImplementation(async input => {
    if (input.generation === 1) await blocked
    return { ...input, state: 'host-written', confirmedBytes: String(input.bytes.length) }
  })
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const term = io.terminals[0]
  term.input('first', true)
  await vi.waitFor(() => expect(io.user).toHaveBeenCalledTimes(1))
  term.input('queued old input', true)
  tabs.tab(tab.tabId)!.status = 'exited'
  tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
  term.input('new generation', true); await flushPromises()
  expect(io.user.mock.calls.map(([input]) => [input.generation, input.inputSeq, input.modeEpoch])).toEqual([[1, '1', '1'], [2, '1', '1']])
  const diagnostics = vi.spyOn(tabs, 'setDiagnostic')
  const activity = vi.spyOn(tabs, 'touch')
  releaseFirst(); await flushPromises()
  expect(io.user).toHaveBeenCalledTimes(2)
  expect(diagnostics).not.toHaveBeenCalled()
  expect(activity).not.toHaveBeenCalled()
  expect(tabs.tab(tab.tabId)).toMatchObject({ generation: 2, status: 'running', errorCode: null })
})

// A new process must not inherit the previous VT screen, input modes or parser state.
it.each([
  { state: 'alternate-and-modes', bytes: [...new TextEncoder().encode('\x1b[?1049h\x1b[?2004h\x1b[?1hOLD-RUN')] },
  { state: 'unterminated-osc', bytes: [...new TextEncoder().encode('\x1b]0;OLD-TITLE')] },
  { state: 'unterminated-csi', bytes: [...new TextEncoder().encode('\x1b[31;')] },
  { state: 'unterminated-dcs', bytes: [...new TextEncoder().encode('\x1bP1;2qOLD-DCS')] },
  { state: 'incomplete-utf8', bytes: [0xe4, 0xb8] },
])('Native_RestartClearsParsedState_022 $state', async ({ state, bytes }) => {
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const term = io.terminals[0]
  const reset = vi.spyOn(term, 'reset')
  io.channels[0].channel.onmessage({ runId: tab.runId, generation: 1, streamEpoch: '1', offset: '0', bytes })
  await vi.waitFor(() => expect(io.ack).toHaveBeenCalledTimes(1))
  await wrapper.setProps({ active: false }); await wrapper.setProps({ active: true })
  await (wrapper.vm as any).recover()
  expect(io.channels).toHaveLength(1)
  expect(reset).not.toHaveBeenCalled()
  tabs.tab(tab.tabId)!.status = 'exited'
  const restarted = tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
  await vi.waitFor(() => expect(io.channels).toHaveLength(2))
  expect(io.terminals).toHaveLength(1)
  const banner = 'NEW-STARTUP-BANNER'
  // The new decoder must ignore an orphan continuation byte rather than finish
  // the old generation's E4 B8 prefix into a leaked character.
  const freshBytes = [...(state === 'incomplete-utf8' ? [0xad] : []), ...new TextEncoder().encode(banner)]
  io.channels[1].channel.onmessage({ runId: restarted.runId, generation: 2, streamEpoch: '1', offset: '0', bytes: freshBytes })
  await vi.waitFor(() => expect(io.ack).toHaveBeenCalledTimes(2))
  expect(term.buffer.active.type).toBe('normal')
  expect(term.modes.bracketedPasteMode).toBe(false)
  expect(term.modes.applicationCursorKeysMode).toBe(false)
  expect(term.buffer.active.getLine(0)!.translateToString(true)).toBe(banner)
  expect(reset).toHaveBeenCalledTimes(1)
  await (wrapper.vm as any).start()
  expect(io.channels).toHaveLength(2)
})

// An actual asynchronous xterm parser handler holds old output across the restart.
// The old DSR and ACK must settle with no route into the new process.
it('Native_RestartFencesPendingParser_023', async () => {
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const term = io.terminals[0]
  let release!: (value: boolean) => void
  let entered = false
  const handler = term.parser.registerOscHandler(777, () => { entered = true; return new Promise<boolean>(resolve => { release = resolve }) })
  const bytes = [...new TextEncoder().encode('\x1b]777;hold\x07\x1b[6n\x1b[?1049h\x1b[?2004hOLD-QUEUED')]
  io.channels[0].channel.onmessage({ runId: tab.runId, generation: 1, streamEpoch: '1', offset: '0', bytes })
  await vi.waitFor(() => expect(entered).toBe(true))
  tabs.tab(tab.tabId)!.status = 'exited'
  const restarted = tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
  await flushPromises()
  const startsDuringDrain = io.channels.length
  term.input('during drain', true); await flushPromises()
  release(true)
  await vi.waitFor(() => expect(io.channels).toHaveLength(2))
  expect(startsDuringDrain).toBe(1)
  expect(io.protocol).not.toHaveBeenCalled()
  expect(io.ack).not.toHaveBeenCalled()
  expect(io.user).not.toHaveBeenCalled()
  expect(term.buffer.active.type).toBe('normal')
  expect(term.modes.bracketedPasteMode).toBe(false)
  const fresh = [...new TextEncoder().encode('NEW\x1b[6n')]
  io.channels[1].channel.onmessage({ runId: restarted.runId, generation: 2, streamEpoch: '1', offset: '0', bytes: fresh })
  await vi.waitFor(() => expect(io.protocol).toHaveBeenCalledTimes(1))
  await vi.waitFor(() => expect(io.ack).toHaveBeenCalledTimes(1))
  expect(io.protocol.mock.calls[0][0]).toEqual({ runId: restarted.runId, generation: 2 })
  expect(io.ack.mock.calls[0][0]).toMatchObject({ runId: restarted.runId, generation: 2 })
  expect(term.buffer.active.getLine(0)!.translateToString(true)).toBe('NEW')
  handler.dispose()
})

it('Native_ParserFenceTimeoutNeverStarts_024', async () => {
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const term = io.terminals[0]
  const reset = vi.spyOn(term, 'reset')
  let release!: (value: boolean) => void
  let entered = false
  const handler = term.parser.registerOscHandler(777, () => { entered = true; return new Promise<boolean>(resolve => { release = resolve }) })
  io.channels[0].channel.onmessage({ runId: tab.runId, generation: 1, streamEpoch: '1', offset: '0', bytes: [...new TextEncoder().encode('\x1b]777;hold\x07\x1b[6n')] })
  await vi.waitFor(() => expect(entered).toBe(true))
  vi.useFakeTimers()
  tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
  await flushPromises()
  await vi.advanceTimersByTimeAsync(4999)
  const startsBeforeDeadline = io.channels.length
  await vi.advanceTimersByTimeAsync(1)
  const afterDeadline = { status: tabs.tab(tab.tabId)!.status, errorCode: tabs.tab(tab.tabId)!.errorCode, starts: io.channels.length }
  release(true); await vi.advanceTimersByTimeAsync(0); await flushPromises()
  expect(startsBeforeDeadline).toBe(1)
  expect(afterDeadline).toEqual({ status: 'failed', errorCode: 'NATIVE_TERMINAL_NOT_READY', starts: 1 })
  expect(reset).not.toHaveBeenCalled()
  expect(io.protocol).not.toHaveBeenCalled()
  expect(io.ack).not.toHaveBeenCalled()
  await (wrapper.vm as any).start()
  expect(io.channels).toHaveLength(1)
  handler.dispose()
})

it.each(['cancel', 'unmount', 'supersede'] as const)('Native_ParserFenceRevokesOldOwner_025 %s', async action => {
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await flushPromises()
  const term = io.terminals[0]
  const reset = vi.spyOn(term, 'reset')
  let release!: (value: boolean) => void
  let entered = false
  const handler = term.parser.registerOscHandler(777, () => { entered = true; return new Promise<boolean>(resolve => { release = resolve }) })
  io.channels[0].channel.onmessage({ runId: tab.runId, generation: 1, streamEpoch: '1', offset: '0', bytes: [...new TextEncoder().encode('\x1b]777;hold\x07\x1b[6n')] })
  await vi.waitFor(() => expect(entered).toBe(true))
  tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
  await flushPromises()
  let cancellation: unknown
  if (action === 'unmount') { wrapper.unmount(); wrapper = null }
  else {
    try { await (wrapper.vm as any).stop() } catch (error) { cancellation = error }
    if (action === 'supersede') {
      expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'failed', errorCode: 'LAUNCH_CANCELLED' })
      tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
    }
  }
  release(true)
  if (action === 'supersede') await vi.waitFor(() => expect(io.channels.some(value => value.input.generation === 3)).toBe(true))
  else await new Promise<void>(resolve => term.write('', resolve))
  await flushPromises()
  expect(cancellation).toBeUndefined()
  expect(io.channels.map(value => value.input.generation)).toEqual(action === 'supersede' ? [1, 3] : [1])
  expect(reset).toHaveBeenCalledTimes(action === 'supersede' ? 1 : 0)
  expect(io.protocol).not.toHaveBeenCalled()
  expect(io.ack).not.toHaveBeenCalled()
  expect(io.stop).not.toHaveBeenCalled()
  if (action === 'cancel') expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'failed', errorCode: 'LAUNCH_CANCELLED' })
  handler.dispose()
})
