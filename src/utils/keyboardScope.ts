/** contenteditable 的空值和继承状态同样属于文本编辑，不能只匹配字面 true。 */
export function isTextEditingTarget(target: EventTarget | null): boolean {
  return target instanceof Element && !!target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])')
}

/** 常驻列表不能凭当前选择认领键盘；事件与真实焦点必须同时落在本表面。 */
export function ownsListKeyboardEvent(event: KeyboardEvent, surface: HTMLElement | null): boolean {
  const modals = document.querySelectorAll('[role="dialog"][aria-modal="true"]')
  const modal = modals[modals.length - 1]
  return !!surface && event.target instanceof Element && surface.contains(event.target)
    && surface.contains(document.activeElement) && !event.defaultPrevented && !event.isComposing && event.keyCode !== 229
    // 归档抽屉中的列表保留自身菜单；背景列表不能穿过当前模态边界。
    && (!modal || modal.contains(event.target) && modal.contains(document.activeElement))
}
