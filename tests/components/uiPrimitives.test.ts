import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DOMWrapper, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { h, nextTick } from 'vue'
import { readFileSync, readdirSync } from 'node:fs'
import { resolve } from 'node:path'
import i18n from '@/i18n'
import AppButton from '@/components/ui/AppButton.vue'
import IconButton from '@/components/ui/IconButton.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import AppMenu from '@/components/ui/AppMenu.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppDrawer from '@/components/ui/AppDrawer.vue'
import AppToastHost from '@/components/ui/AppToastHost.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import LoadingState from '@/components/ui/LoadingState.vue'
import ErrorDetails from '@/components/ui/ErrorDetails.vue'
import { useNotificationsStore } from '@/stores/notifications'

const body = new DOMWrapper(document.body)
const mounted: VueWrapper[] = []
beforeEach(() => { setActivePinia(createPinia()); vi.useFakeTimers() })
afterEach(() => {
  mounted.splice(0).forEach((wrapper) => wrapper.unmount())
  document.body.innerHTML = ''
  vi.useRealTimers()
})

// 菜单跳过隐藏/禁用项，方向键循环移动，Enter 仅选择当前动作。
describe('UiPrimitives_Interactions', () => {
  it('Menu_ArrowEnter_001', async () => {
    const wrapper = mount(AppMenu, { attachTo: document.body, props: {
      open: true, label: 'Session actions', items: [
        { id: 'delete', label: 'Delete', danger: true },
        { id: 'hidden', label: 'Hidden', hidden: true },
        { id: 'disabled', label: 'Disabled', disabled: true },
        { id: 'rename', label: 'Rename' },
        { id: 'archive', label: 'Archive' },
      ],
    } })
    mounted.push(wrapper)
    await nextTick()
    expect(wrapper.findAll('[role="menuitem"]').map((item) => item.text())).toEqual(['Disabled', 'Rename', 'Archive', 'Delete'])
    expect(document.activeElement?.textContent).toBe('Rename')
    await wrapper.get('[role="menu"]').trigger('keydown', { key: 'ArrowDown' })
    expect(document.activeElement?.textContent).toBe('Archive')
    await wrapper.get('[role="menu"]').trigger('keydown', { key: 'ArrowUp' })
    await wrapper.get('[role="menu"]').trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('select')).toEqual([['rename']])
    expect(wrapper.emitted('update:open')).toEqual([[false]])
  })

  // Esc 关闭菜单并将焦点返回打开菜单的控件。
  it('Menu_EscapeReturn_002', async () => {
    const opener = document.createElement('button')
    document.body.append(opener); opener.focus()
    const wrapper = mount(AppMenu, { attachTo: document.body, props: { open: true, label: 'Actions', items: [{ id: 'rename', label: 'Rename' }] } })
    mounted.push(wrapper); await nextTick()
    await wrapper.get('[role="menu"]').trigger('keydown', { key: 'Escape' })
    await wrapper.setProps({ open: false }); await nextTick()
    expect(wrapper.emitted('close')).toHaveLength(1)
    expect(document.activeElement).toBe(opener)
  })

  // Tab 离开菜单时关闭浮层，但不抢回焦点。
  it('Menu_TabClose_003', async () => {
    const wrapper = mount(AppMenu, { attachTo: document.body, props: { open: true, label: 'Actions', items: [{ id: 'rename', label: 'Rename' }] } })
    mounted.push(wrapper); await nextTick()
    const event = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    wrapper.get('[role="menu"]').element.dispatchEvent(event)
    expect(wrapper.emitted('update:open')).toEqual([[false]])
    expect(event.defaultPrevented).toBe(false)
  })

  // 对话框默认聚焦安全按钮，Esc 关闭后返回原控件。
  it('Dialog_SafeFocusReturn_004', async () => {
    const opener = document.createElement('button')
    document.body.append(opener); opener.focus()
    const wrapper = mount(AppDialog, { attachTo: document.body, global: { plugins: [i18n] }, props: { open: true, title: 'Remove session' }, slots: { footer: '<button data-danger="true" autofocus>Delete</button><button>Cancel</button>' } })
    mounted.push(wrapper); await nextTick()
    const dialog = document.querySelector('[role="dialog"]')!
    expect(dialog.getAttribute('aria-modal')).toBe('true')
    expect(document.getElementById(dialog.getAttribute('aria-labelledby')!)?.textContent).toBe('Remove session')
    expect(document.activeElement?.textContent).not.toBe('Delete')
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await wrapper.setProps({ open: false }); await nextTick()
    expect(wrapper.emitted('close')).toHaveLength(1)
    expect(document.activeElement).toBe(opener)
  })

  // 对话框 Tab 在首尾循环，不让焦点落到背景。
  it('Dialog_TrapTab_005', async () => {
    const wrapper = mount(AppDialog, { attachTo: document.body, global: { plugins: [i18n] }, props: { open: true, title: 'Confirm' }, slots: { default: '<input aria-label="Name" />', footer: '<button>Cancel</button><button data-danger="true">Delete</button>' } })
    mounted.push(wrapper); await nextTick()
    const dialog = document.querySelector('[role="dialog"]')!
    const buttons = dialog.querySelectorAll<HTMLButtonElement>('button')
    buttons[buttons.length - 1].focus()
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }))
    expect(document.activeElement).toBe(buttons[0])
    buttons[0].focus()
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true }))
    expect(document.activeElement).toBe(buttons[buttons.length - 1])
  })

  // 无安全控件的对话框聚焦容器，绝不自动执行危险操作。
  it('Dialog_DangerOnly_006', async () => {
    const wrapper = mount(AppDialog, { attachTo: document.body, global: { plugins: [i18n] }, props: { open: true, title: 'Remove', showClose: false }, slots: { footer: '<button data-danger="true">Delete</button>' } })
    mounted.push(wrapper); await nextTick()
    expect(document.activeElement).toBe(document.querySelector('[role="dialog"]'))
  })

  // 抽屉沿用对话框键盘契约，并暴露抽屉表面。
  it('Drawer_EscapeClose_007', async () => {
    const wrapper = mount(AppDrawer, { attachTo: document.body, global: { plugins: [i18n] }, props: { open: true, title: 'Project resources' }, slots: { default: 'Read-only resources' } })
    mounted.push(wrapper); await nextTick()
    const drawer = document.querySelector('[role="dialog"]')!
    expect(drawer.classList.contains('ui-drawer')).toBe(true)
    drawer.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(wrapper.emitted('update:open')).toEqual([[false]])
  })

  // Tooltip 把描述关系设置到实际触发按钮，并保留原描述。
  it('Tooltip_DescribeTrigger_008', async () => {
    const wrapper = mount(AppTooltip, { attachTo: document.body, props: { text: 'Copy session title' }, slots: { default: '<button aria-describedby="field-help">Copy</button>' } })
    mounted.push(wrapper)
    await wrapper.get('button').trigger('focus')
    const tooltip = body.get('[role="tooltip"]')
    expect(wrapper.get('button').attributes('aria-describedby')!.split(' ')).toContain(tooltip.attributes('id'))
    expect(wrapper.get('button').attributes('aria-describedby')).toContain('field-help')
    await wrapper.get('button').trigger('keydown', { key: 'Escape' })
    expect(body.find('[role="tooltip"]').exists()).toBe(false)
    expect(wrapper.get('button').attributes('aria-describedby')).toBe('field-help')
  })

  // IconButton 使用同一文本作为可访问名称与提示，不泄漏图标文本。
  it('IconButton_LabelTooltip_009', async () => {
    const wrapper = mount(IconButton, { attachTo: document.body, props: { label: 'More actions' }, slots: { default: '⋯' } })
    mounted.push(wrapper)
    expect(wrapper.get('button').attributes('aria-label')).toBe('More actions')
    await wrapper.get('button').trigger('focus')
    expect(body.get('[role="tooltip"]').text()).toBe('More actions')
    expect(wrapper.get('button').attributes('aria-describedby')).toBe(body.get('[role="tooltip"]').attributes('id'))
    wrapper.get('button').element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(body.find('[role="tooltip"]').exists()).toBe(false)
  })

  // 加载按钮阻止重复动作，并向辅助技术暴露 busy。
  it('Button_LoadingBlocksClick_010', async () => {
    const wrapper = mount(AppButton, { props: { loading: true }, slots: { default: 'Save' } })
    mounted.push(wrapper)
    expect(wrapper.get('button').attributes('disabled')).toBeDefined()
    expect(wrapper.get('button').attributes('aria-busy')).toBe('true')
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('click')).toBeUndefined()
  })

  // 输入框生成唯一 label 关联，错误描述不会丢失。
  it('Input_LabelAndError_011', async () => {
    const wrapper = mount(AppInput, { props: { modelValue: '', label: 'Session name', invalid: true, describedBy: 'name-error' } })
    mounted.push(wrapper)
    expect(wrapper.get('label').attributes('for')).toBe(wrapper.get('input').attributes('id'))
    expect(wrapper.get('input').attributes('aria-invalid')).toBe('true')
    expect(wrapper.get('input').attributes('aria-describedby')).toBe('name-error')
    await wrapper.get('input').setValue('New name')
    expect(wrapper.emitted('update:modelValue')).toEqual([['New name']])
  })

  // 原生 select 保持键盘语义，变更输出实际 option 值。
  it('Select_EmitValue_012', async () => {
    const wrapper = mount(AppSelect, { props: { modelValue: 'claude', label: 'CLI', options: [{ value: 'claude', label: 'Claude Code' }, { value: 'codex', label: 'Codex CLI' }] } })
    mounted.push(wrapper)
    await wrapper.get('select').setValue('codex')
    expect(wrapper.emitted('update:modelValue')).toEqual([['codex']])
    expect(wrapper.get('label').attributes('for')).toBe(wrapper.get('select').attributes('id'))
  })

  // Toast host 使用本地化键，并允许用户关闭对应消息。
  it('ToastHost_LocalizeDismiss_013', async () => {
    const store = useNotificationsStore()
    const id = store.pushToast({ kind: 'success', messageKey: 'rename' })!
    const wrapper = mount(AppToastHost, { attachTo: document.body, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    expect(wrapper.get('[role="status"]').text()).toContain('Rename')
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    await wrapper.get('button').trigger('click')
    expect(store.toasts.find((toast) => toast.id === id)).toBeUndefined()
  })

  // 内联错误不会升级为全局中断，行动按钮有清晰文本。
  it('Notice_LocalAction_014', async () => {
    const wrapper = mount(InlineNotice, { props: { kind: 'warning', message: 'Resource unavailable', actionLabel: 'Retry' } })
    mounted.push(wrapper)
    expect(wrapper.attributes('role')).toBe('status')
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('action')).toHaveLength(1)
  })

  // 空状态解释原因并要求调用方提供下一步动作。
  it('EmptyState_NextAction_015', async () => {
    const wrapper = mount(EmptyState, { props: { title: 'No projects', description: 'Add a project directory to begin', actionLabel: 'Add project' } })
    mounted.push(wrapper)
    expect(wrapper.text()).toContain('Add a project directory to begin')
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('action')).toHaveLength(1)
  })

  // 加载状态将装饰 skeleton 隐藏于辅助技术，保留一个 busy 描述。
  it('LoadingState_Skeleton_016', () => {
    const wrapper = mount(LoadingState, { props: { label: 'Loading projects', rows: 3 } })
    mounted.push(wrapper)
    expect(wrapper.attributes('aria-busy')).toBe('true')
    expect(wrapper.findAll('.ui-skeleton')).toHaveLength(3)
    expect(wrapper.get('.ui-skeleton').attributes('aria-hidden')).toBe('true')
  })

  // 错误详情只显示映射后的错误码，隐藏原始传输文本。
  it('ErrorDetails_SafeCode_017', async () => {
    const wrapper = mount(ErrorDetails, { props: { code: 'RESOURCE_UNAVAILABLE' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    expect(wrapper.get('details').attributes('open')).toBeUndefined()
    await wrapper.get('summary').trigger('click')
    await wrapper.setProps({ code: 'Authorization: Bearer secret /home/private' })
    expect(wrapper.get('code').text()).toBe('GENERIC_UNAVAILABLE')
    expect(wrapper.html()).not.toContain('Bearer secret')
  })

  // 菜单只有危险动作时聚焦菜单容器，不默认聚焦危险按钮。
  it('Menu_DangerOnlyFocus_019', async () => {
    const wrapper = mount(AppMenu, { attachTo: document.body, props: { open: true, label: 'Actions', items: [{ id: 'delete', label: 'Delete', danger: true }] } })
    mounted.push(wrapper); await nextTick()
    expect(document.activeElement).toBe(wrapper.get('[role="menu"]').element)
    await wrapper.get('[role="menu"]').trigger('keydown', { key: 'ArrowDown' })
    expect(document.activeElement?.textContent).toBe('Delete')
  })

  // 嵌套对话框仅由顶层捕获焦点，关闭后恢复下层控件。
  it('Dialog_NestedFocus_020', async () => {
    const outer = mount(AppDialog, { attachTo: document.body, global: { plugins: [i18n] }, props: { open: true, title: 'Outer' }, slots: { default: '<button id="inner-opener">Open inner</button>' } })
    mounted.push(outer); await nextTick()
    const opener = document.getElementById('inner-opener')!
    opener.focus()
    const inner = mount(AppDialog, { attachTo: document.body, global: { plugins: [i18n] }, props: { open: true, title: 'Inner' } })
    mounted.push(inner); await nextTick()
    expect(document.activeElement?.closest('[role="dialog"]')?.textContent).toContain('Inner')
    const outside = document.createElement('button')
    document.body.append(outside); outside.focus()
    expect(document.activeElement?.closest('[role="dialog"]')?.textContent).toContain('Inner')
    await inner.setProps({ open: false }); await nextTick()
    expect(document.activeElement).toBe(opener)
  })

  // 隐藏的输入框不接收初始焦点，聚焦可见安全控件。
  it('Dialog_SkipHiddenFocus_021', async () => {
    const wrapper = mount(AppDialog, { attachTo: document.body, global: { plugins: [i18n] }, props: { open: true, title: 'Confirm', showClose: false }, slots: { default: '<input type="hidden" /><div style="display: none"><button>Hidden</button></div><button>Cancel</button>' } })
    mounted.push(wrapper); await nextTick()
    expect(document.activeElement?.textContent).toBe('Cancel')
  })

  // 两个输入框各自关联自己的 label，不因默认 ID 碰撞混淆名称。
  it('Input_UniqueLabels_022', () => {
    const first = mount(AppInput, { props: { modelValue: '', label: 'First' } })
    const second = mount(AppInput, { props: { modelValue: '', label: 'Second' } })
    mounted.push(first, second)
    expect(first.get('input').attributes('id')).not.toBe(second.get('input').attributes('id'))
    expect(second.get('label').attributes('for')).toBe(second.get('input').attributes('id'))
  })

  // 原型键也被降级为通用诊断码，详情不会出现空的技术说明。
  it('ErrorDetails_PrototypeKey_023', () => {
    const wrapper = mount(ErrorDetails, { props: { code: '__proto__' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    expect(wrapper.get('code').text()).toBe('GENERIC_UNAVAILABLE')
  })

  // 对话框内的原生 summary 可从关闭按钮到达，并参与首尾 Tab 循环。
  it('Dialog_SummaryTabCycle_024', async () => {
    const wrapper = mount(AppDialog, { attachTo: document.body, global: { plugins: [i18n] }, props: { open: true, title: 'Error details' }, slots: { default: () => h(ErrorDetails, { code: 'RESOURCE_UNAVAILABLE' }) } })
    mounted.push(wrapper); await nextTick()
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!
    const close = dialog.querySelector<HTMLButtonElement>('button')!
    const summary = dialog.querySelector<HTMLElement>('summary')!
    close.focus()
    const next = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    close.dispatchEvent(next)
    expect(next.defaultPrevented).toBe(false)
    // jsdom 不执行浏览器默认 Tab 移动，显式模拟移动到原生 summary。
    summary.focus()
    const wrap = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    summary.dispatchEvent(wrap)
    expect(wrap.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(close)
    const reverse = new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true })
    close.dispatchEvent(reverse)
    expect(document.activeElement).toBe(summary)
  })

  // 触发按钮保持真实焦点时，指针离开不清除 tooltip 和描述关联。
  it('Tooltip_FocusAfterLeave_025', async () => {
    const wrapper = mount(AppTooltip, { attachTo: document.body, props: { text: 'Copy' }, slots: { default: '<button aria-describedby="existing-help">Copy</button>' } })
    mounted.push(wrapper)
    const button = wrapper.get('button')
    ;(button.element as HTMLButtonElement).focus(); await nextTick()
    await button.trigger('mouseenter'); await button.trigger('mouseleave')
    expect(document.activeElement).toBe(button.element)
    const tooltipId = body.get('[role="tooltip"]').attributes('id')
    expect(button.attributes('aria-describedby')).toBe(`existing-help ${tooltipId}`)
  })

  // 指针仍悬停时，真实 blur 不关闭 tooltip。
  it('Tooltip_HoverAfterBlur_026', async () => {
    const wrapper = mount(AppTooltip, { attachTo: document.body, props: { text: 'Copy' }, slots: { default: '<button>Copy</button>' } })
    mounted.push(wrapper)
    const button = wrapper.get('button')
    ;(button.element as HTMLButtonElement).focus(); await nextTick()
    await button.trigger('mouseenter')
    ;(button.element as HTMLButtonElement).blur(); await nextTick()
    expect(body.get('[role="tooltip"]').text()).toBe('Copy')
    expect(button.attributes('aria-describedby')).toBe(body.get('[role="tooltip"]').attributes('id'))
  })

  // Esc 明确关闭后，结束一种交互不会重开；新的 focus/hover 才重开。
  it('Tooltip_EscapeFreshInput_027', async () => {
    const wrapper = mount(AppTooltip, { attachTo: document.body, props: { text: 'Copy' }, slots: { default: '<button>Copy</button>' } })
    mounted.push(wrapper)
    const button = wrapper.get('button')
    ;(button.element as HTMLButtonElement).focus(); await nextTick()
    await button.trigger('mouseenter'); await button.trigger('keydown', { key: 'Escape' })
    await button.trigger('mouseleave'); await button.trigger('focus')
    expect(body.find('[role="tooltip"]').exists()).toBe(false)
    expect(button.attributes('aria-describedby')).toBeUndefined()
    ;(button.element as HTMLButtonElement).blur(); await nextTick()
    ;(button.element as HTMLButtonElement).focus(); await nextTick()
    expect(body.find('[role="tooltip"]').exists()).toBe(true)
    await button.trigger('keydown', { key: 'Escape' })
    await button.trigger('mouseenter')
    expect(body.find('[role="tooltip"]').exists()).toBe(true)
  })

  // 新组件限定尺寸/焦点/阴影 token，禁止 transition all。
  it('Styles_TokensAndMotion_018', () => {
    const directory = resolve(__dirname, '../../src/components/ui')
    const sources = readdirSync(directory).filter((file) => file.endsWith('.vue')).map((file) => readFileSync(resolve(directory, file), 'utf8')).join('\n')
    const css = readFileSync(resolve(__dirname, '../../src/styles/global.css'), 'utf8')
    expect(sources + css).not.toMatch(/transition\s*:\s*all\b/)
    expect(css).toMatch(/--control-height-compact:\s*28px/)
    expect(css).toMatch(/--control-height-normal:\s*32px/)
    expect(css).toMatch(/--control-height-primary:\s*36px/)
    expect(css).toMatch(/outline:\s*2px solid var\(--focus-ring\)/)
    expect(css).toContain('box-shadow: var(--shadow-lg)')
    expect(css).toContain('box-shadow: var(--shadow-xl)')
    expect(css).toMatch(/prefers-reduced-motion:\s*reduce/)
  })
})
