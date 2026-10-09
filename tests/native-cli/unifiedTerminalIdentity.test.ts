import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import { useNativeTabsStore, captureNativeAttempt } from '@/stores/nativeTabs'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useProjectResourcesStore } from '@/stores/projectResources'
import { useWorkspaceStore } from '@/stores/workspace'
import { createLaunchAttempt } from '@/api/cliLaunchAttempt'
import { useHookStore, type ObservationHandler } from '@/stores/hook'
import { useAppStore } from '@/stores/app'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { platform } from '@/utils/platform'

const io = vi.hoisted(() => ({ failBinding: false, terms: [] as any[], fits: [] as any[], bindings: [] as any[], channels: [] as any[], observers: [] as any[], scope: vi.fn(), read: vi.fn(), start: vi.fn(), recover: vi.fn(), cancel: vi.fn(), stop: vi.fn(), copy: vi.fn(), resize: vi.fn() }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {
  options: any; element!: HTMLElement; textarea!: HTMLTextAreaElement
  cols = 80; rows = 24; modes = { applicationCursorKeysMode: false, applicationKeypadMode: false, bracketedPasteMode: false, insertMode: false, mouseTrackingMode: 'none', originMode: false, reverseWraparoundMode: false, sendFocusMode: false, wraparoundMode: true }; output = ''; focus = vi.fn(); dispose = vi.fn(); key: any; selection = ''
  constructor(options: any) { this.options = options; io.terms.push(this) }
  loadAddon() {} open(el: HTMLElement) { this.element = el; this.textarea = document.createElement('textarea'); el.append(this.textarea) }
  parsed = new Set<() => void>(); parsedRegistrations = 0
  onWriteParsed(callback: () => void) { this.parsedRegistrations++; this.parsed.add(callback); return { dispose: () => this.parsed.delete(callback) } }
  onData() { return { dispose() {} } } attachCustomKeyEventHandler(fn: any) { this.key = fn } getSelection() { return this.selection }
  write(data: string) { this.output += data; this.parsed.forEach(callback => callback()) }
} }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit = vi.fn(); constructor() { io.fits.push(this) } } }))
vi.mock('@tauri-apps/api/core', () => ({ Channel: class { onmessage: any; constructor() { io.channels.push(this) } }, invoke: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), cliResize: io.resize, cliStop: io.stop, createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }) }))
vi.mock('@/terminal/nativeLaunchEntry', async original => ({ ...await original<object>(), createNativeLaunchEntry: () => ({ start: io.start, recover: io.recover, cancel: io.cancel, latest: vi.fn() }) }))
vi.mock('@/terminal/deskNativeTerminal', () => ({ createDeskNativeTerminalBinding: (options: any) => {
  if (io.failBinding) throw new Error('XTERM_USER_INPUT_PROVENANCE_UNAVAILABLE')
  const binding = { options, acceptOutput: (frame: any) => { options.term.write(frame.data); return true }, dispose: vi.fn(), sendUserText: vi.fn().mockResolvedValue(undefined), reserveUserPaste: vi.fn(() => ({ inputSeq: '1', settled: Promise.resolve() })) }
  io.bindings.push(binding); return binding
} }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: io.copy }))
const wrappers: VueWrapper[] = []
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); io.failBinding = false
  io.terms.length = 0; io.fits.length = 0; io.bindings.length = 0; io.channels.length = 0; io.observers.length = 0
  vi.stubGlobal('ResizeObserver', class { constructor(fn: any) { io.observers.push(fn) } observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { fn(0); return 1 })
  io.start.mockImplementation(async (input: any) => ({ requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, phase: 'running', revision: '1', failure: null }))
  io.recover.mockImplementation(async (requestId: string) => {
    const input = io.start.mock.calls.map(call => call[0]).find(input => input.requestId === requestId)
    return { requestId, run: { runId: input.runId, generation: input.generation }, phase: 'running', revision: '2', failure: null }
  })
  io.cancel.mockImplementation(async (requestId: string) => {
    const input = io.start.mock.calls.map(call => call[0]).find(input => input.requestId === requestId)
    return { requestId, run: { runId: input.runId, generation: input.generation }, phase: 'running', revision: '1', failure: null }
  })
  io.copy.mockResolvedValue(undefined)
  io.stop.mockResolvedValue(undefined); io.resize.mockResolvedValue(undefined)
  useCliProfilesStore().profiles = [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }]
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); vi.unstubAllGlobals(); vi.useRealTimers(); document.body.innerHTML = '' })
function open(active = true) {
  const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'raw', argv: ['one two', '--flag', ''] } })
  const wrapper = mount(NativeCliTerminal, { attachTo: document.body, props: { tabId: tab.tabId, active } }); wrappers.push(wrapper)
  return { tab, wrapper, vm: wrapper.vm as any }
}

describe('Unified native terminal identity', () => {
  it('Native_ImageFilePasteKeepsCurrentInputOwner_025', async () => {
    const { wrapper } = open(); await flushPromises()
    const image = () => {
      const event = new Event('paste', { bubbles: true, cancelable: true })
      Object.defineProperty(event, 'clipboardData', { value: { getData: () => '', types: ['Files'], items: [{ kind: 'file', type: 'image/png' }] } })
      return event
    }
    const event = image(); wrapper.get('textarea').element.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(true)
    expect(io.bindings[0].sendUserText).toHaveBeenCalledExactlyOnceWith(platform === 'windows' ? '\x1bv' : '\x16')
    expect(io.bindings[0].reserveUserPaste).not.toHaveBeenCalled()
    await wrapper.setProps({ active: false })
    const hidden = image(); wrapper.get('textarea').element.dispatchEvent(hidden)
    expect(hidden.defaultPrevented).toBe(false)
    expect(io.bindings[0].sendUserText).toHaveBeenCalledTimes(1)
  })
  // 隐藏时不测量，后台输出保留并在显示时测量，不重新启动。
  it('Native_VisibilityRetainsOutput_001', async () => {
    const { wrapper, vm } = open(false); await flushPromises()
    expect(io.fits[0].fit).not.toHaveBeenCalled()
    io.channels[0].onmessage({ data: 'background output' })
    io.observers[0](); vm.fitVisible(); await flushPromises()
    expect(io.fits[0].fit).not.toHaveBeenCalled()
    await wrapper.setProps({ active: true }); await flushPromises()
    expect(io.terms[0].output).toBe('background output')
    expect(io.fits[0].fit).toHaveBeenCalled()
    expect(io.start).toHaveBeenCalledTimes(1)
  })
  // GUI主题切换不触发重启；终端字号更新只测量可见实例。
  it('Native_GlobalPreferencesNoReplay_002', async () => {
    const { wrapper } = open(false); await flushPromises()
    const app = useAppStore(); app.fontSize = 17; app.terminalTheme = 'cc-box-dark'; app.theme = 'dark'; await flushPromises()
    expect(io.terms[0].options.fontSize).toBe(17)
    expect(io.terms[0].options.theme).toBeDefined()
    expect(io.fits[0].fit).not.toHaveBeenCalled()
    await wrapper.setProps({ active: true }); await flushPromises()
    expect(io.start).toHaveBeenCalledTimes(1)
  })
  // 停止期间更换代次，旧停止完成不得检查或修改新代次。
  it('Native_StaleStopIsIsolated_003', async () => {
    const { tab, vm } = open(); await flushPromises()
    let finish!: () => void; io.stop.mockReturnValue(new Promise<void>(resolve => { finish = resolve }))
    const stopping = vm.stop(captureNativeAttempt(tab)); await flushPromises()
    const tabs = useNativeTabsStore(); tabs.tab(tab.tabId)!.status = 'exited'
    const newer = tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
    finish(); await stopping
    expect(io.stop).toHaveBeenCalledWith({ runId: tab.runId, generation: 1 })
    expect(io.recover).not.toHaveBeenCalled()
    expect(tabs.tab(tab.tabId)).toMatchObject({ requestId: newer.requestId, status: 'running', generation: 2 })
  })
  // 外部传入旧代次的生命周期请求必须在桥接调用前被拒绝。
  it('Native_RejectsOldAttemptBeforeIO_004', async () => {
    const { tab, vm } = open(); await flushPromises()
    const tabs = useNativeTabsStore(); tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
    await expect(vm.stop(captureNativeAttempt(tab))).rejects.toThrow('STALE_NATIVE_ATTEMPT')
    await expect(vm.recover(captureNativeAttempt(tab))).rejects.toThrow('STALE_NATIVE_ATTEMPT')
    expect(io.stop).not.toHaveBeenCalled(); expect(io.recover).not.toHaveBeenCalled()
  })
  // 旧检查失败不得让新代次失去输入能力或状态。
  it('Native_StaleRecoverIsIsolated_005', async () => {
    const { tab, vm } = open(); await flushPromises()
    let fail!: (reason: Error) => void; io.recover.mockReturnValue(new Promise((_resolve, reject) => { fail = reject }))
    const recovering = vm.recover()
    const tabs = useNativeTabsStore(); tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
    fail(new Error('LAUNCH_STATE_UNKNOWN')); await recovering
    expect(tabs.tab(tab.tabId)!.status).toBe('running')
    expect(io.bindings[1].options.currentTarget()).toMatchObject({ generation: 2 })
  })
  // 未知启动不能重复start或重新创建绑定；隐藏用户输入不进入传输层。
  it('Native_NoReplayAndHiddenInput_006', async () => {
    io.start.mockRejectedValue(new Error('LAUNCH_STATE_UNKNOWN'))
    const { wrapper, vm } = open(false); await flushPromises()
    await vm.start(); await flushPromises()
    expect(io.start).toHaveBeenCalledTimes(1)
    expect(io.bindings).toHaveLength(1)
    const paste = new Event('paste', { bubbles: true, cancelable: true })
    Object.defineProperty(paste, 'clipboardData', { value: { getData: () => 'must not send', types: ['text/plain'] } })
    wrapper.get('textarea').element.dispatchEvent(paste)
    expect(io.bindings[0].reserveUserPaste).not.toHaveBeenCalled()
    vm.focus(); expect(io.terms[0].focus).not.toHaveBeenCalled()
  })
  // 关闭运行中的Native会话先停止精确代次，旧完成不得关闭已重启的会话。
  it('Native_CloseGuardsNewerAttempt_007', async () => {
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    let finish!: () => void
    const stopTab = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [] }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() } })
    const closing = adapter.closeSession(`native-tab:${tab.tabId}`)
    const rejected = expect(closing).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(stopTab).toHaveBeenCalledTimes(1)
    tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
    finish(); await rejected
    expect(tabs.tab(tab.tabId)?.generation).toBe(2)
  })
  // 检查状态网络失败仍属于未知状态，不得转成可重新启动的失败状态。
  it('Native_RecoveryFailureStaysUnknown_008', async () => {
    const { tab, vm } = open(); await flushPromises()
    io.recover.mockRejectedValue(new Error('connection dropped /secret'))
    await vm.recover()
    expect(useNativeTabsStore().tab(tab.tabId)!.status).toBe('unknown')
    await vm.start(); expect(io.start).toHaveBeenCalledTimes(1)
  })
  // 停止回执后状态仍运行，不得向关闭或重启调用方报告已停止。
  it('Native_StopRequiresEndedState_009', async () => {
    vi.useFakeTimers(); const { tab, vm } = open(); await flushPromises()
    io.recover.mockResolvedValue({ requestId: tab.requestId, run: { runId: tab.runId, generation: 1 }, phase: 'running', revision: '2' })
    const stopped = expect(vm.stop()).rejects.toThrow('NATIVE_STOP_UNCONFIRMED')
    await vi.advanceTimersByTimeAsync(5100)
    await stopped
  })
  // 排队的旧关闭请求不能在前一个停止完成后重新捕获新代次。
  it('Native_QueuedCloseKeepsOwnership_010', async () => {
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    let finish!: () => void
    const stopTab = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [] }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() } })
    useProjectsStateStore().loaded = true
    const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([adapter]); await catalog.initialize()
    const stopping = catalog.stopSession(`native-tab:${tab.tabId}`); await flushPromises()
    const closing = catalog.closeSession(`native-tab:${tab.tabId}`)
    const rejected = expect(closing).rejects.toThrow('STALE_SESSION_ATTEMPT')
    tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
    finish(); await stopping; await rejected
    expect(tabs.tab(tab.tabId)?.generation).toBe(2)
    expect(stopTab).toHaveBeenCalledTimes(1)
  })
  // 相同run和generation但不同request的输出必须被拒绝。
  it('Native_OutputIncludesRequestOwner_011', async () => {
    const { tab } = open(); await flushPromises()
    useNativeTabsStore().tab(tab.tabId)!.requestId = 'replacement-request'
    io.channels[0].onmessage({ data: 'old output' })
    expect(io.terms[0].output).toBe('')
  })

  // 关闭排队后同attempt进入starting，不能未经确认停止该进程。
  it('Native_QueuedCloseRechecksPhase_012', async () => {
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    const stopTab = vi.fn().mockResolvedValue(undefined)
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [] }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() } })
    useProjectsStateStore().loaded = true; const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([adapter]); await catalog.initialize()
    const closing = catalog.closeSession(`native-tab:${tab.tabId}`); const rejected = expect(closing).rejects.toThrow('STALE_SESSION_ATTEMPT')
    tabs.markStarting(tab.tabId); await rejected
    expect(stopTab).not.toHaveBeenCalled(); expect(tabs.tab(tab.tabId)).toBeDefined()
  })
  // 隐藏后台进程自然退出仍发布状态，不依赖重新选择该会话。
  it('Native_HiddenStatusRemainsLive_013', async () => {
    vi.useFakeTimers(); const { tab } = open(false); await flushPromises()
    io.recover.mockResolvedValue({ requestId: tab.requestId, run: { runId: tab.runId, generation: 1 }, phase: 'exited', revision: '2', failure: null })
    await vi.advanceTimersByTimeAsync(1500)
    expect(useNativeTabsStore().tab(tab.tabId)!.status).toBe('exited')
    expect(io.fits[0].fit).not.toHaveBeenCalled()
  })
  // 系统Copy仅属于可见且已聚焦的Native终端，隐藏后交给当前目标。
  it('Native_SystemCopyOwnsFocus_014', async () => {
    const { wrapper } = open(); await flushPromises(); io.terms[0].selection = 'native selected'
    ;(wrapper.get('textarea').element as HTMLTextAreaElement).focus()
    const copy = new Event('copy', { bubbles: true, cancelable: true }); window.dispatchEvent(copy)
    expect(copy.defaultPrevented).toBe(true); expect(io.copy).toHaveBeenCalledWith('native selected')
    await wrapper.setProps({ active: false }); io.copy.mockClear()
    const hidden = new Event('copy', { bubbles: true, cancelable: true }); window.dispatchEvent(hidden)
    expect(hidden.defaultPrevented).toBe(false); expect(io.copy).not.toHaveBeenCalled()
  })

  // 旧关闭完成不能通过统一facade清除新代次的当前选择。
  it('Native_StaleClosePreservesSelection_015', async () => {
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    let finish!: () => void; const stopTab = vi.fn(() => new Promise<void>(r => { finish = r }))
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [] }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() } })
    useProjectsStateStore().loaded = true; const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([adapter]); await catalog.initialize(); await catalog.activateSession(`native-tab:${tab.tabId}`)
    const closing = catalog.closeSession(`native-tab:${tab.tabId}`); const rejected = expect(closing).rejects.toThrow('STALE_SESSION_ATTEMPT'); await flushPromises()
    tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); finish(); await rejected
    expect(catalog.activeSessionId).toBe(`native-tab:${tab.tabId}`)
  })

  // 真实 Native 组件错误分支组合实际回执解析失败，资源不得回退当前配置。
  it('Native_InvalidReceiptResourceScope_016', async () => {
    const sent = vi.fn(async () => ({}))
    io.start.mockImplementation(input => createLaunchAttempt({ ...input, profileId: 'cx', expectedProfileRevision: '7' }, 'instance', { start: sent, status: vi.fn() }).start())
    const { tab } = open(); await flushPromises()
    const tabs = useNativeTabsStore(); expect(sent).toHaveBeenCalledOnce()
    expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'unknown', launchRevision: null })
    useWorkspaceStore().projects = [{ projectId: 'project', hostId: 'host', sourcePathKey: 'root', selectedPath: '/repo', canonicalPath: null, alias: { mode: 'inherit' }, hidden: { mode: 'inherit' }, pinned: { mode: 'inherit' } }]
    const sessions = useUnifiedSessionsStore(); sessions.sessions = [{ id: `native-tab:${tab.tabId}`, adapterSessionId: tab.tabId, cli: 'codex', runtime: 'native-cli', projectKey: '/repo', projectPath: '/repo', title: 'Codex',
      processState: 'failed', attentionState: 'none', lastActivityAt: 1, archived: false, resumable: false }]; sessions.activeSessionId = sessions.sessions[0].id
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN' })
    const resources = useProjectResourcesStore(); resources.setActive(true); await flushPromises()
    expect(io.scope.mock.calls).toEqual([[{ kind: 'run', runId: tab.runId, generation: tab.generation }]])
    expect(resources.unavailable).toBe(true); expect(io.read).not.toHaveBeenCalled(); expect(sent).toHaveBeenCalledOnce()
  })
  // 只消费 observer 投影的明确等待状态；失序未知状态不冒充需要回复，旧 run 无法改新 run。
  it('Native_ProjectedAttention_017', async () => {
    let observation: ObservationHandler | undefined
    const unsubscribe = vi.fn()
    const hook = useHookStore()
    const subscribe = vi.spyOn(hook, 'subscribeObservation').mockImplementation((_target, handler) => { observation = handler; return unsubscribe })
    const now = vi.spyOn(Date, 'now').mockReturnValue(1000)
    const profile = useCliProfilesStore().profiles[0]
    useCliProfilesStore().profiles.push({ ...profile, id: 'cc', cli: 'claude' })
    const tab = useNativeTabsStore().create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cc', profileRevision: '7', action: { kind: 'new' } })
    const wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); wrappers.push(wrapper); await flushPromises()
    const original = observation
    expect(original, 'terminal subscribes to the existing exact-run observer projection').toBeTypeOf('function')
    now.mockReturnValue(2000); original!({ kind: 'unknown', runId: tab.runId, generation: tab.generation }, { observation: 'active', activity: 'waiting' })
    expect(useNativeTabsStore().tab(tab.tabId)).toMatchObject({ attentionState: 'needs-user', lastActivityAt: 2000 })
    now.mockReturnValue(3000); original!({ kind: 'unknown', runId: tab.runId, generation: tab.generation }, { observation: 'active', activity: 'waiting' })
    expect(useNativeTabsStore().tab(tab.tabId)!.lastActivityAt).toBe(2000)
    now.mockReturnValue(4000); original!({ kind: 'unknown', runId: tab.runId, generation: tab.generation }, { observation: 'active', activity: 'unknown' })
    expect(useNativeTabsStore().tab(tab.tabId)).toMatchObject({ attentionState: 'none', lastActivityAt: 4000 })
    const tabs = useNativeTabsStore(); tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cc', profileRevision: '7' }); await flushPromises()
    now.mockReturnValue(9000); original!({ kind: 'unknown', runId: tab.runId, generation: tab.generation }, { observation: 'active', activity: 'waiting' })
    expect(tabs.tab(tab.tabId)).toMatchObject({ attentionState: 'none', lastActivityAt: 4000 })
    expect(unsubscribe).toHaveBeenCalled()
    subscribe.mockRestore(); now.mockRestore()
  })

  // 可选观察订阅失败不得阻止 Native 启动或替换输入传输。
  it('Native_ObserverFailureIsOptional_018', async () => {
    const subscribe = vi.spyOn(useHookStore(), 'subscribeObservation').mockImplementation(() => { throw new Error('OBSERVER_CAPACITY') })
    const { tab } = open(); await flushPromises()
    expect(io.start).toHaveBeenCalledTimes(1)
    expect(useNativeTabsStore().tab(tab.tabId)?.status).toBe('running')
    subscribe.mockRestore()
  })

  // 启动回执等待期间只保留准确 run 的最新投影，直到 running 才发布需要回复。
  it.each(['waiting', 'unknown'] as const)('Native_PendingReceiptAttention_019 %s', async latest => {
    let observation: ObservationHandler | undefined
    const subscribe = vi.spyOn(useHookStore(), 'subscribeObservation').mockImplementation((_target, handler) => { observation = handler; return () => {} })
    let finish!: () => void
    io.start.mockImplementation(input => new Promise(resolve => { finish = () => resolve({ requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, phase: 'running', revision: '1', failure: null }) }))
    useCliProfilesStore().profiles.push({ ...useCliProfilesStore().profiles[0], id: 'cc', cli: 'claude' })
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cc', profileRevision: '7', action: { kind: 'new' } })
    const wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); wrappers.push(wrapper); await flushPromises()
    expect(tabs.tab(tab.tabId)?.status).toBe('starting')
    observation!({ kind: 'waiting', runId: tab.runId, generation: tab.generation, eventId: 'event-1', sourceSequence: '1' }, { observation: 'active', activity: 'waiting' })
    if (latest === 'unknown') observation!({ kind: 'unknown', runId: tab.runId, generation: tab.generation, eventId: 'event-2' }, { observation: 'active', activity: 'unknown' })
    expect(tabs.tab(tab.tabId)?.attentionState).toBe('none')
    finish(); await flushPromises()
    expect(tabs.tab(tab.tabId)?.status).toBe('running')
    expect(tabs.tab(tab.tabId)?.attentionState).toBe(latest === 'waiting' ? 'needs-user' : 'none')
    subscribe.mockRestore()
  })

})

// 每个 run 仅持有一个模式监听，重启/卸载释放；隐藏、主题与普通输出保持同一 tracker 和滚动内容。
it('Native_ModeTrackerLifetime_020', async () => {
  const { tab, wrapper } = open(); await flushPromises()
  const term = io.terms[0]
  const firstTarget = io.bindings[0].options.currentTarget
  expect(term.parsed.size).toBe(1)
  expect(firstTarget().modeEpoch).toBe('1')
  term.modes.sendFocusMode = true; term.write('retained scrollback')
  expect(firstTarget().modeEpoch).toBe('2')
  await wrapper.setProps({ active: false })
  useAppStore().terminalTheme = 'cc-box-dark'; await flushPromises()
  term.write(' ordinary output')
  expect(firstTarget().modeEpoch).toBe('2')
  expect(term.parsed.size).toBe(1)
  const tabs = useNativeTabsStore(); tabs.tab(tab.tabId)!.status = 'exited'
  tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
  expect(io.terms).toHaveLength(1)
  expect(term.parsed.size).toBe(1)
  expect(term.output).toBe('retained scrollback ordinary output')
  expect(() => firstTarget()).toThrow('NATIVE_RUN_NOT_WRITABLE')
  const secondTarget = io.bindings[1].options.currentTarget
  expect(secondTarget()).toMatchObject({ generation: 2, modeEpoch: '1' })
  wrapper.unmount()
  expect(term.parsed.size).toBe(0)
  expect(() => secondTarget()).toThrow('NATIVE_RUN_NOT_WRITABLE')
})

// binding 工厂失败时释放已经创建的模式监听，且不能提交启动。
it('Native_FailedBindingDisposesMode_021', async () => {
  io.failBinding = true
  const { tab } = open(); await flushPromises()
  expect(io.terms[0].parsedRegistrations).toBe(1)
  expect(io.terms[0].parsed.size).toBe(0)
  expect(io.start).not.toHaveBeenCalled()
  expect(useNativeTabsStore().tab(tab.tabId)?.status).toBe('failed')
})
