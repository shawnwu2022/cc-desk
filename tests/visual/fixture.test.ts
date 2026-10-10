import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { nextTick } from 'vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import VisualFixtureApp from '@/visual/VisualFixtureApp.vue'
import { FIXTURE_TIME, longProjectName, longSessionTitle } from '@/visual/fixtures'
import { blockedHostCalls, invoke, listen, getCurrentWindow } from '@/visual/tauriStub'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import { expandFixtureProjects, clearFixtureSetupFocus, openFixtureSessionMenu } from './fixtureActions'

// Importing xterm probes canvas in jsdom; constructing it remains forbidden in this fixture.
vi.mock('@xterm/xterm', () => ({ Terminal: class { constructor() { throw new Error('VISUAL_TERMINAL_MOUNT_BLOCKED') } } }))
vi.mock('@tauri-apps/api/window', () => import('@/visual/tauriStub'))
vi.mock('@tauri-apps/api/core', () => import('@/visual/tauriStub'))
vi.mock('@tauri-apps/api/event', () => import('@/visual/tauriStub'))
vi.mock('@tauri-apps/plugin-dialog', () => import('@/visual/tauriStub'))
vi.mock('@tauri-apps/plugin-shell', () => import('@/visual/tauriStub'))
vi.mock('@tauri-apps/plugin-process', () => import('@/visual/tauriStub'))
vi.mock('@tauri-apps/plugin-updater', () => import('@/visual/tauriStub'))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => import('@/visual/tauriStub'))
let wrapper: VueWrapper | undefined
beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(FIXTURE_TIME)
  setActivePinia(createPinia()); localStorage.clear(); blockedHostCalls.value = 0
})
afterEach(() => { wrapper?.unmount(); wrapper = undefined; document.body.innerHTML = ''; vi.useRealTimers() })
async function render(scenario: string, locale = 'en') {
  history.replaceState({}, '', `/__visual__/?scenario=${scenario}&locale=${locale}`)
  wrapper = mount(VisualFixtureApp, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  await flushPromises(); return wrapper
}
describe('Isolated production-component fixture', () => {
  // 状态证据场景仅使用合成投影，渲染生产实心字形，不连接 CLI 或宿主。
  it('Fixture_StatusGlyphEvidence_021', async () => {
    const view = await render('session-status')
    for (const toggle of view.findAll('.project-node > .project-row .expand-arrow')) await toggle.trigger('click')
    const labels = view.findAll('.session-status-icon').map(icon => icon.attributes('aria-label'))
    expect(labels).toContain('Thinking')
    expect(labels).toContain('Subagent working')
    expect(labels).toContain('Waiting for input')
    expect(labels).toContain('Response completed')
    expect(labels).toContain('Activity unknown')
    expect(labels).toContain('Stopped')
    expect(view.findAll('[data-session-launch] button')).toHaveLength(1)
    expect(view.findAll('.xterm')).toHaveLength(0)
    expect(blockedHostCalls.value).toBe(0)
  })
  it.each(['en', 'zh'])('Fixture_NativeReceiptStaysIndependent_020 %s', async locale => {
    const view = await render('native-notice', locale)
    for (const toggle of view.findAll('.project-node > .project-row .expand-arrow')) await toggle.trigger('click')
    const marker = view.get('[data-native-observation-notice]')
    expect(marker.attributes('data-unread')).toBe('true')
    expect(marker.attributes('aria-label')).toBe(locale === 'en'
      ? 'Reply-end notice received (unread); current activity unverified'
      : '收到回复结束通知（未读）；当前活动尚未验证')
    expect(view.get('.session-status-icon').attributes('aria-label')).toBe(locale === 'en' ? 'Activity unknown' : '活动未知')
    expect(view.html()).not.toContain('visual-receipt-id')
    expect(blockedHostCalls.value).toBe(0)
  })
  it('Fixture_SettingsVersionIsStableSyntheticData_019', async () => {
    const view = await render('terminal-settings')
    expect(view.text()).toContain('CC Desk v0.18.1')
    expect(blockedHostCalls.value).toBe(0)
  })
  it('Fixture_UpdateSettingsAreSyntheticAndInstallerRemainsBlocked_018', async () => {
    await expect(invoke('get_updater_settings')).resolves.toEqual({ proxy: null })
    expect(typeof await listen('desktop-update-progress')).toBe('function')
    expect(blockedHostCalls.value).toBe(0)
    for (const command of ['save_updater_settings', 'check_desktop_update', 'install_desktop_update']) {
      expect(() => invoke(command)).toThrow('VISUAL_HOST_ACCESS_BLOCKED')
    }
    expect(blockedHostCalls.value).toBe(3)
  })
  // 两个恢复入口共用真实对话框：单会话只呈现目标，全局保留检索列表；取消不调用宿主。
  it.each(['en', 'zh'].flatMap(locale => ['resume-session', 'resume-history'].map(scenario => ({ locale, scenario }))))('Fixture_ResumeEntry_015: $scenario $locale', async ({ locale, scenario }) => {
    const view = await render(scenario, locale)
    const dialog = document.querySelector('[role="dialog"]')!
    expect(dialog).not.toBeNull()
    if (scenario === 'resume-session') {
      expect(dialog.querySelector('[data-resume-target]')?.textContent).toContain('Review terminal rendering')
      expect(dialog.querySelector('[data-resume-query]')).toBeNull()
      expect(dialog.querySelectorAll('[data-resume-result]')).toHaveLength(0)
      expect(dialog.querySelector('[data-confirm-resume]')).not.toBeNull()
    } else {
      expect(dialog.querySelector('[data-resume-target]')).toBeNull()
      expect(dialog.querySelector('[data-resume-query]')).not.toBeNull()
      expect(dialog.querySelectorAll('[data-resume-result]')).toHaveLength(2)
      expect(dialog.querySelector('[data-confirm-resume]')).toBeNull()
    }
    const cancel = [...dialog.querySelectorAll<HTMLButtonElement>('.resume-actions button')].find(button => button.textContent?.trim() === (locale === 'en' ? en.cancel : zh.cancel))!
    cancel.click(); await flushPromises()
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    expect(view.findAll('.xterm')).toHaveLength(0)
    expect(blockedHostCalls.value).toBe(0)
  })

  // 独立状态场景必须是真正 opened 的对应 CLI/runtime 行，不能借历史或准备失败行代替。
  it.each([
    { variant: 'native-claude', runtime: 'native-cli', cli: 'claude' },
    { variant: 'native-codex', runtime: 'native-cli', cli: 'codex' },
    { variant: 'legacy-claude', runtime: 'legacy-claude', cli: 'claude' },
  ].flatMap((variant, index) => ['starting', 'running', 'needs-user', 'unknown', 'stopped', 'failed'].map((state, offset) => ({
    ...variant, state, locale: (index + offset) % 2 ? 'zh' : 'en',
  }))))('Fixture_OpenedCloseState_014: $variant $state $locale', async ({ variant, runtime, cli, state, locale }) => {
    const view = await render(`close-state&runtime=${variant}&state=${state}`, locale)
    const sessions = useUnifiedSessionsStore().sessions
    expect(sessions, 'each evidence case must contain exactly its requested opened session').toHaveLength(1)
    expect(sessions[0]).toMatchObject({ runtime, cli, opened: true, archived: false,
      processState: state === 'needs-user' ? 'running' : state,
      attentionState: state === 'needs-user' ? 'needs-user' : 'none',
    })
    expect(sessions[0].preparationState, 'opened failed is not an unadmitted preparation failure').toBeUndefined()
    for (const toggle of view.findAll('.project-node > .project-row .expand-arrow')) await toggle.trigger('click')
    expect(view.findAll('[data-session-row]')).toHaveLength(1)
    const row = view.get('[data-session-row]')
    expect(row.attributes('data-session-row')).toBe(`visual-close-${variant}-${state}`)
    expect(row.get('.session-status-icon').classes()).toContain(`session-status-icon--${state === 'unknown' ? 'confirming' : state === 'needs-user' ? 'running' : state}`)
    const close = row.get('.session-primary-action button')
    expect(row.findAll('.session-primary-action button')).toHaveLength(1)
    expect(close.attributes('aria-label')).toBe(locale === 'en' ? 'Close' : '关闭')
    expect(close.get('svg path').attributes('d')).toBe('m6 6 12 12M18 6 6 18')
    ;(close.element as HTMLButtonElement).focus()
    expect(document.activeElement).toBe(close.element)
    await nextTick()
    expect(document.querySelector('[role="tooltip"]')?.textContent).toBe(locale === 'en' ? 'Close' : '关闭')
    // Match the browser's advancing clock after listeners mount. Do not use VTU
    // trigger(): its private timestamp workaround masks frozen-clock event loss.
    vi.setSystemTime(FIXTURE_TIME + 1)
    close.element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()
    expect(document.activeElement).toBe(close.element)
    for (const entry of ['overflow', 'context', 'keyboard']) {
      if (entry === 'overflow') await row.get('.session-overflow-trigger button').trigger('click')
      else if (entry === 'context') await row.trigger('contextmenu')
      else await row.trigger('keydown', { key: 'F10', shiftKey: true })
      await flushPromises()
      const menu = document.querySelector('[role="menu"]')!
      expect(menu, `${entry} must render the actual menu`).not.toBeNull()
      expect(menu.querySelector('[data-item-id="close"], [data-item-id="stop"]')).toBeNull()
      expect(menu.querySelector('[data-item-id="archive"]')).toBeNull()
      expect(row.find('[data-session-archive] button').exists()).toBe(state === 'stopped' || state === 'failed')
      menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      await flushPromises()
      expect(document.querySelector('[role="menu"]')).toBeNull()
    }
    expect(view.findAllComponents({ name: 'NativeCliTerminal' })).toHaveLength(0)
    expect(view.findAllComponents({ name: 'TerminalView' })).toHaveLength(0)
    expect(blockedHostCalls.value).toBe(0)
  })

  // 关闭提示明确终止与输出丢失边界，不承诺未保存历史可恢复。
  it('Fixture_CloseWarning_007', async () => {
    await render('close-confirmation')
    const dialog = document.querySelector('[role="dialog"]')!
    expect(dialog.textContent).toContain(en.confirmCloseDescription)
    expect(dialog.textContent).toContain('Copy any output you need first')
    expect(dialog.querySelector('[data-session-confirm]')!.textContent).toBe('Close')
    expect(blockedHostCalls.value).toBe(0)
  })

  // 每个固定场景只使用内存数据和真实UI组件，不创建终端实例或调用宿主。
  it.each(['empty', 'mixed', 'hover', 'resources', 'projects', 'new-session', 'archived', 'terminal-settings', 'launch-configurations', 'confirmation'])('Fixture_NoHostAccess_001: %s', async scenario => {
    const view = await render(scenario)
    expect(view.attributes('data-visual-ready')).toBe('true')
    expect(view.findAllComponents({ name: 'AppShell' })).toHaveLength(1)
    expect(view.findAll('.xterm')).toHaveLength(0)
    expect(view.attributes('data-blocked-host-calls')).toBe('0')
    expect(longProjectName).toHaveLength(80); expect(longSessionTitle).toHaveLength(200)
    if (scenario === 'resources') expect(document.querySelectorAll('.resource-card')).toHaveLength(3)
    if (scenario === 'new-session' || scenario === 'confirmation' || scenario === 'archived') expect(document.querySelector('[role="dialog"]')).not.toBeNull()
    if (scenario === 'launch-configurations') expect(view.findAll('[data-launch-row]')).toHaveLength(4)
  })
  // 空工作区经过真实宿主的布局边界，但不挂载 Native/Legacy 终端或调用宿主。
  it.each(['empty', 'empty-project'])('Fixture_EmptyUsesActualHost_006: %s', async scenario => {
    const view = await render(scenario)
    const host = view.findComponent({ name: 'UnifiedTerminalHost' })
    expect(host.exists(), 'empty fixture must exercise the production terminal-host layout').toBe(true)
    expect(host.props('sessions')).toEqual([])
    expect(host.props('activeSessionId')).toBeNull()
    expect(host.get('.ui-empty-state').text()).toContain(en.workspaceWelcome)
    expect(host.get('.ui-empty-state button').text()).toBe(scenario === 'empty' ? en.addProject : en.newSession)
    expect(view.findAllComponents({ name: 'NativeCliTerminal' })).toHaveLength(0)
    expect(view.findAllComponents({ name: 'TerminalView' })).toHaveLength(0)
    expect(view.findAll('.xterm')).toHaveLength(0)
    expect(blockedHostCalls.value).toBe(0)
  })
  // 防护计数必须能暴露渲染后的意外宿主调用，不能用静态0掩盖后续访问。
  it('Fixture_ReportsBlockedHostCall_002', async () => {
    const view = await render('mixed')
    expect(() => invoke()).toThrow('VISUAL_HOST_ACCESS_BLOCKED')
    await nextTick()
    expect(view.attributes('data-blocked-host-calls')).toBe('1')
    expect(blockedHostCalls).toBeDefined()
  })
  // Playwright all() creates live nth locators; expanding earlier projects must not shift later targets.
  it.each(['mixed', 'hover', 'menu'])('Fixture_ExpandsEveryProject_003: %s', async scenario => {
    await render(scenario)
    const toggles = '.project-node > .project-row .expand-arrow'
    expect(document.querySelectorAll(toggles)).toHaveLength(4)
    const page = {
      locator(selector: string) {
        return { async all() {
          return Array.from(document.querySelectorAll(selector), (_, index) => {
            const current = () => {
              const node = document.querySelectorAll<HTMLElement>(selector)[index]
              if (!node) throw new Error(`LIVE_LOCATOR_TARGET_MISSING: ${index}`)
              return node
            }
            return {
              async getAttribute(name: string) { return current().getAttribute(name) },
              async click() { current().focus(); current().click(); await nextTick() },
            }
          })
        }, async evaluate(action: (element: Element) => void) { action(document.querySelector(selector)!); await nextTick() } }
      },
    }
    await expandFixtureProjects(page)
    expect(document.querySelectorAll(`${toggles}[aria-expanded="true"]`)).toHaveLength(4)
    expect(document.querySelectorAll('[data-session-row]')).toHaveLength(6)
    expect(document.querySelector('[role="tooltip"]'), 'project setup must not leave the last Collapse tooltip open').toBeNull()
    expect(document.activeElement, 'setup must not leave a whole-pane main focus outline').toBe(document.body)
    expect(document.querySelector('.shell-main')!.hasAttribute('tabindex')).toBe(false)
    // Opening an already expanded fixture must not collapse it on a repeated setup.
    await expandFixtureProjects(page)
    expect(document.querySelectorAll('[data-session-row]')).toHaveLength(6)
    expect(blockedHostCalls.value).toBe(0)
  })
  // The production row enables overflow pointer events on hover/focus. jsdom verifies
  // focus-before-click ordering and the actual menu; rendered hit testing remains pending.
  it('Fixture_FocusesRowBeforePointerMenu_004', async () => {
    const view = await render('menu')
    for (const toggle of view.findAll('.project-node > .project-row .expand-arrow')) await toggle.trigger('click')
    expect(document.querySelector('[role="menu"]')).toBeNull()
    const page = {
      locator(selector: string) {
        return { first() {
          const row = document.querySelector<HTMLElement>(selector)!
          return {
            async focus() { row.focus(); await nextTick() },
            locator(controlSelector: string) { return { async click() {
              if (!row.contains(document.activeElement)) throw new Error('OVERFLOW_POINTER_TARGET_NOT_FOCUSED')
              row.querySelector<HTMLElement>(controlSelector)!.click()
              await nextTick()
            } } },
          }
        } }
      },
    }
    await openFixtureSessionMenu(page)
    await flushPromises()
    expect(document.querySelector('[role="menu"]')).not.toBeNull()
    expect(document.querySelector('[role="menu"]')!.contains(document.activeElement)).toBe(true)
    expect(document.querySelector('[data-item-id="close"]')).toBeNull()
    expect(document.querySelector('[data-item-id="stop"]')).toBeNull()
    expect(document.querySelector('[data-item-id="archive"]')).toBeNull()
    expect(document.querySelector('[data-session-row="visual-session-0"] .session-primary-action button')?.getAttribute('aria-label')).toBe('Close')
    expect(blockedHostCalls.value).toBe(0)
  })
  // 截图准备完成后移除临时焦点，已有 tabindex 必须原样保留。
  it('Fixture_RestoresMainFocusTarget_005', async () => {
    const view = await render('empty')
    const main = document.querySelector<HTMLElement>('.shell-main')!
    main.setAttribute('tabindex', '0')
    ;(view.get('input').element as HTMLElement).focus()
    const page = { locator(selector: string) { return {
      async evaluate(action: (element: Element) => void) { action(document.querySelector(selector)!); await nextTick() },
    } } }
    await clearFixtureSetupFocus(page)
    expect(document.activeElement).toBe(document.body)
    expect(main.getAttribute('tabindex')).toBe('0')
    expect(blockedHostCalls.value).toBe(0)
  })
  // 历史版本截图使用真实设置组件、文档客户端和签名DTO，不触达宿主或网络。
  it('Fixture_HistoryPreparation_007', async () => {
    const view = await render('historical-versions')
    await view.get('[data-history-refresh]').trigger('click'); await flushPromises()
    expect(view.findAll('[data-history-row]')).toHaveLength(2)
    expect(view.findAll('[data-history-select]')[1].attributes('disabled')).toBeDefined()
    await view.findAll('[data-history-select]')[0].trigger('click'); await flushPromises()
    await view.get('[data-history-prepare]').trigger('click'); await flushPromises()
    expect(view.get('[data-history-status]').text()).toContain('Publisher signature, SHA256 and size verified')
    expect(view.get('[data-history-install]').attributes('disabled')).toBeUndefined()
    await view.get('[data-history-install]').trigger('click'); await flushPromises()
    expect(document.querySelector<HTMLButtonElement>('[data-history-begin]')?.disabled).toBe(true)
    expect(blockedHostCalls.value).toBe(0)
  })

  // 合成签发与未知回执仅使用隔离桥，不触发真实安装或进程调用。
  it.each(['admitted', 'unknown', 'aborted'])('Fixture_HistorySwitch_013: %s', async outcome => {
    const view = await render(`historical-versions&historySwitch=${outcome}`)
    await view.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await view.findAll('[data-history-select]')[0].trigger('click'); await flushPromises()
    await view.get('[data-history-prepare]').trigger('click'); await flushPromises()
    await view.get('[data-history-install]').trigger('click'); await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-history-begin]')!.click(); await flushPromises()
    if (outcome === 'unknown') expect(view.get('[data-history-status]').text()).toContain('unknown')
    await view.get('[data-history-inspect]').trigger('click'); await flushPromises()
    expect(view.get('[data-history-status]').text()).toContain(outcome === 'aborted' ? 'confirmed an abort' : 'handed to the version manager')
    expect(view.find('[data-history-cancel]').exists()).toBe(false)
    expect(view.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(view.find('[data-history-prepare-again]').exists()).toBe(outcome === 'aborted')
    expect(blockedHostCalls.value).toBe(0)
  })

})

// 视觉宿主仅使用合成焦点/监听/取消；实际提示请求仍失败且无法连接真实窗口。
it('Fixture_WindowAttentionRemainsIsolated_020', async () => {
  const window = getCurrentWindow()
  await expect(window.isFocused()).resolves.toBe(true)
  expect(typeof await window.onFocusChanged()).toBe('function')
  await expect(window.requestUserAttention(null)).resolves.toBeUndefined()
  expect(blockedHostCalls.value).toBe(0)
  await expect(window.requestUserAttention(1)).rejects.toThrow('VISUAL_HOST_ACCESS_BLOCKED')
  expect(blockedHostCalls.value).toBe(1)
})
