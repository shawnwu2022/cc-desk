import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { readFileSync } from 'node:fs'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import App from '@/App.vue'
import AppShell from '@/components/shell/AppShell.vue'
import PrimaryNav from '@/components/shell/PrimaryNav.vue'
import WorkspaceView from '@/components/workspace/WorkspaceView.vue'
import WorkspaceHeader from '@/components/workspace/WorkspaceHeader.vue'
import SessionsPanel from '@/components/sessions/SessionsPanel.vue'
import SidebarPanel from '@/components/sidebar/SidebarPanel.vue'
import { useShellStore, isCompatibilityEnabled } from '@/stores/shell'
import { useAppStore } from '@/stores/app'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import type { UnifiedSession } from '@/types/unifiedSession'

const host = vi.hoisted(() => ({
  callbacks: new Map<string, (...args: any[]) => void>(),
  cleanup: vi.fn(), minimize: vi.fn(), toggleMaximize: vi.fn(), close: vi.fn(),
  getConfig: vi.fn(), updateConfig: vi.fn(), runChecks: vi.fn(),
}))
vi.mock('@/utils/platform', () => ({ isMac: false, isWindows: true, ctrl: 'Ctrl' }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({
  minimize: host.minimize, toggleMaximize: host.toggleMaximize, close: host.close,
  isMaximized: async () => false, onResized: async () => host.cleanup,
}) }))
vi.mock('@/api/tauri', async (original) => ({ ...await original<object>(),
  getAppConfig: host.getConfig, updateAppConfig: host.updateConfig, runChecks: host.runChecks,
  onMenuSettings: async (callback: () => void) => { host.callbacks.set('settings', callback); return host.cleanup },
  onMenuShortcuts: async (callback: () => void) => { host.callbacks.set('shortcuts', callback); return host.cleanup },
  onConfigFontSize: async (callback: (size: number) => void) => { host.callbacks.set('font', callback); return host.cleanup },
  onOpenDirectory: async (callback: (path: string) => void) => { host.callbacks.set('directory', callback); return host.cleanup },
}))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
// Shell tests own presentation-only fixtures; production bootstrap/dispatch has
// behavioral integration coverage in unifiedWorkspaceRuntime.test.ts.
vi.mock('@/composables/useUnifiedWorkspaceRuntime', async () => {
  const { ref } = await import('vue')
  return { useUnifiedWorkspaceRuntime: () => ({ openSessions: ref([]), cliAvailability: ref({}), error: ref(null) }) }
})
const wrappers: VueWrapper[] = []
let i18n: ReturnType<typeof createI18n>
beforeEach(() => {
  vi.clearAllMocks(); host.callbacks.clear()
  host.getConfig.mockResolvedValue({ theme: 'dark', terminalTheme: 'cc-box-light', language: 'en', claudeEnvVars: { TEST: 'private' } })
  host.updateConfig.mockResolvedValue(undefined)
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1280 })
  setActivePinia(createPinia()); useProjectsStateStore().loaded = true
  i18n = createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
})
afterEach(() => {
  wrappers.splice(0).forEach(wrapper => wrapper.unmount())
  document.body.innerHTML = ''
  document.head.querySelectorAll('[data-test-shell]').forEach(style => style.remove())
  document.documentElement.removeAttribute('data-theme')
  vi.restoreAllMocks()
})
function render(component: any, options: Record<string, any> = {}) {
  const wrapper = mount(component, { attachTo: document.body, ...options,
    global: { plugins: [i18n], stubs: { SettingsView: { template: '<div data-settings-view><button @click="$emit(\'close\')">Close</button></div>' }, ...options.global?.stubs } } })
  wrappers.push(wrapper); return wrapper
}
function css(file: string, selector: string) {
  const style = document.createElement('style'); style.dataset.testShell = ''
  style.textContent = readFileSync(file, 'utf8').match(/<style[^>]*>([\s\S]*?)<\/style>/)![1]
  document.head.append(style)
  const rule = [...style.sheet!.cssRules].find(rule => rule instanceof CSSStyleRule && rule.selectorText === selector) as CSSStyleRule
  expect(rule, `Missing layout contract ${selector} in ${file}`).toBeDefined()
  return rule.style
}

// 删除任何第四个入口门禁、独立列、缩放阈值或请求转发会破坏以下行为。
describe('Unified application shell', () => {
  // 一级导航只有工作区、项目、设置，资源入口不进入全局导航。
  it('Shell_ThreePrimaryDestinations_001', async () => {
    const wrapper = render(AppShell)
    const buttons = wrapper.findAll('[data-primary-section]')
    expect(buttons.map(button => button.attributes('data-primary-section'))).toEqual(['workspace', 'projects', 'settings'])
    expect(buttons.map(button => button.attributes('aria-label'))).toEqual(['Workspace', 'Projects', 'Settings'])
    await buttons[1].trigger('click')
    expect(useShellStore().section).toBe('projects')
    expect(buttons[1].attributes('aria-current')).toBe('page')
    expect(wrapper.findAll('nav[aria-label="Primary navigation"]')).toHaveLength(1)
  })
  // 支持窗口的逻辑最小尺寸是1024×640，不以CSS最小宽度制造横向溢出。
  it('Shell_MinimumWindowContract_002', () => {
    const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'))
    expect(config.app.windows[0]).toMatchObject({ minWidth: 1024, minHeight: 640 })
    expect(config.app.windows[0].width).toBeGreaterThanOrEqual(1024)
    expect(config.app.windows[0].height).toBeGreaterThanOrEqual(640)
    const root = css('src/components/shell/AppShell.vue', '.app-shell')
    expect(root.getPropertyValue('min-width')).toBe('0')
    expect(root.getPropertyValue('overflow')).toBe('hidden')
  })
  // 1180为停靠边界，1179及1024使用覆盖抽屉，中央列宽度不减小。
  it('Shell_DrawerOverlayThreshold_003', async () => {
    const wrapper = render(AppShell, { slots: { context: '<p>Read-only context</p>' } })
    const store = useShellStore(); store.setViewportWidth(1180); store.drawerVisible = true
    await nextTick()
    expect(wrapper.find('[data-inline-context]').exists()).toBe(true)
    expect(wrapper.get('.shell-columns').attributes('style')).toContain('--context-column-width: 344px')
    store.setViewportWidth(1179); await nextTick(); await nextTick()
    expect(wrapper.find('[data-inline-context]').exists()).toBe(false)
    expect(wrapper.get('.shell-columns').attributes('style')).toContain('--context-column-width: 0px')
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Read-only context')
    store.setViewportWidth(1024); await nextTick()
    expect(store.responsiveMode).toBe('overlay')
  })
  // 900保留会话列，899默认收起；点击恢复按钮能够打开并再次收起。
  it('Shell_CompactSessionCollapse_004', async () => {
    const wrapper = render(AppShell, { slots: { default: '<div>Content</div>', sidebar: '<div>Tree</div>' } })
    const store = useShellStore(); store.setViewportWidth(900); await nextTick()
    expect(wrapper.find('[data-session-column]').isVisible()).toBe(true)
    store.setViewportWidth(899); await nextTick()
    expect(store.responsiveMode).toBe('compact')
    expect(wrapper.find('[data-session-column]').isVisible()).toBe(false)
    store.toggleSidebar(); await nextTick()
    expect(wrapper.find('[data-session-column]').isVisible()).toBe(true)
    store.toggleSidebar(); store.setViewportWidth(1024); await nextTick()
    expect(store.sidebarVisible).toBe(true)
  })
  // 宽度调整强制遵循会话240–360及资源300–420，非有限输入保留现值。
  it('Shell_ColumnWidthBounds_005', () => {
    const store = useShellStore()
    store.setSidebarWidth(200); expect(store.sidebarWidth).toBe(240)
    store.setSidebarWidth(500); expect(store.sidebarWidth).toBe(360)
    store.setDrawerWidth(200); expect(store.drawerWidth).toBe(300)
    store.setDrawerWidth(500); expect(store.drawerWidth).toBe(420)
    store.setSidebarWidth(NaN); store.setDrawerWidth(Infinity)
    expect(store.sidebarWidth).toBe(360); expect(store.drawerWidth).toBe(420)
  })
  // 所有全局列都允许收缩且全局禁止水平滚动，中央内容使用minmax(0,1fr)。
  it('Shell_NoGlobalHorizontalOverflow_006', () => {
    const grid = css('src/components/shell/AppShell.vue', '.shell-columns')
    expect(grid.getPropertyValue('grid-template-columns')).toBe('44px var(--session-column-width) minmax(0, 1fr) var(--context-column-width)')
    expect(grid.getPropertyValue('overflow')).toBe('hidden')
    for (const selector of ['.shell-main', '.shell-sidebar', '.shell-context']) {
      expect(css('src/components/shell/AppShell.vue', selector).getPropertyValue('min-width')).toBe('0')
    }
  })
  // 200字符项目与会话标题保留全文且只有标题省略，不挤占窗口控制。
  it('Shell_LongContextTitleEllipsis_007', () => {
    const title = '长'.repeat(200)
    const wrapper = render(WorkspaceHeader, { props: { projectTitle: title, sessionTitle: title } })
    expect(wrapper.get('[data-project-title]').text()).toBe(title)
    expect(wrapper.get('[data-session-title]').attributes('title')).toBe(title)
    const rule = css('src/components/workspace/WorkspaceHeader.vue', '.context-title')
    expect(rule.getPropertyValue('white-space')).toBe('nowrap')
    expect(rule.getPropertyValue('overflow')).toBe('hidden')
    expect(rule.getPropertyValue('text-overflow')).toBe('ellipsis')
    const bar = css('src/components/TitleBar.vue', '.win-app-title')
    expect(bar.getPropertyValue('text-overflow')).toBe('ellipsis')
  })
  // 切换一级视图不销毁工作区内容，也不改变统一会话选择。
  it('Shell_SectionPreservesWorkspace_008', async () => {
    const wrapper = render(App); await flushPromises()
    const workspace = wrapper.getComponent(WorkspaceView).element
    useUnifiedSessionsStore().activeSessionId = 'selected-session'
    useShellStore().navigate('settings'); await flushPromises()
    expect(wrapper.getComponent(WorkspaceView).element).toBe(workspace)
    expect(wrapper.getComponent(WorkspaceView).isVisible()).toBe(false)
    useShellStore().navigate('workspace'); await nextTick()
    expect(wrapper.getComponent(WorkspaceView).element).toBe(workspace)
    expect(useUnifiedSessionsStore().activeSessionId).toBe('selected-session')
  })
  // 正常根组件不挂载旧产品页，也不会触发旧Claude检查或隐式启动。
  it('Shell_NormalAppHasNoOldRuntime_009', async () => {
    const wrapper = render(App); await flushPromises()
    expect(wrapper.findComponent({ name: 'TerminalView' }).exists()).toBe(false)
    expect(wrapper.findComponent({ name: 'NativeCliWorkbench' }).exists()).toBe(false)
    expect(wrapper.findComponent({ name: 'LegacyCompatibilityApp' }).exists()).toBe(false)
    expect(wrapper.find('.native-workbench-toggle').exists()).toBe(false)
    expect(host.runChecks).not.toHaveBeenCalled()
    expect(wrapper.text()).not.toContain('Native CLI')
  })
  // 正常根组件挂载统一宿主；跨导航保留同一个实例。
  it('Shell_ConnectsUnifiedHost_018', async () => {
    const wrapper = render(App); await flushPromises()
    const terminal = wrapper.get('[data-unified-terminal-host]').element
    useShellStore().navigate('projects'); await nextTick()
    expect(wrapper.get('[data-unified-terminal-host]').element).toBe(terminal)
    useShellStore().navigate('settings'); await nextTick()
    expect(wrapper.get('[data-unified-terminal-host]').element).toBe(terminal)
  })
  // 兼容入口必须同时满足DEV与显式标志，生产环境单独设置标志无效。
  it('Shell_CompatibilityRequiresDev_010', () => {
    expect(isCompatibilityEnabled(false, '1')).toBe(false)
    expect(isCompatibilityEnabled(false, 'true')).toBe(false)
    expect(isCompatibilityEnabled(true, undefined)).toBe(false)
    expect(isCompatibilityEnabled(true, '0')).toBe(false)
    expect(isCompatibilityEnabled(true, '1')).toBe(true)
  })
  // 配置加载与GUI主题不等待某个CLI可用性；终端主题和选择保持独立。
  it('Shell_ConfigAndThemeIndependent_011', async () => {
    render(App); await flushPromises()
    const app = useAppStore(); const sessions = useUnifiedSessionsStore()
    expect(host.getConfig).toHaveBeenCalledOnce()
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
    expect(app.terminalTheme).toBe('cc-box-light')
    sessions.activeSessionId = 'selected'; app.theme = 'light'; await nextTick()
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
    expect(app.terminalTheme).toBe('cc-box-light')
    expect(sessions.activeSessionId).toBe('selected')
  })
  // 系统菜单直接路由到设置或快捷键小节，目录请求只进入项目视图。
  it('Shell_SafeMenuNavigation_012', async () => {
    render(App); await flushPromises()
    host.callbacks.get('settings')!(); await nextTick()
    expect(useShellStore().section).toBe('settings')
    host.callbacks.get('shortcuts')!(); await nextTick()
    expect(useShellStore().section).toBe('settings')
    host.callbacks.get('directory')!('/work/new-project'); await nextTick()
    expect(useShellStore().section).toBe('projects')
    expect(useShellStore().pendingRequest).toEqual({ kind: 'open-project', projectPath: '/work/new-project' })
    expect(useAppStore().cwd).toBe('')
  })
  // 无论CLI检查是否失败，导航与另一个CLI仍可用，错误以每CLI安全文案展示。
  it('Shell_PartialCliAvailability_013', async () => {
    const project = { projectKey: '/work/game', projectPath: '/work/game' }
    const wrapper = render(WorkspaceView, { props: { project, projectTitle: 'Game', cliAvailability: { claude: 'unavailable', codex: 'available' } } })
    expect(wrapper.find('[data-cli-unavailable="claude"]').text()).toContain('Claude Code')
    expect(wrapper.find('[data-cli-unavailable="codex"]').exists()).toBe(false)
    await wrapper.get('button[data-new-session]').trigger('click')
    expect(wrapper.emitted('new-session-request')).toEqual([[project]])
    expect(wrapper.find('.check-failed-overlay').exists()).toBe(false)
  })
  // Task9的新建请求完整转发；不能转成旧newSession启动事件。
  it('Shell_TypedTreeRequestBoundary_014', async () => {
    const sessions = useUnifiedSessionsStore()
    const session: UnifiedSession = { id: 'claude-1', projectKey: '/work/game', projectPath: '/work/game', cli: 'claude', runtime: 'native-cli', title: 'Work', processState: 'running', attentionState: 'none', lastActivityAt: 0, archived: false, resumable: true, adapterSessionId: 'a1' }
    sessions.sessions = [session]
    const wrapper = render(App); await flushPromises()
    await wrapper.get('[data-project-quick-action]').trigger('click'); await flushPromises()
    ;(document.querySelector('[data-item-id=codex]') as HTMLButtonElement).click(); await flushPromises()
    expect(useShellStore().pendingRequest).toEqual({ kind: 'new-session', project: { projectKey: '/work/game', projectPath: '/work/game', intent: 'codex' } })
    expect(wrapper.getComponent(SidebarPanel).emitted('newSession')).toBeUndefined()
    expect(wrapper.getComponent(SessionsPanel).emitted('new-session-request')).toEqual([[{ projectKey: '/work/game', projectPath: '/work/game', intent: 'codex' }]])
  })
  // Windows原生窗口操作保持可点，标题栏不再提供Native顶级入口。
  it('Shell_WindowControlsPreserved_015', async () => {
    const wrapper = render(AppShell); await flushPromises()
    await wrapper.get('[data-window-action="minimize"]').trigger('click')
    await wrapper.get('[data-window-action="maximize"]').trigger('click')
    await wrapper.get('[data-window-action="close"]').trigger('click')
    expect(host.minimize).toHaveBeenCalledOnce()
    expect(host.toggleMaximize).toHaveBeenCalledOnce()
    expect(host.close).toHaveBeenCalledOnce()
  })
  // resize监听实时更新逻辑视口，卸载后移除，不将操作系统缩放倍率当CSS像素。
  it('Shell_ViewportResizeCleanup_016', async () => {
    const wrapper = render(AppShell)
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 819 })
    window.dispatchEvent(new Event('resize')); await nextTick()
    expect(useShellStore().responsiveMode).toBe('compact')
    wrapper.unmount(); wrappers.splice(wrappers.indexOf(wrapper), 1)
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1280 })
    window.dispatchEvent(new Event('resize'))
    expect(useShellStore().responsiveMode).toBe('compact')
  })
  // 抽屉退出工作区时关闭显示，回到工作区恢复选项，不重建主要内容。
  it('Shell_ContextIsWorkspaceScoped_017', async () => {
    const wrapper = render(AppShell, { slots: { context: '<p>Context</p>' } })
    const store = useShellStore(); store.drawerVisible = true; await nextTick()
    store.navigate('projects'); await nextTick()
    expect(wrapper.find('[data-inline-context]').exists()).toBe(false)
    store.navigate('workspace'); await nextTick()
    expect(wrapper.find('[data-inline-context]').exists()).toBe(true)
  })
  // 中英文导航复用同一组三个控件，不把本地化标签变成额外入口。
  it('Shell_LocalizedNavigation_018', async () => {
    const wrapper = render(PrimaryNav)
    ;(i18n.global.locale as any).value = 'zh'; await nextTick()
    expect(wrapper.findAll('[data-primary-section]').map(button => button.attributes('aria-label'))).toEqual(['工作区', '项目', '设置'])
  })
  // 会话栏隐藏时Escape不会改变其桌面显示偏好。
  it('Shell_HiddenPanelIgnoresEscape_019', async () => {
    render(App); await flushPromises()
    useShellStore().navigate('settings'); await flushPromises()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    useShellStore().navigate('workspace'); await nextTick()
    expect(useShellStore().sidebarVisible).toBe(true)
  })
  // 从当前项目标题发出的新建请求只有项目身份，不携带整个目录投影。
  it('Shell_HeaderRequestExactIdentity_020', async () => {
    useUnifiedSessionsStore().sessions = [{ id: 's1', projectKey: '/work/game', projectPath: '/work/game', cli: 'codex', runtime: 'native-cli', title: 'Work', processState: 'running', attentionState: 'none', lastActivityAt: 0, archived: false, resumable: true, adapterSessionId: 'a1' }]
    const wrapper = render(App); await flushPromises()
    await wrapper.get('button[data-new-session]').trigger('click')
    expect(useShellStore().pendingRequest).toEqual({ kind: 'new-session', project: { projectKey: '/work/game', projectPath: '/work/game' } })
  })

  // 面板持久挂载时，其传送到body的归档弹层必须随活动工作区退出而关闭。
  it.each(['settings-menu', 'shortcuts-menu', 'settings-store', 'projects-store'])(
    'Shell_ArchiveSurfaceOwnership_021 %s', async destination => {
      const sessions = useUnifiedSessionsStore()
      const normal: UnifiedSession = { id: 'normal', projectKey: '/work/game', projectPath: '/work/game', cli: 'codex', runtime: 'native-cli', title: 'Work', processState: 'running', attentionState: 'none', lastActivityAt: 0, archived: false, resumable: true, adapterSessionId: 'a1' }
      sessions.sessions = [normal, { ...normal, id: 'archived', archived: true, processState: 'stopped', adapterSessionId: 'a2' }]
      sessions.activeSessionId = normal.id
      const wrapper = render(App); await flushPromises()
      const workspaceHost = wrapper.getComponent(WorkspaceView).element
      const panel = wrapper.getComponent(SessionsPanel)
      await panel.get('.project-main').trigger('click')
      await panel.get('.search-input').setValue('Work')
      await panel.get('[data-view-archived]').trigger('click'); await nextTick(); await nextTick()
      expect(document.querySelector('[role="dialog"]')).not.toBeNull()
      if (destination === 'settings-menu') host.callbacks.get('settings')!()
      else if (destination === 'shortcuts-menu') host.callbacks.get('shortcuts')!()
      else useShellStore().navigate(destination === 'settings-store' ? 'settings' : 'projects')
      await flushPromises()
      expect(document.querySelector('[role="dialog"]')).toBeNull()
      const destinationControl = destination === 'projects-store'
        ? wrapper.get('.projects-content button').element : wrapper.get('[data-settings-view] button').element
      ;(destinationControl as HTMLElement).focus()
      expect(document.activeElement).toBe(destinationControl)
      useShellStore().navigate('workspace'); await nextTick()
      expect(wrapper.getComponent(WorkspaceView).element).toBe(workspaceHost)
      expect(sessions.activeSessionId).toBe(normal.id)
      expect(panel.get('.search-input').element).toHaveProperty('value', 'Work')
      await panel.get('.search-input').setValue('')
      expect(panel.findAll('.session-item')).toHaveLength(1)
      expect(document.querySelector('[role="dialog"]')).toBeNull()
    },
  )

  // 项目与会话行菜单传送到body，不能依赖指针事件来结束隐藏面板的菜单。
  it.each(['project', 'session'])('Shell_MenuSurfaceOwnership_022 %s', async owner => {
    const sessions = useUnifiedSessionsStore()
    sessions.sessions = [{ id: 's1', projectKey: '/work/game', projectPath: '/work/game', cli: 'codex', runtime: 'native-cli', title: 'Work', processState: 'running', attentionState: 'none', lastActivityAt: 0, archived: false, resumable: true, adapterSessionId: 'a1' }]
    sessions.activeSessionId = 's1'
    const wrapper = render(App); await flushPromises()
    const workspaceHost = wrapper.getComponent(WorkspaceView).element
    const panel = wrapper.getComponent(SessionsPanel)
    await panel.get('.project-main').trigger('click')
    await panel.get(owner === 'project' ? '.project-overflow-trigger button' : '.session-overflow-trigger button').trigger('click')
    await nextTick(); await nextTick()
    expect(document.querySelector('[role="menu"]')).not.toBeNull()
    host.callbacks.get(owner === 'project' ? 'shortcuts' : 'settings')!()
    await flushPromises()
    expect(document.querySelector('[role="menu"]')).toBeNull()
    const settingsControl = wrapper.get('[data-settings-view] button').element as HTMLElement
    settingsControl.focus(); expect(document.activeElement).toBe(settingsControl)
    useShellStore().navigate('workspace'); await nextTick()
    expect(wrapper.getComponent(WorkspaceView).element).toBe(workspaceHost)
    expect(panel.findAll('.session-item')).toHaveLength(1)
    expect(sessions.activeSessionId).toBe('s1')
    expect(document.querySelector('[role="menu"]')).toBeNull()
  })

})
