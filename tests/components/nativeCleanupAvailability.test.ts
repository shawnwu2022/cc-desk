import { afterEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import NativeCleanupAvailability from '@/components/sessions/NativeCleanupAvailability.vue'
import type { UnifiedSession } from '@/types/unifiedSession'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
let wrapper: VueWrapper | undefined
afterEach(() => { wrapper?.unmount(); wrapper = undefined })
const session = (cli: UnifiedSession['cli'], overrides: Partial<UnifiedSession> = {}): UnifiedSession => ({
  id: `${cli}-desk`, projectKey: 'project', projectPath: '/synthetic/project', cli, runtime: 'native-cli', title: `${cli} fixture`,
  processState: 'stopped', attentionState: 'none', lastActivityAt: 1, archived: true, resumable: true, adapterSessionId: 'fixture',
  nativeSessionId: `${cli}-native`, nativeRootKey: 'fixture-root', launchConfigId: 'fixture-profile',
  nativeOrigin: { cli, profileId: 'fixture-profile', profileRevision: '7', projectId: 'project', projectPath: '/synthetic/project' }, ...overrides,
})
function render(sessions: UnifiedSession[]) {
  wrapper = mount(NativeCleanupAvailability, { props: { sessions, active: true }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  return wrapper
}
describe('native cleanup conservative capability check', () => {
  it('CleanupAvailability_ExactObservedIdentityButNoCompletePreview_001', async () => {
    const view = render([session('codex')])
    await view.get('select').setValue('codex-desk')
    expect(view.get('[data-cleanup-target]').text()).toContain('codex-native')
    expect(view.get('[data-cleanup-target]').text()).toContain('fixture-profile')
    expect(view.get('[data-cleanup-target]').text()).toContain('fixture-root')
    expect(view.text()).toContain('complete child-session range is unverified')
    expect(view.text()).toContain('cannot lock the confirmed child-session range')
    expect(view.get('[data-native-cleanup-delete]').attributes('disabled')).toBeDefined()
    expect(view.get('[data-native-cleanup-archive]').attributes('disabled')).toBeDefined()
    expect(view.text()).toContain('Desk metadata')
    const eventsBefore = Object.keys(view.emitted())
    await view.get('[data-native-cleanup-delete]').trigger('click')
    expect(Object.keys(view.emitted())).toEqual(eventsBefore)
  })
  it('CleanupAvailability_OpencodeInterruptAndClaudeUnsupportedAreDistinct_002', async () => {
    const view = render([session('opencode'), session('claude')])
    await view.get('select').setValue('opencode-desk')
    expect(view.text()).toContain('may interrupt active sessions')
    await view.get('select').setValue('claude-desk')
    expect(view.text()).toContain('No verified single-session deletion interface')
    expect(view.text()).not.toContain('may interrupt active sessions')
  })
  it('CleanupAvailability_ProtectsActiveAndRestoringAndClearsRemovedTarget_003', async () => {
    const view = render([session('codex', { processState: 'running', resumePending: true })])
    await view.get('select').setValue('codex-desk')
    expect(view.text()).toContain('Active, restoring or uncertain session')
    await view.setProps({ sessions: [] })
    expect(view.find('[data-cleanup-target]').exists()).toBe(false)
    expect((view.get('select').element as HTMLSelectElement).value).toBe('')
    await view.setProps({ sessions: [session('codex')] }); await view.get('select').setValue('codex-desk')
    await view.setProps({ active: false }); await view.setProps({ active: true })
    expect(view.find('[data-cleanup-target]').exists()).toBe(false)
  })
})
