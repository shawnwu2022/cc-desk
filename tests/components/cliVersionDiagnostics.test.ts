import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { randomUUID } from 'node:crypto'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { createI18n } from 'vue-i18n'
import AboutSection from '@/components/settings/sections/AboutSection.vue'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useCliSupportStore } from '@/stores/cliSupport'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
const clipboard = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: clipboard }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => vi.fn()) }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), getUpdaterSettings: async () => ({ proxy: null, channel: 'stable' }) }))
let wrapper: VueWrapper | undefined
let listUnavailable = false
const profile = (id: string) => ({ id, revision: '7', cli: 'codex', name: `SECRET ${id}`, launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} })
beforeEach(() => {
  listUnavailable = false
  localStorage.clear(); setActivePinia(createPinia()); vi.clearAllMocks(); vi.stubGlobal('crypto', { randomUUID })
  mockIPC(command => command === 'cli_list_profiles' ? listUnavailable ? Promise.reject(new Error('TOKEN /private/path')) : { revision: '9', profiles: [profile('first'), profile('second')] }
    : command === 'get_app_config' ? { language: 'en', terminalTheme: 'cc-box-light' } : undefined)
})
afterEach(() => { wrapper?.unmount(); wrapper = undefined; clearMocks(); vi.unstubAllGlobals(); Reflect.deleteProperty(window, '__CC_DESK_DOCUMENT__'); localStorage.clear() })
describe('CLI version diagnostics UI', () => {
  it('VersionDiagnostics_SelectedProfileOnlyAndExplicitCopy_001', async () => {
    const calls: { command: string; payload: any }[] = []
    Reflect.set(window, '__CC_DESK_DOCUMENT__', { instanceId: 'window-one', invoke: async (command: string, payload: any) => {
      calls.push({ command, payload })
      if (command === 'cli_list_profiles') {
        if (listUnavailable) throw new Error('TOKEN /private/path')
        return { revision: '9', profiles: [profile('first'), profile('second')] }
      }
      if (command === 'native_probe_cli_version') return { requestId: payload.requestId, profileId: payload.profileId, profileRevision: '7', cli: 'codex', state: 'known', version: '0.166.0', reason: null }
      return undefined
    } })
    useCliSupportStore().setCliEnabled('codex', true)
    await useCliProfilesStore().load(); useCliProfilesStore().select('codex', 'second')
    wrapper = mount(AboutSection, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    await flushPromises(); expect(calls.filter(call => call.command === 'native_probe_cli_version')).toHaveLength(0)
    await wrapper.get('[data-copy-diagnostics]').trigger('click'); await flushPromises()
    expect(calls.filter(call => call.command === 'native_probe_cli_version')).toHaveLength(0)
    await wrapper.get('[data-collect-cli-versions]').trigger('click'); await flushPromises()
    const probes = calls.filter(call => call.command === 'native_probe_cli_version')
    expect(probes).toHaveLength(1); expect(probes[0]?.payload.profileId).toBe('second')
    expect(wrapper.get('[data-cli-version="codex"]').text()).toContain('0.166.0')
    await wrapper.get('[data-copy-diagnostics]').trigger('click'); await flushPromises()
    const copied = clipboard.mock.calls[clipboard.mock.calls.length - 1]![0] as string
    expect(JSON.parse(copied).update.cliVersions.codex).toMatchObject({ version: '0.166.0', source: 'selected-profile-1' })
    expect(copied).not.toMatch(/SECRET|first|second|profileId|requestId/)
    listUnavailable = true
    await wrapper.get('[data-collect-cli-versions]').trigger('click'); await flushPromises()
    await wrapper.get('[data-copy-diagnostics]').trigger('click'); await flushPromises()
    const afterFailedRefresh = clipboard.mock.calls[clipboard.mock.calls.length - 1]![0] as string
    expect(JSON.parse(afterFailedRefresh).update.cliVersions.codex).toMatchObject({ state: 'unknown', version: null, reason: 'unavailable' })
    expect(afterFailedRefresh).not.toMatch(/0\.166\.0|TOKEN|private/)
    useCliProfilesStore().select('codex', 'first'); await flushPromises()
    expect(wrapper.get('[data-cli-version="codex"]').text()).toContain('Not collected')
  })
})
