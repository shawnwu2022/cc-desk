import type { ComputedRef, InjectionKey } from 'vue'
export const APP_SHORTCUT_ACTIONS = ['new-session', 'close-session', 'projects', 'settings', 'rename'] as const
export type AppShortcutAction = typeof APP_SHORTCUT_ACTIONS[number]
export type ShortcutBindings = Record<AppShortcutAction, string | null>
export const DEFAULT_SHORTCUT_BINDINGS: Readonly<ShortcutBindings> = Object.freeze({
  'new-session': 'Mod+KeyN', 'close-session': 'Mod+KeyW', projects: 'Mod+KeyP', settings: 'Mod+Comma', rename: 'F2',
})
export const SHORTCUT_LABELS: Record<AppShortcutAction, string> = {
  'new-session': 'shortcut_newSession', 'close-session': 'shortcutCloseSession', projects: 'shortcut_openProjects', settings: 'shortcut_toggleSettings', rename: 'sessionActionRename',
}
const codePattern = /^(Key[A-Z]|Digit[0-9]|F(?:[1-9]|1[0-2])|Comma|Period|Slash|Backquote|Minus|Equal|Space|Tab|ArrowLeft|ArrowRight|ArrowUp|ArrowDown|Home|End|Delete|Insert)$/
export function validShortcutBinding(value: unknown): value is string {
  if (typeof value !== 'string' || value.length > 45) return false
  const parts = value.split('+'); const code = parts.pop()!
  if (!codePattern.test(code)) return false
  const modifiers = ['Mod', 'Alt', 'Shift'].filter(modifier => parts.includes(modifier))
  return parts.join('+') === modifiers.join('+') && (parts.length > 0 || /^F\d+$/.test(code))
}
export function validShortcutBindings(value: unknown): value is ShortcutBindings {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const values = APP_SHORTCUT_ACTIONS.map(action => Object.prototype.hasOwnProperty.call(value, action) ? (value as ShortcutBindings)[action] : undefined)
  return values.every(binding => binding === null || validShortcutBinding(binding))
    && new Set(values.filter(binding => binding !== null)).size === values.filter(binding => binding !== null).length
}
export function readShortcutBindings(value: unknown): ShortcutBindings {
  return validShortcutBindings(value) ? Object.fromEntries(APP_SHORTCUT_ACTIONS.map(action => [action, value[action]])) as ShortcutBindings : { ...DEFAULT_SHORTCUT_BINDINGS }
}
export function captureShortcut(event: KeyboardEvent): string | null {
  if (event.isComposing || event.repeat || event.getModifierState('AltGraph') || event.ctrlKey && event.metaKey) return null
  const fallback = event.key === ',' ? 'Comma' : /^[a-z]$/i.test(event.key) ? `Key${event.key.toUpperCase()}` : event.key
  const code = event.code || fallback
  const result = [...(event.ctrlKey || event.metaKey ? ['Mod'] : []), ...(event.altKey ? ['Alt'] : []), ...(event.shiftKey ? ['Shift'] : []), code].join('+')
  return validShortcutBinding(result) ? result : null
}
export function formatShortcut(binding: string | null, mac = false): string {
  if (!binding) return '—'
  return binding.replace('Mod', mac ? 'Cmd' : 'Ctrl').replace(/Key([A-Z])/, '$1').replace(/Digit([0-9])/, '$1').replace('Comma', ',').replace('Period', '.').replace('Slash', '/').replace('Space', 'Space')
}

export const APP_RENAME_SHORTCUT = Symbol.for('cc-desk.rename-shortcut') as InjectionKey<ComputedRef<string | null>>
