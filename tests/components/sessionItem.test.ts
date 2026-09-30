import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { createPinia } from 'pinia'
import { nextTick } from 'vue'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import SessionItem from '@/components/sessions/SessionItem.vue'
import SessionList from '@/components/sessions/SessionList.vue'
import SessionOverflowMenu from '@/components/sessions/SessionOverflowMenu.vue'
import { selectSessionMenuActions } from '@/utils/sessionPresentation'
import type { SessionMenuAction, UnifiedSession } from '@/types/unifiedSession'
import type { TerminalTab } from '@/stores/session'

const now = new Date(2026, 8, 30, 9, 0).getTime()
const base: UnifiedSession = {
  id: 'catalog-codex-1', projectKey: '/work/game', projectPath: '/work/game',
  cli: 'codex', runtime: 'native-cli', title: 'Fix a very long login title',
  processState: 'running', attentionState: 'none', lastActivityAt: now - 6 * 60_000,
  archived: false, resumable: true, adapterSessionId: 'run-1', nativeSessionId: 'native-1',
}
const mounted: VueWrapper[] = []
let i18n = createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(now)
  i18n = createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
})
afterEach(() => {
  mounted.splice(0).forEach((wrapper) => wrapper.unmount())
  document.body.innerHTML = ''
  document.head.querySelectorAll('[data-test-session-row]').forEach((style) => style.remove())
  vi.useRealTimers()
})
function row(session = base, extra: Record<string, unknown> = {}) {
  const wrapper = mount(SessionItem, {
    attachTo: document.body, props: { session, selected: false, ...extra },
    global: { plugins: [i18n] },
  })
  mounted.push(wrapper)
  return wrapper
}
function menuIds() {
  return Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'), (item) => item.dataset.itemId)
}
function styleRules() {
  const source = readFileSync(resolve('src/components/sessions/SessionItem.vue'), 'utf8')
  const style = document.createElement('style')
  style.dataset.testSessionRow = ''
  style.textContent = source.match(/<style[^>]*>([\s\S]*?)<\/style>/)![1]
  document.head.append(style)
  return Array.from(style.sheet!.cssRules).filter((rule): rule is CSSStyleRule => rule instanceof CSSStyleRule)
}

describe('Unified SessionItem', () => {
  // 生产样式固定五列与尾部覆盖层；不把 jsdom 的 CSS 检查当作真实布局验收。
  it('Row_FixedGeometry_001', () => {
    const rules = styleRules()
    const grid = rules.find((rule) => rule.selectorText === '.session-item')!
    expect(grid.style.getPropertyValue('display')).toBe('grid')
    expect(grid.style.getPropertyValue('grid-template-columns')).toBe('16px 18px minmax(0, 1fr) 38px 20px')
    expect(grid.style.getPropertyValue('column-gap')).toBe('6px')
    expect(Number.parseInt(grid.style.getPropertyValue('height'))).toBeGreaterThanOrEqual(36)
    expect(Number.parseInt(grid.style.getPropertyValue('height'))).toBeLessThanOrEqual(38)
    const title = rules.find((rule) => rule.selectorText === '.session-name')!
    expect(title.style.getPropertyValue('white-space')).toBe('nowrap')
    expect(title.style.getPropertyValue('overflow')).toBe('hidden')
    expect(title.style.getPropertyValue('text-overflow')).toBe('ellipsis')
    const age = rules.find((rule) => rule.selectorText === '.session-time')!
    expect(age.style.getPropertyValue('font-variant-numeric')).toBe('tabular-nums')
    expect(age.style.getPropertyValue('text-align')).toBe('right')
    expect(age.style.getPropertyValue('text-overflow')).toBe('')
    const action = rules.find((rule) => rule.selectorText === '.session-primary-action')!
    expect(action.style.getPropertyValue('position')).toBe('absolute')
    expect(action.style.getPropertyValue('opacity')).toBe('0')
    expect(rules.some((rule) => /:hover/.test(rule.selectorText) && /:focus-within/.test(rule.selectorText) && rule.selectorText.includes('.session-primary-action') && rule.style.getPropertyValue('opacity') === '1')).toBe(true)
    const movingRules = rules.filter((rule) => /:hover|:focus-within/.test(rule.selectorText))
    for (const rule of movingRules) {
      for (const property of ['width', 'grid-template-columns', 'column-gap', 'padding', 'margin']) {
        expect(rule.style.getPropertyValue(property), `${rule.selectorText} must not move row columns`).toBe('')
      }
    }
    const selection = rules.find((rule) => rule.selectorText === '.session-item.active::before')!
    expect(selection.style.getPropertyValue('width')).toBe('3px')
    expect(selection.style.getPropertyValue('background')).toBe('var(--accent-gold)')
  })

  // 默认只显示标题和紧凑年龄，状态与应用只用图标及可访问名称。
  it('Row_CompactTimeAndIcons_002', async () => {
    const wrapper = row()
    expect(wrapper.get('.session-time').text()).toBe('6m')
    expect(wrapper.get('.session-name').text()).toBe(base.title)
    expect(wrapper.get('.session-status-icon').attributes('aria-label')).toBe('Running')
    expect(wrapper.get('.cli-app-icon').attributes('aria-label')).toBe('Codex CLI')
    expect(wrapper.text()).toBe(`${base.title}6m`)
    await wrapper.get('.session-time').trigger('focus')
    const fullDate = new Date(base.lastActivityAt).toLocaleString('en')
    expect(wrapper.findAll('[role="tooltip"]').some((tooltip) => tooltip.text() === fullDate)).toBe(true)
    i18n.global.locale.value = 'zh'
    await wrapper.setProps({ session: { ...base, lastActivityAt: now - 20_000 } })
    expect(wrapper.get('.session-time').text()).toBe('刚刚')
    i18n.global.locale.value = 'en'
    await nextTick()
    expect(wrapper.get('.session-time').text()).toBe('now')
  })

  // 每个运行状态最多保留一个高频动作；需要回复及不可恢复会话保留时间。
  it.each([
    { name: 'Row_StartingAction_003', session: { ...base, processState: 'starting' }, action: 'cancel-start' },
    { name: 'Row_RunningAction_004', session: base, action: 'stop' },
    { name: 'Row_AttentionAction_005', session: { ...base, attentionState: 'needs-user' }, action: null },
    { name: 'Row_UnknownAction_006', session: { ...base, processState: 'unknown' }, action: 'confirm-status' },
    { name: 'Row_StoppedAction_007', session: { ...base, processState: 'stopped' }, action: 'resume' },
    { name: 'Row_FailedAction_008', session: { ...base, processState: 'failed' }, action: 'retry' },
    { name: 'Row_ArchivedAction_009', session: { ...base, processState: 'stopped', archived: true }, action: 'restore-archive' },
    { name: 'Row_NotResumable_010', session: { ...base, processState: 'stopped', resumable: false }, action: null },
  ] as const)('$name', async ({ session, action }) => {
    const wrapper = row(session)
    expect(wrapper.findAll('.session-primary-action button')).toHaveLength(action ? 1 : 0)
    expect(wrapper.classes('has-primary')).toBe(!!action)
    if (action) {
      await wrapper.get('.session-primary-action button').trigger('click')
      expect(wrapper.emitted('primary-action')).toEqual([[base.id, action]])
      expect(wrapper.emitted('activate')).toBeUndefined()
    }
  })

  // 点击、Enter 与 Space 激活统一 ID；选择变化不改变当前标题。
  it('Row_ActivationAndSelection_011', async () => {
    const wrapper = row(base, { selected: true })
    expect(wrapper.classes('active')).toBe(true)
    expect(wrapper.attributes('aria-selected')).toBe('true')
    await wrapper.trigger('click')
    await wrapper.trigger('keydown', { key: 'Enter' })
    await wrapper.trigger('keydown', { key: ' ' })
    expect(wrapper.emitted('activate')).toEqual([[base.id], [base.id], [base.id]])
    await wrapper.get('.session-overflow-trigger button').trigger('click')
    expect(wrapper.emitted('activate')).toHaveLength(3)
  })

  // F2 在标题原列编辑，保存仅提交显示名称，不触发激活或进程动作。
  it('Row_F2RenameCommit_012', async () => {
    const wrapper = row()
    await wrapper.trigger('keydown', { key: 'F2' })
    const input = wrapper.get('input.rename-input')
    expect(document.activeElement).toBe(input.element)
    expect((input.element as HTMLInputElement).value).toBe(base.title)
    expect(wrapper.get('.session-primary-action button').attributes('aria-label')).toBe('Save name')
    await input.setValue('  Fix auth  ')
    await input.trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('rename-commit')).toEqual([[base.id, 'Fix auth']])
    expect(wrapper.find('input').exists()).toBe(false)
    expect(wrapper.emitted('activate')).toBeUndefined()
    expect(wrapper.emitted('primary-action')).toBeUndefined()
    expect(document.activeElement).toBe(wrapper.element)
  })

  // Escape 取消一次；空名与控制字符不保存；保存按钮点击不会被 input blur 抢先取消。
  it('Row_RenameCancelAndSaveButton_013', async () => {
    const wrapper = row()
    await wrapper.trigger('keydown', { key: 'F2' })
    await wrapper.get('input').setValue('ignored')
    await wrapper.get('input').trigger('keydown', { key: 'Escape' })
    expect(wrapper.emitted('rename-cancel')).toEqual([[base.id]])
    expect(wrapper.emitted('rename-commit')).toBeUndefined()
    await wrapper.trigger('keydown', { key: 'F2' })
    await wrapper.get('input').setValue('  ')
    await wrapper.get('input').trigger('keydown', { key: 'Enter' })
    expect(wrapper.find('input').exists()).toBe(true)
    expect(wrapper.get('input').attributes('aria-invalid')).toBe('true')
    await wrapper.get('input').setValue('bad\u0000name')
    await wrapper.get('input').trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('rename-commit')).toBeUndefined()
    await wrapper.get('input').setValue('Saved by button')
    await wrapper.get('input').trigger('blur')
    await wrapper.get('.session-primary-action button').trigger('click')
    expect(wrapper.emitted('rename-commit')).toEqual([[base.id, 'Saved by button']])
  })

  // 外部保存态禁用编辑；复用成另一个会话时清除旧草稿，不能给新 ID 提交旧名。
  it('Row_RenameOwnership_014', async () => {
    const wrapper = row({ ...base, renameState: 'saving' })
    expect(wrapper.get('input').attributes('disabled')).toBeDefined()
    expect(wrapper.get('.session-primary-action button').attributes('disabled')).toBeDefined()
    await wrapper.setProps({ session: { ...base, renameState: 'idle' } })
    await wrapper.trigger('keydown', { key: 'F2' })
    await wrapper.get('input').setValue('old draft')
    await wrapper.setProps({ session: { ...base, id: 'catalog-2', title: 'New session' } })
    expect(wrapper.find('input').exists()).toBe(false)
    expect(wrapper.get('.session-name').text()).toBe('New session')
    expect(wrapper.emitted('rename-commit')).toBeUndefined()
  })

  // 右键、更多及键盘菜单共享一组动作，选择的 typed ID 不会激活会话。
  it('Row_SharedContextAndOverflow_015', async () => {
    const wrapper = row()
    const trigger = wrapper.get('.session-overflow-trigger button')
    ;(trigger.element as HTMLElement).focus()
    await trigger.trigger('click')
    await nextTick()
    const overflowIds = menuIds()
    expect(overflowIds).toEqual(selectSessionMenuActions(base).map((action) => action.id))
    document.querySelector<HTMLElement>('[role="menu"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(document.activeElement).toBe(trigger.element)
    await wrapper.trigger('contextmenu', { clientX: 1010, clientY: 750 })
    await nextTick()
    expect(menuIds()).toEqual(overflowIds)
    const menu = document.querySelector<HTMLElement>('[role="menu"]')!
    expect(Number.parseFloat(menu.style.left)).toBeLessThanOrEqual(window.innerWidth - 200)
    const copy = menu.querySelector<HTMLElement>('[data-item-id="copy-session-id"]')!
    copy.click()
    await nextTick()
    expect(wrapper.emitted('menu-action')).toEqual([[base.id, 'copy-session-id']])
    expect(wrapper.emitted('activate')).toBeUndefined()
    expect(document.querySelector('[role="menu"]')).toBeNull()
    await wrapper.trigger('keydown', { key: 'F10', shiftKey: true })
    await nextTick()
    expect(menuIds()).toEqual(overflowIds)
  })

  // 列表配置只隐藏指定动作；未列出的动作使用统一状态规则。
  it('Row_ActionVisibilityAndMenuRename_016', async () => {
    const wrapper = row(base, { menuActionVisibility: { stop: false, 'view-diagnostics': false }, primaryAction: null })
    expect(wrapper.find('.session-primary-action button').exists()).toBe(false)
    await wrapper.trigger('contextmenu')
    await nextTick()
    expect(menuIds()).not.toContain('stop')
    expect(menuIds()).not.toContain('view-diagnostics')
    document.querySelector<HTMLElement>('[data-item-id="rename"]')!.click()
    await nextTick()
    expect(wrapper.emitted('menu-action')).toEqual([[base.id, 'rename']])
    expect(wrapper.find('input').exists()).toBe(true)
    expect(document.activeElement).toBe(wrapper.get('input').element)
    expect(wrapper.get('.session-primary-action button').attributes('aria-label')).toBe('Save name')
  })

  // 多行只创建一个刷新器；移除最后一行销毁定时器，恢复列表时取当前时间。
  it('Row_OneSharedClock_017', async () => {
    const first = row()
    const second = row({ ...base, id: 'catalog-2' })
    expect(vi.getTimerCount()).toBe(1)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(first.get('.session-time').text()).toBe('7m')
    expect(second.get('.session-time').text()).toBe('7m')
    first.unmount()
    expect(vi.getTimerCount()).toBe(1)
    second.unmount()
    expect(vi.getTimerCount()).toBe(0)
    vi.setSystemTime(now + 3 * 60_000)
    expect(row().get('.session-time').text()).toBe('9m')
  })

  // 打开状态再次点击更多必须关闭，不能被共享菜单的 outside pointer 提前关闭又重开。
  it('Row_OverflowToggle_027', async () => {
    const wrapper = row()
    const trigger = wrapper.get('.session-overflow-trigger button')
    await trigger.trigger('click')
    await nextTick()
    expect(document.querySelector('[role="menu"]')).not.toBeNull()
    await trigger.trigger('pointerdown')
    await trigger.trigger('click')
    await nextTick()
    expect(document.querySelector('[role="menu"]')).toBeNull()
    expect(trigger.attributes('aria-expanded')).toBe('false')
  })

  // 仅当前已打开行拦截自己的 opener；另一个行必须把 pointerdown 交给 outside-dismiss。
  it('Row_AnotherOverflowDismissesPrevious_028', async () => {
    const wrapper = mount(SessionList, { attachTo: document.body,
      props: { sessions: [base, { ...base, id: 'catalog-2', title: 'Second session' }] },
      global: { plugins: [i18n] },
    })
    mounted.push(wrapper)
    const triggers = wrapper.findAll('.session-overflow-trigger button')
    await triggers[0].trigger('pointerdown')
    await triggers[0].trigger('click')
    await nextTick()
    expect(document.querySelectorAll('[role="menu"]')).toHaveLength(1)
    await triggers[1].trigger('pointerdown')
    await triggers[1].trigger('click')
    await nextTick()
    expect(document.querySelectorAll('[role="menu"]')).toHaveLength(1)
    expect(triggers.map((trigger) => trigger.attributes('aria-expanded'))).toEqual(['false', 'true'])
    expect(document.activeElement?.closest('[role="menu"]')).toBe(document.querySelector('[role="menu"]'))
    await triggers[1].trigger('pointerdown')
    await triggers[1].trigger('click')
    await nextTick()
    expect(document.querySelectorAll('[role="menu"]')).toHaveLength(0)
    expect(triggers.map((trigger) => trigger.attributes('aria-expanded'))).toEqual(['false', 'false'])
  })
})

describe('Unified session menu model', () => {
  const common: SessionMenuAction[] = ['rename', 'copy-session-id', 'open-project-directory', 'view-diagnostics']
  // 每个状态提供完整能力并把危险动作排在最后；未知态禁止再次启动。
  it.each([
    { name: 'Menu_Running_018', session: base, actions: [...common, 'stop', 'restart', 'close', 'archive'] },
    { name: 'Menu_Stopped_019', session: { ...base, processState: 'stopped' }, actions: [...common, 'resume', 'restart', 'close', 'archive'] },
    { name: 'Menu_Failed_020', session: { ...base, processState: 'failed' }, actions: [...common, 'retry', 'restart', 'close', 'archive'] },
    { name: 'Menu_Archived_021', session: { ...base, processState: 'stopped', archived: true }, actions: [...common, 'restore-archive'] },
    { name: 'Menu_Unknown_022', session: { ...base, processState: 'unknown' }, actions: [...common, 'confirm-status', 'close'] },
    { name: 'Menu_Starting_023', session: { ...base, processState: 'starting' }, actions: [...common, 'cancel-start', 'close'] },
  ] as const)('$name', ({ session, actions }) => {
    const definitions = selectSessionMenuActions(session)
    expect(definitions.map((action) => action.id).sort()).toEqual([...actions].sort())
    const firstDanger = definitions.findIndex((action) => action.danger)
    if (firstDanger >= 0) expect(definitions.slice(firstDanger).every((action) => action.danger)).toBe(true)
    expect(new Set(definitions.map((action) => action.id)).size).toBe(definitions.length)
  })

  // 运行态归档文字明确停止后归档，英中菜单文案均完整；组件只发出 typed 动作。
  it('Menu_LocalizedLabels_024', async () => {
    const wrapper = mount(SessionOverflowMenu, { attachTo: document.body,
      props: { open: true, actions: selectSessionMenuActions(base), anchor: { x: 10, y: 10 } },
      global: { plugins: [i18n] },
    })
    mounted.push(wrapper)
    await nextTick()
    expect(document.querySelector('[data-item-id="archive"]')!.textContent).toBe('Stop and archive')
    i18n.global.locale.value = 'zh'
    await nextTick()
    expect(document.querySelector('[data-item-id="archive"]')!.textContent).toBe('停止并归档')
    expect(document.querySelector('[role="menu"]')!.textContent).not.toMatch(/sessionAction|Profile|Native|Legacy/)
    document.querySelector<HTMLElement>('[data-item-id="open-project-directory"]')!.click()
    await nextTick()
    expect(wrapper.emitted('menu-action')).toEqual([['open-project-directory']])
  })
})

describe('SessionList unified and temporary legacy boundary', () => {
  // 统一列表直传统一 ID 与动作，不依赖旧 attention store。
  it('List_UnifiedEvents_025', async () => {
    const wrapper = mount(SessionList, { props: { sessions: [base], selectedId: base.id }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    await wrapper.get('.session-item').trigger('click')
    await wrapper.get('.session-primary-action button').trigger('click')
    expect(wrapper.emitted('activate')).toEqual([[base.id]])
    expect(wrapper.emitted('primary-action')).toEqual([[base.id, 'stop']])
    expect(wrapper.get('.session-item').classes('active')).toBe(true)
  })

  // 过渡旧调用只映射数据与事件，停止 tab 可恢复；历史归档与点击恢复继续有效。
  it('List_LegacySupportedActions_026', async () => {
    const tab: TerminalTab = { tabId: 'tab-1', projectPath: '/work/game', ptyId: null,
      sessionId: 'history-1', name: 'Stopped tab', status: 'stopped', createdAt: now,
      lastActiveAt: now - 2 * 60_000, working: false, pending: false, isResume: true }
    const wrapper = mount(SessionList, { attachTo: document.body,
      props: { tabs: [tab], history: [{ sessionId: 'history-2', name: 'History', projectPath: '/work/game', lastActiveAt: now }], activeId: 'tab-1', closable: true },
      global: { plugins: [i18n, createPinia()] },
    })
    mounted.push(wrapper)
    const rows = wrapper.findAllComponents(SessionItem)
    expect(rows).toHaveLength(2)
    expect(rows[0].props('session').cli).toBe('claude')
    expect(rows[0].props('session').runtime).toBe('legacy-claude')
    expect(rows[0].get('.session-time').text()).toBe('2m')
    await rows[0].get('.session-primary-action button').trigger('click')
    expect(wrapper.emitted('restart')).toEqual([['tab-1']])
    await rows[0].trigger('keydown', { key: 'F2' })
    await rows[0].get('input').setValue('Renamed tab')
    await rows[0].get('input').trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('rename')).toEqual([['tab-1', 'Renamed tab']])
    await rows[0].trigger('contextmenu')
    await nextTick()
    expect(menuIds()).toEqual(expect.arrayContaining(['rename', 'restart', 'close']))
    expect(menuIds()).not.toContain('view-diagnostics')
    document.querySelector<HTMLElement>('[data-item-id="close"]')!.click()
    await nextTick()
    expect(wrapper.emitted('close')).toEqual([['tab-1']])
    await rows[1].trigger('click')
    expect(wrapper.emitted('switch')).toEqual([['history-2']])
    await rows[1].trigger('contextmenu')
    await nextTick()
    document.querySelector<HTMLElement>('[data-item-id="archive"]')!.click()
    await nextTick()
    expect(wrapper.emitted('archive')).toEqual([['history-2']])
  })
})
