import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, h, nextTick, onMounted, onUnmounted } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import UnifiedTerminalHost from '@/components/workspace/UnifiedTerminalHost.vue'
import { SESSION_DRAG_MIME } from '@/utils/sessionReference'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { useAppStore } from '@/stores/app'
import { useShellStore } from '@/stores/shell'
import en from '@/i18n/locales/en'
const seen = { mounts: [] as string[], unmounts: [] as string[], fits: [] as string[], focus: [] as string[], stops: [] as any[], drops: [] as [string, string[]][], refDrops: [] as [string, string][] }
const drag = vi.hoisted(() => ({ handler: null as null | ((event: any) => void) }))
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async (handler: (event: any) => void) => { drag.handler = handler; return () => { drag.handler = null } } }),
}))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isFocused: async () => true, onFocusChanged: async () => () => {}, requestUserAttention: async () => {} }) }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
const Child = defineComponent({ props: { tabId: String, active: { type: Boolean, default: undefined }, visible: Boolean }, setup(props, { expose }) {
  const id = props.tabId ?? 'legacy'
  onMounted(() => seen.mounts.push(id)); onUnmounted(() => seen.unmounts.push(id))
  expose({ fitVisible: () => seen.fits.push(id), focus: () => seen.focus.push(id), stop: (attempt: any) => seen.stops.push([id, attempt]), recover: vi.fn(), startTab: vi.fn(), stopTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn(), dropPaths: (paths: string[]) => { seen.drops.push([id, paths]) }, dropSessionReference: (text: string) => { seen.refDrops.push([id, text]) } })
  return () => h('div', { 'data-child': id, 'data-visible': String(props.active ?? props.visible) }, id)
} })
const wrappers: VueWrapper[] = []
beforeEach(() => { clearMocks(); mockIPC(command => command === 'get_app_config' ? { theme: 'light', terminalTheme: 'cc-box-light', language: 'en' } : undefined); setActivePinia(createPinia()); Object.values(seen).forEach(values => { values.length = 0 }); drag.handler = null; vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} }) })
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); vi.unstubAllGlobals(); clearMocks() })
const sessions = [
  { id: 'legacy-tab:old-a', adapterSessionId: 'old-a', runtime: 'legacy-claude' },
  { id: 'legacy-tab:old-b', adapterSessionId: 'old-b', runtime: 'legacy-claude' },
  { id: 'native-tab:cx', adapterSessionId: 'cx', runtime: 'native-cli' },
  { id: 'native-tab:cc', adapterSessionId: 'cc', runtime: 'native-cli' },
]
function render() {
  const w = mount(UnifiedTerminalHost, { props: { activeSessionId: sessions[0].id, sessions: sessions as any, visible: true }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { TerminalView: Child, NativeCliTerminal: Child } } }); wrappers.push(w); return w
}
describe('Unified terminal host', () => {
  // 两个Legacy会话共用一个聚合器，混合切换及导航不会重建终端。
  it('Host_KeepsSingleOwnersMounted_001', async () => {
    const w = render(); await flushPromises()
    expect(seen.mounts.sort()).toEqual(['cc', 'cx', 'legacy'])
    await w.setProps({ activeSessionId: 'native-tab:cx' }); await flushPromises()
    expect(w.get('[data-child="legacy"]').attributes('data-visible')).toBe('false')
    expect(w.get('[data-child="cx"]').attributes('data-visible')).toBe('true')
    await w.setProps({ visible: false }); await flushPromises()
    expect(w.findAll('[data-visible="true"]')).toHaveLength(0)
    await w.setProps({ visible: true, activeSessionId: 'legacy-tab:old-b' }); await flushPromises()
    expect(seen.mounts).toHaveLength(3); expect(seen.unmounts).toEqual([])
  })
  // 会话栏/资源栏尺寸变更只通知当前终端测量，后台子组件保留延迟测量标记。
  it('Host_LayoutFitsVisibleOnly_002', async () => {
    const w = render(); await w.setProps({ activeSessionId: 'native-tab:cc' }); await flushPromises()
    seen.fits.length = 0; seen.focus.length = 0
    const shell = useShellStore(); shell.setSidebarWidth(330); shell.setDrawerWidth(380); shell.drawerVisible = true; await flushPromises()
    expect(seen.fits).toEqual(['cc']); expect(seen.focus).toEqual([])
    await w.setProps({ visible: false }); await flushPromises(); seen.fits.length = 0
    shell.setSidebarWidth(280); await flushPromises(); expect(seen.fits).toEqual([])
  })
  // 生命周期根据指定会话路由，不读取当前选中会话来替代目标。
  it('Host_RoutesExactNativeTarget_003', async () => {
    const w = render(); await flushPromises()
    const attempt = { requestId: 'request', runId: 'run', generation: 9 }
    await (w.vm as any).stopNative('cx', attempt)
    expect(seen.stops).toEqual([['cx', attempt]])
    await expect((w.vm as any).stopNative('missing', attempt)).rejects.toThrow('NATIVE_TERMINAL_NOT_READY')
  })
  // GUI主题/密度和会话栏保存只影响布局，所有runtime宿主与终端表面保持原有身份。
  it('Host_GuiPreferencesPreserveOwners_004', async () => {
    const w = render(); await w.setProps({ activeSessionId: 'native-tab:cc' }); await flushPromises()
    const surface = w.get('[data-unified-terminal-host]').attributes('style')
    const children = w.findAll('[data-child]').map(child => child.element)
    const app = useAppStore(); await app.setTheme('dark'); await app.setGuiDensity('compact'); await app.setSidebarWidth(320); await flushPromises()
    expect(w.findAll('[data-child]').map(child => child.element)).toEqual(children)
    expect(w.get('[data-unified-terminal-host]').attributes('style')).toBe(surface)
    expect(w.props('activeSessionId')).toBe('native-tab:cc'); expect(seen.mounts).toHaveLength(3); expect(seen.unmounts).toEqual([])
    expect(seen.stops).toEqual([])
  })
  // 拖放由 host 统一监听并只分发给当前活跃会话的终端，两个 runtime 行为一致。
  it('Host_DropDispatchesToActiveTerminal_005', async () => {
    const w = render(); await flushPromises()
    expect(drag.handler).not.toBeNull()
    const paths = ['E:/repo/src/a.ts']
    await w.setProps({ activeSessionId: 'native-tab:cx' }); await flushPromises()
    drag.handler!({ payload: { type: 'drop', paths } })
    expect(seen.drops).toEqual([['cx', paths]])
    await w.setProps({ activeSessionId: 'legacy-tab:old-b' }); await flushPromises()
    drag.handler!({ payload: { type: 'drop', paths } })
    expect(seen.drops).toEqual([['cx', paths], ['legacy', paths]])
  })
  // host 不可见时 drop 不分发，拖拽悬停态也不进入激活视觉。
  it('Host_DropIgnoredWhenHidden_006', async () => {
    const w = render(); await flushPromises()
    await w.setProps({ visible: false }); await flushPromises()
    drag.handler!({ payload: { type: 'drop', paths: ['E:/repo/x'] } })
    expect(seen.drops).toEqual([])
    drag.handler!({ payload: { type: 'enter', paths: [] } })
    expect(w.find('[data-unified-terminal-host]').classes()).not.toContain('drag-over')
  })
  // 应用内会话引用拖拽在拖入时高亮、drop 时只分发给当前活跃终端，drop 后清除高亮。
  it('Host_RefDragHighlightsAndDropsToActive_007', async () => {
    const w = render(); await flushPromises()
    await w.setProps({ activeSessionId: 'native-tab:cx' }); await flushPromises()
    const host = w.get('[data-unified-terminal-host]')
    fireDrag(host, 'dragenter', { types: [SESSION_DRAG_MIME] })
    await nextTick()
    expect(host.classes()).toContain('drag-over')
    const payload = JSON.stringify({ title: 'Fix login bug', cli: 'claude', projectPath: 'E:/repo/app', sessionId: 'native-1' })
    const event = fireDrag(host, 'drop', { types: [SESSION_DRAG_MIME], data: payload })
    await nextTick()
    expect(event.defaultPrevented).toBe(true)
    expect(seen.refDrops).toEqual([['cx', 'Session: Fix login bug (cli: claude, project: E:/repo/app, id: native-1)']])
    expect(seen.drops).toEqual([])
    expect(host.classes()).not.toContain('drag-over')
  })
  // 非 MIME 拖拽与非法载荷不分发；拖拽离开宿主后清除悬停态。
  it('Host_RefDragForeignOrInvalidIgnored_008', async () => {
    const w = render(); await flushPromises()
    const host = w.get('[data-unified-terminal-host]')
    fireDrag(host, 'dragover', { types: ['text/plain'] })
    await nextTick()
    expect(host.classes()).not.toContain('drag-over')
    fireDrag(host, 'dragover', { types: [SESSION_DRAG_MIME] })
    await nextTick()
    expect(host.classes()).toContain('drag-over')
    fireDrag(host, 'dragleave', { types: [SESSION_DRAG_MIME], relatedTarget: host.element })
    await nextTick()
    expect(host.classes()).toContain('drag-over')
    fireDrag(host, 'dragleave', { types: [SESSION_DRAG_MIME], relatedTarget: null })
    await nextTick()
    expect(host.classes()).not.toContain('drag-over')
    fireDrag(host, 'drop', { types: [SESSION_DRAG_MIME], data: 'not json' })
    expect(seen.refDrops).toEqual([])
  })
  // host 不可见时引用 drop 同样不分发。
  it('Host_RefDropIgnoredWhenHidden_009', async () => {
    const w = render(); await flushPromises()
    await w.setProps({ visible: false }); await flushPromises()
    fireDrag(w.get('[data-unified-terminal-host]'), 'drop', {
      types: [SESSION_DRAG_MIME],
      data: JSON.stringify({ title: 't', cli: 'codex', projectPath: '/p', sessionId: 's1' }),
    })
    expect(seen.refDrops).toEqual([])
  })

})
// jsdom 的 DragEvent/dataTransfer 支持不完整，用普通 Event 附加受控 dataTransfer 桩。
function fireDrag(wrapper: { element: Element }, type: string, init: { types?: string[]; data?: string; relatedTarget?: Node | null }) {
  const event = new Event(type, { bubbles: true, cancelable: true }) as any
  event.dataTransfer = {
    types: init.types ?? [],
    getData: (mime: string) => (mime === SESSION_DRAG_MIME ? init.data ?? '' : ''),
    dropEffect: 'none',
  }
  if (init.relatedTarget !== undefined) event.relatedTarget = init.relatedTarget
  wrapper.element.dispatchEvent(event)
  return event
}
