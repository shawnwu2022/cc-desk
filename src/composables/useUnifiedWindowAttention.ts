import { computed, watch, type InjectionKey, type Ref } from 'vue'
import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window'
import { useWindowAttention } from './useWindowAttention'
import { useAttentionStore } from '@/stores/attention'
import { useSessionStore } from '@/stores/session'
import { captureNativeAttempt, useNativeTabsStore } from '@/stores/nativeTabs'

export const UNIFIED_WINDOW_FOCUS: InjectionKey<Ref<boolean>> = Symbol('cc-desk.unified-window-focus')

/** One window notification owner for both runtime hosts. This consumes only
 * owned Legacy causes, Native's ordered activity, and independent receipt notices. */
export function useUnifiedWindowAttention(options: { activeSessionId: Ref<string | null>; visible: Ref<boolean> }) {
  const { isFocused, focusReady } = useWindowAttention()
  const win = getCurrentWindow()
  const attention = useAttentionStore()
  const legacy = useSessionStore()
  const native = useNativeTabsStore()
  const seen = new Map<string, string>()
  // Receipt delivery is independent of ordered activity. Keep each exact
  // attempt's sent IDs, so state changes cannot re-flash an earlier unread event.
  const sentReplyNotices = new Map<string, Set<string>>()
  watch(() => [focusReady.value, isFocused.value, options.visible.value, options.activeSessionId.value,
    legacy.activeTabId, native.activeTabId, attention.queue,
    [...legacy.tabs.values()].map(tab => [tab.tabId, tab.ptyId, tab.ptyGeneration, tab.status, tab.sessionId, tab.cli]),
    [...native.tabs.values()].map(tab => [tab.tabId, tab.requestId, tab.runId, tab.generation, tab.status,
      tab.cli, tab.action.kind, tab.observationState, tab.activityState, tab.observationNotice?.unreadReplyEnd]),
  ], () => {
    if (!focusReady.value) return
    const liveOwners = new Set<string>()
    const liveNoticeOwners = new Set<string>()
    let notify = false
    for (const item of attention.queue) {
      const tab = [...legacy.tabs.values()].find(tab => tab.ptyId === item.ptyId
        && tab.status === 'running' && (!tab.cli || tab.cli === 'claude')
        && (!item.sessionId || !tab.sessionId || item.sessionId === tab.sessionId))
      if (!tab) continue
      const owner = JSON.stringify(['legacy', tab.tabId, tab.ptyGeneration ?? 0, item.ptyId])
      const receipt = JSON.stringify([item.kind, item.createdAt])
      liveOwners.add(owner)
      const fresh = seen.get(owner) !== receipt
      seen.set(owner, receipt)
      if (isFocused.value && options.visible.value && options.activeSessionId.value === `legacy-tab:${tab.tabId}`
        && legacy.activeTabId === tab.tabId) attention.ackPty(item.ptyId)
      else if (!isFocused.value && fresh) notify = true
    }
    for (const tab of native.tabs.values()) {
      const noticeOwner = JSON.stringify(['native-reply-notice', tab.tabId, tab.requestId, tab.runId, tab.generation])
      liveNoticeOwners.add(noticeOwner)
      if (tab.status !== 'running' || tab.cli !== 'claude' || tab.action.kind === 'raw') continue
      const owner = JSON.stringify(['native', tab.tabId, tab.requestId, tab.runId, tab.generation])
      liveOwners.add(owner)
      const state = tab.observationState === 'active' ? tab.activityState ?? 'unknown' : 'unknown'
      const fresh = seen.get(owner) !== state
      seen.set(owner, state)
      // A waiting transition can request generic attention; it cannot supply
      // permission/completion/error causes absent from the source projection.
      if (!isFocused.value && fresh && state === 'waiting') notify = true

      const notice = tab.observationNotice?.unreadReplyEnd
      const attempt = captureNativeAttempt(tab)
      if (!notice || notice.kind !== 'reply-ended' || notice.runId !== tab.runId || notice.generation !== tab.generation
        || !native.hasOwnedObservationNotice(tab.tabId, attempt)) continue
      if (isFocused.value && options.visible.value && options.activeSessionId.value === `native-tab:${tab.tabId}`
        && native.activeTabId === tab.tabId) {
        // Reading a historical receipt does not require current observer activity.
        native.ackReplyEndNotice(tab.tabId, attempt, notice.eventId)
      } else if (!isFocused.value && tab.observationState === 'active') {
        const sent = sentReplyNotices.get(noticeOwner) ?? new Set<string>()
        // Retain the bound even if a corrupted/replayed DTO escapes producer
        // dedup. Exhaustion never evicts an old ID and re-flashes it.
        if (!sent.has(notice.eventId) && sent.size < 1024) {
          sent.add(notice.eventId)
          sentReplyNotices.set(noticeOwner, sent)
          notify = true
        }
      }
    }
    for (const owner of seen.keys()) if (!liveOwners.has(owner)) seen.delete(owner)
    for (const owner of sentReplyNotices.keys()) if (!liveNoticeOwners.has(owner)) sentReplyNotices.delete(owner)
    if (notify) void win.requestUserAttention(UserAttentionType.Critical).catch(() => {})
  }, { immediate: true })
  return { isFocused: computed(() => focusReady.value && isFocused.value) }
}
