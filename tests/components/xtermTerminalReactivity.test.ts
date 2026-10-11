import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import XTermTerminal from '@/components/XTermTerminal.vue'
import { useSessionStore } from '@/stores/session'

/**
 * XTermTerminal 响应式开销回归：
 * - terminalInstances 曾用 reactive(new Map()) 深度代理 xterm Terminal/FitAddon，
 *   PTY 输出热路径（每次 write）都经过两层 Proxy；修复后实例对象 markRaw，
 *   仅保留 Map 键集合的响应语义（模板 v-for 只依赖键）。
 * - createTerminal 内部 loadAddon 的 fit 实例曾被丢弃，调用方又 new 一个：
 *   修复后 createTerminal 返回/复用同一 fitAddon，每 tab 只有一个 FitAddon 实例。
 */
const io = vi.hoisted(() => ({
  terminals: [] as import('@xterm/xterm').Terminal[],
  fits: [] as unknown[],
  output: null as ((payload: { id: string; data: string }) => void) | null,
}))
vi.mock('@xterm/xterm', async original => {
  // 真实 xterm 类在模块加载时会探测 canvas；jsdom 未实现 getContext，按
  // unifiedTerminalParser.test.ts 的做法在工厂内临时置空再还原。
  const canvas = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  const actual = await original<typeof import('@xterm/xterm')>()
  canvas.mockRestore()
  return { ...actual, Terminal: class extends actual.Terminal {
    constructor(options: any) { super(options); io.terminals.push(this) }
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
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { constructor() { io.fits.push(this) } activate() {} dispose() {} fit() {} } }))
vi.mock('@tauri-apps/api/core', async original => ({ ...await original<object>(), Channel: class { onmessage: any } }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMinimized: async () => false }) }))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  ptyInput: vi.fn().mockResolvedValue(undefined), ptyKill: vi.fn().mockResolvedValue(undefined), ptyResize: vi.fn().mockResolvedValue(undefined),
  ptySpawn: async ({ id }: any) => ({ id }), logMessage: vi.fn().mockResolvedValue(undefined),
  onPtyOutput: async (callback: any) => { io.output = callback; return () => {} }, onPtyExit: async () => () => {},
}))

let wrapper: VueWrapper | null = null
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks()
  io.terminals.length = 0; io.fits.length = 0; io.output = null
  vi.stubGlobal('crypto', { randomUUID: () => 'owned-tab' })
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', (fn: FrameRequestCallback) => { fn(0); return 1 })
})
afterEach(() => { wrapper?.unmount(); wrapper = null; vi.unstubAllGlobals(); document.body.innerHTML = '' })

// 既有行为守护：Map 键集合变化仍驱动模板渲染（markRaw 不影响键响应）。
function mountWithRunningTab() {
  const sessions = useSessionStore()
  const id = sessions.createTab('/repo')
  sessions.setTabPty(id, 'owned-pty')
  sessions.setActiveTab(id)
  wrapper = mount(XTermTerminal, { props: { visible: true } })
  return { sessions, id }
}

describe('XTermTerminal_ReactivityOverhead', () => {
  it('FitAddon_SingleInstancePerTab_001 — 每 tab 只创建一个 FitAddon（createTerminal 复用，不再双重注册）', async () => {
    mountWithRunningTab(); await flushPromises()
    expect(io.terminals).toHaveLength(1)
    expect(io.fits).toHaveLength(1)
  })

  it('TerminalMap_KeysStillDriveTemplate_002 — 实例 markRaw 后键集合变化仍增删终端容器', async () => {
    const { sessions, id } = mountWithRunningTab(); await flushPromises()
    expect(wrapper!.element.querySelector(`.terminal-wrapper[data-tab="${id}"]`)).not.toBeNull()
    sessions.removeTab(id); await flushPromises()
    expect(wrapper!.element.querySelector(`.terminal-wrapper[data-tab="${id}"]`)).toBeNull()
  })

  it('InstancesMarkedRaw_SourceContract_003 — terminalInstances 写入处使用 markRaw 阻断深度代理', () => {
    const source = readFileSync('src/components/XTermTerminal.vue', 'utf8')
    expect(source).toMatch(/terminalInstances\.set\(tabId, markRaw\(\{/)
    expect(source).toMatch(/import \{[^}]*markRaw[^}]*\} from 'vue'/)
  })
})
