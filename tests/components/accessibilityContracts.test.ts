import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { DOMWrapper, mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import { h } from 'vue'
import { readFileSync } from 'node:fs'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import IconButton from '@/components/ui/IconButton.vue'
import SessionItem from '@/components/sessions/SessionItem.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
const body = new DOMWrapper(document.body)
const wrappers: VueWrapper[] = []
function localePlugin() { return createI18n({ legacy: false, locale: 'en', messages: { en, zh } }) }
let i18n: ReturnType<typeof localePlugin>
beforeEach(() => {
  setActivePinia(createPinia()); i18n = localePlugin()
  vi.stubGlobal('innerWidth', 1024); vi.stubGlobal('innerHeight', 640)
  const style = document.createElement('style'); style.dataset.contractStyle = ''; style.textContent = readFileSync('src/styles/global.css', 'utf8'); document.head.append(style)
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); document.body.innerHTML = ''; document.head.querySelectorAll('[data-contract-style]').forEach(el => el.remove()); vi.restoreAllMocks(); vi.unstubAllGlobals() })
// 折叠 details 内的 autofocus 不得抢焦点；Tab 循环只考虑用户能访问的控件。
it('A11y_CollapsedDetailsFocus_001', async () => {
  const w = mount(AppDialog, { attachTo: document.body, props: { open: true, title: 'Options', showClose: false }, global: { plugins: [i18n] }, slots: { default: '<button data-first>Safe action</button><details><summary>Developer options</summary><input autofocus aria-label="Hidden value"><details open><summary>Nested</summary><button data-hidden>Hidden action</button></details></details>' } }); wrappers.push(w); await flushPromises()
  const first = document.querySelector<HTMLElement>('[data-first]')!
  expect(document.activeElement).toBe(first)
  first.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true }))
  expect(document.activeElement?.textContent).toBe('Developer options')
  document.querySelector('details')!.open = true; first.focus()
  first.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true }))
  expect(document.activeElement).toBe(document.querySelector('[data-hidden]'))
})
// 关闭弹窗不把焦点返回已经隐藏的导航表面。
it('A11y_NoHiddenFocusReturn_002', async () => {
  const surface = document.createElement('section'); const opener = document.createElement('button'); surface.append(opener); document.body.append(surface); opener.focus()
  const w = mount(AppDialog, { attachTo: document.body, props: { open: true, title: 'Confirm' }, global: { plugins: [i18n] } }); wrappers.push(w); await flushPromises()
  surface.style.display = 'none'; await w.setProps({ open: false }); await flushPromises()
  expect(document.activeElement).not.toBe(opener)
})
// 提示按实际触发点的视口坐标定位，不能被会话栏 overflow 裁掉或越过右/下边缘。
it('A11y_ViewportBoundTooltip_003', async () => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    return (this.classList.contains('ui-tooltip') ? { x: 0, y: 0, left: 0, top: 0, right: 220, bottom: 42, width: 220, height: 42 } : { x: 990, y: 608, left: 990, top: 608, right: 1018, bottom: 636, width: 28, height: 28 }) as DOMRect
  })
  const w = mount(IconButton, { attachTo: document.body, props: { label: 'A long project path ' + 'x'.repeat(160) }, global: { plugins: [i18n] }, slots: { default: '⋯' } }); wrappers.push(w)
  ;(w.get('button').element as HTMLElement).focus(); await flushPromises()
  const tooltip = body.get('[role="tooltip"]').element as HTMLElement
  expect(getComputedStyle(tooltip).position).toBe('fixed')
  const left = Number.parseFloat(tooltip.style.left), top = Number.parseFloat(tooltip.style.top)
  expect(left).toBeGreaterThanOrEqual(12); expect(left + 220).toBeLessThanOrEqual(1012)
  expect(top + 42).toBeLessThanOrEqual(602)
  expect(w.get('button').attributes('aria-describedby')).toBe(tooltip.id)
  await w.get('button').trigger('keydown', { key: 'Escape' }); expect(body.find('[role="tooltip"]').exists()).toBe(false)
})
// 中英文真实会话行保留图标语义；状态仅在焦点提示出现，菜单键不触发会话动作。
it.each(['en', 'zh'] as const)('A11y_RowKeyboard_004_%s', async locale => {
  i18n.global.locale.value = locale
  const w = mount(SessionItem, { attachTo: document.body, global: { plugins: [i18n] }, props: { selected: true, session: { id: 'row', cli: 'codex', runtime: 'native-cli', adapterSessionId: 'tab', projectKey: '/repo', projectPath: '/repo', title: 'A'.repeat(200), processState: 'running', activityState: 'idle', attentionState: 'none', archived: false, resumable: true, lastActivityAt: Date.now() } } }); wrappers.push(w)
  expect(w.text()).not.toContain(locale === 'en' ? en.sessionStatusRunning : zh.sessionStatusRunning)
  const icon = w.get('.session-status-icon'); (icon.element as HTMLElement).focus(); await flushPromises()
  expect(body.get('[role="tooltip"]').text()).toBe(locale === 'en' ? en.sessionStatusRunning : zh.sessionStatusRunning)
  await icon.trigger('keydown', { key: 'Escape' }); await w.trigger('keydown', { key: 'ContextMenu' }); await flushPromises()
  const menu = document.querySelector<HTMLElement>('[role="menu"]')!
  expect(menu).not.toBeNull(); menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true, cancelable: true }))
  expect(document.activeElement).toBe(menu.querySelector('[data-item-id="view-diagnostics"]'))
  menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })); await flushPromises()
  expect(document.querySelector('[role="menu"]')).toBeNull(); expect(document.activeElement).toBe(w.element)
  await w.trigger('keydown', { key: 'ContextMenu' }); await flushPromises()
  const reopened = document.querySelector<HTMLElement>('[role="menu"]')!
  reopened.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true, cancelable: true }))
  expect(document.activeElement?.getAttribute('data-item-id')).toBe('rename')
  reopened.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); await flushPromises()
  expect(w.emitted('menu-action')).toEqual([['row', 'rename']]); expect(w.find('input').exists()).toBe(false)
  await w.setProps({ session: { ...w.props('session'), renameState: 'editing' } }); await flushPromises()
  expect(w.find('input').exists()).toBe(true)
  await w.get('input').trigger('keydown', { key: 'Escape' }); expect(w.emitted('rename-cancel')).toEqual([['row']])
  expect(w.emitted('primary-action')).toBeUndefined()
})


// 减弱动效同时覆盖公共加载/按钮、状态图标和树的折叠箭头，不靠截图猜测。
it('A11y_ReducedMotionContracts_005', () => {
  const css = readFileSync('src/styles/global.css', 'utf8')
  expect(css).toMatch(/@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{\s*\.ui-spinner, \.ui-skeleton\s*\{\s*animation:\s*none/s)
  expect(css).toMatch(/\.ui-button, \.ui-menu-item, \.ui-input, \.ui-select\s*\{\s*transition:\s*none/s)
  expect(readFileSync('src/components/sessions/SessionStatusIcon.vue', 'utf8')).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*animation: none/)
  expect(readFileSync('src/components/sessions/ProjectNode.vue', 'utf8')).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*transition: none/)
  expect(css).toMatch(/:focus-visible[^}]*outline:\s*2px solid var\(--focus-ring\)/s)
})

// 嵌套弹窗关闭后允许返回父弹窗的程序化焦点容器，tabindex=-1 仅排除顺序 Tab。
it('A11y_NestedContainerReturn_006', async () => {
  const parent = mount(AppDialog, { attachTo: document.body, props: { open: true, title: 'Parent', showClose: false }, attrs: { 'data-parent-dialog': '' }, global: { plugins: [i18n] }, slots: { footer: '<button data-danger="true">Delete</button>' } }); wrappers.push(parent); await flushPromises()
  const container = document.querySelector<HTMLElement>('[data-parent-dialog]')!
  expect(document.activeElement).toBe(container)
  const child = mount(AppDialog, { attachTo: document.body, props: { open: true, title: 'Child' }, global: { plugins: [i18n] } }); wrappers.push(child); await flushPromises()
  await child.setProps({ open: false }); await flushPromises()
  expect(document.activeElement).toBe(container)
})

// transformed/overflow 祖先不能拥有提示的定位上下文，焦点仍由原按钮持有。
it('A11y_TooltipEscapesClipping_007', async () => {
  const w = mount({ render: () => h('section', { style: 'transform: translateZ(0); overflow: hidden' }, [
    h(AppTooltip, { text: 'Outside the clipped project' }, { default: () => h('button', { 'aria-describedby': 'existing-help' }, 'Inspect project') }),
  ]) }, { attachTo: document.body }); wrappers.push(w)
  const trigger = w.get('button')
  ;(trigger.element as HTMLElement).focus(); await flushPromises()
  const tooltip = document.querySelector<HTMLElement>('[role="tooltip"]')!
  expect(tooltip?.textContent).toBe('Outside the clipped project')
  expect(w.element.contains(tooltip), 'tooltip must escape the transformed clipping ancestor').toBe(false)
  expect(trigger.attributes('aria-describedby')).toBe(`existing-help ${tooltip.id}`)
  expect(document.activeElement).toBe(trigger.element)
  expect(tooltip.tabIndex).toBe(-1)
  await trigger.trigger('keydown', { key: 'Escape' })
  expect(document.querySelector('[role="tooltip"]')).toBeNull()
  expect(trigger.attributes('aria-describedby')).toBe('existing-help')
  expect(document.activeElement).toBe(trigger.element)
})

// 模态框内提示浮在遮罩上但不增加焦点所有者；第一次 Esc 只关提示。
it('A11y_TooltipKeepsModalOwner_008', async () => {
  const w = mount(AppDialog, { attachTo: document.body, props: { open: true, title: 'Inspect', showClose: false }, global: { plugins: [i18n] }, slots: {
    default: () => h(AppTooltip, { text: 'Project details' }, { default: () => h('button', 'Inspect project') }),
  } }); wrappers.push(w); await flushPromises()
  const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!
  const trigger = dialog.querySelector('button')!
  const tooltip = document.querySelector<HTMLElement>('[role="tooltip"]')!
  expect(document.activeElement).toBe(trigger)
  expect(tooltip?.textContent).toBe('Project details')
  expect(dialog.contains(tooltip)).toBe(false)
  expect(Number(getComputedStyle(tooltip).zIndex)).toBeGreaterThan(Number(getComputedStyle(dialog.parentElement!).zIndex))
  trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })); await flushPromises()
  expect(document.querySelector('[role="tooltip"]')).toBeNull()
  expect(w.emitted('close')).toBeUndefined()
  expect(document.activeElement).toBe(trigger)
  trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })); await flushPromises()
  expect(w.emitted('close')).toHaveLength(1)
})
