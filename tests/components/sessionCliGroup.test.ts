import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import SessionCliGroup from '@/components/sessions/SessionCliGroup.vue'
import SessionItem from '@/components/sessions/SessionItem.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import type { UnifiedSession } from '@/types/unifiedSession'

const now = new Date(2026, 8, 30, 9, 0).getTime()
function row(extra: Partial<UnifiedSession> = {}): UnifiedSession {
  return { id: 'cc-1', projectKey: '/work/game', projectPath: '/work/game', cli: 'claude',
    runtime: 'legacy-claude', title: 'Claude work', processState: 'running', attentionState: 'none',
    lastActivityAt: now - 60_000, archived: false, opened: true, resumable: true, adapterSessionId: 'adapter-1', ...extra }
}
const mounted: VueWrapper[] = []
let i18n: ReturnType<typeof createI18n>
beforeEach(() => {
  setActivePinia(createPinia())
  i18n = createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
})
afterEach(() => { mounted.splice(0).forEach(w => w.unmount()); document.body.innerHTML = '' })
function mountGroup(props: Record<string, unknown> = {}) {
  const wrapper = mount(SessionCliGroup, { attachTo: document.body,
    props: { cli: 'claude', sessions: [row(), row({ id: 'cc-2', title: 'Second' })], showHeader: true, toggleDisabled: false, collapsed: false, ...props },
    global: { plugins: [i18n] } })
  mounted.push(wrapper); return wrapper
}

describe('CLI group header', () => {
  // 多 CLI 项目渲染组头：名称、活跃/总数、以及指向本组列表根元素的 aria-controls。
  it('CliGroup_HeaderNamesCountAndControls_001', () => {
    const wrapper = mountGroup()
    // 根元素保留 data-cli-group 测试钩子且不带 role——SessionList 自带 role="group"，
    // 再包一层会形成嵌套 group。
    const root = wrapper.get('.cli-group')
    expect(root.attributes('data-cli-group')).toBe('claude')
    expect(root.attributes('role')).toBeUndefined()
    const header = wrapper.get('[data-cli-group-header]')
    expect(header.attributes('data-cli-group-header')).toBe('claude')
    expect(header.get('.cli-group-name').text()).toBe('Claude Code')
    // 两行都是 running，所以活跃/总数是 2/2。
    expect(header.get('.cli-group-count').text()).toBe('2/2')
    expect(header.attributes('aria-expanded')).toBe('true')
    const list = wrapper.get('.session-list')
    expect(list.attributes('role')).toBe('group')
    expect(header.attributes('aria-controls')).toBe(list.attributes('id'))
    expect(list.attributes('id')).toBeTruthy()
  })

  // 单 CLI 项目零视觉变化：不渲染组头，行恒可见。
  it('CliGroup_SingleCliRendersNoHeader_002', () => {
    const wrapper = mountGroup({ showHeader: false })
    expect(wrapper.find('[data-cli-group-header]').exists()).toBe(false)
    expect(wrapper.findAllComponents(SessionItem)).toHaveLength(2)
    expect((wrapper.get('.session-list').element as HTMLElement).style.display).not.toBe('none')
  })

  // 折叠用 v-show：行隐藏但 DOM 节点保持存活，未提交的重命名草稿不丢。
  it('CliGroup_CollapseHidesWithoutUnmounting_003', async () => {
    const wrapper = mountGroup({ collapsed: true })
    // 节点身份：v-if 卸载再挂载会产生新元素对象，v-show 不会。
    const before = wrapper.findAll('[data-session-row]').map(row => row.element)
    expect(before).toHaveLength(2)
    expect((wrapper.get('.session-list').element as HTMLElement).style.display).toBe('none')
    expect(wrapper.get('[data-cli-group-header]').attributes('aria-expanded')).toBe('false')
    await wrapper.setProps({ collapsed: false })
    expect((wrapper.get('.session-list').element as HTMLElement).style.display).not.toBe('none')
    expect(wrapper.get('[data-cli-group-header]').attributes('aria-expanded')).toBe('true')
    expect(wrapper.findAll('[data-session-row]').map(row => row.element)).toEqual(before)
  })

  // 搜索/改名期间强制展开并禁用开关，不出现「aria-expanded=true 却点击无效」。
  it('CliGroup_ForcedExpandDisablesSwitch_004', async () => {
    const wrapper = mountGroup({ collapsed: true, toggleDisabled: true })
    const header = wrapper.get('[data-cli-group-header]')
    expect(header.attributes('aria-expanded')).toBe('true')
    expect((header.element as HTMLButtonElement).disabled).toBe(true)
    expect((wrapper.get('.session-list').element as HTMLElement).style.display).not.toBe('none')
  })

  // 组头是原生 button：Enter/Space 由浏览器默认行为激活，代码无需自定义 keydown。
  it('CliGroup_HeaderIsNativeButton_005', async () => {
    const wrapper = mountGroup()
    const header = wrapper.get('[data-cli-group-header]')
    expect(header.element.tagName).toBe('BUTTON')
    expect(header.attributes('type')).toBe('button')
    expect(header.attributes('title')).toBeUndefined()
    await header.trigger('click')
    expect(wrapper.emitted('toggle')).toEqual([[]])
  })

  // 收起时徽章只统计本组行。SessionCliGroup 结构上拿不到项目聚合，所以这里用
  // 「单行组只能显示 1」来钉住计数口径——若误传项目总数就会是 2。
  it('CliGroup_CollapsedBadgeCountsOwnRowsOnly_006', () => {
    const wrapper = mountGroup({ collapsed: true, sessions: [row()] })
    const badge = wrapper.get('[data-cli-group-attention]')
    expect(badge.attributes('aria-label')).toBe('Running sessions: 1')
    expect(wrapper.findAll('[data-cli-group-attention]')).toHaveLength(1)
  })

  // 展开时不显示徽章（徽章是收起态的补偿信息）。
  it('CliGroup_ExpandedHidesBadge_007', () => {
    expect(mountGroup().find('[data-cli-group-attention]').exists()).toBe(false)
  })

  // 组头不重复会话行的 CLI 图标：那会让组头读成「多出来的一条会话行」。
  // 折叠指示用独立的箭头，且不制造额外的 Tab 止点。
  it('CliGroup_HeaderOmitsRowIconAndStaysSingleTabStop_008', () => {
    const wrapper = mountGroup()
    const header = wrapper.get('[data-cli-group-header]')
    expect(header.find('.cli-app-icon').exists()).toBe(false)
    expect(header.get('.cli-group-chevron').attributes('aria-hidden')).toBe('true')
    expect(header.attributes('tabindex')).toBeUndefined()
  })

  // 组头是紧凑的分节标签：两侧细线 + 居中标记，文字比会话标题更小。
  it('CliGroup_HeaderIsCenteredRuleLabel_009', () => {
    const wrapper = mountGroup()
    const header = wrapper.get('[data-cli-group-header]')
    expect(header.findAll('.cli-group-rule')).toHaveLength(2)
    const mark = header.get('.cli-group-mark')
    expect(mark.get('.cli-group-name').text()).toBe('Claude Code')
    expect(mark.get('.cli-group-count').text()).toBe('2/2')
  })

  // 活跃数按本组行计算且含 starting：与聚合口径一致，不显示项目总数。
  it('CliGroup_ActiveCountIncludesStarting_010', () => {
    const wrapper = mountGroup({ sessions: [
      row({ id: 'a', processState: 'running' }),
      row({ id: 'b', processState: 'starting' }),
      row({ id: 'c', processState: 'stopped', opened: false }),
    ] })
    expect(wrapper.get('.cli-group-count').text()).toBe('2/3')
  })
})
