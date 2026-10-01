import { beforeEach, afterEach, describe, it, expect, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { readFileSync } from 'node:fs'
import SettingsView from '@/components/settings/SettingsView.vue'
import AppShell from '@/components/shell/AppShell.vue'
import NewSessionMenu from '@/components/sessions/NewSessionMenu.vue'
import { useAppStore } from '@/stores/app'
import { useSidebarStore } from '@/stores/sidebar'
import { useShellStore } from '@/stores/shell'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isMaximized: async () => false, onResized: async () => () => {} }) }))
const wrappers: VueWrapper[] = []
let i18n: ReturnType<typeof createI18n>
beforeEach(() => {
  clearMocks(); localStorage.clear(); setActivePinia(createPinia())
  mockIPC(command => command === 'get_app_config' ? { theme: 'light', terminalTheme: 'cc-box-light', language: 'en', claudeEnvVars: { TEST: 'keep' } } : undefined)
  i18n = createI18n({ legacy: false, locale: 'en', messages: { en, zh } })
})
afterEach(() => { wrappers.splice(0).forEach(wrapper => wrapper.unmount()); document.body.innerHTML = ''; useAppStore().$dispose(); document.documentElement.removeAttribute('data-density'); document.documentElement.removeAttribute('data-theme'); document.documentElement.classList.remove('light', 'dark'); clearMocks(); vi.restoreAllMocks(); vi.unstubAllGlobals() })
function render() {
  const wrapper = mount(SettingsView, { attachTo: document.body, global: { plugins: [i18n], stubs: { UpdateSection: true, AboutSection: true, ShortcutsSection: true } } })
  wrappers.push(wrapper); return wrapper
}
describe('Unified settings shell', () => {
  // 正常设置导航恰好七个真实分类，不再以旧startup名称作为一级入口。
  it('Settings_SevenSections_001', async () => {
    const wrapper = render()
    expect(wrapper.findAll('[data-settings-section]').map(item => item.attributes('data-settings-section'))).toEqual(['general', 'appearance', 'terminal', 'launch-configurations', 'shortcuts', 'update', 'about'])
    expect(useSidebarStore().activeSettingsSection).toBe('general')
    useSidebarStore().openSettings('startup'); await flushPromises()
    expect(useSidebarStore().activeSettingsSection).toBe('general')
  })
  // 通用显示语言和真正支持的启动/关闭行为，不编造托盘选项。
  it('Settings_GeneralSupportedOnly_002', () => {
    const wrapper = render()
    expect(wrapper.find('[data-settings-general]').exists()).toBe(true)
    expect(wrapper.find('[data-settings-language]').exists()).toBe(true)
    expect(wrapper.find('[data-startup-destination]').exists()).toBe(true)
    expect(wrapper.text()).not.toMatch(/minimize to tray|最小化到托盘/i)
  })
  // 外观只管理GUI，终端主题/字体/渲染器不混入该分类。
  it('Settings_GuiTerminalIndependent_003', async () => {
    useSidebarStore().activeSettingsSection = 'appearance'
    const app = useAppStore(); app.terminalTheme = 'dracula'; app.fontSize = 16
    const wrapper = render(); await wrapper.get('[data-gui-theme]').setValue('dark'); await flushPromises()
    expect(app.theme).toBe('dark'); expect(app.terminalTheme).toBe('dracula'); expect(app.fontSize).toBe(16)
    expect(wrapper.find('[data-terminal-theme]').exists()).toBe(false)
    expect(wrapper.find('.terminal-theme-select').exists()).toBe(false)
    expect(wrapper.find('.font-size-control').exists()).toBe(false)
  })
  // 持久化失败回退已确认的GUI值，并只显示固定安全反馈。
  it('Settings_ImmediateSaveRollback_004', async () => {
    let fail!: (reason: unknown) => void
    mockIPC(command => command === 'get_app_config' ? { theme: 'light', terminalTheme: 'cc-box-light', claudeEnvVars: { TEST: 'keep' } } : new Promise((_resolve, reject) => { fail = reject }))
    const app = useAppStore()
    const saving = app.setTheme('dark')
    expect(saving).toBeInstanceOf(Promise); expect(app.theme).toBe('dark')
    await flushPromises(); fail(new Error('/private/env TOKEN=secret')); await saving
    expect(app.theme).toBe('light'); expect(document.documentElement.dataset.theme).toBe('light')
    expect((app as any).settingsSaveError).toBe('settingsSaveFailed')
  })
  // 较旧失败不能回滚较新的意图，较新失败只回退最近一次确认成功。
  it('Settings_LatestSaveOwnsRollback_005', async () => {
    const writes: Array<{ resolve: () => void; reject: (reason: unknown) => void }> = []
    mockIPC(command => command === 'get_app_config' ? { theme: 'light', terminalTheme: 'cc-box-light', claudeEnvVars: { TEST: 'keep' } } : new Promise<void>((resolve, reject) => { writes.push({ resolve, reject }) }))
    const app = useAppStore(); const first = app.setTheme('dark')
    expect(first).toBeInstanceOf(Promise)
    const latest = app.setTheme('light'); await flushPromises()
    expect(app.theme).toBe('light'); expect(writes).toHaveLength(1)
    writes[0].reject(new Error('earlier failure')); await first; await flushPromises()
    expect(app.theme).toBe('light'); expect(writes).toHaveLength(2)
    writes[1].resolve(); await latest; expect(app.theme).toBe('light')
  })
  // 较晚配置读取不能覆盖用户已经选择的GUI主题。
  it('Settings_HydrationPreservesIntent_006', async () => {
    let finish!: (value: unknown) => void
    mockIPC(command => command === 'get_app_config' ? new Promise(resolve => { finish = resolve }) : undefined)
    const app = useAppStore(); const loading = app.loadAppConfig(); await flushPromises()
    const saving = app.setTheme('dark')
    finish({ theme: 'light', terminalTheme: 'cc-box-light', claudeEnvVars: { TEST: 'keep' } }); await loading; await saving
    expect(app.theme).toBe('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')
  })
  // 会话栏默认288，保存宽度钳位到240–360并真正传到全局布局。
  it('Settings_SidebarWidthPropagates_007', async () => {
    const app = useAppStore() as any; const shell = useShellStore()
    expect(typeof app.setSidebarWidth).toBe('function'); expect(app.sidebarWidth).toBe(288)
    const wrapper = mount(AppShell, { global: { plugins: [i18n] } }); wrappers.push(wrapper)
    await app.setSidebarWidth(500); await flushPromises()
    expect(app.sidebarWidth).toBe(360); expect(shell.sidebarWidth).toBe(360)
    expect(wrapper.get('.shell-columns').attributes('style')).toContain('--session-column-width: 360px')
    await app.setSidebarWidth(200); expect(shell.sidebarWidth).toBe(240)
  })
  // 150%逻辑宽度下导航和内容允许收缩并切为单列，没有固定横向最小宽度。
  it('Settings_ScaledLayoutContract_008', () => {
    const source = readFileSync('src/components/settings/SettingsView.vue', 'utf8')
    expect(source).toMatch(/min-width:\s*0/)
    expect(source).toMatch(/minmax\(0,\s*1fr\)/)
    expect(source).toMatch(/@media\s*\(max-width:\s*760px\)/)
    const wrapper = render(); expect(wrapper.find('.settings-content').exists()).toBe(true)
  })
  // 默认新建CLI影响现有选择器/高级表单，不会创建或启动任何会话。
  it('Settings_DefaultToolIsPresentation_009', async () => {
    const app = useAppStore() as any; expect(typeof app.setDefaultNewCli).toBe('function')
    await app.setDefaultNewCli('codex')
    useNewSessionDraftStore().open({ projectKey: '/repo', projectPath: '/repo' })
    expect(useNewSessionDraftStore().cli).toBe('codex'); expect(useUnifiedSessionsStore().sessions).toHaveLength(0)
    const wrapper = mount(NewSessionMenu, { attachTo: document.body, props: { open: true, anchor: { x: 0, y: 0 } }, global: { plugins: [i18n] } }); wrappers.push(wrapper)
    await flushPromises(); expect(document.querySelector('[role="menuitem"]')?.getAttribute('data-item-id')).toBe('codex')
  })
  // 未知提交回执只能读回真实值，不能盲目回滚或重复写入。
  it('Settings_UnknownSaveReconciles_010', async () => {
    let stored = { theme: 'light', guiThemeMode: 'light', terminalTheme: 'cc-box-light', language: 'en' }; let writes = 0
    mockIPC((command, payload) => {
      if (command === 'get_app_config') return stored
      if (command === 'update_app_config') { writes++; stored = { ...stored, ...(payload as any).updates }; throw { code: 'COMMIT_STATE_UNKNOWN', message: 'TOKEN=private' } }
      return undefined
    })
    const app = useAppStore(); await app.loadSettingsPreferences()
    expect(await app.setTheme('dark')).toBe(false)
    expect(app.theme).toBe('dark'); expect(writes).toBe(1)
    expect(app.settingsSaveError).toBe('settingsSaveUnconfirmed')
  })
  // 最新失败回退到前一个成功提交，而不是更早的默认值。
  it('Settings_RollbackConfirmedSuccess_011', async () => {
    const writes: Array<{ resolve: () => void; reject: (reason: unknown) => void }> = []
    mockIPC(command => command === 'get_app_config' ? { theme: 'light', language: 'en' } : new Promise<void>((resolve, reject) => { writes.push({ resolve, reject }) }))
    const app = useAppStore(); const first = app.setTheme('dark'); const second = app.setTheme('system'); await flushPromises()
    writes[0].resolve(); await first; await flushPromises(); writes[1].reject(new Error('latest failed')); await second
    expect(app.guiThemeMode).toBe('dark'); expect(app.theme).toBe('dark')
  })
  // 系统主题与密度会影响GUI，但不改动终端偏好或任何会话选择。
  it('Settings_SystemAndDensityAreGuiOnly_012', async () => {
    let notify!: (event: MediaQueryListEvent) => void
    vi.stubGlobal('matchMedia', () => ({ matches: true, addEventListener: (_type: string, callback: typeof notify) => { notify = callback }, removeEventListener: vi.fn() }))
    const app = useAppStore(); app.terminalTheme = 'dracula'; app.fontSize = 17
    useUnifiedSessionsStore().activeSessionId = 'held'
    await app.setTheme('system'); await app.setGuiDensity('compact')
    expect(app.theme).toBe('dark'); expect(document.documentElement.dataset.density).toBe('compact')
    notify({ matches: false } as MediaQueryListEvent)
    expect(app.theme).toBe('light'); expect(app.terminalTheme).toBe('dracula'); expect(app.fontSize).toBe(17)
    expect(useUnifiedSessionsStore().activeSessionId).toBe('held')
    vi.unstubAllGlobals()
  })

  // 启动迁移与GUI保存共享写入顺序，较晚迁移不能在磁盘覆盖新的设置。
  it('Settings_StartupWritesOrdered_013', async () => {
    let release!: () => void; let guiWrites = 0
    mockIPC((command, payload) => {
      if (command === 'get_app_config') return { theme: 'light', terminalTheme: 'cc-box-light', language: 'en', claudeEnvVars: { TEST: 'keep' } }
      if (command === 'update_app_config') {
        const updates = (payload as any).updates
        if ('guiThemeMode' in updates) { guiWrites++; return }
        return new Promise<void>(resolve => { release = resolve })
      }
      return undefined
    })
    const app = useAppStore(); const startup = app.loadAppConfig(); await flushPromises()
    const saving = app.setTheme('dark'); await flushPromises()
    const beforeStartupFinished = guiWrites
    release(); await startup; await saving
    expect(beforeStartupFinished).toBe(0); expect(guiWrites).toBe(1); expect(app.theme).toBe('dark')
  })

  // 未知保存后读取失败时，新的意图也不能越过成功读取直接提交。
  it('Settings_UnknownReadBlocksWrites_014', async () => {
    let reads = 0; let writes = 0; let readable = false
    let stored = { theme: 'light', guiThemeMode: 'light', language: 'en' }
    mockIPC((command, payload) => {
      if (command === 'get_app_config') {
        reads++
        if (reads > 1 && !readable) throw new Error('private failed read')
        return stored
      }
      if (command === 'update_app_config') {
        writes++; stored = { ...stored, ...(payload as any).updates }
        if (writes === 1) throw { code: 'COMMIT_STATE_UNKNOWN' }
        return
      }
      return undefined
    })
    const app = useAppStore(); await app.loadSettingsPreferences()
    expect(await app.setTheme('dark')).toBe(false); expect(app.settingsSaveError).toBe('settingsSaveReloadFailed')
    expect(await app.setTheme('light')).toBe(false); expect(writes).toBe(1)
    readable = true
    expect(await app.setTheme('light')).toBe(true); expect(writes).toBe(2)
    expect(app.theme).toBe('light'); expect(app.settingsSaveError).toBeNull()
  })

  // 配置重试加入更早的可见性读取时，读取发起时的版本保护已确认值和回退基线。
  it('Settings_SharedReadOriginPreservesCommit_015', async () => {
    const initial = { theme: 'light', guiThemeMode: 'light', terminalTheme: 'cc-box-light', language: 'en', hiddenProjects: [], claudeEnvVars: { TEST: 'keep' } }
    const app = useAppStore(); await app.loadSettingsPreferences()
    let finish!: (value: unknown) => void; let rejectWrites = false
    mockIPC(command => {
      if (command === 'get_app_config') return new Promise(resolve => { finish = resolve })
      if (command === 'update_app_config' && rejectWrites) throw new Error('save rejected')
      return undefined
    })
    const visibility = app.loadProjectVisibility(true); await flushPromises()
    expect(await app.setTheme('dark')).toBe(true)
    const reload = app.loadAppConfig(); await flushPromises()
    finish(initial); await visibility; await reload
    expect(app.guiThemeMode).toBe('dark')
    rejectWrites = true
    expect(await app.setTheme('system')).toBe(false)
    expect(app.guiThemeMode).toBe('dark')
  })

  // GUI未知写后恢复读取失败，已经排队的启动迁移也不能越过同一个恢复屏障。
  it('Settings_StartupRespectsUnknownWriteBarrier_016', async () => {
    const app = useAppStore(); await app.loadSettingsPreferences()
    let failSave!: (failure: unknown) => void; let reads = 0; let writes = 0
    mockIPC(command => {
      if (command === 'get_app_config') {
        if (++reads >= 2) throw new Error('read unavailable')
        return { theme: 'light', terminalTheme: 'cc-box-light', language: 'en', claudeEnvVars: { TEST: 'keep' } }
      }
      if (command === 'update_app_config' && ++writes === 1) return new Promise((_resolve, reject) => { failSave = reject })
      return undefined
    })
    const saving = app.setTheme('dark'); await flushPromises()
    const startup = app.loadAppConfig(); const startupResult = startup.catch(() => undefined); await flushPromises()
    failSave({ code: 'COMMIT_STATE_UNKNOWN' }); await saving; await startupResult
    expect(app.settingsSaveError).toBe('settingsSaveReloadFailed')
    expect(app.loadStatus).toBe('error'); expect(writes).toBe(1)
  })

  // 启动迁移本身的未知提交也要保留屏障；下一次GUI保存须先成功读回，绝不重放迁移。
  it('Settings_UnknownMigrationBlocksGuiWrite_017', async () => {
    let reads = 0; let writes = 0; let readable = false
    mockIPC(command => {
      if (command === 'get_app_config') {
        if (++reads > 1 && !readable) throw new Error('read unavailable')
        return { theme: 'light', terminalTheme: 'cc-box-light', language: 'en', claudeEnvVars: { TEST: 'keep' } }
      }
      if (command === 'update_app_config' && ++writes === 1) throw { code: 'COMMIT_STATE_UNKNOWN' }
      return undefined
    })
    const app = useAppStore(); await app.loadAppConfig().catch(() => undefined)
    expect(await app.setTheme('dark')).toBe(false); expect(writes).toBe(1)
    readable = true
    expect(await app.setTheme('dark')).toBe(true); expect(writes).toBe(2)
  })

  // 宽度的编辑草稿保留中间数字，完整输入失焦后一次保存并传到真实栏宽。
  it('Settings_SidebarWidthEditableDraft_018', async () => {
    const app = useAppStore(); await app.loadSettingsPreferences()
    let writes = 0; let failNext = false
    mockIPC(command => {
      if (command === 'update_app_config') { ++writes; if (failNext) throw new Error('width save rejected') }
      return undefined
    })
    useSidebarStore().activeSettingsSection = 'appearance'
    const wrapper = render(); const input = wrapper.get('[data-settings-sidebar-width]')
    await input.setValue(''); await flushPromises()
    expect((input.element as HTMLInputElement).value).toBe(''); expect(app.sidebarWidth).toBe(288); expect(writes).toBe(0)
    await input.setValue('3'); await flushPromises()
    expect((input.element as HTMLInputElement).value).toBe('3'); expect(app.sidebarWidth).toBe(288); expect(writes).toBe(0)
    await input.setValue('32'); await input.setValue('320'); await flushPromises()
    expect((input.element as HTMLInputElement).value).toBe('320'); expect(app.sidebarWidth).toBe(288)
    await input.trigger('blur'); await flushPromises()
    expect(app.sidebarWidth).toBe(320); expect(writes).toBe(1)
    await input.trigger('blur'); await flushPromises(); expect(writes).toBe(1)
    failNext = true
    await input.setValue('340'); await input.trigger('keydown', { key: 'Enter' }); await flushPromises()
    expect(app.sidebarWidth).toBe(320); expect((input.element as HTMLInputElement).value).toBe('320')
    expect(writes).toBe(2); expect(app.settingsSaveError).toBe('settingsSaveFailed')
    await input.trigger('blur'); await flushPromises(); expect(writes).toBe(2)
  })

  // 恢复等待旧共享读取时，晚加入旧读取的调用方不能废弃真正较新的恢复快照。
  it('Settings_RecoveryUsesActualReadPublication_019', async () => {
    const initial = { theme: 'light', guiThemeMode: 'light', terminalTheme: 'cc-box-light', language: 'en', hiddenProjects: [], claudeEnvVars: { TEST: 'keep' } }
    const app = useAppStore(); await app.loadSettingsPreferences()
    let finishOld!: (value: unknown) => void; let reads = 0; let writes = 0; let failNext = false
    let stored = { ...initial }
    mockIPC((command, payload) => {
      if (command === 'get_app_config') {
        if (++reads === 1) return new Promise(resolve => { finishOld = resolve })
        return { ...stored }
      }
      if (command === 'update_app_config') {
        ++writes
        if (failNext) throw new Error('known later failure')
        stored = { ...stored, ...(payload as any).updates }
        if (writes === 1) throw { code: 'COMMIT_STATE_UNKNOWN' }
      }
      return undefined
    })
    const visibility = app.loadProjectVisibility(true); await flushPromises()
    const saving = app.setTheme('dark'); await flushPromises()
    expect(reads).toBe(1)
    const reload = app.loadAppConfig(); await flushPromises()
    finishOld(initial); await visibility; expect(await saving).toBe(false); await reload
    expect(reads).toBe(2); expect(writes).toBe(2)
    expect(stored.guiThemeMode).toBe('dark'); expect(app.guiThemeMode).toBe('dark')
    expect(app.settingsSaveError).toBe('settingsSaveUnconfirmed')
    failNext = true
    expect(await app.setTheme('system')).toBe(false); expect(app.guiThemeMode).toBe('dark')
  })

})
