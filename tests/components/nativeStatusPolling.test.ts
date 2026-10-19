import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, type VueWrapper } from '@vue/test-utils'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useCliProfilesStore } from '@/stores/cliProfiles'

/**
 * NativeCliTerminal 后台生命周期回归：Channel 只承载输出，observer 也不拥有 phase。
 * launched tab 切到后台后仍须保留低频状态同步，否则退出进程会永久显示 running。
 */
const io = vi.hoisted(() => ({
  statusReads: 0,
  phase: 'running' as 'running' | 'exited',
  starts: [] as any[],
  terms: [] as import('@xterm/xterm').Terminal[],
  observation: null as ((payload: import('@/types/hook').HookEventPayload) => void) | null,
  stop: vi.fn(),
}))
vi.mock('@xterm/xterm', async original => {
  const canvas = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  const actual = await original<typeof import('@xterm/xterm')>()
  canvas.mockRestore()
  return { ...actual, Terminal: class extends actual.Terminal {
    open(element: HTMLElement) {
      io.terms.push(this)
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
vi.mock('@/api/observer', () => ({ onNativeObservation: async (handler: typeof io.observation) => { io.observation = handler; return () => {} } }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMinimized: async () => false }) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  cliResize: vi.fn().mockResolvedValue(undefined), cliStop: io.stop,
  ptyInput: vi.fn().mockResolvedValue(undefined), ptyKill: vi.fn().mockResolvedValue(undefined), ptyResize: vi.fn().mockResolvedValue(undefined),
  logMessage: vi.fn().mockResolvedValue(undefined),
  onPtyOutput: async () => () => {}, onPtyExit: async () => () => {},
}))

let wrapper: VueWrapper | null = null
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks()
  io.statusReads = 0; io.phase = 'running'; io.observation = null
  io.terms.length = 0
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: {
    instanceId: 'test-backend',
    async invoke(command: string, input: any) {
      if (command === 'cli_start') {
        io.starts.push(input)
        return { instanceId: 'test-backend', requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, phase: 'running', revision: '1', failure: null }
      }
      if (command === 'cli_get_launch_status') {
        io.statusReads++
        // status 查询只携带 requestId；回显 cli_start 冻结请求里的原始 run 身份
        const original = io.starts.find(value => value.requestId === input.requestId)
        if (!original) throw new Error('UNKNOWN_REQUEST_ID')
        return { instanceId: 'test-backend', requestId: input.requestId, run: { runId: original.runId, generation: original.generation }, phase: io.phase, revision: '2', failure: null }
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

describe('NativeStatusPolling_BackgroundLife', () => {
  it.each(['claude', 'codex', 'unmount'] as const)('NativeTitleSignal_OwnedParser_002 %s', async mode => {
    const cli = mode === 'codex' ? 'codex' : 'claude'
    useCliProfilesStore().profiles[0].cli = cli
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli, projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    expect(tabs.tab(tab.tabId)?.status).toBe('running')
    io.terms[0].write('\x1b]0;✳ CLI renamed\x07')
    await vi.advanceTimersByTimeAsync(10)
    if (mode === 'unmount') { wrapper.unmount(); wrapper = null }
    await vi.advanceTimersByTimeAsync(600)
    expect(tabs.tab(tab.tabId)?.titleChangeRevision ?? 0).toBe(mode === 'claude' ? 1 : 0)
    expect(tabs.tab(tab.tabId)?.title).not.toContain('CLI renamed')
  })

  // active=false 时进程退出仍须通过 owner status 同步投影到 store。
  it('InactiveTab_TracksExit_001 — 后台 tab 保留生命周期同步并发布 exited', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    expect(tabs.tab(tab.tabId)?.status).toBe('running')

    await wrapper.setProps({ active: false })
    await vi.advanceTimersByTimeAsync(0)
    const readsBeforeExit = io.statusReads
    io.phase = 'exited'
    await vi.advanceTimersByTimeAsync(1500)
    expect(io.statusReads).toBeGreaterThan(readsBeforeExit)
    expect(tabs.tab(tab.tabId)?.status).toBe('exited')
  })
})
