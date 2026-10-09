import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { nextTick } from 'vue'
import SessionItem from '@/components/sessions/SessionItem.vue'
import { selectSessionObservationNotice } from '@/utils/sessionPresentation'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { NativeObservationNoticeKind } from '@/types/nativeObservationNotice'

const mounted: VueWrapper[] = []
beforeEach(() => { setActivePinia(createPinia()) })
afterEach(() => { mounted.splice(0).forEach(wrapper => wrapper.unmount()); document.body.innerHTML = '' })
function row(kind: NativeObservationNoticeKind = 'reply-ended', unread = true): UnifiedSession {
  const notice = { kind, eventId: 'opaque-event', receivedAt: 100, runId: 'opaque-run', generation: 1 }
  return { id: 'native', cli: 'claude', runtime: 'native-cli', projectKey: '/repo', projectPath: '/repo', title: 'Native row',
    processState: 'running', activityState: 'unknown', attentionState: 'none', opened: true, archived: false,
    resumable: false, adapterSessionId: 'tab', lastActivityAt: Date.now(), observationNotice: { recent: notice, unreadReplyEnd: unread ? notice : null } }
}
function render(session: UnifiedSession, locale = 'en') {
  const wrapper = mount(SessionItem, { attachTo: document.body, props: { session },
    global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] } })
  mounted.push(wrapper); return wrapper
}

// 独立通知标记应可访问、双语且不把收到回复结束事件伪装为当前已完成状态。
describe('Honest Native event notice presentation', () => {
  it.each([
    ['en', 'Reply-end notice received (unread); current activity unverified'],
    ['zh', '收到回复结束通知（未读）；当前活动尚未验证'],
  ])('Notice_UnreadIsSeparateFromActivity_001 %s', async (locale, expected) => {
    const wrapper = render(row(), locale)
    const marker = wrapper.get('[data-native-observation-notice]')
    expect(marker.attributes('aria-label')).toBe(expected)
    expect(marker.attributes('data-unread')).toBe('true')
    expect(wrapper.get('.session-status-icon').attributes('aria-label')).toBe(locale === 'en' ? 'Activity unknown' : '活动未知')
    expect(wrapper.html()).not.toContain('opaque-event')
    expect(wrapper.html()).not.toContain('opaque-run')
    expect(wrapper.text()).not.toMatch(/completed|已完成/i)
    await marker.trigger('focus'); await nextTick()
    expect(document.querySelector('[role="tooltip"]')?.textContent).toBe(expected)
    await marker.trigger('click')
    expect(wrapper.emitted('activate')).toBeUndefined()
  })
  it('Notice_UnreadSurvivesLaterPromptAndAckShowsRecentReceipt_002', async () => {
    const session = row()
    session.observationNotice!.recent = { ...session.observationNotice!.recent!, kind: 'prompt-submitted', eventId: 'later' }
    const wrapper = render(session)
    expect(wrapper.get('[data-native-observation-notice]').attributes('aria-label')).toContain('Reply-end notice received (unread)')
    await wrapper.setProps({ session: { ...session, observationNotice: { ...session.observationNotice!, unreadReplyEnd: null } } })
    expect(wrapper.get('[data-native-observation-notice]').attributes('data-unread')).toBe('false')
    expect(wrapper.get('[data-native-observation-notice]').attributes('aria-label')).toBe('Recent notice: prompt submitted; current activity unverified')
    expect(wrapper.get('.session-status-icon').attributes('aria-label')).toBe('Activity unknown')
  })
  it.each(['legacy-claude', 'codex', 'stopped', 'archived', 'empty'] as const)('Notice_HiddenOutsideNativeReceiptScope_003 %s', invalidation => {
    const session = row()
    if (invalidation === 'legacy-claude') session.runtime = invalidation
    else if (invalidation === 'codex') session.cli = invalidation
    else if (invalidation === 'stopped') session.processState = invalidation
    else if (invalidation === 'archived') session.archived = true
    else session.observationNotice = { recent: null, unreadReplyEnd: null }
    expect(render(session).find('[data-native-observation-notice]').exists()).toBe(false)
  })
  it.each(['prompt-submitted', 'tool-started', 'tool-ended', 'tool-failed', 'reply-ended', 'reply-failed',
    'permission-requested', 'input-requested', 'subagent-started', 'subagent-ended', 'compaction-started', 'compaction-ended'] as const)
  ('Notice_EveryReceiptHasLocalizedPresentation_004 %s', kind => {
    const presentation = selectSessionObservationNotice(row(kind, false))!
    expect(presentation).toBeTruthy()
    expect(en[presentation.labelKey as keyof typeof en]).toBeTruthy()
    expect(zh[presentation.labelKey as keyof typeof zh]).toBeTruthy()
    expect(en[presentation.labelKey as keyof typeof en]).not.toMatch(/completed/i)
    expect(zh[presentation.labelKey as keyof typeof zh]).not.toContain('已完成')
  })
})
