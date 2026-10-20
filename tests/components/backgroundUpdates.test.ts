import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { useBackgroundUpdateCheck } from '@/composables/useBackgroundUpdateCheck'
import { useUpdateStore } from '@/stores/update'
import { useSidebarStore } from '@/stores/sidebar'
import { useShellStore } from '@/stores/shell'
import PrimaryNav from '@/components/shell/PrimaryNav.vue'
import UpdateSection from '@/components/settings/sections/UpdateSection.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import type { UpdateInfo } from '@/types'

const io = vi.hoisted(() => ({ check: vi.fn(), install: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), checkForUpdates: io.check, getUpdaterSettings: async () => ({ proxy: null }), installDesktopUpdate: io.install }))
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
const official = (): UpdateInfo => ({ version: '0.99.0', currentVersion: '0.23.4', hasUpdate: true, releaseNotes: 'Release notes', downloadUrl: '', platformAsset: null,
  channel: 'stable', installEligible: true, admissionId: '00000000-0000-4000-8000-000000000001', officialRelease: { id: 1, tag: 'v0.99.0', sourceSha: 'a'.repeat(40) } })
const wrappers: VueWrapper[] = []
beforeEach(() => { setActivePinia(createPinia()); io.check.mockReset().mockResolvedValue(official()); io.install.mockReset() })
afterEach(() => { wrappers.splice(0).forEach(wrapper => wrapper.unmount()); vi.useRealTimers(); document.body.innerHTML = '' })
function background() {
  const wrapper = mount(defineComponent({ setup() { useBackgroundUpdateCheck(); return () => null } }))
  wrappers.push(wrapper); return wrapper
}
function render(component: any) {
  const wrapper = mount(component, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })], stubs: { HistoricalVersionsPanel: true } } })
  wrappers.push(wrapper); return wrapper
}

// 首屏不发请求，定期只读检查；离开组件后不留下定时器。
it('Update_BackgroundDelayAndInterval_001', async () => {
  vi.useFakeTimers(); const wrapper = background()
  await vi.advanceTimersByTimeAsync(29_999); expect(io.check).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(1); expect(io.check).toHaveBeenCalledTimes(1)
  expect(useUpdateStore().availableVersion).toBe('0.99.0')
  await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000 - 1); expect(io.check).toHaveBeenCalledTimes(1)
  await vi.advanceTimersByTimeAsync(1); expect(io.check).toHaveBeenCalledTimes(2)
  expect(io.install).not.toHaveBeenCalled()
  wrapper.unmount(); expect(vi.getTimerCount()).toBe(0)
})

it('Update_BackgroundUnmountFencesReply_002', async () => {
  vi.useFakeTimers(); let finish!: (info: UpdateInfo) => void
  io.check.mockImplementation(() => new Promise<UpdateInfo>(resolve => { finish = resolve }))
  const wrapper = background(); await vi.advanceTimersByTimeAsync(30_000)
  expect(io.check).toHaveBeenCalledTimes(1)
  wrapper.unmount(); finish(official()); await flushPromises()
  expect(useUpdateStore().hasUpdate).toBe(false)
  expect(vi.getTimerCount()).toBe(0)
})

// 已确认版本透传到设置入口和现有更新页，失败不弹出窗口，也不保留过期安装准入。
it('Update_SettingsNoticeAndFailure_003', async () => {
  const nav = render(PrimaryNav), section = render(UpdateSection); await flushPromises()
  await useUpdateStore().checkForUpdate(); await flushPromises()
  const settings = nav.get('[data-primary-section="settings"]')
  expect(settings.attributes('aria-label')).toContain('0.99.0')
  expect(nav.find('[data-settings-update-badge]').exists()).toBe(true)
  await settings.trigger('click')
  expect(useShellStore().section).toBe('settings')
  expect(useSidebarStore().activeSettingsSection).toBe('update')
  expect(section.get('[data-update-package]').text()).toContain('0.99.0')
  expect(document.querySelector('[data-update-confirmation]')).toBeNull()
  io.check.mockRejectedValue({ code: 'UPDATER_TIMEOUT', stage: 'check' })
  await useUpdateStore().checkForUpdate().catch(() => {}); await flushPromises()
  expect(nav.find('[data-settings-update-badge]').exists()).toBe(true)
  expect(section.get('[data-update-install]').attributes('disabled')).toBeDefined()
  expect(section.get('[data-update-package]').text()).not.toContain(en.updateSourceUnverified)
  expect(section.find('[data-update-error]').exists()).toBe(false)
  expect(io.install).not.toHaveBeenCalled()
})

it('Update_UnverifiedDoesNotNotify_004', async () => {
  useUpdateStore().setUpdateInfo({ ...official(), channel: 'candidate', installEligible: false, admissionId: null })
  const nav = render(PrimaryNav)
  expect(nav.find('[data-settings-update-badge]').exists()).toBe(false)
  expect(nav.get('[data-primary-section="settings"]').attributes('aria-label')).toBe(en.settings)
})

it.each(['candidate', 'test-only', 'unverified'] as const)('Update_SettingsNegativeReceipt_005 %s', async channel => {
  useUpdateStore().setUpdateInfo(official())
  const nav = render(PrimaryNav), section = render(UpdateSection); await flushPromises()
  expect(nav.find('[data-settings-update-badge]').exists()).toBe(true)
  io.check.mockResolvedValue({ ...official(), channel, installEligible: false, admissionId: null, eligibilityReason: 'UPDATER_SIGNATURE_INVALID' })
  await useUpdateStore().checkForUpdate(); await flushPromises()
  const labels = { candidate: en.updateCandidate, 'test-only': en.updateTestOnly, unverified: en.updateUnverified }
  expect(nav.find('[data-settings-update-badge]').exists()).toBe(false)
  expect(section.get('[data-update-channel]').text()).toBe(labels[channel])
  expect(section.get('[data-update-package]').text()).toContain(en.updateSourceUnverified)
  expect(section.get('[data-update-package]').text()).not.toContain(en.updateCheckBeforeInstall)
})
