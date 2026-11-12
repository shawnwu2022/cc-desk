import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
let view: VueWrapper | undefined
beforeEach(() => setActivePinia(createPinia()))
afterEach(() => { view?.unmount(); document.body.innerHTML = '' })
it('ArchiveRetirement_OrdinaryDrawerHasNoSeparateFeatureEntrypoints_001', async () => {
  view = mount(ArchivedSessionsDrawer, { props: { open: true, sessions: [] }, attachTo: document.body,
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  await flushPromises()
  expect(document.querySelector('[data-recycle-open]')).toBeNull()
  expect(document.querySelector('[data-opencode-archive-open]')).toBeNull()
  expect(document.querySelector('[data-recycle-select]')).toBeNull()
  expect(document.querySelector('[data-opencode-archive-select]')).toBeNull()
  expect(document.body.textContent).toContain(en.noArchivedSessions)
})
