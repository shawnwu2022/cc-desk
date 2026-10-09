import { beforeEach, afterEach, describe, it, expect, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import ShortcutsSection from '@/components/settings/sections/ShortcutsSection.vue'
import UpdateSection from '@/components/settings/sections/UpdateSection.vue'
import AboutSection from '@/components/settings/sections/AboutSection.vue'
import { useAppStore } from '@/stores/app'
import { useUpdateStore } from '@/stores/update'
import { useSidebarStore } from '@/stores/sidebar'
import { useSessionStore } from '@/stores/session'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
const io = vi.hoisted(() => ({ write: vi.fn(), open: vi.fn(), check: vi.fn(), relaunch: vi.fn(), summary: vi.fn(), settings: vi.fn(), saveProxy: vi.fn(), install: vi.fn() }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: io.write }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: io.open }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => vi.fn()) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), checkForUpdates: io.summary, getUpdaterSettings: io.settings, saveUpdaterSettings: io.saveProxy, installDesktopUpdate: io.install, check: io.check, relaunch: io.relaunch }))
const wrappers: VueWrapper[] = []
beforeEach(() => { setActivePinia(createPinia()); vi.clearAllMocks(); clearMocks(); mockIPC(command => command === 'get_app_config' ? { language: 'en', terminalTheme: 'cc-box-light' } : undefined); io.write.mockResolvedValue(undefined); io.open.mockResolvedValue(undefined); io.settings.mockResolvedValue({ proxy: null }); io.saveProxy.mockResolvedValue(undefined); io.install.mockResolvedValue(undefined) })
afterEach(() => { wrappers.splice(0).forEach(wrapper => wrapper.unmount()); document.body.innerHTML = ''; useAppStore().$dispose(); clearMocks(); vi.restoreAllMocks() })
function render(component: any, props: Record<string, unknown> = {}) { const wrapper = mount(component, { props, attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } }); wrappers.push(wrapper); return wrapper }
const candidate = (channel: string) => ({ version: '0.99.0', currentVersion: '0.17.7', hasUpdate: true, releaseNotes: 'Notes', downloadUrl: '', platformAsset: null, channel, installEligible: false })
describe('Remaining settings sections', () => {
  it('Settings_ProxyHydrationSurvivesInactiveNavigation_016', async () => {
    let finish!: (value: { proxy: string }) => void
    io.settings.mockReturnValue(new Promise(resolve => { finish = resolve }))
    const wrapper = render(UpdateSection)
    expect(wrapper.get('[data-update-check]').attributes('disabled')).toBeDefined()
    await wrapper.setProps({ active: false }); await wrapper.setProps({ active: true })
    finish({ proxy: 'http://localhost:1080/' }); await flushPromises()
    expect((wrapper.get('[data-update-proxy]').element as HTMLInputElement).value).toBe('http://localhost:1080/')
    expect(wrapper.get('[data-update-check]').attributes('disabled')).toBeUndefined()
    expect(io.saveProxy).not.toHaveBeenCalled()
  })
  it('Settings_UnknownInstallRevokesReceiptAndCannotReplay_017', async () => {
    const id = '8e111fa0-8baf-4ef2-8d2a-71be6e100321'
    useUpdateStore().setUpdateInfo({ ...candidate('stable'), channel: 'stable', version: '0.18.2', installEligible: true, admissionId: id,
      officialRelease: { id: 123, tag: 'v0.18.2', sourceSha: 'a'.repeat(40) } })
    const wrapper = render(UpdateSection); await flushPromises()
    useNativeTabsStore().tabs.set('busy', { tabId: 'busy', status: 'unknown' } as any)
    await wrapper.get('[data-update-install]').trigger('click'); await flushPromises()
    const confirm = document.querySelector<HTMLButtonElement>('[data-update-confirm-install]')!
    expect(confirm.disabled).toBe(true); confirm.click(); expect(io.install).not.toHaveBeenCalled()
    useNativeTabsStore().tabs.clear(); await flushPromises()
    io.install.mockRejectedValue({ code: 'UPDATER_INSTALL_OUTCOME_UNKNOWN', stage: 'install', secret: 'TOKEN=SECRET' })
    confirm.click(); await flushPromises(); confirm.click(); await flushPromises()
    expect(io.install).toHaveBeenCalledTimes(1)
    expect(useUpdateStore().updateInfo?.admissionId).toBeNull()
    expect(wrapper.get('[data-update-error]').text()).toContain('UPDATER_INSTALL_OUTCOME_UNKNOWN')
    expect(wrapper.text()).not.toMatch(/TOKEN|SECRET/)
    expect(io.relaunch).not.toHaveBeenCalled()
  })
  it('Settings_OfficialUpdateOffersConfirmedInstall_014', async () => {
    const id = '8e111fa0-8baf-4ef2-8d2a-71be6e100321'
    useUpdateStore().setUpdateInfo({ ...candidate('stable'), channel: 'stable', version: '0.18.2', installEligible: true, admissionId: id,
      officialRelease: { id: 123, tag: 'v0.18.2', sourceSha: 'a'.repeat(40) } })
    const wrapper = render(UpdateSection); await flushPromises()
    expect(useUpdateStore().hasUpdate).toBe(true)
    expect(wrapper.get('[data-update-install]').attributes('disabled')).toBeUndefined()
    await wrapper.get('[data-update-install]').trigger('click'); await flushPromises()
    const confirm = document.querySelector<HTMLButtonElement>('[data-update-confirmation] [data-update-confirm-install]')!
    expect(confirm).not.toBeNull(); expect(confirm.disabled).toBe(false)
    confirm.click(); confirm.click(); await flushPromises()
    expect(io.install).toHaveBeenCalledTimes(1); expect(io.install).toHaveBeenCalledWith(id)
    expect(io.check).not.toHaveBeenCalled(); expect(io.relaunch).not.toHaveBeenCalled()
  })
  it('Settings_UpdaterProxyAndSpecificError_015', async () => {
    const wrapper = render(UpdateSection); await flushPromises()
    await wrapper.get('[data-update-proxy]').setValue('http://localhost:1080')
    await wrapper.get('[data-update-proxy-save]').trigger('click'); await flushPromises()
    expect(io.saveProxy).toHaveBeenCalledWith('http://localhost:1080')
    io.summary.mockRejectedValue({ code: 'UPDATER_MANIFEST_INVALID', stage: 'check', secret: '/private TOKEN=value' })
    await wrapper.get('[data-update-check]').trigger('click'); await flushPromises()
    expect(wrapper.get('[data-update-error]').text()).toContain('UPDATER_MANIFEST_INVALID')
    expect(wrapper.text()).not.toMatch(/TOKEN|\/private|signed candidates only/)
  })
  // 操作和按键可搜索，冲突捕获只在明确Replace后一次保存两个绑定。
  it('Settings_ShortcutCaptureConflict_001', async () => {
    const wrapper = render(ShortcutsSection); await flushPromises()
    await wrapper.get('[data-shortcut-edit="new-session"]').trigger('click')
    const dialog = document.querySelector<HTMLElement>('[data-shortcut-capture]')!
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'w', code: 'KeyW', ctrlKey: true, bubbles: true, cancelable: true })); await flushPromises()
    const app = useAppStore() as any
    expect(app.shortcutBindings['new-session']).toBe('Mod+KeyN'); expect(app.shortcutBindings['close-session']).toBe('Mod+KeyW')
    expect(document.querySelector('[data-shortcut-conflict]')).not.toBeNull()
    document.querySelector<HTMLButtonElement>('[data-shortcut-replace]')!.click(); await flushPromises()
    expect(app.shortcutBindings['new-session']).toBe('Mod+KeyW'); expect(app.shortcutBindings['close-session']).toBeNull()
    await wrapper.get('[data-shortcut-search]').setValue('projects'); expect(wrapper.findAll('[data-shortcut-row]')).toHaveLength(1)
    await wrapper.get('[data-shortcut-search]').setValue('Ctrl+W'); expect(wrapper.findAll('[data-shortcut-row]')).toHaveLength(1)
  })
  // 单项恢复可能冲突，全部恢复要求共享确认，取消和界面失活都不写。
  it('Settings_ShortcutResetAndActivity_002', async () => {
    const app = useAppStore() as any; await app.loadSettingsPreferences(); expect(typeof app.setShortcutBindings).toBe('function')
    await app.setShortcutBindings({ ...app.shortcutBindings, 'new-session': 'Mod+KeyK' })
    const wrapper = render(ShortcutsSection); await flushPromises()
    await wrapper.get('[data-shortcut-reset="new-session"]').trigger('click'); await flushPromises()
    expect(app.shortcutBindings['new-session']).toBe('Mod+KeyN')
    await app.setShortcutBindings({ ...app.shortcutBindings, 'new-session': 'Mod+KeyK' })
    await wrapper.get('[data-shortcut-reset-all]').trigger('click'); await flushPromises()
    expect(app.shortcutBindings['new-session']).toBe('Mod+KeyK')
    document.querySelector<HTMLButtonElement>('[data-shortcut-reset-confirm]')!.click(); await flushPromises()
    expect(app.shortcutBindings['new-session']).toBe('Mod+KeyN')
    await wrapper.get('[data-shortcut-edit="new-session"]').trigger('click'); await wrapper.setProps({ active: false }); await flushPromises()
    expect(document.querySelector('[data-shortcut-capture]')).toBeNull()
  })
  // 测试包或候选包即使伪装普通版本也不能给普通更新徽章或执行安装。
  it.each(['test-only', 'candidate', 'unverified', 'stable'])('Settings_UpdateExcludes_%s_003', async channel => {
    const info = { ...candidate(channel), installEligible: true }; useUpdateStore().setUpdateInfo(info as any); useSidebarStore().setUpdateInfo(info as any)
    const wrapper = render(UpdateSection)
    expect(useUpdateStore().hasUpdate).toBe(false); expect(useSidebarStore().updateAvailable).toBe(false)
    expect(wrapper.find('[data-update-install]').attributes('disabled')).toBeDefined()
    expect(wrapper.find('[data-update-policy]').text().length).toBeGreaterThan(20)
    expect(io.check).not.toHaveBeenCalled(); expect(io.relaunch).not.toHaveBeenCalled()
  })
  // 安装检查明确数出两个真实运行 owner，准备中/未知单独列出且不停止它们。
  it('Settings_UpdateCountsAndNoInstall_004', async () => {
    useUpdateStore().setUpdateInfo(candidate('unverified') as any)
    useSessionStore().tabs.set('same', { tabId: 'same', projectPath: '/private', status: 'running' } as any)
    useNativeTabsStore().tabs.set('same', { tabId: 'same', status: 'running' } as any)
    useNativeTabsStore().tabs.set('waiting', { tabId: 'waiting', status: 'unknown' } as any)
    const wrapper = render(UpdateSection); await wrapper.get('[data-update-review]').trigger('click'); await flushPromises()
    expect(document.querySelector('[data-update-running-count]')?.textContent).toContain('2')
    expect(document.querySelector('[data-update-unknown-count]')?.textContent).toContain('1')
    expect(io.check).not.toHaveBeenCalled(); expect(io.relaunch).not.toHaveBeenCalled()
    await wrapper.setProps({ active: false }); await flushPromises(); expect(document.querySelector('[data-update-confirmation]')).toBeNull()
    await wrapper.get('[data-update-review]').trigger('click'); await wrapper.setProps({ active: true }); await flushPromises()
    expect(document.querySelector('[data-update-confirmation]')).toBeNull()
  })
  // 不反射更新原始异常，失活后的迟到检查不能覆盖后来状态。
  it('Settings_UpdateSafeStaleCheck_005', async () => {
    let finish!: (value: unknown) => void
    io.summary.mockImplementation(() => new Promise(resolve => { finish = resolve }))
    const wrapper = render(UpdateSection); await flushPromises(); await wrapper.get('[data-update-check]').trigger('click'); await flushPromises()
    await wrapper.setProps({ active: false }); finish(candidate('test-only')); await flushPromises()
    expect(useUpdateStore().updateInfo).toBeNull()
    await wrapper.setProps({ active: true }); io.summary.mockRejectedValue(new Error('/private TOKEN=secret'))
    await wrapper.get('[data-update-check]').trigger('click'); await flushPromises()
    expect(wrapper.text()).not.toMatch(/TOKEN|\/private/)
    expect(wrapper.find('[data-update-error]').exists()).toBe(true)
  })
  // 关于可复制结构化白名单，存储里的路径/提示/输出/错误/环境不进入摘要。
  it('Settings_AboutSafeDiagnostic_006', async () => {
    const app = useAppStore(); app.cwd = '/PRIVATE_PATH'; app.claudeEnvVars = { TOKEN: 'SECRET_VALUE' }
    useSessionStore().tabs.set('secret', { tabId: 'secret', projectPath: '/PRIVATE_PATH', name: 'PROMPT_BODY', status: 'running', response: 'OUTPUT_BODY', error: 'RAW_ERROR' } as any)
    const wrapper = render(AboutSection)
    expect(wrapper.find('[data-build-commit]').exists()).toBe(true); expect(wrapper.text()).toContain('MIT')
    await wrapper.get('[data-copy-diagnostics]').trigger('click'); await flushPromises()
    const text = io.write.mock.calls[0][0]; const summary = JSON.parse(text)
    expect(summary.product).toBe('CC Desk'); expect(summary.sessions.running).toBe(1)
    expect(text).not.toMatch(/PRIVATE_PATH|PROMPT_BODY|OUTPUT_BODY|RAW_ERROR|SECRET_VALUE|TOKEN/)
    expect(wrapper.findAll('[data-about-link]')).toHaveLength(4)
    await wrapper.get('[data-about-link="codex"]').trigger('click'); expect(io.open).toHaveBeenCalledWith('https://developers.openai.com/learn/codex')
  })
  // 编辑必须先读取已有绑定，不能在加载时用默认值覆盖用户保存的绑定。
  it('Settings_ShortcutHydrationAdmission_007', async () => {
    let finish!: (value: unknown) => void
    mockIPC(command => command === 'get_app_config' ? new Promise(resolve => { finish = resolve }) : undefined)
    const wrapper = render(ShortcutsSection); await flushPromises()
    expect(wrapper.get('[data-shortcut-edit="new-session"]').attributes('disabled')).toBeDefined()
    const app = useAppStore() as any
    finish({ language: 'en', terminalTheme: 'cc-box-light', shortcutBindings: { ...app.shortcutBindings, 'new-session': 'Mod+KeyK' } }); await flushPromises()
    expect(app.shortcutBindings['new-session']).toBe('Mod+KeyK')
    expect(wrapper.get('[data-shortcut-edit="new-session"]').attributes('disabled')).toBeUndefined()
  })

  // 关闭旧界面再打开时，尚未确认的整组绑定保存仍拥有准入，不允许隐式重放其它绑定。
  it('Settings_ShortcutPendingAdmission_008', async () => {
    const app = useAppStore() as any; await app.loadSettingsPreferences()
    let finish!: () => void
    mockIPC(command => command === 'update_app_config' ? new Promise<void>(resolve => { finish = resolve }) : {})
    const saving = app.setShortcutBindings({ ...app.shortcutBindings, 'new-session': 'Mod+KeyK' }); await flushPromises()
    const wrapper = render(ShortcutsSection); await flushPromises()
    expect(wrapper.get('[data-shortcut-edit="projects"]').attributes('disabled')).toBeDefined()
    finish(); expect(await saving).toBe(true); await flushPromises()
    expect(wrapper.get('[data-shortcut-edit="projects"]').attributes('disabled')).toBeUndefined()
  })

  // 捕获后的外部变化使冲突选择失效，不能清除用户后来保存的另一操作。
  it('Settings_ShortcutConflictOwner_009', async () => {
    const wrapper = render(ShortcutsSection); await flushPromises()
    await wrapper.get('[data-shortcut-edit="new-session"]').trigger('click')
    document.querySelector<HTMLElement>('[data-shortcut-capture]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'w', code: 'KeyW', ctrlKey: true, bubbles: true, cancelable: true })); await flushPromises()
    const app = useAppStore() as any
    await app.setShortcutBindings({ ...app.shortcutBindings, projects: 'Mod+KeyK' })
    document.querySelector<HTMLButtonElement>('[data-shortcut-replace]')!.click(); await flushPromises()
    expect(app.shortcutBindings['new-session']).toBe('Mod+KeyN'); expect(app.shortcutBindings.projects).toBe('Mod+KeyK')
    expect(document.querySelector('[data-shortcut-replace]')).toBeNull()
  })

})
