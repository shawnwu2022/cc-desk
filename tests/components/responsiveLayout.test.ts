import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { h, defineComponent } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { readFileSync } from 'node:fs'
import AppShell from '@/components/shell/AppShell.vue'
import ProjectNode from '@/components/sessions/ProjectNode.vue'
import SettingsView from '@/components/settings/SettingsView.vue'
import { useShellStore } from '@/stores/shell'
import { useSidebarStore } from '@/stores/sidebar'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import type { UnifiedSession, UnifiedProjectGroup } from '@/types/unifiedSession'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isMaximized: async () => false, onResized: async () => () => {} }) }))
const wrappers: VueWrapper[] = []
beforeEach(() => { setActivePinia(createPinia()) })
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); document.body.innerHTML = ''; document.documentElement.removeAttribute('data-theme'); vi.restoreAllMocks(); vi.unstubAllGlobals() })
// 把逻辑 viewport 与 DPR 作为独立输入；jsdom 不证明 Windows 实际缩放或像素几何。
const matrix = [[1024, 640], [1280, 720], [1366, 768], [1440, 900], [1920, 1080]].flatMap(([width, height]) => [1, 1.25, 1.5].flatMap(scale => ['en', 'zh'].flatMap(locale => ['light', 'dark'].map(theme => ({ width, height, scale, locale, theme, label: `${width}_${scale}_${locale}_${theme}` })))))
it.each(matrix)('Layout_Matrix_001_$label', async ({ width, height, scale, locale, theme }) => {
  const logicalWidth = width
  vi.stubGlobal('innerWidth', logicalWidth); vi.stubGlobal('innerHeight', height); vi.stubGlobal('devicePixelRatio', scale); document.documentElement.dataset.theme = theme
  const row: UnifiedSession = { id: 'long', runtime: 'native-cli', cli: 'codex', adapterSessionId: 'tab', title: 'T'.repeat(200), projectKey: '/repo', projectPath: 'C:/'+ 'directory/'.repeat(30), processState: 'running', attentionState: 'none', archived: false, resumable: true, lastActivityAt: Date.now() - 60_000 }
  const project: UnifiedProjectGroup = { projectKey: '/repo', projectPath: row.projectPath, name: 'P'.repeat(80), sessions: [row], pinned: true, hidden: false, runningCount: 1, needsUserCount: 0, lastActivityAt: row.lastActivityAt }
  const shell = useShellStore(); shell.drawerVisible = true
  const w = mount(AppShell, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] }, slots: { sidebar: () => h(ProjectNode, { project, expanded: true }), default: '<button data-host-slot>Terminal host slot</button>', context: '<button data-context-content>Resource content</button>' } }); wrappers.push(w); await flushPromises()
  const mode = logicalWidth < 900 ? 'compact' : logicalWidth < 1180 ? 'overlay' : 'wide'
  expect(w.attributes('data-responsive-mode')).toBe(mode)
  const columns = w.get('.shell-columns').element as HTMLElement
  expect(columns.style.getPropertyValue('--context-column-width')).toBe(mode === 'wide' ? '344px' : '0px')
  expect(columns.style.getPropertyValue('--session-column-width')).toBe(mode === 'compact' ? '0px' : '288px')
  const item = w.get('[data-session-row]')
  expect(item.get('.session-status-icon').attributes('aria-label')).toBe(locale === 'zh' ? zh.sessionStatusRunning : en.sessionStatusRunning)
  expect(item.find('.cli-app-icon').exists()).toBe(true); expect(item.get('.session-name').text()).toHaveLength(200)
  expect(item.get('.session-time').text()).toBe('1m'); expect(item.find('.session-overflow-trigger button').exists()).toBe(true)
  expect(w.get('.project-name').text()).toHaveLength(80)
  const host = w.get('[data-host-slot]').element
  if (mode !== 'wide') {
    expect(document.querySelector('[role="dialog"]')).not.toBeNull()
    shell.drawerVisible = false; shell.sidebarVisible = true; await flushPromises()
    expect(document.querySelector('[role="dialog"]')).toBeNull(); expect(w.get('[data-host-slot]').element).toBe(host)
  }
})
// 断点使用 CSS viewport，不重复乘 DPR；窄屏选择不覆盖桌面栏选择。
it('Layout_PinnedBreakpoints_002', async () => {
  const shell = useShellStore(); shell.setSidebarWidth(999); shell.setDrawerWidth(1)
  expect(shell.sidebarWidth).toBe(360); expect(shell.drawerWidth).toBe(300)
  for (const [width, mode] of [[1180, 'wide'], [1179, 'overlay'], [900, 'overlay'], [899, 'compact']] as const) { shell.setViewportWidth(width); expect(shell.responsiveMode).toBe(mode) }
  shell.sidebarVisible = true; shell.setViewportWidth(1440); shell.sidebarVisible = false
  shell.setViewportWidth(899); expect(shell.sidebarVisible).toBe(true)
  shell.setViewportWidth(1440); expect(shell.sidebarVisible).toBe(false)
})
// 真实设置编辑器在表面失活时关闭，不能将焦点返还隐藏栏。
it('Layout_SettingsModalOwnership_003', async () => {
  const shell = useShellStore(); shell.navigate('settings'); useSidebarStore().activeSettingsSection = 'launch-configurations'
  const profiles = useCliProfilesStore(); profiles.status = 'loaded'; profiles.profiles = [{ id: 'cc', revision: '1', cli: 'claude', name: 'P'.repeat(80), launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }]
  const composition = defineComponent({ setup: () => () => h(AppShell, {}, { default: () => h(SettingsView, { active: shell.section === 'settings', style: { display: shell.section === 'settings' ? '' : 'none' } }) }) })
  const w = mount(composition, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } }); wrappers.push(w)
  const edit = w.get('[data-launch-edit]').element as HTMLButtonElement; edit.focus(); edit.click(); await flushPromises()
  expect(document.querySelector('[data-launch-save]')).not.toBeNull()
  shell.navigate('workspace'); await flushPromises()
  expect(document.querySelector('[role="dialog"]')).toBeNull(); expect(document.activeElement).not.toBe(edit)
})
// 静态布局约束只验证 CSS 策略，不声称 jsdom 测出了溢出或字体宽度。
it('Layout_PreservesColumnsAndWrap_004', () => {
  const session = readFileSync('src/components/sessions/SessionItem.vue', 'utf8')
  expect(session).toMatch(/grid-template-columns:\s*16px 18px minmax\(0, 1fr\) 38px 20px/)
  expect(session).toMatch(/\.session-name\s*\{[^}]*overflow:\s*hidden[^}]*text-overflow:\s*ellipsis[^}]*white-space:\s*nowrap/s)
  const shell = readFileSync('src/components/shell/AppShell.vue', 'utf8'); expect(shell).toContain('minmax(0, 1fr)'); expect(shell).toContain('overflow: hidden')
  const settings = readFileSync('src/components/settings/SettingsView.vue', 'utf8'); expect(settings).toContain('minmax(0, 1fr)'); expect(settings).toContain('overflow-x: hidden')
  const css = readFileSync('src/styles/global.css', 'utf8')
  expect(css).toMatch(/\.ui-dialog-footer \.ui-button[^}]*white-space:\s*normal/s)
  expect(css).toMatch(/\.ui-menu-item\s*\{[^}]*overflow-wrap:\s*anywhere/s)
})

// 换行后的弹窗动作仍保留 compact/normal/primary 的最小高度，不能统一压成 32px。
it('Layout_FooterControlSizes_005', () => {
  const css = readFileSync('src/styles/global.css', 'utf8')
  expect(css).toMatch(/\.ui-dialog-footer \.ui-button[^}]*min-height:\s*var\(--control-height-normal\)/s)
  for (const size of ['compact', 'primary']) expect(css).toMatch(new RegExp('\\.ui-dialog-footer \\.ui-control--' + size + '[^}]*min-height:\\s*var\\(--control-height-' + size + '\\)', 's'))
})


// 小于 900 CSS px 的真实 shell 可收起/展开会话栏，变化不替换主内容宿主。
it('Layout_CompactShellKeepsHost_006', async () => {
  vi.stubGlobal('innerWidth', 899); vi.stubGlobal('devicePixelRatio', 1.5)
  const shell = useShellStore()
  const w = mount(AppShell, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'zh', messages: { en, zh } })] }, slots: { sidebar: '<button>会话</button>', default: '<section data-preserved-host />' } }); wrappers.push(w); await flushPromises()
  const host = w.get('[data-preserved-host]').element
  const columns = w.get('.shell-columns').element as HTMLElement
  expect(w.attributes('data-responsive-mode')).toBe('compact'); expect(columns.style.getPropertyValue('--session-column-width')).toBe('0px')
  shell.toggleSidebar(); await flushPromises(); expect(columns.style.getPropertyValue('--session-column-width')).toBe('288px')
  vi.stubGlobal('innerWidth', 1180); window.dispatchEvent(new Event('resize')); await flushPromises()
  expect(w.attributes('data-responsive-mode')).toBe('wide'); expect(w.get('[data-preserved-host]').element).toBe(host)
})
