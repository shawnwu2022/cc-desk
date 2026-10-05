import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DOMWrapper, mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { nextTick, computed } from 'vue'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import SessionItem from '@/components/sessions/SessionItem.vue'
import SessionList from '@/components/sessions/SessionList.vue'
import SessionOverflowMenu from '@/components/sessions/SessionOverflowMenu.vue'
import { selectSessionMenuActions } from '@/utils/sessionPresentation'
import type { SessionMenuAction, UnifiedSession } from '@/types/unifiedSession'

const body = new DOMWrapper(document.body)
const now = new Date(2026, 8, 30, 9, 0).getTime()
const base: UnifiedSession = {
  id: 'catalog-codex-1', projectKey: '/work/game', projectPath: '/work/game',
  cli: 'codex', runtime: 'native-cli', title: 'Fix a very long login title',
  processState: 'running', attentionState: 'none', lastActivityAt: now - 6 * 60_000,
  archived: false, opened: true, resumable: true, adapterSessionId: 'run-1', nativeSessionId: 'native-1',
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
  // 真实 IconButton/AppButton 组合切换焦点后，Escape 关闭提示但保留按钮焦点。
  it('Row_CloseTooltipEscape_038', async () => {
    const wrapper = row()
    const close = wrapper.get('.session-primary-action button')
    const overflow = wrapper.get('.session-overflow-trigger button')
    ;(overflow.element as HTMLButtonElement).focus()
    await nextTick()
    ;(close.element as HTMLButtonElement).focus()
    await nextTick()
    expect(body.get('[role="tooltip"]').text()).toBe('Close')
    close.element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(body.find('[role="tooltip"]').exists()).toBe(false)
    expect(document.activeElement).toBe(close.element)
    expect(close.attributes('aria-describedby')).toBeUndefined()
    expect(wrapper.emitted('primary-action')).toBeUndefined()
  })

  // 两种运行时的所有已打开状态只通过行内关闭按钮退出，菜单不重复关闭或停止。
  it.each(['native-cli', 'legacy-claude'] as const)('Row_SingleClose_035: %s', async runtime => {
    for (const processState of ['starting', 'running', 'unknown', 'stopped', 'failed'] as const) {
      const wrapper = row({ ...base, runtime, processState })
      const button = wrapper.get('.session-primary-action button')
      expect(button.attributes('aria-label')).toBe('Close')
      expect(button.attributes('disabled')).toBeUndefined()
      ;(button.element as HTMLButtonElement).focus()
      expect(document.activeElement).toBe(button.element)
      await button.trigger('keydown', { key: 'Enter' })
      await button.trigger('click')
      expect(wrapper.emitted('primary-action')).toEqual([[base.id, 'close']])
      expect(wrapper.emitted('activate')).toBeUndefined()
      await wrapper.trigger('keydown', { key: 'F10', shiftKey: true })
      expect(menuIds()).not.toContain('close')
      expect(menuIds()).not.toContain('stop')
      if (processState === 'running') expect(menuIds()).not.toContain('archive')
      await wrapper.setProps({ surfaceActive: false })
    }
  })

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
    expect(body.findAll('[role="tooltip"]').some((tooltip) => tooltip.text() === fullDate)).toBe(true)
    i18n.global.locale.value = 'zh'
    await wrapper.setProps({ session: { ...base, lastActivityAt: now - 20_000 } })
    expect(wrapper.get('.session-time').text()).toBe('刚刚')
    i18n.global.locale.value = 'en'
    await nextTick()
    expect(wrapper.get('.session-time').text()).toBe('now')
  })

  // 已打开会话所有状态共用关闭入口，归档条目仍提供恢复。
  it.each([
    { name: 'Row_StartingAction_003', session: { ...base, processState: 'starting' }, action: 'close' },
    { name: 'Row_RunningAction_004', session: base, action: 'close' },
    { name: 'Row_AttentionAction_005', session: { ...base, attentionState: 'needs-user' }, action: 'close' },
    { name: 'Row_UnknownAction_006', session: { ...base, processState: 'unknown' }, action: 'close' },
    { name: 'Row_StoppedAction_007', session: { ...base, processState: 'stopped' }, action: 'close' },
    { name: 'Row_FailedAction_008', session: { ...base, processState: 'failed' }, action: 'close' },
    { name: 'Row_PreparingCancel_036', session: { ...base, opened: false, preparationState: 'pending', processState: 'starting' }, action: 'cancel-start' },
    { name: 'Row_PreparingRetry_037', session: { ...base, opened: false, preparationState: 'failed', processState: 'failed' }, action: 'retry' },
    { name: 'Row_ArchivedAction_009', session: { ...base, processState: 'stopped', archived: true }, action: 'restore-archive' },
    { name: 'Row_NotResumable_010', session: { ...base, processState: 'stopped', resumable: false }, action: 'close' },
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
    const wrapper = row(base, { selected: true })
    await wrapper.trigger('keydown', { key: 'F2' })
    expect(wrapper.emitted('menu-action')?.slice(-1)[0]).toEqual([base.id, 'rename'])
    await wrapper.setProps({ session: { ...base, renameState: 'editing' } })
    await nextTick()
    const input = wrapper.get('input.rename-input')
    expect(document.activeElement).toBe(input.element)
    expect((input.element as HTMLInputElement).value).toBe(base.title)
    expect(wrapper.get('.session-primary-action button').attributes('aria-label')).toBe('Save name')
    await input.setValue('  Fix auth  ')
    await input.trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('rename-commit')).toEqual([[base.id, 'Fix auth']])
    await wrapper.setProps({ session: { ...base, renameState: 'idle' } })
    expect(wrapper.find('input').exists()).toBe(false)
    expect(wrapper.emitted('activate')).toBeUndefined()
    expect(wrapper.emitted('primary-action')).toBeUndefined()
    expect(document.activeElement).toBe(wrapper.element)
  })

  // Escape 取消一次；空名与控制字符不保存；保存按钮点击不会被 input blur 抢先取消。
  it('Row_RenameCancelAndSaveButton_013', async () => {
    const wrapper = row(base, { selected: true })
    await wrapper.trigger('keydown', { key: 'F2' })
    expect(wrapper.emitted('menu-action')?.slice(-1)[0]).toEqual([base.id, 'rename'])
    await wrapper.setProps({ session: { ...base, renameState: 'editing' } })
    await nextTick()
    await wrapper.get('input').setValue('ignored')
    await wrapper.get('input').trigger('keydown', { key: 'Escape' })
    expect(wrapper.emitted('rename-cancel')).toEqual([[base.id]])
    await wrapper.setProps({ session: { ...base, renameState: 'idle' } })
    expect(wrapper.emitted('rename-commit')).toBeUndefined()
    await wrapper.trigger('keydown', { key: 'F2' })
    expect(wrapper.emitted('menu-action')?.slice(-1)[0]).toEqual([base.id, 'rename'])
    await wrapper.setProps({ session: { ...base, renameState: 'editing' } })
    await nextTick()
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
    const wrapper = row({ ...base, renameState: 'saving' }, { selected: true })
    expect(wrapper.get('input').attributes('disabled')).toBeDefined()
    expect(wrapper.get('.session-primary-action button').attributes('disabled')).toBeDefined()
    await wrapper.setProps({ session: { ...base, renameState: 'idle' } })
    await wrapper.trigger('keydown', { key: 'F2' })
    expect(wrapper.emitted('menu-action')?.slice(-1)[0]).toEqual([base.id, 'rename'])
    await wrapper.setProps({ session: { ...base, renameState: 'editing' } })
    await nextTick()
    await wrapper.get('input').setValue('old draft')
    await wrapper.setProps({ session: { ...base, id: 'catalog-2', title: 'New session' } })
    expect(wrapper.find('input').exists()).toBe(false)
    expect(wrapper.get('.session-name').text()).toBe('New session')
    expect(wrapper.emitted('rename-commit')).toBeUndefined()
  })

  // 右键、更多及键盘菜单共享一组动作，选择的 typed ID 不会激活会话。
  it('Row_SharedContextAndOverflow_015', async () => {
    const wrapper = row(base, { selected: true })
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
    const wrapper = row(base, { selected: true, menuActionVisibility: { stop: false, 'view-diagnostics': false }, primaryAction: null })
    expect(wrapper.find('.session-primary-action button').exists()).toBe(false)
    await wrapper.trigger('contextmenu')
    await nextTick()
    expect(menuIds()).not.toContain('stop')
    expect(menuIds()).not.toContain('view-diagnostics')
    document.querySelector<HTMLElement>('[data-item-id="rename"]')!.click()
    await nextTick()
    expect(wrapper.emitted('menu-action')).toEqual([[base.id, 'rename']])
    expect(wrapper.find('input').exists(), 'opening waits for canonical admission').toBe(false)
    await wrapper.setProps({ session: { ...base, renameState: 'editing' } })
    await nextTick()
    await nextTick()
    expect(wrapper.find('input').exists()).toBe(true)
    expect(document.activeElement).toBe(wrapper.get('input').element)
    expect(wrapper.get('.session-primary-action button').attributes('aria-label')).toBe('Save name')
  })

  // 未选择行的更多、右键和F2都不能进入重命名；激活后才出现相同入口。
  it('Row_RenameNeedsSelection_031', async () => {
    const wrapper = row(base, { menuActionVisibility: { rename: true } })
    await wrapper.trigger('contextmenu')
    expect(menuIds()).not.toContain('rename')
    await wrapper.trigger('keydown', { key: 'F2' })
    expect(wrapper.emitted('menu-action')).toBeUndefined()
    await wrapper.setProps({ selected: true })
    expect(menuIds()).toContain('rename')
  })

  // 第一次点击激活即使同步改变selected，同一双击也只能激活一次，下一次双击才重命名。
  it('Row_DoubleClickKeepsFirstIntent_032', async () => {
    const wrapper = row()
    wrapper.get('.session-name').element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await nextTick()
    await wrapper.setProps({ selected: true })
    wrapper.get('.session-name').element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await nextTick()
    wrapper.get('.session-name').element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await nextTick()
    expect(wrapper.emitted('activate')).toEqual([[base.id]])
    expect(wrapper.emitted('menu-action')).toBeUndefined()
    wrapper.get('.session-name').element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await nextTick()
    wrapper.get('.session-name').element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await nextTick()
    wrapper.get('.session-name').element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await nextTick()
    expect(wrapper.emitted('menu-action')).toEqual([[base.id, 'rename']])
    expect(wrapper.emitted('primary-action')).toBeUndefined()
  })

  // 同一ID的新投影可能属于重启后的尝试，不能继承第一次点击的重命名意图。
  it('Row_DoubleClickRejectsReplacement_035', async () => {
    const wrapper = row(base, { selected: true })
    wrapper.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await nextTick()
    await wrapper.setProps({ session: { ...base } })
    wrapper.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await nextTick()
    wrapper.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await nextTick()
    expect(wrapper.emitted('menu-action')).toBeUndefined()
  })

  // 双击嵌套按钮、隐藏界面或禁用能力不能借冒泡进入重命名。
  it('Row_DoubleClickRespectsControls_033', async () => {
    const wrapper = row(base, { selected: true })
    wrapper.get('.session-primary-action button').element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await nextTick()
    wrapper.get('.session-overflow-trigger button').element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await nextTick()
    expect(wrapper.emitted('menu-action')).toBeUndefined()
    await wrapper.setProps({ menuActionVisibility: { rename: false } })
    wrapper.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await nextTick()
    expect(wrapper.emitted('menu-action')).toBeUndefined()
    await wrapper.setProps({ surfaceActive: false, menuActionVisibility: {} })
    wrapper.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await nextTick()
    expect(wrapper.emitted('menu-action')).toBeUndefined()
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
    { name: 'Menu_Running_018', session: base, actions: [...common, 'restart'] },
    { name: 'Menu_Stopped_019', session: { ...base, processState: 'stopped' }, actions: [...common, 'resume', 'restart', 'archive'] },
    { name: 'Menu_Failed_020', session: { ...base, processState: 'failed' }, actions: [...common, 'retry', 'restart', 'archive'] },
    { name: 'Menu_Archived_021', session: { ...base, processState: 'stopped', archived: true }, actions: [...common, 'restore-archive'] },
    { name: 'Menu_Unknown_022', session: { ...base, processState: 'unknown' }, actions: [...common, 'confirm-status'] },
    { name: 'Menu_Starting_023', session: { ...base, processState: 'starting' }, actions: [...common, 'cancel-start'] },
  ] as const)('$name', ({ session, actions }) => {
    const definitions = selectSessionMenuActions(session)
    expect(definitions.map((action) => action.id).sort()).toEqual([...actions].sort())
    const firstDanger = definitions.findIndex((action) => action.danger)
    if (firstDanger >= 0) expect(definitions.slice(firstDanger).every((action) => action.danger)).toBe(true)
    expect(new Set(definitions.map((action) => action.id)).size).toBe(definitions.length)
  })

  // 结束状态不能证明终端仍打开；历史条目不显示重启和关闭，显式开启只用于菜单呈现。
  it('Menu_ClosedHistoryOmitsLifecycle_034', () => {
    for (const runtime of ['legacy-claude', 'native-cli'] as const) {
      const history = { ...base, runtime, processState: 'stopped' as const, opened: false }
      const ids = selectSessionMenuActions(history, { restart: true, close: true }).map(action => action.id)
      expect(ids).not.toContain('restart')
      expect(ids).not.toContain('close')
      expect(ids).toContain('resume')
      expect(ids).toContain('archive')
      expect(selectSessionMenuActions({ ...history, opened: true }).map(action => action.id)).toEqual(expect.arrayContaining(['restart']))
    }
  })

  // 非运行历史的英中归档文案完整；组件只发出 typed 动作。
  it('Menu_LocalizedLabels_024', async () => {
    const wrapper = mount(SessionOverflowMenu, { attachTo: document.body,
      props: { open: true, actions: selectSessionMenuActions({ ...base, opened: false, processState: 'stopped' }), anchor: { x: 10, y: 10 } },
      global: { plugins: [i18n] },
    })
    mounted.push(wrapper)
    await nextTick()
    expect(document.querySelector('[data-item-id="archive"]')!.textContent).toBe('Archive')
    i18n.global.locale.value = 'zh'
    await nextTick()
    expect(document.querySelector('[data-item-id="archive"]')!.textContent).toBe('归档')
    expect(document.querySelector('[role="menu"]')!.textContent).not.toMatch(/sessionAction|Profile|Native|Legacy/)
    document.querySelector<HTMLElement>('[data-item-id="open-project-directory"]')!.click()
    await nextTick()
    expect(wrapper.emitted('menu-action')).toEqual([['open-project-directory']])
  })
})

describe('SessionList unified boundary', () => {
  // 统一列表直传统一 ID 与动作，不依赖旧 attention store。
  it('List_UnifiedEvents_025', async () => {
    const wrapper = mount(SessionList, { props: { sessions: [base], selectedId: base.id }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    await wrapper.get('.session-item').trigger('click')
    await wrapper.get('.session-primary-action button').trigger('click')
    expect(wrapper.emitted('activate')).toEqual([[base.id]])
    expect(wrapper.emitted('primary-action')).toEqual([[base.id, 'close']])
    expect(wrapper.get('.session-item').classes('active')).toBe(true)
  })

  // ProjectNode now provides unified identities only; no old events or legacy attention store are retained.
  it('List_UnifiedMenuAndRenameOnly_026', async () => {
    const history = { ...base, id: 'history-2', title: 'History', opened: false, processState: 'stopped' as const }
    const wrapper = mount(SessionList, { attachTo: document.body,
      props: { sessions: [base, history], selectedId: base.id }, global: { plugins: [i18n] },
    })
    mounted.push(wrapper)
    const rows = wrapper.findAllComponents(SessionItem)
    expect(rows).toHaveLength(2)
    await rows[0].trigger('keydown', { key: 'F2' })
    await wrapper.setProps({ sessions: [{ ...base, renameState: 'editing' }, history] })
    await rows[0].get('input').setValue('Renamed session')
    await rows[0].get('input').trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('rename-commit')).toEqual([[base.id, 'Renamed session']])
    expect(wrapper.emitted('rename')).toBeUndefined()
    await rows[1].get('.session-primary-action button').trigger('click')
    expect(wrapper.emitted('primary-action')).toEqual([[history.id, 'resume']])
    await rows[1].trigger('contextmenu'); await nextTick()
    document.querySelector<HTMLElement>('[data-item-id="archive"]')!.click()
    await nextTick()
    expect(wrapper.emitted('menu-action')).toEqual([[base.id, 'rename'], [history.id, 'archive']])
    expect(wrapper.emitted('archive')).toBeUndefined()
    const source = readFileSync(resolve('src/components/sessions/SessionList.vue'), 'utf8')
    expect(source).not.toMatch(/TerminalTab|HistorySession|useAttentionStore|isLegacy|legacyVisibility/)
  })
})

// 自定义重命名键也传到同一行编辑器，旧F2不再是绕过用户配置的第二个入口。
it('Row_ConfiguredRenameShortcut_033', async () => {
  const wrapper = mount(SessionItem, { props: { session: base, selected: true }, global: { plugins: [i18n], provide: { [Symbol.for('cc-desk.rename-shortcut')]: computed(() => 'Mod+KeyK') } } })
  mounted.push(wrapper)
  await wrapper.trigger('keydown', { key: 'F2', code: 'F2' }); expect(wrapper.find('input').exists()).toBe(false)
  await wrapper.trigger('keydown', { key: 'k', code: 'KeyK', ctrlKey: true })
  expect(wrapper.emitted('menu-action')).toEqual([[base.id, 'rename']])
  expect(wrapper.find('input').exists()).toBe(false)
  await wrapper.setProps({ session: { ...base, renameState: 'editing' } })
    await nextTick()
  expect(wrapper.find('input').exists()).toBe(true)
})

// The catalog preserves the external editing owner across ordinary runtime projection refreshes.
it('Row_ExternalRenameSurvivesProjection_034', async () => {
  const wrapper = row({ ...base, renameState: 'editing' }, { selected: true })
  await nextTick(); await wrapper.get('input').setValue('Typed display name')
  await wrapper.setProps({ session: { ...base, renameState: 'editing', lastActivityAt: base.lastActivityAt + 1 } })
  expect(wrapper.find('input').exists()).toBe(true)
  expect((wrapper.get('input').element as HTMLInputElement).value).toBe('Typed display name')
  await wrapper.setProps({ session: { ...base, renameState: 'idle' } })
  expect(wrapper.find('input').exists()).toBe(false)
})

// 选择移走后不保留可操作的旧编辑器，旧输入事件也不能提交。
it('Row_InactiveEditorCannotSave_036', async () => {
  const wrapper = row({ ...base, renameState: 'editing' }, { selected: true })
  const input = wrapper.get('input')
  await input.setValue('Old draft')
  await wrapper.setProps({ selected: false })
  expect(wrapper.find('input').exists()).toBe(false)
  await input.trigger('keydown', { key: 'Enter' })
  expect(wrapper.emitted('rename-commit')).toBeUndefined()
})

// 取消新建只对明确未准入的失败占位显示，不用于历史、已打开终端或未知准入。
it('Menu_DiscardRequiresFailedCreation_037', () => {
  const failed = { ...base, processState: 'failed' as const, opened: false, preparationState: 'failed' as const }
  expect(selectSessionMenuActions(failed).map(action => action.id)).toContain('discard-creation')
  for (const session of [base, { ...failed, opened: true }, { ...failed, preparationState: undefined }, { ...failed, preparationState: 'unknown' as const }]) {
    expect(selectSessionMenuActions(session).map(action => action.id)).not.toContain('discard-creation')
  }
})
