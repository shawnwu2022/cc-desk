import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { captureNativeAttempt, useNativeTabsStore } from '@/stores/nativeTabs'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import SessionItem from '@/components/sessions/SessionItem.vue'
import SessionAppIcon from '@/components/sessions/SessionAppIcon.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import type { UnifiedSession } from '@/types/unifiedSession'

afterEach(() => { document.body.innerHTML = ''; vi.unstubAllGlobals(); clearMocks() })
describe('Merged session app status', () => {
  // 认证身份 hook 不产生 TUI 活动；尚未收到原生状态时不能伪装成空闲或工作。
  it.each([
    ['new', 'en', 'Activity unknown', 'Closed'], ['resume-id', 'en', 'Activity unknown', 'Closed'],
    ['new', 'zh', '活动未知', '已关闭'], ['resume-id', 'zh', '活动未知', '已关闭'],
  ] as const)('Codex_IdentityReceiptsCannotInventActivity_007 %s %s', async (kind, locale, runningLabel, closedLabel) => {
    setActivePinia(createPinia())
    mockIPC(() => undefined)
    const tabs = useNativeTabsStore()
    const nativeSessionId = '11111111-1111-4111-8111-111111111111'
    const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'codex', profileRevision: '1',
      action: kind === 'new' ? { kind } : { kind, nativeSessionId } })
    const owner = captureNativeAttempt(tab)
    const launch = (phase: LaunchStatus['phase'], revision: string): LaunchStatus => ({ instanceId: 'host', requestId: owner.requestId,
      run: { runId: owner.runId, generation: owner.generation }, phase, revision, failure: null })
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [] },
      runtime: { createTab: () => tab, restartTab: () => tab, stopTab: async () => {}, renameTab: async () => {} },
      archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
    const project = async () => (await adapter.listSessions())[0]
    const wrapper = mount(SessionItem, { props: { session: await project() },
      global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] } })
    expect(wrapper.get('[data-session-badge]').attributes('data-session-badge')).toBe('starting')
    tabs.applyLaunchStatus(tab.tabId, launch('running', '1'))
    const running = await project()
    expect(running).toMatchObject({ processState: 'running', activityState: 'unknown', observationState: 'unavailable', attentionState: 'none' })
    await wrapper.setProps({ session: running })
    expect(wrapper.find('[data-session-badge="unknown"]').exists()).toBe(true)
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toBe(`Codex CLI; ${runningLabel}`)
    for (const event of ['sessionStart', 'userPromptSubmit', 'stop']) {
      expect(tabs.applyObservationIdentity(tab.tabId, owner, nativeSessionId, event)).toBe(true)
      const session = await project()
      expect(session.nativeSessionId).toBe(nativeSessionId)
      expect(session.activityState).toBe('unknown')
      expect(session.attentionKind).toBeUndefined()
      expect(session.observationNotice).toBeUndefined()
      await wrapper.setProps({ session, selected: true })
      expect(wrapper.find('[data-session-badge="unknown"]').exists()).toBe(true)
      expect(wrapper.find('[data-completion-entry]').exists()).toBe(false)
    }
    expect(tabs.applyObservationIdentity(tab.tabId, { ...owner, generation: owner.generation + 1 }, nativeSessionId)).toBe(false)
    tabs.applyLaunchStatus(tab.tabId, launch('exited', '2'))
    await wrapper.setProps({ session: await project() })
    expect(wrapper.get('[data-session-badge]').attributes('data-session-badge')).toBe('stopped')
    expect(tabs.applyObservationIdentity(tab.tabId, owner, nativeSessionId, 'stop')).toBe(false)
    await wrapper.setProps({ session: { ...await project(), opened: false } })
    expect(wrapper.find('[data-session-badge]').exists()).toBe(false)
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toBe(`Codex CLI; ${closedLabel}`)
    wrapper.unmount()
  })

  it('Observer_ReasonDistinguishesIdleOffAndUnavailable_006', async () => {
    const session: UnifiedSession = { id:'one',projectKey:'/repo',projectPath:'/repo',cli:'claude',runtime:'native-cli',title:'One',processState:'running',activityState:'unknown',observationState:'unavailable',observerReasonCode:'OBSERVER_PLUGIN_UNAVAILABLE',attentionState:'none',opened:true,archived:false,resumable:true,adapterSessionId:'one',lastActivityAt:1 }
    const wrapper = mount(SessionItem,{props:{session},global:{plugins:[createI18n({legacy:false,locale:'en',messages:{en,zh}})]}})
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toContain('Observer plugin unavailable')
    await wrapper.setProps({session:{...session,observationState:'off',observerReasonCode:'OBSERVER_DISABLED'}})
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toContain('Activity observation disabled')
    await wrapper.setProps({session:{...session,observationState:'active',observerReasonCode:undefined,activityState:'idle'}})
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toBe('Claude Code; Running')
    expect(wrapper.find('[data-session-badge]').exists()).toBe(false)
    wrapper.unmount()
  })
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
    expect(wrapper.find('[data-session-badge]').exists()).toBe(false)
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toContain('Closed')
    expect(wrapper.get('.session-app-icon').attributes('aria-label')).toContain('current activity unverified')
    expect(wrapper.get('.session-app-icon').attributes('data-unread')).toBe('true')
    await wrapper.get('.session-app-icon').trigger('click')
    expect(wrapper.emitted('primary-action')).toBeUndefined()
    wrapper.unmount()
  })
  // 关闭态即使带回执或归档上下文也不加角标；状态切换仍保留必要提示。
  it('AppIcon_ClosedNeverAddsBadge_005', async () => {
    const wrapper = mount(SessionAppIcon, { props: { cli: 'codex', state: 'closed' }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    const image = wrapper.get('img').element
    for (const context of [{}, { notice: { labelKey: 'nativeNoticeReplyEndUnread', unread: true } }, { archived: true }]) {
      await wrapper.setProps(context)
      expect(wrapper.find('[data-session-badge]').exists()).toBe(false)
      expect(wrapper.get('img').element).toBe(image)
      expect(wrapper.get('.session-app-icon').attributes('aria-label')).toContain('Codex CLI')
    }
    await wrapper.setProps({ archived: false, notice: null })
    for (const state of ['working', 'needs-user', 'permission', 'error', 'stopped'] as const) {
      await wrapper.setProps({ state })
      expect(wrapper.get('[data-session-badge]').attributes('data-session-badge')).toBe(state)
    }
    wrapper.unmount()
  })
})
