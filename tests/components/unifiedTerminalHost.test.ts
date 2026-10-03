import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, h, onMounted, onUnmounted } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import UnifiedTerminalHost from '@/components/workspace/UnifiedTerminalHost.vue'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { useAppStore } from '@/stores/app'
import { useShellStore } from '@/stores/shell'
import en from '@/i18n/locales/en'
const seen = { mounts: [] as string[], unmounts: [] as string[], fits: [] as string[], focus: [] as string[], stops: [] as any[] }
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
const Child = defineComponent({ props: { tabId: String, active: { type: Boolean, default: undefined }, visible: Boolean }, setup(props, { expose }) {
  const id = props.tabId ?? 'legacy'
  onMounted(() => seen.mounts.push(id)); onUnmounted(() => seen.unmounts.push(id))
  expose({ fitVisible: () => seen.fits.push(id), focus: () => seen.focus.push(id), stop: (attempt: any) => seen.stops.push([id, attempt]), recover: vi.fn(), startTab: vi.fn(), stopTab: vi.fn(), restartTab: vi.fn(), renameTab: vi.fn() })
  return () => h('div', { 'data-child': id, 'data-visible': String(props.active ?? props.visible) }, id)
} })
const wrappers: VueWrapper[] = []
beforeEach(() => { clearMocks(); mockIPC(command => command === 'get_app_config' ? { theme: 'light', terminalTheme: 'cc-box-light', language: 'en' } : undefined); setActivePinia(createPinia()); Object.values(seen).forEach(values => { values.length = 0 }); vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} }) })
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

})
