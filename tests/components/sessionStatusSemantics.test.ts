import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import SessionStatusIcon from '@/components/sessions/SessionStatusIcon.vue'
import ProjectNode from '@/components/sessions/ProjectNode.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { SessionVisualState, UnifiedProjectGroup, UnifiedSession } from '@/types/unifiedSession'

const mounted: VueWrapper[] = []
beforeEach(() => { setActivePinia(createPinia()) })
afterEach(() => { mounted.splice(0).forEach(wrapper => wrapper.unmount()); document.body.innerHTML = '' })
function i18n(locale = 'en') { return createI18n({ legacy: false, locale, messages: { en, zh } }) }

describe('Preserved status rendering', () => {
  it.each([
    ['running', 'Running', '运行中', 'idle-dot'],
    ['working', 'Working', '工作中', 'work-dot'], ['permission', 'Waiting for permission', '等待权限', 'permission-bars'],
    ['completed', 'Response completed', '响应已完成', 'completed-ring'], ['error', 'Error', '出错', 'alert-circle'],
    ['stopped', 'Stopped', '已停止', 'stopped-dot'], ['closed', 'Closed', '已关闭', 'closed-ring'],
    ['unknown', 'Activity unknown', '活动未知', 'unknown-dot'],
  ])('Icon_DistinctSemanticState_001 %s', (state, english, chinese, shape) => {
    for (const [locale, label] of [['en', english], ['zh', chinese]]) {
      const wrapper = mount(SessionStatusIcon, { props: { state: state as SessionVisualState }, global: { plugins: [i18n(locale)] } })
      mounted.push(wrapper)
      expect(wrapper.get('[role="img"]').attributes('aria-label')).toBe(label)
      expect(wrapper.get('svg').attributes('data-shape')).toBe(shape)
      expect(wrapper.text()).toBe('')
    }
  })
  it('Icon_WorkAndPermissionPulseRespectReducedMotion_003', () => {
    const source = readFileSync(resolve('src/components/sessions/SessionStatusIcon.vue'), 'utf8')
    const style = document.createElement('style')
    style.textContent = source.match(/<style[^>]*>([\s\S]*?)<\/style>/)![1]
    document.head.append(style)
    const rules = Array.from(style.sheet!.cssRules)
    for (const state of ['working', 'permission']) {
      const animation = rules.find(rule => rule instanceof CSSStyleRule && rule.selectorText.includes(`--${state}`)
        && rule.style.getPropertyValue('animation')) as CSSStyleRule | undefined
      expect(animation?.style.getPropertyValue('animation')).toMatch(/infinite$/)
    }
    const reduced = rules.find(rule => rule instanceof CSSMediaRule && rule.conditionText === '(prefers-reduced-motion: reduce)') as CSSMediaRule
    expect(Array.from(reduced.cssRules).some(rule => rule instanceof CSSStyleRule
      && rule.selectorText === '.session-status-icon .session-status-icon__shape'
      && rule.style.getPropertyValue('animation') === 'none')).toBe(true)
    style.remove()
  })
  it('Icon_WorkDiffersFromIdleWithoutMotion_004', () => {
    const work = mount(SessionStatusIcon, { props: { state: 'working' }, global: { plugins: [i18n()] } })
    const idle = mount(SessionStatusIcon, { props: { state: 'running' }, global: { plugins: [i18n()] } })
    mounted.push(work, idle)
    expect(work.get('svg').element.innerHTML).not.toBe(idle.get('svg').element.innerHTML)
  })
  it('Project_CausePriorityAndCounts_002', async () => {
    const project: UnifiedProjectGroup = { projectKey: '/repo', projectPath: '/repo', name: 'Repo', sessions: [],
      pinned: false, hidden: false, runningCount: 4, needsUserCount: 6, lastActivityAt: 1,
      errorCount: 1, permissionCount: 2, completedCount: 3 }
    const wrapper = mount(ProjectNode, { props: { project, expanded: false }, global: { plugins: [i18n()] } })
    mounted.push(wrapper)
    expect(wrapper.get('[data-project-attention]').attributes('data-status-kind')).toBe('error')
    expect(wrapper.get('[data-project-attention]').attributes('aria-label')).toBe('Errors: 1')
    expect(wrapper.get('[data-project-attention]').text()).toBe('1')
    await wrapper.setProps({ project: { ...project, errorCount: 0 } })
    expect(wrapper.get('[data-project-attention]').attributes('data-status-kind')).toBe('permission')
    expect(wrapper.get('[data-project-attention]').attributes('aria-label')).toBe('Waiting for permission: 2')
    await wrapper.setProps({ project: { ...project, errorCount: 0, permissionCount: 0 } })
    expect(wrapper.get('[data-project-attention]').attributes('data-status-kind')).toBe('completed')
    await wrapper.setProps({ project: { ...project, errorCount: 0, permissionCount: 0, completedCount: 0, needsUserCount: 0 } })
    expect(wrapper.get('[data-project-attention]').attributes('data-status-kind')).toBe('running')
    expect(wrapper.get('[data-project-attention]').attributes('aria-label')).toBe('Running sessions: 4')
    await wrapper.setProps({ expanded: true })
    expect(wrapper.find('[data-project-attention]').exists()).toBe(false)
  })
  it('Project_RegisteredProjectionRetainsRowCauses_005', () => {
    const row: UnifiedSession = { id: 'one', projectKey: '/repo', projectPath: '/repo', runtime: 'legacy-claude', cli: 'claude',
      title: 'One', processState: 'running', attentionState: 'needs-user', attentionKind: 'permission',
      opened: true, archived: false, resumable: true, adapterSessionId: 'one', lastActivityAt: 1 }
    const project: UnifiedProjectGroup = { projectKey: '/repo', projectPath: '/repo', name: 'Repo',
      sessions: [row, { ...row, id: 'archived', archived: true, attentionKind: 'error' }],
      pinned: false, hidden: false, runningCount: 1, needsUserCount: 1, lastActivityAt: 1 }
    const wrapper = mount(ProjectNode, { props: { project, expanded: false }, global: { plugins: [i18n()] } })
    mounted.push(wrapper)
    expect(wrapper.get('[data-project-attention]').attributes('data-status-kind')).toBe('permission')
    expect(wrapper.get('[data-project-attention]').attributes('aria-label')).toBe('Waiting for permission: 1')
  })
})
