import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import VersionManagerApp from '@/manager/VersionManagerApp.vue'
import wire from '../fixtures/version-manager-wire.json'
import ordinaryWire from '../fixtures/version-manager-ordinary-wire.json'

const invoke = vi.fn()
const wrappers: VueWrapper[] = []
beforeEach(() => {
  vi.clearAllMocks()
  window.__CC_DESK_VERSION_MANAGER__ = { invoke }
  invoke.mockResolvedValue(wire.installedUnconfirmed)
})
afterEach(() => {
  wrappers.splice(0).forEach(wrapper => wrapper.unmount())
  delete window.__CC_DESK_VERSION_MANAGER__
  document.body.innerHTML = ''
  vi.useRealTimers()
})
function render(locale = 'en') {
  const wrapper = mount(VersionManagerApp, { attachTo: document.body,
    global: { plugins: [createI18n({ legacy: false, locale, fallbackLocale: 'en', messages: { en: { close: 'Close' }, zh: { close: '关闭' } } })] } })
  wrappers.push(wrapper)
  return wrapper
}

describe('isolated historical version recovery manager', () => {
  // 普通安装准备阶段即说明备份未完成，不展示自动返回或已保存的假保证。
  it('ManagerUI_OrdinaryPending_018', async () => {
    invoke.mockResolvedValue(ordinaryWire.preparing)
    const wrapper = render(); await flushPromises()
    expect(wrapper.get('[data-manager-ordinary-pending]').text()).toContain('backup has not been verified')
    expect(wrapper.find('[data-manager-ordinary-backup]').exists()).toBe(false)
    expect(wrapper.find('[data-manager-return]').exists()).toBe(false)
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
    expect(wrapper.get('[data-manager-boundary]').text()).toContain('manual restoration')
    expect(wrapper.text()).not.toContain('Returning restores the saved matching context')
  })

  // 普通安装必须展示真实保留备份、手动恢复说明和仅启动安装程序的状态。
  it.each(['en', 'zh'])('ManagerUI_OrdinaryBackup_019: %s', async locale => {
    invoke.mockResolvedValue(ordinaryWire.installerHandedOff)
    const wrapper = render(locale); await flushPromises()
    expect(wrapper.get('[data-manager-ordinary-backup]').text()).toContain(ordinaryWire.installerHandedOff.ordinaryInstall.backupLocation)
    expect(wrapper.get('[data-manager-ordinary-restore]').text()).toContain(locale === 'en' ? 'Close every CC Desk instance' : '关闭所有 CC Desk 实例')
    expect(wrapper.get('[data-manager-phase]').text()).toBe(locale === 'en' ? 'Official installer started' : '官方安装程序已启动')
    expect(wrapper.find('[data-manager-return]').exists()).toBe(false)
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
    expect(wrapper.get('[data-manager-refresh]').attributes('disabled')).toBeUndefined()
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(['inspect_version_switch'])
  })

  // 备份完成后失败仍保留实际位置，只提供检查而非自动恢复能力。
  it('ManagerUI_OrdinaryRecovery_020', async () => {
    invoke.mockResolvedValue(ordinaryWire.recoveryRequired)
    const wrapper = render(); await flushPromises()
    expect(wrapper.get('[data-manager-ordinary-backup]').text()).toContain(ordinaryWire.recoveryRequired.ordinaryInstall.backupLocation)
    expect(wrapper.get('[data-manager-ordinary-restore]').text()).toContain('manual restoration')
    expect(wrapper.find('[data-manager-return]').exists()).toBe(false)
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
  })
  // 安装未检查不能显示成功，并清楚说明 Desk 与共享 CLI 数据边界。
  it('ManagerUI_UnconfirmedBoundary_001', async () => {
    const wrapper = render(); await flushPromises()
    expect(invoke).toHaveBeenCalledWith('inspect_version_switch', {})
    expect(wrapper.get('[data-manager-phase]').text()).toContain('Installed, awaiting confirmation')
    expect(wrapper.text()).toContain('0.18.0')
    expect(wrapper.text()).toContain('0.17.7')
    expect(wrapper.get('[data-manager-boundary]').text()).toMatch(/CLI history.*credentials.*project files/)
    expect(wrapper.text()).toContain('not a sandbox')
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(true)
    expect(wrapper.find('[data-manager-return]').exists()).toBe(true)
    expect(wrapper.find('[data-manager-cancel]').exists()).toBe(false)
  })

  // 后端仅提供刷新时，恢复界面不得根据状态自行添加按钮。
  it('ManagerUI_OnlyOfferedActions_002', async () => {
    invoke.mockResolvedValue({ ...wire.recoveryRequired, allowedActions: ['refresh'] })
    const wrapper = render(); await flushPromises()
    expect(wrapper.get('[data-manager-block]').text()).toContain('installer outcome is unknown')
    expect(wrapper.find('[data-manager-return]').exists()).toBe(false)
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
    expect(wrapper.get('[data-manager-refresh]').attributes('disabled')).toBeUndefined()
  })

  // 返回确认使用共享对话框，默认聚焦安全的返回按钮，Esc 不取消事务。
  it('ManagerUI_ReturnReviewFocus_003', async () => {
    const wrapper = render(); await flushPromises()
    const opener = wrapper.get('[data-manager-return]').element as HTMLButtonElement
    opener.focus(); opener.click(); await flushPromises()
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!
    expect(dialog.textContent).toContain('0.18.0')
    expect(dialog.textContent).toContain('will not be merged')
    expect(document.activeElement?.getAttribute('data-manager-back')).toBe('')
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await flushPromises()
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    expect(document.activeElement).toBe(opener)
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  // 二次点击与未知回执不重复恢复；只允许重新检查日志。
  it('ManagerUI_UnknownNoReplay_004', async () => {
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-manager-return]').trigger('click'); await flushPromises()
    let reject!: (reason: unknown) => void
    invoke.mockImplementationOnce(() => new Promise((_resolve, failure) => { reject = failure }))
    const submit = document.querySelector<HTMLButtonElement>('[data-manager-submit]')!
    submit.click(); submit.click(); await flushPromises()
    expect(invoke.mock.calls.filter(([command]) => command === 'restore_previous_version')).toHaveLength(1)
    reject({ code: 'UNKNOWN', details: 'C:\\private SECRET' }); await flushPromises()
    expect(wrapper.get('[data-manager-error]').text()).toContain('not confirmed')
    expect(wrapper.text()).not.toMatch(/SECRET|private/)
    await wrapper.get('[data-manager-refresh]').trigger('click'); await flushPromises()
    expect(wrapper.get('[data-manager-return]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-manager-uncertain]').text()).toContain('not be repeated')
    expect(invoke.mock.calls.filter(([command]) => command === 'restore_previous_version')).toHaveLength(1)
  })

  // 重挂载期间迟到的恢复回执不能覆盖新界面或触发第二次写入。
  it('ManagerUI_RemountPending_005', async () => {
    const first = render(); await flushPromises()
    await first.get('[data-manager-return]').trigger('click'); await flushPromises()
    let finish!: (value: unknown) => void
    invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    document.querySelector<HTMLButtonElement>('[data-manager-submit]')!.click(); await flushPromises()
    first.unmount()
    const second = render(); await flushPromises()
    expect(second.get('[data-manager-return]').attributes('disabled')).toBeDefined()
    finish({ ...wire.restored, generation: '18' }); await flushPromises()
    expect(second.get('[data-manager-phase]').text()).not.toContain('Previous version restored')
    expect(invoke.mock.calls.filter(([command]) => command === 'restore_previous_version')).toHaveLength(1)
  })

  // 已撤销的文档不能切换到普通桥或替换桥，错误详情只显示安全文案。
  it('ManagerUI_DocumentRevoked_006', async () => {
    const wrapper = render(); await flushPromises()
    const replacement = vi.fn()
    window.__CC_DESK_VERSION_MANAGER__ = { invoke: replacement }
    await wrapper.get('[data-manager-refresh]').trigger('click'); await flushPromises()
    expect(wrapper.get('[data-manager-error]').text()).toContain('Reopen the version manager')
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
    expect(replacement).not.toHaveBeenCalled()
  })

  // 进度刷新只执行检查；安装状态不能通过经过时间自动宣告成功。
  it('ManagerUI_ProgressReadsOnly_007', async () => {
    vi.useFakeTimers()
    invoke.mockResolvedValue(wire.installing)
    const wrapper = render(); await flushPromises()
    await vi.advanceTimersByTimeAsync(4500); await flushPromises()
    expect(invoke.mock.calls.length).toBeGreaterThan(1)
    expect(invoke.mock.calls.every(([command]) => command === 'inspect_version_switch')).toBe(true)
    expect(wrapper.get('[data-manager-phase]').text()).toContain('Installing historical version')
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
  })

  // 中英文和独立外观控件不依赖普通 App 配置存储。
  it('ManagerUI_LanguageTheme_008', async () => {
    const wrapper = render('zh'); await flushPromises()
    expect(wrapper.get('h1').text()).toContain('历史版本')
    await wrapper.get('[data-manager-language]').setValue('en')
    expect(wrapper.get('h1').text()).toContain('Historical version')
    await wrapper.get('[data-manager-theme]').setValue('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(document.documentElement.lang).toBe('en')
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  // 没有原生认证桥时只说明管理器不可用，不降级到普通调用。
  it('ManagerUI_MissingBridge_009', async () => {
    delete window.__CC_DESK_VERSION_MANAGER__
    const wrapper = render(); await flushPromises()
    expect(wrapper.get('[data-manager-error]').text()).toContain('Reopen the version manager')
    expect(invoke).not.toHaveBeenCalled()
  })

  // 页面完成加载前的 FORBIDDEN 只重试检查，真正成功回执才显示状态。
  it('ManagerUI_StartupAdmission_010', async () => {
    vi.useFakeTimers()
    invoke.mockRejectedValueOnce({ code: 'FORBIDDEN' }).mockResolvedValueOnce(wire.installedUnconfirmed)
    const wrapper = render(); await flushPromises()
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
    await vi.advanceTimersByTimeAsync(200); await flushPromises()
    expect(wrapper.get('[data-manager-phase]').text()).toContain('Installed, awaiting confirmation')
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(['inspect_version_switch', 'inspect_version_switch'])
  })

  // 有界启动重试耗尽只显示未就绪，显式重查仍然可用。
  it('ManagerUI_StartupBounded_011', async () => {
    vi.useFakeTimers()
    invoke.mockRejectedValue({ code: 'FORBIDDEN', field: 'SECRET' })
    const wrapper = render(); await flushPromises()
    await vi.advanceTimersByTimeAsync(2000); await flushPromises()
    expect(invoke).toHaveBeenCalledTimes(4)
    expect(wrapper.get('[data-manager-error]').text()).toContain('not ready')
    expect(wrapper.text()).not.toContain('SECRET')
    await wrapper.get('[data-manager-refresh]').trigger('click'); await flushPromises()
    expect(invoke).toHaveBeenCalledTimes(5)
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
  })

  // 首次成功准入后的 FORBIDDEN 不允许计时器、刷新或重挂载继续使用该桥。
  it('ManagerUI_RevokedNoRetry_012', async () => {
    vi.useFakeTimers()
    const wrapper = render(); await flushPromises()
    invoke.mockRejectedValueOnce({ code: 'FORBIDDEN' })
    await wrapper.get('[data-manager-refresh]').trigger('click'); await flushPromises()
    expect(wrapper.get('[data-manager-refresh]').attributes('disabled')).toBeDefined()
    wrapper.unmount()
    const next = render(); await flushPromises()
    await vi.advanceTimersByTimeAsync(5000); await flushPromises()
    expect(next.get('[data-manager-error]').text()).toContain('Reopen the version manager')
    expect(invoke).toHaveBeenCalledTimes(2)
  })

  // 对话框提交后若页面在实际发送前卸载，旧意图不得迟到发送。
  it('ManagerUI_UnmountBeforeSend_013', async () => {
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-manager-return]').trigger('click'); await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-manager-submit]')!.click()
    wrapper.unmount()
    await flushPromises()
    expect(invoke.mock.calls.filter(([command]) => command === 'restore_previous_version')).toHaveLength(0)
  })

  // 首次启动确认必须由用户明确提交，不能由轮询代替。
  it('ManagerUI_ExplicitConfirmation_014', async () => {
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-manager-confirm]').trigger('click'); await flushPromises()
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain('does not launch the app')
    invoke.mockResolvedValueOnce({ ...wire.historicalActive, generation: '18' })
    document.querySelector<HTMLButtonElement>('[data-manager-submit]')!.click(); await flushPromises()
    expect(invoke).toHaveBeenLastCalledWith('confirm_historical_version', { expectedGeneration: '17' })
    expect(wrapper.get('[data-manager-phase]').text()).toContain('Historical version confirmed')
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
    expect(wrapper.find('[data-manager-return]').exists()).toBe(true)
  })

  // 重开后的返回仍需二次确认；请求未完成时只轮询真实状态，不重复派发。
  it('ManagerUI_ReentryReturnProgress_015', async () => {
    vi.useFakeTimers()
    invoke.mockResolvedValue({ ...wire.recoveryRequired, blockedReason: null,
      allowedActions: ['refresh', 'return-to-previous'] })
    const wrapper = render(); await flushPromises()
    await wrapper.get('[data-manager-return]').trigger('click'); await flushPromises()
    expect(invoke.mock.calls.filter(([command]) => command === 'restore_previous_version')).toHaveLength(0)
    let finish!: (value: unknown) => void
    invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    document.querySelector<HTMLButtonElement>('[data-manager-submit]')!.click(); await flushPromises()
    invoke.mockResolvedValue({ ...wire.returning, generation: '18' })
    await vi.advanceTimersByTimeAsync(1500); await flushPromises()
    expect(invoke).toHaveBeenLastCalledWith('inspect_version_switch', {})
    expect(wrapper.get('[data-phase]').attributes('data-phase')).toBe('returning')
    expect(wrapper.find('[data-manager-return]').exists()).toBe(false)
    expect(wrapper.get('[data-manager-refresh]').attributes('disabled')).toBeDefined()
    invoke.mockResolvedValue({ ...wire.restored, generation: '19' })
    finish({ ...wire.restored, generation: '19' }); await flushPromises()
    expect(wrapper.get('[data-manager-phase]').text()).toContain('Previous version restored')
    expect(invoke.mock.calls.filter(([command]) => command === 'restore_previous_version')).toHaveLength(1)
  })

  // 已claim或证据未知的重开状态只有检查；已完成投影也不再次显示恢复。
  it('ManagerUI_ReentryBlockedAndCompletedAreReadOnly_016', async () => {
    invoke.mockResolvedValue({ ...wire.recoveryRequired, blockedReason: 'RECOVERY_EVIDENCE_UNAVAILABLE', allowedActions: ['refresh'] })
    const wrapper = render(); await flushPromises()
    expect(wrapper.find('[data-manager-return]').exists()).toBe(false)
    expect(wrapper.find('[data-manager-confirm]').exists()).toBe(false)
    invoke.mockResolvedValue({ ...wire.restored, generation: '19' })
    await wrapper.get('[data-manager-refresh]').trigger('click'); await flushPromises()
    expect(wrapper.get('[data-manager-phase]').text()).toContain('Previous version restored')
    expect(wrapper.find('[data-manager-return]').exists()).toBe(false)
    expect(invoke.mock.calls.every(([command]) => command === 'inspect_version_switch')).toBe(true)
  })
})
