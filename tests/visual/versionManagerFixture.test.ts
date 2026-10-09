import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { nextTick } from 'vue'
import ManagerVisualFixtureApp from '@/visual/ManagerVisualFixtureApp.vue'
import { installManagerFixture, MANAGER_FIXTURE_PHASES } from '@/visual/managerFixture'
import { blockedHostCalls, invoke as blocked } from '@/visual/tauriStub'

let wrapper: VueWrapper | undefined
beforeEach(() => { blockedHostCalls.value = 0; history.replaceState({}, '', '/__visual__/version-manager/') })
afterEach(() => { wrapper?.unmount(); wrapper = undefined; delete window.__CC_DESK_VERSION_MANAGER__; document.body.innerHTML = '' })
async function render(query: string) {
  history.replaceState({}, '', `/__visual__/version-manager/?${query}`)
  wrapper = mount(ManagerVisualFixtureApp, { attachTo: document.body, global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en: { close: 'Close' }, zh: { close: '关闭' } } })],
  } })
  await flushPromises()
  return wrapper
}

describe('isolated real manager visual fixture', () => {
  // 八个有限状态通过真实管理器、严格解析器和共享 Rust 样本显示，不调用宿主。
  it.each(MANAGER_FIXTURE_PHASES)('ManagerFixture_PhaseNoHost_001:%s', async phase => {
    const view = await render(`phase=${phase}`)
    expect(view.findComponent({ name: 'VersionManagerApp' }).exists()).toBe(true)
    expect(view.get('[data-phase]').attributes('data-phase')).toBe(phase)
    expect(view.attributes('data-manager-inspects')).toBe('1')
    expect(view.attributes('data-blocked-host-calls')).toBe('0')
    expect(view.findAll('.xterm')).toHaveLength(0)
    expect(view.findAllComponents({ name: 'AppShell' })).toHaveLength(0)
  })

  // 已提供的确认操作经过真实审核对话框，确认后仍可返回原版本。
  it('ManagerFixture_ConfirmReturn_002', async () => {
    const view = await render('phase=installed-unconfirmed')
    await view.get('[data-manager-confirm]').trigger('click'); await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-manager-submit]')!.click(); await flushPromises()
    expect(view.get('[data-phase]').attributes('data-phase')).toBe('historical-active')
    expect(view.attributes('data-manager-confirms')).toBe('1')
    await view.get('[data-manager-return]').trigger('click'); await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-manager-submit]')!.click(); await flushPromises()
    expect(view.get('[data-phase]').attributes('data-phase')).toBe('returning')
    expect(view.attributes('data-manager-returns')).toBe('1')
    await view.get('[data-manager-refresh]').trigger('click'); await flushPromises()
    expect(view.get('[data-phase]').attributes('data-phase')).toBe('restored')
    expect(blockedHostCalls.value).toBe(0)
  })

  // 未知回执刷新后保持相同 generation，真实客户端不得重复恢复。
  it('ManagerFixture_UnknownNoReplay_003', async () => {
    const view = await render('phase=installed-unconfirmed&outcome=unknown')
    await view.get('[data-manager-return]').trigger('click'); await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-manager-submit]')!.click(); await flushPromises()
    expect(view.get('[data-manager-error]').text()).toContain('not confirmed')
    await view.get('[data-manager-refresh]').trigger('click'); await flushPromises()
    expect(view.get('[data-manager-return]').attributes('disabled')).toBeDefined()
    expect(view.get('[data-manager-uncertain]').text()).toContain('not be repeated')
    expect(view.attributes('data-manager-returns')).toBe('1')
    expect(blockedHostCalls.value).toBe(0)
  })

  // 后端只提供刷新时，fixture 不添加缺失动作；宿主计数是可变化的防护证据。
  it('ManagerFixture_OfferedAndCount_004', async () => {
    const view = await render('phase=installed-unconfirmed&actions=refresh-only')
    expect(view.find('[data-manager-confirm]').exists()).toBe(false)
    expect(view.find('[data-manager-return]').exists()).toBe(false)
    expect(() => blocked()).toThrow('VISUAL_HOST_ACCESS_BLOCKED')
    await nextTick()
    expect(view.attributes('data-blocked-host-calls')).toBe('1')
  })

  // 即使测试模块被直接导入，也不能替换已有的认证管理器文档。
  it('ManagerFixture_NoExistingBridge_005', () => {
    const original = { invoke: async () => ({}) }
    window.__CC_DESK_VERSION_MANAGER__ = original
    expect(() => installManagerFixture()).toThrow('VISUAL_DOCUMENT_ALREADY_PRESENT')
    expect(window.__CC_DESK_VERSION_MANAGER__).toBe(original)
  })

  // 普通路径和不在有限集合内的场景都必须拒绝安装模拟桥。
  it('ManagerFixture_RejectWrongContext_006', () => {
    history.replaceState({}, '', '/version-manager.html')
    expect(() => installManagerFixture()).toThrow('VISUAL_FIXTURE_DISABLED')
    history.replaceState({}, '', '/__visual__/version-manager/?phase=all-done')
    expect(() => installManagerFixture()).toThrow('VISUAL_MANAGER_SCENARIO_INVALID')
    expect(window.__CC_DESK_VERSION_MANAGER__).toBeUndefined()
  })

  // 额外路径、事务身份或错误 generation 不能作为模拟恢复权限。
  it('ManagerFixture_StrictPayload_007', async () => {
    const fixture = installManagerFixture()
    try {
      await expect(window.__CC_DESK_VERSION_MANAGER__!.invoke('restore_previous_version', { expectedGeneration: '17', path: '/private' }))
        .rejects.toThrow('VISUAL_HOST_ACCESS_BLOCKED')
      expect(fixture.calls.returns).toBe(0)
      expect(blockedHostCalls.value).toBe(1)
    } finally { fixture.dispose() }
  })

  // 已存在普通或原生宿主上下文时，禁止安装可替代认证的模拟桥。
  it('ManagerFixture_RejectNativeHost_008', () => {
    for (const name of ['__TAURI_INTERNALS__', '__CC_DESK_DOCUMENT__']) {
      Object.defineProperty(window, name, { value: {}, configurable: true })
      try {
        expect(() => installManagerFixture()).toThrow('VISUAL_NATIVE_CONTEXT_PRESENT')
        expect(window.__CC_DESK_VERSION_MANAGER__).toBeUndefined()
      } finally { Reflect.deleteProperty(window, name) }
    }
  })
})
