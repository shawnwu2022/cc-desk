import { defineStore } from 'pinia'
import { onScopeDispose, ref } from 'vue'
import en from '@/i18n/locales/en'

export type ToastKind = 'success' | 'info' | 'warning' | 'error'
export interface ToastInput {
  kind: ToastKind
  messageKey: keyof typeof en
  /** An opaque action identity, never error text, a path, or credentials. */
  dedupeKey?: string
}
export interface Toast extends ToastInput { id: string }
const TOAST_LIFETIME_MS = 5000
const KINDS: readonly string[] = ['success', 'info', 'warning', 'error']

export const useNotificationsStore = defineStore('notifications', () => {
  const toasts = ref<Toast[]>([])
  let sequence = 0
  const expiry = new Map<string, { timer: ReturnType<typeof setTimeout> | null; remaining: number; started: number }>()

  function dismissToast(id: string) {
    const clock = expiry.get(id)
    if (clock?.timer != null) clearTimeout(clock.timer)
    expiry.delete(id)
    toasts.value = toasts.value.filter((toast) => toast.id !== id)
  }
  function resumeToast(id: string) {
    const clock = expiry.get(id)
    if (!clock || clock.timer != null) return
    clock.started = Date.now()
    clock.timer = setTimeout(() => dismissToast(id), clock.remaining)
  }
  function pauseToast(id: string) {
    const clock = expiry.get(id)
    if (!clock || clock.timer == null) return
    clearTimeout(clock.timer)
    clock.timer = null
    clock.remaining = Math.max(0, clock.remaining - (Date.now() - clock.started))
  }
  function pushToast(input: ToastInput): string | null {
    // Runtime callers are also checked: never stringify Error or copy raw transport fields.
    if (!input || typeof input !== 'object' || input instanceof Error || !KINDS.includes(input.kind)
      || typeof input.messageKey !== 'string' || !Object.prototype.hasOwnProperty.call(en, input.messageKey)) return null
    if (input.dedupeKey !== undefined && (typeof input.dedupeKey !== 'string' || !/^[\w:.-]{1,128}$/.test(input.dedupeKey))) return null
    const existing = input.dedupeKey ? toasts.value.find((toast) => toast.dedupeKey === input.dedupeKey) : undefined
    if (existing) return existing.id
    const id = `toast-${++sequence}`
    const toast: Toast = { id, kind: input.kind, messageKey: input.messageKey }
    if (input.dedupeKey) toast.dedupeKey = input.dedupeKey
    if (toasts.value.length === 3) dismissToast(toasts.value[0].id)
    toasts.value.push(toast)
    expiry.set(id, { timer: null, remaining: TOAST_LIFETIME_MS, started: Date.now() })
    resumeToast(id)
    return id
  }
  function clearToasts() {
    for (const id of expiry.keys()) dismissToast(id)
    toasts.value = []
  }
  onScopeDispose(clearToasts)
  return { toasts, pushToast, dismissToast, pauseToast, resumeToast, clearToasts }
})
