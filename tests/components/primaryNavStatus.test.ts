import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { nextTick } from 'vue'
import { readFileSync } from 'node:fs'
import PrimaryNav from '@/components/shell/PrimaryNav.vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useShellStore } from '@/stores/shell'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import type { UnifiedSession } from '@/types/unifiedSession'

const mounted: VueWrapper[] = []
beforeEach(() => { setActivePinia(createPinia()) })
afterEach(() => { mounted.splice(0).forEach(wrapper => wrapper.unmount()); document.body.innerHTML = '' })
function row(id: string, extra: Partial<UnifiedSession> = {}): UnifiedSession {
  return { id, projectKey: '/repo', projectPath: '/repo', cli: 'claude', runtime: 'legacy-claude',
    title: 'Session', processState: 'running', attentionState: 'needs-user', archived: false,
    adapterSessionId: id, resumable: true, lastActivityAt: 1, ...extra }
}
function render(locale = 'en') {
  const i18n = createI18n({ legacy: false, locale, messages: { en, zh } })
  const wrapper = mount(PrimaryNav, { attachTo: document.body, global: { plugins: [i18n] } })
  mounted.push(wrapper)
  return { wrapper, i18n }
}
function description(wrapper: VueWrapper): HTMLElement {
  const id = wrapper.get('[data-primary-section="workspace"]').attributes('aria-describedby')
  expect(id).toBeTruthy()
  const result = document.getElementById(id!)!
  expect(result).not.toBeNull()
  expect(result.closest('[aria-hidden="true"]')).toBeNull()
  return result
}

describe('Workspace global status badge', () => {
  // 没有全局原因聚合或红优先会破坏实际渲染/可访问描述；完成不进入全局徽标。
  it('Badge_ErrorBeforePermissionAndReactiveRemoval_001', async () => {
    const catalog = useUnifiedSessionsStore()
    catalog.sessions = [row('permission', { attentionKind: 'permission' }), row('error', { attentionKind: 'error' }),
      row('error-detail', { activityState: 'error' }), row('completed', { attentionKind: 'completed' })]
    const { wrapper } = render()
    expect(wrapper.get('[data-workspace-status-badge]').attributes('data-status-kind')).toBe('error')
    expect(description(wrapper).textContent).toBe('Sessions with errors: 2')
    expect(wrapper.get('[data-primary-section="workspace"]').attributes('aria-label')).toBe('Workspace')
    expect(wrapper.get('[data-workspace-status-badge]').element.closest('[aria-hidden="true"]')).not.toBeNull()
    catalog.sessions = catalog.sessions.filter(session => session.attentionKind !== 'error' && session.activityState !== 'error')
    await nextTick()
    expect(wrapper.get('[data-workspace-status-badge]').attributes('data-status-kind')).toBe('permission')
    expect(description(wrapper).textContent).toBe('Sessions waiting for permission: 1')
    catalog.sessions = [row('completed', { attentionKind: 'completed' })]
    await nextTick()
    expect(wrapper.find('[data-workspace-status-badge]').exists()).toBe(false)
    expect(wrapper.get('[data-primary-section="workspace"]').attributes('aria-describedby')).toBeUndefined()
  })
  it('Badge_OnlyRunningNonArchivedExplicitCauses_002', () => {
    useUnifiedSessionsStore().sessions = [
      row('archived', { archived: true, attentionKind: 'error' }),
      row('stopped', { processState: 'stopped', attentionKind: 'error' }),
      row('failed', { processState: 'failed', attentionKind: 'error' }),
      row('starting', { processState: 'starting', attentionKind: 'permission' }),
      row('unknown', { processState: 'unknown', attentionKind: 'permission' }),
      row('pending'), row('working', { activityState: 'working' }), row('completed', { attentionKind: 'completed' }),
    ]
    const { wrapper } = render()
    expect(wrapper.find('[data-workspace-status-badge]').exists()).toBe(false)
  })
  it('Badge_LocalizedDetailPermissionSurvivesNavigation_003', async () => {
    useUnifiedSessionsStore().sessions = [row('detail', { activityState: 'waiting_permission' })]
    const { wrapper, i18n } = render()
    expect(description(wrapper).textContent).toBe('Sessions waiting for permission: 1')
    useShellStore().navigate('settings'); await nextTick()
    expect(wrapper.get('[data-workspace-status-badge]').attributes('data-status-kind')).toBe('permission')
    i18n.global.locale.value = 'zh'; await nextTick()
    expect(wrapper.get('[data-primary-section="workspace"]').attributes('aria-label')).toBe('工作区')
    expect(description(wrapper).textContent).toBe('等待权限的会话：1')
    useUnifiedSessionsStore().sessions = [row('error', { attentionKind: 'error' })]; await nextTick()
    expect(description(wrapper).textContent).toBe('出错的会话：1')
    expect(wrapper.findAll('[data-primary-section]')).toHaveLength(3)
  })
  it('Badge_UsesSemanticErrorAndPermissionColors_004', () => {
    const source = readFileSync('src/components/shell/PrimaryNav.vue', 'utf8')
    const style = document.createElement('style')
    style.textContent = source.match(/<style[^>]*>([\s\S]*?)<\/style>/)![1]
    document.head.append(style)
    const rules = Array.from(style.sheet!.cssRules).filter((rule): rule is CSSStyleRule => rule instanceof CSSStyleRule)
    for (const [kind, token] of [['error', '--status-error'], ['permission', '--accent-gold']]) {
      const rule = rules.find(rule => rule.selectorText === `.workspace-status-badge--${kind}`)
      expect(rule?.style.getPropertyValue('background')).toBe(`var(${token})`)
    }
    style.remove()
  })
})
