import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import App from '@/App.vue'
import { useShellStore } from '@/stores/shell'
import { useAppStore } from '@/stores/app'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useProjectManagementStore } from '@/stores/projectManagement'
import { useProjectsStateStore } from '@/stores/projectsState'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
vi.mock('@/utils/platform', () => ({ isMac: false, isWindows: true, ctrl: 'Ctrl' }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isMaximized: async () => false, onResized: async () => () => {} }) }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
vi.mock('@/composables/useUnifiedWorkspaceRuntime', async () => { const { ref } = await import('vue'); return { useUnifiedWorkspaceRuntime: () => ({ openSessions: ref([]), cliAvailability: ref({}), error: ref(null) }) } })
const mounted: VueWrapper[] = []
beforeEach(() => {
  clearMocks(); setActivePinia(createPinia()); useProjectsStateStore().loaded = true
  useAppStore().loadStatus = 'loaded'
  mockIPC(command => {
    if (command === 'get_app_config') return { language: 'en', terminalTheme: 'cc-box-light', claudeEnvVars: { TEST: 'x' } }
    if (command === 'update_app_config') return undefined
    if (command === 'cli_list_projects') return { revision: '0', projects: [] }
    if (command === 'get_projects') return []
    return undefined
  })
})
afterEach(() => { mounted.splice(0).forEach(w => w.unmount()); document.body.innerHTML = ''; clearMocks(); vi.restoreAllMocks() })
function render(locale = 'en') {
  useShellStore().navigate('projects')
  const wrapper = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })], stubs: { UnifiedTerminalHost: true, SettingsView: true } } })
  mounted.push(wrapper); return wrapper
}
describe('Unified project management page', () => {
  // 页面采用真实项目列表，50个项目支持名称/路径搜索和固定优先排序。
  it('Projects_SearchSortFifty_001', async () => {
    const app = useAppStore()
    app.cachedProjects = Array.from({ length: 50 }, (_, i) => ({ path: `C:/work/project-${i.toString().padStart(2, '0')}`, name: `Project ${i.toString().padStart(2, '0')}`, lastDuration: i }))
    useProjectsStateStore().pinnedProjects = [app.cachedProjects[17].path]
    const catalog = useUnifiedSessionsStore()
    catalog.sessions = [3, 8].map(i => ({ id: `activity-${i}`, projectKey: `c:/work/project-${i.toString().padStart(2, '0')}`, projectPath: `C:/work/project-${i.toString().padStart(2, '0')}`, cli: 'codex', runtime: 'native-cli', title: 'History', processState: 'stopped', attentionState: 'none', lastActivityAt: i, archived: false, resumable: true, adapterSessionId: `activity-${i}` }))
    const wrapper = render(); await flushPromises()
    expect(wrapper.findAll('[data-project-row]')).toHaveLength(50)
    expect(wrapper.findAll('[data-project-row]')[0].attributes('data-project-path')).toBe(app.cachedProjects[17].path)
    expect(wrapper.findAll('[data-project-row]')[1].attributes('data-project-path')).toBe('C:/work/project-08')
    await wrapper.get('[data-project-sort]').setValue('name')
    expect(wrapper.findAll('[data-project-row]')[1].attributes('data-project-path')).toBe('C:/work/project-00')
    await wrapper.get('[data-project-search]').setValue('project-49')
    expect(wrapper.findAll('[data-project-row]')).toHaveLength(1)
    expect(wrapper.get('[data-project-row]').text()).toContain('Project 49')
  })
  // 超长路径保留首尾可辨识内容，完整路径通过title提供。
  it('Projects_LongPathMiddle_002', async () => {
    const path = `C:/work/${'nested-folder/'.repeat(15)}final-project`
    useAppStore().cachedProjects = [{ path, name: 'Final project' }]
    const wrapper = render(); await flushPromises()
    const element = wrapper.get('[data-project-path-display]')
    expect(element.attributes('title')).toBe(path)
    expect(element.text()).toContain('…'); expect(element.text().endsWith('final-project')).toBe(true)
  })
  // 点击移除只打开清晰确认，不把移除等同删除文件或停止会话。
  it('Projects_RemoveRequiresConfirmation_003', async () => {
    useAppStore().cachedProjects = [{ path: 'C:/work/desk', name: 'Desk' }]
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-project-overflow]').trigger('click'); await flushPromises()
    document.querySelector<HTMLElement>('[data-item-id="remove-project"]')!.click(); await flushPromises()
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Local files and CLI history will stay')
    expect(document.querySelector('[data-confirm-project-remove]')).not.toBeNull()
    document.querySelector<HTMLElement>('[data-cancel-project-action]')!.click(); await flushPromises()
    expect(document.querySelector('[role="dialog"]')).toBeNull()
  })
  // 打开另一项目改变上下文，但不会停止或关闭先前项目的终端。
  it('Projects_OpenChangesContext_004', async () => {
    const catalog = useUnifiedSessionsStore()
    catalog.sessions = [{ id: 'old', projectKey: 'c:/work/old', projectPath: 'C:/work/old', cli: 'codex', runtime: 'native-cli', title: 'Old session', processState: 'running', attentionState: 'none', lastActivityAt: 1, archived: false, resumable: false, adapterSessionId: 'old' }]
    catalog.activeSessionId = 'old'
    useAppStore().cachedProjects = [{ path: 'C:/work/new', name: 'New project' }]
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-project-path="C:/work/new"] .project-name').trigger('click'); await flushPromises()
    expect(useShellStore().section).toBe('workspace')
    expect(wrapper.get('[data-project-title]').text()).toBe('New project')
    expect(catalog.activeSessionId).toBeNull()
    expect(catalog.sessions.find(row => row.id === 'old')?.processState).toBe('running')
  })
  // 即使目录快照仍显示已结束，真实打开终端也阻止项目移除且不触发停止。
  it('Projects_RemoveBlocksOwnedTabs_005', async () => {
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'p', projectPath: 'C:/work/desk', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
    useAppStore().cachedProjects = [{ path: 'C:/work/desk', name: 'Desk' }]
    const management = useProjectManagementStore()
    management.beginRemove({ projectKey: 'c:/work/desk', projectPath: 'C:/work/desk' })
    expect(management.dialog).toBeNull(); expect(management.error).toBe('projectRemoveOpenSessions')
    expect(useNativeTabsStore().tab(tab.tabId)).toBeDefined()
    expect(management.visibleGroups.find(row => row.projectPath === 'C:/work/desk')).toBeDefined()
  })
  // 隐藏目录默认不出现，但明确打开隐藏项目筛选后仍能找回。
  it('Projects_HiddenFilterPreservesAccess_006', async () => {
    mockIPC(command => {
      if (command === 'get_app_config') return { language: 'en', terminalTheme: 'cc-box-light', hiddenProjects: ['C:/work/desk'], claudeEnvVars: { TEST: 'x' } }
      if (command === 'get_projects') return [{ path: 'C:/work/desk', name: 'Desk' }]
      if (command === 'cli_list_projects') return { revision: '0', projects: [] }
      return undefined
    })
    const wrapper = render(); await flushPromises()
    expect(wrapper.findAll('[data-project-row]')).toHaveLength(0)
    await wrapper.get('[data-show-hidden]').setValue(true)
    expect(wrapper.findAll('[data-project-row]')).toHaveLength(1)
    expect(wrapper.get('.project-name').attributes('disabled')).toBeDefined()
  })

  // 固定和重命名通过统一状态写入，树和列表在同一响应之后更新。
  it('Projects_PinRenameRealState_007', async () => {
    useAppStore().cachedProjects = [{ path: 'C:/work/desk', name: 'Desk' }]
    let saved = { pinnedProjects: [] as string[], archivedSessions: {}, displayNames: {} as Record<string, string> }
    mockIPC((command, payload) => {
      if (command === 'get_app_config') return { language: 'en', terminalTheme: 'cc-box-light', hiddenProjects: [], claudeEnvVars: { TEST: 'x' } }
      if (command === 'get_projects') return []
      if (command === 'cli_list_projects') return { revision: '0', projects: [] }
      if (command === 'get_projects_state') return saved
      if (command === 'pin_project') { saved = { ...saved, pinnedProjects: [(payload as any).path] }; return saved }
      if (command === 'set_display_name') { saved = { ...saved, displayNames: { 'c:/work/desk': (payload as any).alias } }; return saved }
      return undefined
    })
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-project-overflow]').trigger('click'); await flushPromises()
    document.querySelector<HTMLElement>('[data-item-id="pin"]')!.click(); await flushPromises()
    expect(useProjectsStateStore().pinnedProjects).toEqual(['C:/work/desk'])
    expect(wrapper.find('.project-pinned').exists()).toBe(true)
    await wrapper.get('[data-project-overflow]').trigger('click'); await flushPromises()
    document.querySelector<HTMLElement>('[data-item-id="rename"]')!.click(); await flushPromises()
    const input = document.querySelector<HTMLInputElement>('[role="dialog"] input')!; input.value = '开发工作'; input.dispatchEvent(new Event('input', { bubbles: true }))
    document.querySelector<HTMLElement>('[data-confirm-project-rename]')!.click(); await flushPromises()
    expect(wrapper.get('.project-name').text()).toBe('开发工作')
    expect(document.querySelector('[role="dialog"]')).toBeNull()
  })
  // 隐藏页面不能保留菜单焦点，目录行的新建入口进入现有统一创建流程。
  it('Projects_NewSessionAndInactiveMenu_008', async () => {
    useAppStore().cachedProjects = [{ path: 'C:/work/desk', name: 'Desk' }]
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-project-overflow]').trigger('click'); await flushPromises()
    useShellStore().navigate('settings'); await flushPromises()
    expect(document.querySelector('[role="menu"]')).toBeNull()
    useShellStore().navigate('projects'); await flushPromises()
    await wrapper.get('[data-project-overflow]').trigger('click'); await flushPromises()
    document.querySelector<HTMLElement>('[data-item-id="new-session"]')!.click(); await flushPromises()
    expect(useShellStore().section).toBe('workspace')
    expect(useShellStore().pendingRequest).toMatchObject({ kind: 'new-session', project: { projectPath: 'C:/work/desk' } })
  })

  // 行尾菜单脱离滚动列表，不被最后一行或长路径的裁剪边界遮住。
  it('Projects_MenuEscapesScrollClip_009', async () => {
    useAppStore().cachedProjects = [{ path: 'C:/work/desk', name: 'Desk' }]
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-project-overflow]').trigger('click'); await flushPromises()
    const menu = document.querySelector<HTMLElement>('[role="menu"]')!
    expect(menu.closest('.projects-list')).toBeNull()
    expect(menu.style.position).toBe('fixed')
  })

  // 仅剩归档的项目移除后，不会被树的归档项目壳重新加入常规列表。
  it('Projects_RemoveHidesArchivedShell_010', async () => {
    let hidden: string[] = []
    const catalog = useUnifiedSessionsStore()
    catalog.sessions = [{ id: 'archived', projectKey: 'c:/work/desk', projectPath: 'C:/work/desk', cli: 'codex', runtime: 'native-cli', title: 'Archive', processState: 'stopped', attentionState: 'none', lastActivityAt: 1, archived: true, resumable: true, adapterSessionId: 'history' }]
    mockIPC((command, payload) => {
      if (command === 'get_app_config') return { language: 'en', terminalTheme: 'cc-box-light', hiddenProjects: hidden, claudeEnvVars: { TEST: 'x' } }
      if (command === 'update_app_config' && (payload as any).updates.hiddenProjects) hidden = (payload as any).updates.hiddenProjects
      if (command === 'get_projects') return []
      if (command === 'cli_list_projects') return { revision: '0', projects: [] }
      if (command === 'get_projects_state') return { pinnedProjects: [], archivedSessions: {} }
      return undefined
    })
    const wrapper = render(); await flushPromises()
    const management = useProjectManagementStore()
    management.beginRemove({ projectKey: 'c:/work/desk', projectPath: 'C:/work/desk' }); await management.remove(); await flushPromises()
    useShellStore().navigate('workspace'); await flushPromises()
    expect(wrapper.findAll('.project-node')).toHaveLength(0)
    expect(catalog.sessions[0].archived).toBe(true)
  })

})
