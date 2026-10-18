import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, h, nextTick, onMounted, onUnmounted } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import UnifiedTerminalHost from '@/components/workspace/UnifiedTerminalHost.vue'
import SessionItem from '@/components/sessions/SessionItem.vue'
import type { UnifiedSession } from '@/types/unifiedSession'
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
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); vi.unstubAllGlobals(); vi.restoreAllMocks(); clearMocks() })
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
  // 真实会话行的指针拖拽无需 DOM/Tauri drop 事件，也不应激活或恢复来源会话。
  it('Host_PointerReferenceDrop_012', async () => {
    const w = render(); await flushPromises()
    await w.setProps({ activeSessionId: 'native-tab:cx' }); await flushPromises()
    const source = mount(SessionItem, { attachTo: document.body, props: { session: {
      id: 'source', projectKey: '/repo', projectPath: '/repo', title: 'Source session', cli: 'claude',
      runtime: 'native-cli', processState: 'stopped', attentionState: 'none', lastActivityAt: 0,
      archived: false, opened: false, resumable: true, adapterSessionId: 'source', nativeSessionId: 'session-1',
    } satisfies UnifiedSession }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { TerminalView: Child, NativeCliTerminal: Child } } })
    wrappers.push(source)
    const host = w.get('[data-unified-terminal-host]')
    document.elementFromPoint = () => host.element
    try {
      firePointer(source.element, 'pointerdown', 10, 10)
      firePointer(window, 'pointermove', 100, 100)
      await nextTick()
      expect(host.classes()).toContain('drag-over')
      firePointer(window, 'pointerup', 100, 100)
      source.element.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }))
      firePointer(window, 'pointerup', 100, 100)
      await nextTick()
      expect(seen.refDrops).toEqual([['cx', 'Session: Source session (cli: claude, project: /repo, id: session-1)']])
      expect(host.classes()).not.toContain('drag-over')
      expect(source.emitted('activate')).toBeUndefined()
      expect(source.emitted('primary-action')).toBeUndefined()
      expect(seen.focus[seen.focus.length - 1]).toBe('cx')
      expect(seen.drops).toEqual([])
    } finally { delete (document as { elementFromPoint?: unknown }).elementFromPoint }
  })

  // 取消拖拽后没有延迟投递；离开终端、窗口失焦和来源卸载都释放当前拖拽。
  it.each(['outside', 'escape', 'cancel', 'blur', 'source-unmount', 'hidden', 'empty'] as const)('Host_CancelPointerReference_013 %s', async reason => {
    const w = render(); await flushPromises()
    const source = mount(SessionItem, { attachTo: document.body, props: { session: {
      id: 'source', projectKey: '/repo', projectPath: '/repo', title: 'Source session', cli: 'claude',
      runtime: 'native-cli', processState: 'stopped', attentionState: 'none', lastActivityAt: 0,
      archived: false, opened: false, resumable: true, adapterSessionId: 'source', nativeSessionId: 'session-1',
    } satisfies UnifiedSession }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { TerminalView: Child, NativeCliTerminal: Child } } })
    wrappers.push(source)
    const sourceElement = source.element
    const host = w.get('[data-unified-terminal-host]')
    document.elementFromPoint = () => host.element
    try {
      firePointer(source.element, 'pointerdown', 10, 10)
      firePointer(window, 'pointermove', 100, 100)
      await nextTick()
      expect(host.classes()).toContain('drag-over')
      if (reason === 'outside') { document.elementFromPoint = () => document.body; firePointer(window, 'pointermove', 200, 200) }
      else if (reason === 'escape') window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      else if (reason === 'cancel') firePointer(window, 'pointercancel', 100, 100)
      else if (reason === 'blur') window.dispatchEvent(new Event('blur'))
      else if (reason === 'source-unmount') { wrappers.splice(wrappers.indexOf(source), 1); source.unmount() }
      else if (reason === 'hidden') await w.setProps({ visible: false })
      else await w.setProps({ activeSessionId: null })
      firePointer(window, 'pointerup', 100, 100)
      sourceElement.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }))
      await nextTick()
      expect(seen.refDrops).toEqual([])
      expect(source.emitted('primary-action')).toBeUndefined()
      expect(host.classes()).not.toContain('drag-over')
    } finally { delete (document as { elementFromPoint?: unknown }).elementFromPoint }
  })

  // 普通点击不变；编辑器和行内按钮不会启动拖拽，也不会吞掉按下默认行为。
  it('Host_ClickAndRenameAdmission_014', async () => {
    const w = render(); await flushPromises()
    const source = mount(SessionItem, { attachTo: document.body, props: { session: {
      id: 'source', projectKey: '/repo', projectPath: '/repo', title: 'Source session', cli: 'claude',
      runtime: 'native-cli', processState: 'stopped', attentionState: 'none', lastActivityAt: 0,
      archived: false, opened: false, resumable: true, adapterSessionId: 'source', nativeSessionId: 'session-1',
    } satisfies UnifiedSession }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { TerminalView: Child, NativeCliTerminal: Child } } })
    wrappers.push(source)
    document.elementFromPoint = () => w.get('[data-unified-terminal-host]').element
    try {
      firePointer(source.element, 'pointerdown', 10, 10)
      firePointer(window, 'pointermove', 12, 12)
      firePointer(window, 'pointerup', 12, 12)
      await source.trigger('click')
      expect(source.emitted('primary-action')).toEqual([['source', 'resume']])
      expect(seen.refDrops).toEqual([])
      const buttonDown = firePointer(source.get('.session-overflow-trigger button').element, 'pointerdown', 10, 10)
      firePointer(window, 'pointermove', 100, 100)
      firePointer(window, 'pointerup', 100, 100)
      expect(buttonDown.defaultPrevented).toBe(false)
      expect(seen.refDrops).toEqual([])
      await source.setProps({ session: { ...source.props('session'), renameState: 'editing' } })
      const down = firePointer(source.get('input').element, 'pointerdown', 10, 10)
      firePointer(window, 'pointermove', 100, 100)
      firePointer(window, 'pointerup', 100, 100)
      expect(down.defaultPrevented).toBe(false)
      expect(seen.refDrops).toEqual([])
    } finally { delete (document as { elementFromPoint?: unknown }).elementFromPoint }
  })

  // 指针坐标已是 CSS 像素，150% 缩放不再除以 DPR；原生空 drop 不参与引用投递。
  it('Host_ReferenceCoordinates_015', async () => {
    const w = render(); await flushPromises()
    const source = mount(SessionItem, { attachTo: document.body, props: { session: {
      id: 'source', projectKey: '/repo', projectPath: '/repo', title: 'Source session', cli: 'codex',
      runtime: 'native-cli', processState: 'stopped', attentionState: 'none', lastActivityAt: 0,
      archived: false, opened: false, resumable: true, adapterSessionId: 'source', nativeSessionId: 'session-1',
    } satisfies UnifiedSession }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { TerminalView: Child, NativeCliTerminal: Child } } })
    wrappers.push(source)
    vi.stubGlobal('devicePixelRatio', 1.5)
    document.elementFromPoint = (x, y) => x === 150 && y === 120 ? w.element : document.body
    try {
      firePointer(source.element, 'pointerdown', 10, 10)
      firePointer(window, 'pointermove', 150, 120)
      firePointer(window, 'pointerup', 150, 120)
      drag.handler!({ payload: { type: 'drop', paths: [], position: { x: 150, y: 120 } } })
      expect(seen.refDrops).toEqual([['legacy', 'Session: Source session (cli: codex, project: /repo, id: session-1)']])
      drag.handler!({ payload: { type: 'drop', paths: ['C:/a.txt'], position: { x: 150, y: 120 } } })
      expect(seen.drops).toEqual([['legacy', ['C:/a.txt']]])
    } finally { delete (document as { elementFromPoint?: unknown }).elementFromPoint }
  })

})
function firePointer(target: EventTarget, type: string, clientX: number, clientY: number) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, buttons: type === 'pointerup' ? 0 : 1, clientX, clientY })
  Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: 'mouse' }, isPrimary: { value: true } })
  target.dispatchEvent(event)
  return event
}
