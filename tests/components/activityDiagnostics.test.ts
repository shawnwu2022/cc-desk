import { afterEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import ActivityDiagnosticsPanel from '@/components/settings/ActivityDiagnosticsPanel.vue'
import { activityDiagnostics } from '@/utils/activityDiagnostics'
import en from '@/i18n/locales/en'

const clipboard = vi.hoisted(() => vi.fn(async (_text: string) => {}))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: clipboard }))

afterEach(() => activityDiagnostics.disable())

it('ActivityDiagnostics_Settings_004 toggles and copies only displayed snapshot', async () => {
  const wrapper = mount(ActivityDiagnosticsPanel, {
    props: { active: true },
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] },
  })
  try {
    const [toggle, preview] = wrapper.findAll('button')
    expect(toggle.attributes('aria-pressed')).toBe('false')
    await toggle.trigger('click')
    const operation = activityDiagnostics.start('archive', 'codex')!
    await preview.trigger('click')
    expect(clipboard).not.toHaveBeenCalled()
    const shown = wrapper.get('pre').element.textContent
    operation.record('adapter', 'ok')
    await wrapper.findAll('button')[2].trigger('click')
    await flushPromises()
    expect(clipboard).toHaveBeenCalledExactlyOnceWith(shown)
    await toggle.trigger('click')
    expect(toggle.attributes('aria-pressed')).toBe('false')
    operation.record('list', 'ok')
    expect(activityDiagnostics.snapshot().events.map(e => e.stage)).toEqual(['ui','adapter'])
  } finally {
    wrapper.unmount()
  }
})
