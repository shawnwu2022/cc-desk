import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, type VueWrapper } from '@vue/test-utils'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { imagePasteBytes } from '@/utils/pasteText'
import { platform } from '@/utils/platform'

/**
 * NativeCliTerminal 组件级粘贴回归：Ctrl+V 的原生 paste 事件必须穿过
 * bindClipboard 守卫链（active/inputEnabled/target 包含）并携带
 * buildPastePayload 的字节到达 binding.reserveUserPaste。
 * 守卫链任何一环静默丢弃都会表现为用户侧「Ctrl+V 失效」。
 */
const io = vi.hoisted(() => ({
  binding: null as any,
  bindings: [] as any[],
  terminals: [] as any[],
  starts: [] as any[],
  readText: vi.fn(),
  readImage: vi.fn(),
  failReservation: false,
}))
vi.mock('@/terminal/deskNativeTerminal', () => ({
  createDeskNativeTerminalBinding: vi.fn((options: any) => {
    const next = {
      acceptOutput: vi.fn(() => true),
      sendUserText: vi.fn().mockResolvedValue(undefined),
      reserveUserPaste: vi.fn((produce: () => Promise<Uint8Array>) => {
        let pending = true
        let cancelled = false
        const cancel = vi.fn(() => {
          if (!pending) return false
          pending = false
          cancelled = true
          return true
        })
        const produced = Promise.resolve().then(produce)
        const settled = produced.then(() => {
          pending = false
          if (io.failReservation && !cancelled) throw new Error('WRITE_FAILED')
        }, error => {
          pending = false
          throw error
        })
        return { inputSeq: '1', produced, settled, cancel }
      }),
      drainInput: vi.fn().mockResolvedValue(undefined),
      dispose: vi.fn(() => input.dispose()),
    }
    const input = options.term.onData((data: string) => { void next.sendUserText(data) })
    io.binding = next
    io.bindings.push(next)
    return next
  }),
}))
vi.mock('@xterm/xterm', async original => {
  const canvas = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  const actual = await original<typeof import('@xterm/xterm')>()
  canvas.mockRestore()
  return { ...actual, Terminal: class extends actual.Terminal {
    constructor(options: import('@xterm/xterm').ITerminalOptions) {
      super(options)
      io.terminals.push(this)
    }
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
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { activate() {} dispose() {} fit() {} } }))
vi.mock('@tauri-apps/api/core', async original => ({ ...await original<object>(), Channel: class { onmessage: any } }))
vi.mock('@/api/observer', () => ({ onNativeObservation: async () => () => {} }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({
  writeText: vi.fn().mockResolvedValue(undefined),
  readText: (...args: unknown[]) => io.readText(...args),
  readImage: (...args: unknown[]) => io.readImage(...args),
}))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMinimized: async () => false }) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  cliResize: vi.fn().mockResolvedValue(undefined), cliStop: vi.fn().mockResolvedValue(undefined),
  ptyInput: vi.fn().mockResolvedValue(undefined), ptyKill: vi.fn().mockResolvedValue(undefined), ptyResize: vi.fn().mockResolvedValue(undefined),
  logMessage: vi.fn().mockResolvedValue(undefined),
  onPtyOutput: async () => () => {}, onPtyExit: async () => () => {},
}))

let wrapper: VueWrapper | null = null
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks()
  io.binding = null; io.bindings = []; io.terminals = []; io.starts = []; io.failReservation = false
  io.readText.mockResolvedValue('')
  io.readImage.mockRejectedValue(new Error('NO_IMAGE'))
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') {
        io.starts.push(input)
        return { instanceId: 'test-backend', requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, phase: 'running', revision: '1', failure: null }
      }
      if (command === 'cli_get_launch_status') {
        const original = io.starts.find(value => value.requestId === input.requestId)
        if (!original) throw new Error('UNKNOWN_REQUEST_ID')
        return { instanceId: 'test-backend', requestId: input.requestId, run: { runId: original.runId, generation: original.generation }, phase: 'running', revision: '2', failure: null }
      }
      throw new Error('UNEXPECTED_BRIDGE_COMMAND: ' + command)
    },
  } })
  let id = 0
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => `tab-${++id}` })
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { fn(0); return 1 })
  useCliProfilesStore().profiles = [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }]
  vi.useFakeTimers()
})
afterEach(() => {
  wrapper?.unmount(); wrapper = null
  Reflect.deleteProperty(window, '__CC_DESK_DOCUMENT__')
  vi.unstubAllGlobals(); vi.useRealTimers(); document.body.innerHTML = ''
})

function pasteEvent(text: string): Event {
  const event = new Event('paste', { bubbles: true, cancelable: true })
  Object.defineProperty(event, 'clipboardData', {
    value: { getData: (type: string) => type === 'text/plain' ? text : '', types: ['text/plain'], items: [] },
  })
  return event
}

describe('NativeTerminalPaste_TextEventRoutesToBinding', () => {
  it('Running_ActivePasteReachesBinding_001 — running 激活 tab 的文本 paste 到达 binding 且字节正确', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    expect(tabs.tab(tab.tabId)?.status).toBe('running')
    expect(io.binding).not.toBeNull()

    const textarea = wrapper.find('textarea').element
    textarea.dispatchEvent(pasteEvent('hello'))
    await vi.advanceTimersByTimeAsync(0)

    expect(io.binding.reserveUserPaste).toHaveBeenCalledOnce()
    const produce = io.binding.reserveUserPaste.mock.calls[0][0] as () => Promise<Uint8Array>
    expect(new TextDecoder().decode(await produce())).toBe('hello')
  })

  it('Running_KeyDownCtrlVRoutesViaClipboardRead_002 — WebView2 不派发 paste 事件时，Ctrl+V keydown 经插件读剪贴板路由', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    expect(tabs.tab(tab.tabId)?.status).toBe('running')
    io.readText.mockResolvedValue('from-clipboard')

    const textarea = wrapper.find('textarea').element
    const keydown = new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true })
    textarea.dispatchEvent(keydown)
    await vi.advanceTimersByTimeAsync(0)

    // paste 事件从未派发的环境里，keydown 路由是唯一入口
    expect(keydown.defaultPrevented).toBe(true)
    expect(io.binding.reserveUserPaste).toHaveBeenCalledOnce()
    const produce = io.binding.reserveUserPaste.mock.calls[0][0] as () => Promise<Uint8Array>
    expect(new TextDecoder().decode(await produce())).toBe('from-clipboard')
  })

  it('Paste_EmptyCancels_003 — 空剪贴板取消预留且后续键盘仍可提交', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    io.readText.mockResolvedValue('')

    const textarea = wrapper.find('textarea').element
    textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))
    await vi.advanceTimersByTimeAsync(0)
    expect(io.binding.reserveUserPaste).toHaveBeenCalledOnce()
    const reservation = io.binding.reserveUserPaste.mock.results[0].value as { settled: Promise<void>; cancel: ReturnType<typeof vi.fn> }
    await expect(reservation.settled).resolves.toBeUndefined()
    expect(reservation.cancel).toHaveBeenCalledOnce()
    expect(tabs.tab(tab.tabId)?.errorCode).toBeNull()

    io.terminals[0].input('\r', true)
    await vi.advanceTimersByTimeAsync(0)
    expect(io.binding.sendUserText).toHaveBeenCalledWith('\r')
  })

  it('Running_KeyDownImageClipboardSendsShortcut_004 — 无文本且剪贴板有图时发送 CLI 官方图片粘贴键位', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    io.readText.mockResolvedValue('')
    const close = vi.fn().mockResolvedValue(undefined)
    io.readImage.mockResolvedValue({ close })

    const textarea = wrapper.find('textarea').element
    textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))
    await vi.advanceTimersByTimeAsync(0)
    expect(io.binding.reserveUserPaste).toHaveBeenCalledOnce()
    const reservation = io.binding.reserveUserPaste.mock.results[0].value as { produced: Promise<Uint8Array> }
    expect(new TextDecoder().decode(await reservation.produced)).toBe(imagePasteBytes(platform))
    expect(io.binding.sendUserText).not.toHaveBeenCalled()
    expect(close).toHaveBeenCalledOnce()
  })

  // 异步读剪贴板前必须同步预留输入序号，后续键盘输入才能排在粘贴之后。
  it('Paste_ReservesBeforeRead_005 — Ctrl+V 在 readText 完成前预留当前 intent', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    let finishRead!: (text: string) => void
    io.readText.mockReturnValue(new Promise(resolve => { finishRead = resolve }))

    wrapper.find('textarea').element.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))

    expect(io.binding.reserveUserPaste).toHaveBeenCalledOnce()
    finishRead('ordered')
    await vi.advanceTimersByTimeAsync(0)
    const reservation = io.binding.reserveUserPaste.mock.results[0].value as { produced: Promise<Uint8Array> }
    expect(new TextDecoder().decode(await reservation.produced)).toBe('ordered')
  })

  // restart 后旧 Ctrl+V 完成不得通过 mutable binding 把旧 attempt 内容写进新 run。
  it('Paste_StaleCancels_006 — restart 后旧剪贴板结果取消旧预留', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    const oldBinding = io.binding
    let finishRead!: (text: string) => void
    io.readText.mockReturnValue(new Promise(resolve => { finishRead = resolve }))
    wrapper.find('textarea').element.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))
    expect(oldBinding.reserveUserPaste).toHaveBeenCalledOnce()

    tabs.applyLaunchStatus(tab.tabId, {
      instanceId: 'test-backend', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation },
      phase: 'exited', revision: '2', failure: null,
    })
    tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
    await vi.advanceTimersByTimeAsync(0)
    expect(io.binding).not.toBe(oldBinding)

    finishRead('stale')
    await vi.advanceTimersByTimeAsync(0)
    const reservation = oldBinding.reserveUserPaste.mock.results[0].value as { settled: Promise<void>; cancel: ReturnType<typeof vi.fn> }
    await expect(reservation.settled).resolves.toBeUndefined()
    expect(reservation.cancel).toHaveBeenCalledOnce()
    expect(io.binding.reserveUserPaste).not.toHaveBeenCalled()
    expect(tabs.tab(tab.tabId)?.errorCode).toBeNull()
  })

  it('Paste_UnreadableCancels_007 — 剪贴板不可读时取消预留且后续键盘仍可提交', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    io.readText.mockRejectedValue(new Error('TEXT_DENIED'))
    io.readImage.mockRejectedValue(new Error('IMAGE_DENIED'))

    wrapper.find('textarea').element.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))
    await vi.advanceTimersByTimeAsync(0)
    const reservation = io.binding.reserveUserPaste.mock.results[0].value as { settled: Promise<void>; cancel: ReturnType<typeof vi.fn> }
    await expect(reservation.settled).resolves.toBeUndefined()
    expect(reservation.cancel).toHaveBeenCalledOnce()
    expect(tabs.tab(tab.tabId)?.errorCode).toBeNull()

    io.terminals[0].input('next', true)
    await vi.advanceTimersByTimeAsync(0)
    expect(io.binding.sendUserText).toHaveBeenCalledWith('next')
  })

  it('Paste_WriteFailureReports_008 — 已产生 payload 的传输失败仍显示输入暂停诊断', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    io.readText.mockResolvedValue('payload')
    io.failReservation = true

    wrapper.find('textarea').element.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))
    await vi.advanceTimersByTimeAsync(0)

    expect(tabs.tab(tab.tabId)?.errorCode).toBe('NATIVE_INPUT_PAUSED')
    const reservation = io.binding.reserveUserPaste.mock.results[0].value as { cancel: ReturnType<typeof vi.fn> }
    expect(reservation.cancel).not.toHaveBeenCalled()
  })
})
