import { APP_SHORTCUT_ACTIONS, captureShortcut, type AppShortcutAction } from '@/config/appShortcuts'
import { useAppStore } from '@/stores/app'
import { isTextEditingTarget } from '@/utils/keyboardScope'

/** Normal shell action routing. Consequential effects remain with the unified runtime. */
export function useAppShortcuts(port: { onAction: (action: AppShortcutAction, event: KeyboardEvent) => void }) {
  const app = useAppStore()
  function handleKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing || event.keyCode === 229 || event.repeat || document.querySelector('[role="dialog"][aria-modal="true"]')) return
    const target = event.target instanceof HTMLElement ? event.target : null
    if (isTextEditingTarget(target) && !target?.classList.contains('xterm-helper-textarea')) return
    const binding = captureShortcut(event)
    if (!binding) return
    const action = APP_SHORTCUT_ACTIONS.find(value => app.shortcutBindings[value] === binding)
    // 重命名只由聚焦的会话行认领；窗口捕获不能抢在 xterm/编辑控件之前消费。
    if (!action || action === 'rename') return
    event.preventDefault(); event.stopImmediatePropagation()
    port.onAction(action, event)
  }
  function setupShortcutListeners() {
    window.addEventListener('keydown', handleKeydown, true)
    return [() => window.removeEventListener('keydown', handleKeydown, true)]
  }
  return { setupShortcutListeners }
}
