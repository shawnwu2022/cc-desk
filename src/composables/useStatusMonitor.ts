import { watch, onMounted, onUnmounted, type Ref } from 'vue'
import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window'
import { useHookStore, type HookEventType, type HookEventHandler } from '@/stores/hook'
import { useSessionStore } from '@/stores/session'
import type { HookEventPayload } from '@/types/hook'

const STATUS_EVENTS: HookEventType[] = [
  'sessionStart',
  'userPromptSubmit',
  'stop',
  'stopFailure',
  'notification',
  'sessionEnd',
]

export function useStatusMonitor(options: { isFocused: Ref<boolean> }) {
  const hookStore = useHookStore()
  const sessionStore = useSessionStore()
  const win = getCurrentWindow()

  let unsubscribe: (() => void) | null = null

  const handler: HookEventHandler = (payload: HookEventPayload) => {
    const ptyId = payload.ptyId!
    const tab = sessionStore.getTabByPtyId(ptyId)
    if (!tab || tab.status !== 'running') return

    // sessionStart：直接分配 session_id
    if (payload.detail.type === 'sessionStart') {
      const sessionId = payload.sessionId
      if (sessionId) {
        const data = payload.detail.data as { model?: string }
        sessionStore.assignSessionIdByPtyId(ptyId, sessionId, data.model)
      }
      return
    }

    // userPromptSubmit → 进入 working
    if (payload.detail.type === 'userPromptSubmit') {
      tab.working = true
      return
    }

    // stop/stopFailure/notification/sessionEnd：仅在 working 时才处理
    if (!tab.working) return

    tab.working = false

    // sessionEnd 不需要 pending/attention 提示
    if (payload.detail.type === 'sessionEnd') return

    // 第一层：应用失焦 → 任务栏跳动 + pending
    if (!options.isFocused.value) {
      win.requestUserAttention(UserAttentionType.Critical).catch(() => {})
      tab.pending = true
      return
    }

    // 第二层：应用聚焦 + Tab 未激活 → pending
    if (tab.tabId !== sessionStore.activeTabId) {
      tab.pending = true
    }

    // 第三层（隐式兜底）：应用聚焦 + Tab 激活 → 无操作
  }

  // 聚焦 + tab 可见 → 清除 pending
  watch(
    [() => options.isFocused.value, () => sessionStore.activeTabId],
    ([focused, activeTabId]) => {
      if (focused && activeTabId) {
        const tab = sessionStore.tabs.get(activeTabId)
        if (tab) tab.pending = false
      }
    },
    { immediate: true }
  )

  onMounted(() => {
    unsubscribe = hookStore.subscribe(STATUS_EVENTS, handler)
  })

  onUnmounted(() => {
    unsubscribe?.()
    unsubscribe = null
  })
}
