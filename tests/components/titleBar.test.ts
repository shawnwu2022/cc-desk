import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import TitleBar from '@/components/TitleBar.vue'
import en from '@/i18n/locales/en'

const platform = vi.hoisted(() => ({ isMac: false, isWindows: true }))
const host = vi.hoisted(() => ({
  minimize: vi.fn(), toggleMaximize: vi.fn(), close: vi.fn(), cleanup: vi.fn(),
}))
vi.mock('@/utils/platform', () => platform)
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({
  minimize: host.minimize, toggleMaximize: host.toggleMaximize, close: host.close,
  isMaximized: async () => false, onResized: async () => host.cleanup,
}) }))

let wrapper: VueWrapper
beforeEach(() => {
  vi.clearAllMocks()
  platform.isMac = false
  platform.isWindows = true
})
afterEach(() => { wrapper?.unmount() })

describe('Title bar native interaction boundary', () => {
  // Tauri读取实际鼠标目标的属性；标题文本、图标和空白区域都必须显式声明拖动。
  it.each(['windows', 'macos', 'linux'])('TitleBar_DragTargets_001_%s', async (os) => {
    platform.isMac = os === 'macos'
    platform.isWindows = os === 'windows'
    wrapper = mount(TitleBar, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
    await flushPromises()

    const targets = ['.title-bar', '.win-title-left', '.win-app-title',
      platform.isMac ? '.traffic-light-spacer' : '.win-app-icon']
    for (const selector of targets) {
      expect(wrapper.get(selector).element.hasAttribute('data-tauri-drag-region'),
        `${os}: ${selector} must be a native drag target`).toBe(true)
    }
  })

  // 应用图片禁用HTML图片拖放，避免与窗口拖动竞争。
  it('TitleBar_DisablesImageDrag_002', async () => {
    wrapper = mount(TitleBar, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
    await flushPromises()
    expect((wrapper.get('.win-app-icon').element as HTMLImageElement).draggable).toBe(false)
  })

  // 窗口按钮与SVG子元素均不进入拖动区；点击图形仍只执行对应窗口操作。
  it('TitleBar_ControlTargets_003', async () => {
    wrapper = mount(TitleBar, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
    await flushPromises()

    for (const target of wrapper.findAll('.window-controls, .window-controls *')) {
      expect(target.element.hasAttribute('data-tauri-drag-region'),
        `${target.element.tagName} in the window controls must not be draggable`).toBe(false)
    }
    await wrapper.get('[data-window-action="minimize"] rect').trigger('click')
    await wrapper.get('[data-window-action="maximize"] rect').trigger('click')
    await wrapper.get('[data-window-action="close"] line').trigger('click')
    expect(host.minimize).toHaveBeenCalledOnce()
    expect(host.toggleMaximize).toHaveBeenCalledOnce()
    expect(host.close).toHaveBeenCalledOnce()
  })

  // Tauri已负责拖动区双击最大化，Vue不得再次切换导致窗口恢复。
  it('TitleBar_NativeDoubleClick_004', async () => {
    wrapper = mount(TitleBar, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
    await flushPromises()
    await wrapper.get('.title-bar').trigger('dblclick')
    await wrapper.get('.win-app-title').trigger('dblclick')
    await wrapper.get('[data-window-action="minimize"] rect').trigger('dblclick')
    expect(host.toggleMaximize).not.toHaveBeenCalled()
  })
})
