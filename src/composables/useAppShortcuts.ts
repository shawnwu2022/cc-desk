import { APP_SHORTCUT_ACTIONS, captureShortcut, type AppShortcutAction } from '@/config/appShortcuts'
import { useAppStore } from '@/stores/app'

/** Normal shell action routing. Consequential effects remain with the unified runtime. */
export function useAppShortcuts(port: { onAction: (action: AppShortcutAction, event: KeyboardEvent) => void }) {
  const app = useAppStore()
  function handleKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing || event.repeat || document.querySelector('[role="dialog"][aria-modal="true"]')) return
    const target = event.target instanceof HTMLElement ? event.target : null
    if (target?.closest('input, textarea, select, [contenteditable="true"]') && !target.classList.contains('xterm-helper-textarea')) return
    const binding = captureShortcut(event)
    if (!binding) return
    const action = APP_SHORTCUT_ACTIONS.find(value => app.shortcutBindings[value] === binding)
    if (!action || action === 'rename' && target?.closest('[data-session-row]')) return
    event.preventDefault(); event.stopImmediatePropagation()
    port.onAction(action, event)
  }
  function setupShortcutListeners() {
    window.addEventListener('keydown', handleKeydown, true)
    return [() => window.removeEventListener('keydown', handleKeydown, true)]
  }
  return { setupShortcutListeners }
}
