import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { nextTick } from 'vue'
import SessionsPanel from '@/components/sessions/SessionsPanel.vue'
import ProjectNode from '@/components/sessions/ProjectNode.vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useCliSupportStore } from '@/stores/cliSupport'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import type { SessionAdapter, UnifiedProjectGroup, UnifiedSession } from '@/types/unifiedSession'

function session(id: string, extra: Partial<UnifiedSession> = {}): UnifiedSession {
  return { id, adapterSessionId: id, projectKey: '/work/game', projectPath: '/work/game',
    cli: 'claude', runtime: 'native-cli', title: id, processState: 'running',
    attentionState: 'none', activityState: 'idle', opened: true, archived: false,
    resumable: true, createdAt: 10, lastActivityAt: 10, ...extra }
}
function group(sessions: UnifiedSession[], extra: Partial<UnifiedProjectGroup> = {}): UnifiedProjectGroup {
  return { projectKey: '/work/game', projectPath: '/work/game', name: 'Game', sessions,
    pinned: false, hidden: false, runningCount: 0, needsUserCount: 0, lastActivityAt: 10, ...extra }
}
let wrapper: VueWrapper
function makeI18n() {
  return createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
}
let i18n: ReturnType<typeof makeI18n>
beforeEach(() => {
  localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify({ claude: true, codex: true }))
  setActivePinia(createPinia())
  useProjectsStateStore().loaded = true
  i18n = makeI18n()
})
afterEach(() => { wrapper?.unmount(); document.body.innerHTML = ''; localStorage.clear() })
function panel(projectGroups?: UnifiedProjectGroup[]) {
  wrapper = mount(SessionsPanel, { attachTo: document.body,
    props: { ...(projectGroups ? { projectGroups } : {}), loading: false }, global: { plugins: [i18n] } })
  return wrapper
}
function rows() { return wrapper.findAll('[data-session-row]').map(row => row.attributes('data-session-row')) }
function toggle() { return wrapper.get('button[data-active-session-filter]') }

describe('Active session quick filter', () => {
  // 默认显示历史，开启后以打开身份覆盖空闲、等待、结束等状态，不依据标题或时间猜测。
  it('ActiveFilter_OpenLifecycle_001', async () => {
    const opened = [
      session('idle', { runtime: 'legacy-claude' }),
      session('working', { activityState: 'working' }),
      session('permission', { activityState: 'waiting_permission', attentionState: 'needs-user' }),
      session('ended', { processState: 'stopped' }),
      session('failed', { processState: 'failed' }),
      session('unknown', { processState: 'unknown', cli: 'codex' }),
      session('starting', { processState: 'starting' }),
    ]
    panel([group([...opened,
      session('recent active history', { opened: false, lastActivityAt: Date.now(), createdAt: 30 }),
      session('unproven', { opened: undefined }),
      session('archived', { archived: true }),
    ])])
    await wrapper.get('.project-main').trigger('click')
    const all = rows()
    expect(all).toContain('recent active history')
    expect(toggle().text()).toBe('Active sessions only')
    expect(toggle().attributes('aria-pressed')).toBe('false')
    await toggle().trigger('click')
    expect(toggle().attributes('aria-pressed')).toBe('true')
    expect(new Set(rows())).toEqual(new Set(opened.map(row => row.id)))
    expect(wrapper.emitted('activate')).toBeUndefined()
    await toggle().trigger('click')
    expect(rows()).toEqual(all)
  })

  // 活跃过滤先收窄会话，项目名和标题搜索只匹配保留下来的会话；空项目不冒充结果。
  it('ActiveFilter_SearchProjects_002', async () => {
    panel([group([session('needle idle'), session('needle history', { opened: false }),
      session('other idle', { cli: 'codex' })]),
    group([session('closed elsewhere', { opened: false })], { projectKey: '/work/closed', name: 'Closed', projectPath: '/work/closed' })])
    await toggle().trigger('click')
    await wrapper.get('input').setValue('game')
    expect(new Set(rows())).toEqual(new Set(['needle idle', 'other idle']))
    expect(wrapper.findAll('.project-main')).toHaveLength(1)
    await wrapper.get('input').setValue('needle')
    expect(rows()).toEqual(['needle idle'])
    await wrapper.get('input').setValue('closed')
    expect(rows()).toEqual([])
    expect(wrapper.get('.empty-hint').text()).toBe('No sessions found')
    await wrapper.get('input').setValue('')
    expect(wrapper.findAll('.project-main')).toHaveLength(1)
  })

  // 切换保持项目置顶顺序、CLI 分组和各组创建序，活动变化不重新排队。
  it('ActiveFilter_StableOrder_003', async () => {
    const game = group([session('older', { createdAt: 1 }), session('history', { opened: false, createdAt: 3 }),
      session('newer', { createdAt: 2 }), session('codex', { cli: 'codex', createdAt: 4 })])
    const pinned = group([session('pinned')], { projectKey: '/work/z', projectPath: '/work/z', name: 'Z', pinned: true })
    panel([game, pinned])
    for (const project of wrapper.findAll('.project-main')) await project.trigger('click')
    const all = rows()
    await toggle().trigger('click')
    expect(wrapper.findAll('.project-main').map(project => project.attributes('aria-label'))).toEqual(['Z', 'Game'])
    expect(rows()).toEqual(all.filter(id => id !== 'history'))
    expect(wrapper.findAll('[data-cli-group-header]')).toHaveLength(2)
    await wrapper.setProps({ projectGroups: [{ ...game, sessions: game.sessions.map(row => ({ ...row, lastActivityAt: 100, activityState: 'working' })) }, pinned] })
    expect(rows()).toEqual(all.filter(id => id !== 'history'))
    await toggle().trigger('click')
    expect(rows()).toEqual(all)
  })

  // 真实目录 close/refresh 生命周期替换为关闭历史后即时移除，关闭过滤即可找回。
  it('ActiveFilter_CloseRefresh_004', async () => {
    const open = session('owner', { processState: 'stopped' })
    let source = [open]
    const catalog = useUnifiedSessionsStore()
    catalog.configureAdapters([{ runtime: 'native-cli', listSessions: async () => source,
      captureOwnership: () => () => true,
      closeSession: async () => { source = [session('history', { opened: false, processState: 'stopped' })] },
    } as unknown as SessionAdapter])
    await catalog.refresh()
    panel()
    await wrapper.get('.project-main').trigger('click')
    await toggle().trigger('click')
    expect(rows()).toEqual(['owner'])
    await catalog.closeSession(open.id); await nextTick()
    expect(rows()).toEqual([])
    expect(wrapper.get('.empty-hint').text()).toBe('No active sessions')
    await toggle().trigger('click')
    expect(rows()).toEqual(['history'])
    source = [...source, session('new owner')]
    await catalog.refresh(); await nextTick()
    await toggle().trigger('click')
    expect(rows()).toEqual(['new owner'])
  })

  // 无活跃会话时给出双语空状态，返回全部不丢失搜索；新挂载仍默认关闭。
  it('ActiveFilter_EmptyLocale_005', async () => {
    panel([group([session('history', { opened: false })])])
    await toggle().trigger('click')
    expect(wrapper.get('.empty-hint').text()).toBe('No active sessions')
    i18n.global.locale.value = 'zh'; await nextTick()
    expect(toggle().text()).toBe('仅显示活跃会话')
    expect(wrapper.get('.empty-hint').text()).toBe('暂无活跃会话')
    await wrapper.get('input').setValue('missing')
    await toggle().trigger('click')
    expect((wrapper.get('input').element as HTMLInputElement).value).toBe('missing')
    expect(wrapper.get('.empty-hint').text()).toBe('未找到会话')
    wrapper.unmount()
    panel([group([])])
    expect(toggle().attributes('aria-pressed')).toBe('false')
  })

  // 终端键盘事件不会触发过滤或搬移焦点，控件保留原生按钮语义。
  it('ActiveFilter_KeyboardScope_006', async () => {
    panel([group([session('idle')])])
    const terminal = document.createElement('textarea')
    terminal.className = 'xterm-helper-textarea'; document.body.append(terminal); terminal.focus()
    for (const key of ['a', 'Enter', ' ', 'Escape', 'ArrowDown']) {
      terminal.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    }
    expect(toggle().attributes('aria-pressed')).toBe('false')
    expect(document.activeElement).toBe(terminal)
    expect(toggle().attributes('type')).toBe('button')
    ;(toggle().element as HTMLButtonElement).focus()
    await toggle().trigger('click')
    expect(document.activeElement).toBe(toggle().element)
    expect(wrapper.emitted('activate')).toBeUndefined()
  })

  // 停用 CLI 原有的历史隐藏规则继续生效，已打开的该 CLI 终端仍可见。
  it('ActiveFilter_CliSupport_007', async () => {
    const catalog = useUnifiedSessionsStore()
    const support = useCliSupportStore()
    catalog.configureAdapters([{ runtime: 'native-cli', listSessions: async () => [
      session('claude history', { opened: false }), session('claude idle'),
      session('codex history', { cli: 'codex', opened: false }), session('codex idle', { cli: 'codex' }),
    ] } as unknown as SessionAdapter])
    support.setCliEnabled('codex', false)
    await catalog.refresh()
    panel()
    await wrapper.get('.project-main').trigger('click')
    expect(new Set(rows())).toEqual(new Set(['claude history', 'claude idle', 'codex idle']))
    await toggle().trigger('click')
    expect(new Set(rows())).toEqual(new Set(['claude idle', 'codex idle']))
    await wrapper.get('input').setValue('codex')
    expect(rows()).toEqual(['codex idle'])
    await toggle().trigger('click')
    expect(rows()).toEqual(['codex idle'])
    support.setCliEnabled('codex', true)
    await catalog.refresh(); await nextTick()
    expect(new Set(rows())).toEqual(new Set(['codex history', 'codex idle']))
  })

  // 单项目状态变化保持无关项目的过滤投影身份，避免重渲染其整个会话子树。
  it('ActiveFilter_LocalRefresh_008', async () => {
    const first = group([session('a')])
    const other = group([session('b')], { projectKey: '/work/other', projectPath: '/work/other', name: 'Other' })
    panel([first, other])
    await toggle().trigger('click')
    const otherProps = () => wrapper.findAllComponents(ProjectNode).find(node => node.props('project').projectKey === other.projectKey)!.props('project')
    const before = otherProps()
    await wrapper.setProps({ projectGroups: [{ ...first, sessions: [session('a', { activityState: 'working' })] }, other] })
    expect(otherProps()).toBe(before)
    await wrapper.setProps({ projectGroups: [{ ...first, sessions: [session('a', { opened: false })] }, other] })
    expect(wrapper.findAll('.project-main')).toHaveLength(1)
    expect(otherProps()).toBe(before)
  })
})
