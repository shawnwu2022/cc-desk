import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import SessionItem from '@/components/sessions/SessionItem.vue'
import SessionAppIcon from '@/components/sessions/SessionAppIcon.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import type { UnifiedSession } from '@/types/unifiedSession'

afterEach(() => { document.body.innerHTML = ''; vi.unstubAllGlobals() })
describe('Merged session app status', () => {
  // 完成仅在实际状态或新回执进入时柔和亮起一次，稳定刷新与重新挂载不重播。
  it('AppIcon_CompletionCueOnce_003', async () => {
    const global = { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] }
    const wrapper = mount(SessionAppIcon, { props: { cli: 'claude', state: 'working' }, global })
    await wrapper.setProps({ state: 'completed' })
    expect(wrapper.get('.session-app-icon').attributes('data-completion-entry')).toBe('true')
    await wrapper.get('.session-app-icon').trigger('animationend', { animationName: 'session-app-completion' })
    expect(wrapper.get('.session-app-icon').attributes('data-completion-entry')).toBeUndefined()
    await wrapper.setProps({ state: 'completed', activityState: 'idle' })
    expect(wrapper.get('.session-app-icon').attributes('data-completion-entry')).toBeUndefined()
    wrapper.unmount()
    const remounted = mount(SessionAppIcon, { props: { cli: 'claude', state: 'completed' }, global })
    expect(remounted.get('.session-app-icon').attributes('data-completion-entry')).toBeUndefined()
    remounted.unmount()
  })
  // 减少动态时仍保留工作字形和真实应用图像，等待输入使用独立静态角标。
  it('AppIcon_ReducedStaticDetails_004', async () => {
    vi.stubGlobal('matchMedia', () => ({ matches: true }))
    const wrapper = mount(SessionAppIcon, { props: { cli: 'codex', state: 'working', activityState: 'thinking' }, global: { plugins: [createI18n({ legacy: false, locale: 'zh', messages: { en, zh } })] } })
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toBe('Codex CLI; 思考中')
    expect(wrapper.get('[data-session-badge]').attributes('data-session-badge')).toBe('working')
    await wrapper.setProps({ state: 'completed' })
    expect(wrapper.get('.session-app-icon').attributes('data-completion-entry')).toBeUndefined()
    await wrapper.setProps({ state: 'needs-user', activityState: 'waiting_input' })
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toBe('Codex CLI; 等待输入')
    expect(wrapper.get('[data-session-badge]').attributes('data-session-badge')).toBe('needs-user')
    wrapper.unmount()
  })
  // 单槽保留真实应用标识，选中态不改变后台活动或触发完成提示。
  it('AppIcon_SelectionKeepsState_001', async () => {
    const session: UnifiedSession = { id: 'one', projectKey: '/repo', projectPath: '/repo', cli: 'codex', runtime: 'native-cli', title: 'One', processState: 'running', activityState: 'working', attentionState: 'none', opened: true, archived: false, resumable: true, adapterSessionId: 'one', lastActivityAt: 100 }
    const wrapper = mount(SessionItem, { props: { session }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    expect(wrapper.findAll('.session-app-icon')).toHaveLength(1)
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toBe('Codex CLI; Working')
    await wrapper.get('.session-app-icon').trigger('click')
    expect(wrapper.emitted('activate')?.[0]).toEqual(['one'])
    const image = wrapper.get('img').element
    await wrapper.setProps({ selected: true })
    expect(wrapper.get('img').element).toBe(image)
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toBe('Codex CLI; Working')
    expect(wrapper.find('[data-completion-entry]').exists()).toBe(false)
    wrapper.unmount()
  })
  // 结束回执保留发生事实；权限/错误和新工作不会被旧绿色勾替代。
  it('AppIcon_ReceiptKeepsCause_002', async () => {
    const notice = { kind: 'reply-ended' as const, eventId: 'reply', runId: 'run', generation: 1, receivedAt: 1 }
    const session: UnifiedSession = { id: 'one', projectKey: '/repo', projectPath: '/repo', cli: 'claude', runtime: 'native-cli', title: 'One', processState: 'running', activityState: 'unknown', attentionState: 'none', opened: true, archived: false, resumable: true, adapterSessionId: 'one', lastActivityAt: 100, observationNotice: { recent: notice, unreadReplyEnd: notice } }
    const wrapper = mount(SessionItem, { props: { session }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toContain('current activity unverified')
    expect(wrapper.get('[data-session-badge]').attributes('data-session-badge')).toBe('completed')
    for (const [activityState, badge] of [['working', 'working'], ['waiting_permission', 'permission'], ['error', 'error']] as const) {
      await wrapper.setProps({ session: { ...session, activityState } })
      expect(wrapper.get('[data-session-badge]').attributes('data-session-badge')).toBe(badge)
    }
    await wrapper.setProps({ session: { ...session, processState: 'stopped', opened: false }, selected: true })
    expect(wrapper.get('[data-session-badge]').attributes('data-session-badge')).toBe('completed')
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toContain('Closed')
    await wrapper.get('.session-app-icon').trigger('click')
    expect(wrapper.emitted('primary-action')).toBeUndefined()
    wrapper.unmount()
  })
})
