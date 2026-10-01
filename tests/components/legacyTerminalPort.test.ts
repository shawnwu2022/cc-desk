import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import TerminalView from '@/components/TerminalView.vue'
import { useAppStore } from '@/stores/app'
import { useHookStore } from '@/stores/hook'
import type { HookEventPayload } from '@/types/hook'
import { useSessionStore } from '@/stores/session'
import en from '@/i18n/locales/en'

const host = vi.hoisted(() => ({
  start: vi.fn(), stop: vi.fn(), restart: vi.fn(), rename: vi.fn(), recover: vi.fn(), fit: vi.fn(), focus: vi.fn(),
  history: vi.fn(), config: vi.fn(), cleanup: vi.fn(), hook: null as ((payload: HookEventPayload) => void) | null,
}))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
vi.mock('@tauri-apps/api/window', () => ({ UserAttentionType: { Informational: 2 }, getCurrentWindow: () => ({
  isFocused: async () => true, onFocusChanged: async () => host.cleanup,
  requestUserAttention: async () => {}, setTitle: async () => {},
}) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), getSessions: host.history, getProjectConfig: host.config, onHookEvent: async (callback: (payload: HookEventPayload) => void) => { host.hook = callback; return host.cleanup } }))
const Child = defineComponent({ props: { visible: Boolean }, setup(props, { expose }) {
  expose({ startTab: host.start, stopTab: host.stop, restartTab: host.restart, renameTab: host.rename,
    recover: host.recover, fitVisible: host.fit, focus: host.focus })
  return () => h('div', { 'data-legacy-terminal': String(props.visible) })
} })
const wrappers: VueWrapper[] = []
beforeEach(() => {
  vi.clearAllMocks(); setActivePinia(createPinia()); host.hook = null
  vi.stubGlobal('crypto', { randomUUID: () => 'owned-tab' })
  host.start.mockResolvedValue({ ok: true }); host.stop.mockResolvedValue(undefined)
  host.history.mockResolvedValue([]); host.config.mockResolvedValue({})
})
afterEach(() => { wrappers.splice(0).forEach(wrapper => wrapper.unmount()); vi.unstubAllGlobals() })
function render(visible = true) {
  const wrapper = mount(TerminalView, { props: { visible }, global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { XTermTerminal: Child },
  } }); wrappers.push(wrapper); return wrapper
}
describe('Legacy terminal port', () => {
  // 未传旧embedded标志也只能挂载聚合器，挂载与旧事件不能启动CLI或改写会话。
  it('LegacyPort_MountIsInert_001', async () => {
    useAppStore().cwd = '/repo'
    const wrapper = render(); await flushPromises()
    expect(wrapper.findAll('nav')).toHaveLength(0)
    expect(wrapper.findAll('button')).toHaveLength(0)
    expect(wrapper.find('[data-legacy-terminal]').exists()).toBe(true)
    expect(host.history).not.toHaveBeenCalled(); expect(host.config).not.toHaveBeenCalled()
    window.dispatchEvent(new Event('terminal:newSession'))
    window.dispatchEvent(new Event('terminal:restartSession'))
    await flushPromises()
    expect(useSessionStore().tabs.size).toBe(0)
    expect(host.start).not.toHaveBeenCalled(); expect(host.restart).not.toHaveBeenCalled()
  })
  // 统一宿主显式端口保留失败传播、精确tab参数和等待停止完成语义。
  it('LegacyPort_PreservesOwnedCalls_002', async () => {
    const wrapper = render(); await flushPromises(); const port = wrapper.vm as any
    await port.startTab('owned-tab'); expect(host.start).toHaveBeenCalledWith('owned-tab')
    host.start.mockResolvedValue({ ok: false, error: '/private failure' })
    await expect(port.startTab('failed-tab')).rejects.toThrow('LEGACY_LAUNCH_FAILED')
    let stopped!: () => void
    host.stop.mockImplementation(() => new Promise<void>(resolve => { stopped = resolve }))
    let finished = false; const stopping = port.stopTab('owned-tab').then(() => { finished = true })
    await flushPromises(); expect(finished).toBe(false)
    expect(host.stop).toHaveBeenCalledWith('owned-tab'); stopped(); await stopping
    await port.restartTab('owned-tab'); expect(host.restart).toHaveBeenCalledWith('owned-tab')
    await port.renameTab('owned-tab', 'Title'); expect(host.rename).toHaveBeenCalledWith('owned-tab', 'Title')
    await port.recover(); expect(host.recover).toHaveBeenCalledOnce()
  })
  // 隐藏只改变可见性；终端保持同一实例，不触发启动/停止或抢焦点。
  it('LegacyPort_HiddenKeepsOwner_003', async () => {
    const wrapper = render(); await flushPromises(); const element = wrapper.get('[data-legacy-terminal]').element
    await wrapper.setProps({ visible: false }); (wrapper.vm as any).focus()
    expect(host.focus).not.toHaveBeenCalled()
    await wrapper.setProps({ visible: true }); (wrapper.vm as any).focus()
    expect(host.focus).toHaveBeenCalledOnce()
    expect(wrapper.get('[data-legacy-terminal]').element).toBe(element)
    expect(host.start).not.toHaveBeenCalled(); expect(host.stop).not.toHaveBeenCalled()
  })
  // 实际状态订阅继续绑定PTY；隐藏的结束提示在重新显示时确认，卸载后不再接收事件。
  it('LegacyPort_PreservesMonitor_004', async () => {
    const sessions = useSessionStore(); const id = sessions.createTab('/repo')
    sessions.setTabPty(id, 'owned-pty'); sessions.setActiveTab(id)
    await useHookStore().init()
    const wrapper = render(false); await flushPromises()
    host.hook!({ ptyId: 'owned-pty', sessionId: 'session-id', eventName: 'SessionStart', state: 'idle', timestamp: 1,
      detail: { type: 'sessionStart', data: { model: 'model' } } })
    expect(sessions.tabs.get(id)?.sessionId).toBe('session-id')
    host.hook!({ ptyId: 'owned-pty', sessionId: 'session-id', eventName: 'PreToolUse', state: 'tool_executing', timestamp: 2,
      detail: { type: 'preToolUse', data: {} } })
    host.hook!({ ptyId: 'owned-pty', sessionId: 'session-id', eventName: 'Stop', state: 'idle', timestamp: 3,
      detail: { type: 'stop', data: {} } })
    expect(sessions.tabs.get(id)?.pending).toBe(true)
    await wrapper.setProps({ visible: true }); expect(sessions.tabs.get(id)?.pending).toBe(false)
    wrapper.unmount(); wrappers.splice(wrappers.indexOf(wrapper), 1)
    host.hook!({ ptyId: 'owned-pty', sessionId: 'session-id', eventName: 'UserPromptSubmit', state: 'thinking', timestamp: 4,
      detail: { type: 'userPromptSubmit', data: {} } })
    expect(sessions.tabs.get(id)?.working).toBe(false)
  })

  // PTY启动保留项目发现；新会话刷新自己的历史，恢复不会重复读取，迟到PTY事件被忽略。
  it.each([false, true])('LegacyPort_AdoptsStartedProject_005: resume=%s', async resume => {
    const app = useAppStore(); app.cwd = '/other'
    const sessions = useSessionStore(); const id = sessions.createTab('/owned', resume ? { sessionId: 'saved' } : {})
    sessions.setTabPty(id, 'owned-pty')
    const wrapper = render(); await flushPromises()
    const child = wrapper.getComponent(Child)
    child.vm.$emit('pty-started', id, 'stale-pty'); await flushPromises()
    expect(app.cachedProjects).toEqual([]); expect(host.history).not.toHaveBeenCalled()
    child.vm.$emit('pty-started', id, 'owned-pty'); await flushPromises()
    expect(app.cachedProjects.map(project => project.path)).toEqual(['/owned'])
    if (resume) expect(host.history).not.toHaveBeenCalled()
    else { expect(host.history).toHaveBeenCalledOnce(); expect(host.history.mock.calls[0][0]).toBe('/owned') }
    expect(app.cwd).toBe('/other')
  })

})
