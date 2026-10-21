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
  codexSupport: true,
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
vi.mock('@/api/cliPasteSupport', () => ({
  codexNativeTextPasteForAttempt: () => Promise.resolve(io.codexSupport),
}))
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
  io.codexSupport = true
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
  it.each([[false, true], [true, false]])('CodexCompatibility_AttemptFreezesPasteCapability_076 (%s → %s)', async (before, after) => {
    io.codexSupport = before
    io.readText.mockResolvedValue('safe Desk paste')
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    io.codexSupport = after
    const paste = async () => {
      const active = io.binding
      wrapper!.find('textarea').element.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))
      await vi.advanceTimersByTimeAsync(0)
      return new TextDecoder().decode(await active.reserveUserPaste.mock.results.at(-1).value.produced)
    }
    expect(await paste()).toBe(before ? '\x16' : 'safe Desk paste')
    tabs.applyLaunchStatus(tab.tabId, {
      instanceId: 'test-backend', requestId: tab.requestId,
      run: { runId: tab.runId, generation: tab.generation },
      phase: 'exited', revision: '2', failure: null,
    })
    tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
    await vi.advanceTimersByTimeAsync(0)
    expect(await paste()).toBe(after ? '\x16' : 'safe Desk paste')
    expect(io.starts).toHaveLength(2)
  })

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

  // codex ≥0.163.0 探测通过后，\x16 由 codex 自行读剪贴板处理文本和图片；
  // 预留仍走同步 reserveUserPaste 保持输入排序契约，仅 producer 返回 1 字节。
  it('Running_KeyDownCtrlVCodexSendsNativeChar_002 — codex Ctrl+V 探测通过后经预留转发 \x16', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    expect(tabs.tab(tab.tabId)?.status).toBe('running')

    const textarea = wrapper.find('textarea').element
    const keydown = new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true })
    textarea.dispatchEvent(keydown)
    await vi.advanceTimersByTimeAsync(0)

    expect(keydown.defaultPrevented).toBe(true)
    expect(io.binding.reserveUserPaste).toHaveBeenCalledOnce()
    const produce = io.binding.reserveUserPaste.mock.calls[0][0] as () => Promise<Uint8Array>
    expect(new TextDecoder().decode(await produce())).toBe('\x16')
    // codex 原生路径不读剪贴板插件，文本和图片都由 CLI 自行处理。
    expect(io.readText).not.toHaveBeenCalled()
    expect(io.readImage).not.toHaveBeenCalled()
  })

  // 探测不支持（codex 0.162.x 无 Windows 文本回退）时回退剪贴板读取路径。
  it('Running_KeyDownCtrlVCodexUnsupportedFallsBackToClipboard_002c — 探测不过时 codex Ctrl+V 走剪贴板路径', async () => {
    io.codexSupport = false
    useCliProfilesStore().profiles[0].cli = 'codex'
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    io.readText.mockResolvedValue('from-clipboard')

    const textarea = wrapper.find('textarea').element
    textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))
    await vi.advanceTimersByTimeAsync(0)

    expect(io.binding.reserveUserPaste).toHaveBeenCalledOnce()
    const produce = io.binding.reserveUserPaste.mock.calls[0][0] as () => Promise<Uint8Array>
    expect(new TextDecoder().decode(await produce())).toBe('from-clipboard')
  })

  // claude 没有 codex 那样的原生 Ctrl+V 剪贴板读取，仍走剪贴板插件读取路由。
  it('Running_KeyDownCtrlVClaudeRoutesViaClipboardRead_002b — claude Ctrl+V 经插件读剪贴板路由', async () => {
    useCliProfilesStore().profiles[0].cli = 'claude'
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    io.readText.mockResolvedValue('from-clipboard')

    const textarea = wrapper.find('textarea').element
    textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true }))
    await vi.advanceTimersByTimeAsync(0)

    expect(io.binding.reserveUserPaste).toHaveBeenCalledOnce()
    const produce = io.binding.reserveUserPaste.mock.calls[0][0] as () => Promise<Uint8Array>
    expect(new TextDecoder().decode(await produce())).toBe('from-clipboard')
  })

  it('Paste_EmptyCancels_003 — 空剪贴板取消预留且后续键盘仍可提交', async () => {
    useCliProfilesStore().profiles[0].cli = 'claude'
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
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

  // 探测通过时 codex 的 \x16 路径由 codex 自行读剪贴板，同时处理文本和图片，
  // 无需 CC Desk 分流；图片字节也不必转发。
  it('Running_KeyDownCtrlVCodexImageHandledNatively_004 — codex Ctrl+V 转发 \x16 后由 CLI 原生处理图片', async () => {
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
    const produce = io.binding.reserveUserPaste.mock.calls[0][0] as () => Promise<Uint8Array>
    expect(new TextDecoder().decode(await produce())).toBe('\x16')
    // codex 原生路径不走剪贴板插件读取，CC Desk 不需要读图片。
    expect(io.readImage).not.toHaveBeenCalled()
  })

  // 探测不支持时 codex 与 claude 相同：无文本走图片键位分流。
  it('Running_KeyDownCtrlVCodexUnsupportedImageFallback_004c — 探测不过时 codex 图片走键位分流', async () => {
    io.codexSupport = false
    useCliProfilesStore().profiles[0].cli = 'codex'
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
    expect(close).toHaveBeenCalledOnce()
  })

  // claude 没有原生 Ctrl+V 粘贴，无文本时走图片键位分流。
  it('Running_KeyDownCtrlVClaudeImageFallback_004b — claude Ctrl+V 无文本时发送图片粘贴键位', async () => {
    useCliProfilesStore().profiles[0].cli = 'claude'
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
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
    expect(close).toHaveBeenCalledOnce()
  })

  // 异步读剪贴板前必须同步预留输入序号，后续键盘输入才能排在粘贴之后。
  // claude 没有原生 Ctrl+V 剪贴板读取，仍走此路径。
  it('Paste_ReservesBeforeRead_005 — claude Ctrl+V 在 readText 完成前预留当前 intent', async () => {
    useCliProfilesStore().profiles[0].cli = 'claude'
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
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
    useCliProfilesStore().profiles[0].cli = 'claude'
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
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
    useCliProfilesStore().profiles[0].cli = 'claude'
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
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

  // claude 没有原生 Ctrl+V 粘贴，仍走剪贴板插件读取 + reserve 路径的写入失败诊断。
  it('Paste_WriteFailureReports_008 — 已产生 payload 的传输失败仍显示输入暂停诊断', async () => {
    useCliProfilesStore().profiles[0].cli = 'claude'
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
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

  // 拖入文件路径与键盘输入同语义：项目内路径相对化，含空格的外部路径加引号。
  it('Drop_PathsRelativizeAndQuote_009', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)

    const terminal = wrapper.vm as unknown as { dropPaths: (paths: string[]) => boolean }
    expect(terminal.dropPaths(['/repo/src/a.ts', '/outside/my file.txt'])).toBe(true)
    await vi.advanceTimersByTimeAsync(0)
    expect(io.binding.sendUserText).toHaveBeenCalledWith('src/a.ts "/outside/my file.txt"')
  })

  it.each(['\r', '\n', '\x1b', '\t'])('Drop_RejectWholeUnsafeBatch_013 %j', async control => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    expect((wrapper.vm as any).dropPaths(['/repo/safe.txt', '/repo/bad' + control + 'file'])).toBe(false)
    expect(io.binding.sendUserText).not.toHaveBeenCalled()
  })

  // 非激活 tab 的 drop 必须静默拒绝，避免后台终端收到不属于自己的输入。
  it('Drop_InactiveTabRejects_010', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: false } })
    await vi.advanceTimersByTimeAsync(0)

    const terminal = wrapper.vm as unknown as { dropPaths: (paths: string[]) => boolean }
    expect(terminal.dropPaths(['/repo/src/a.ts'])).toBe(false)
    await vi.advanceTimersByTimeAsync(0)
    expect(io.binding.sendUserText).not.toHaveBeenCalled()
  })
})

// 直接 rename 端口在编辑器安全未验证时拒绝，不产生输入或暂停诊断。
it('Rename_UnverifiedEditorRejectsBeforeWriter_011', async () => {
  useCliProfilesStore().profiles[0].cli = 'claude'
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await vi.advanceTimersByTimeAsync(0)
  io.binding.sendUserText.mockRejectedValueOnce(new Error('STALE_SESSION_ATTEMPT'))
  await expect((wrapper.vm as any).renameTab('New name')).rejects.toThrow('CLI_RENAME_EDITOR_UNVERIFIED')
  expect(io.binding.sendUserText).not.toHaveBeenCalled()
  expect(tabs.tab(tab.tabId)?.errorCode).toBeNull()
})

// 旧 writer 故障不构成编辑器安全证明；拒绝 rename 不尝试 writer。
it('Rename_WriterFaultCannotAuthorizeEditor_012', async () => {
  useCliProfilesStore().profiles[0].cli = 'claude'
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } }); await vi.advanceTimersByTimeAsync(0)
  io.binding.sendUserText.mockRejectedValueOnce(new Error('NATIVE_INPUT_PAUSED'))
  await expect((wrapper.vm as any).renameTab('New name')).rejects.toThrow('CLI_RENAME_EDITOR_UNVERIFIED')
  expect(io.binding.sendUserText).not.toHaveBeenCalled()
  expect(tabs.tab(tab.tabId)?.errorCode).toBeNull()
})
