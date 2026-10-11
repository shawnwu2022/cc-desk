import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, type VueWrapper } from '@vue/test-utils'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useCliProfilesStore } from '@/stores/cliProfiles'

/**
 * NativeCliTerminal 后台轮询回归：
 * 每个 launched tab 曾持有独立 1500ms statusTimer，切到后台（active=false）后
 * 依旧持续调用 cli_get_launch_status。修复后：非活跃 tab 停掉 timer，
 * 重新激活时立即 syncStatus 并重启轮询。
 */
const io = vi.hoisted(() => ({
  statusReads: 0,
  starts: [] as any[],
  observation: null as ((payload: import('@/types/hook').HookEventPayload) => void) | null,
  stop: vi.fn(),
}))
vi.mock('@xterm/xterm', async original => {
  const canvas = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  const actual = await original<typeof import('@xterm/xterm')>()
  canvas.mockRestore()
  return { ...actual, Terminal: class extends actual.Terminal {
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
  io.statusReads = 0; io.observation = null
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

describe('NativeStatusPolling_BackgroundPause', () => {
  it('InactiveTab_StopsPolling_001 — active=false 停止轮询，重新激活立即同步并重启', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    wrapper = mount(NativeCliTerminal, { props: { tabId: tab.tabId, active: true } })
    await vi.advanceTimersByTimeAsync(0)
    expect(tabs.tab(tab.tabId)?.status).toBe('running')

    // 活跃 tab 保持 1500ms 轮询
    await vi.advanceTimersByTimeAsync(1500)
    const readsWhileActive = io.statusReads
    expect(readsWhileActive).toBeGreaterThan(0)

    // 切后台：轮询必须停摆
    await wrapper.setProps({ active: false })
    await vi.advanceTimersByTimeAsync(0)
    const readsAtPause = io.statusReads
    await vi.advanceTimersByTimeAsync(4500)
    expect(io.statusReads).toBe(readsAtPause)

    // 重新激活：立即补偿同步，且轮询恢复
    await wrapper.setProps({ active: true })
    await vi.advanceTimersByTimeAsync(0)
    expect(io.statusReads).toBeGreaterThan(readsAtPause)
    const readsAfterResume = io.statusReads
    await vi.advanceTimersByTimeAsync(1500)
    expect(io.statusReads).toBeGreaterThan(readsAfterResume)
  })
})
