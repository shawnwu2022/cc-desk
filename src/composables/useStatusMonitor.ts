import { watch, onMounted, onUnmounted, type Ref } from 'vue'
import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window'
import { useHookStore, type HookEventType, type HookEventHandler } from '@/stores/hook'
import { useSessionStore } from '@/stores/session'
import { useAttentionStore } from '@/stores/attention'
import type { HookEventPayload, NotificationData } from '@/types/hook'
import { fromClaudeHook } from '@/integrations/claudeObserver'
import { createObservationReducer, type ObservationReducer } from '@/integrations/registry'

const STATUS_EVENTS: HookEventType[] = [
  'sessionStart',
  'userPromptSubmit',
  'preToolUse',
  'postToolUse',
  'postToolUseFailure',
  'subagentStart',
  'subagentStop',
  'stop',
  'stopFailure',
  'notification',
  'sessionEnd',
  'preCompact',
  'postCompact',
]

/** 表示 Claude 正在主动工作的事件 */
const ACTIVITY_EVENTS: Set<HookEventType> = new Set([
  'preToolUse',
  'postToolUse',
  'postToolUseFailure',
  'subagentStart',
  'subagentStop',
  'preCompact',
  'postCompact',
])

export function useStatusMonitor(options: { isFocused: Ref<boolean>; isTerminalVisible: Ref<boolean>; requestWindowAttention?: boolean }) {
  const hookStore = useHookStore()
  const sessionStore = useSessionStore()
  const attentionStore = useAttentionStore()
  const win = getCurrentWindow()

  let unsubscribe: (() => void) | null = null

  /** 跟踪每个 tab 的回合是否已结束（Stop 后 recap 等内部操作不应恢复 working） */
  const turnEnded = new Map<string, boolean>()
  const subagentRuns = new Map<string, { ptyId: string; reducer: ObservationReducer }>()

  // A replaced/ended PTY cannot carry a previous run's causal identities.
  watch(() => [...sessionStore.tabs.values()].map(tab => [tab.tabId, tab.ptyId, tab.status,
    tab.cli, tab.observerEnabled, tab.observation]), () => {
    for (const [tabId, run] of subagentRuns) {
      const tab = sessionStore.tabs.get(tabId)
      if (!tab || tab.ptyId !== run.ptyId || tab.status !== 'running') {
        run.reducer.accept({ kind: 'off', runId: run.ptyId, generation: 1 })
        subagentRuns.delete(tabId)
      } else if ((tab.cli ?? 'claude') !== 'claude' || tab.observerEnabled === false
        || tab.observation === 'off' || tab.observation === 'unavailable') {
        // Keep dedupe and invalidated IDs for the same PTY if observation resumes.
        run.reducer.accept({ kind: 'timeout', runId: run.ptyId, generation: 1 })
        tab.activity = 'unknown'
        tab.working = false
      }
    }
  }, { flush: 'sync' })

  const handler: HookEventHandler = (payload: HookEventPayload) => {
    const ptyId = payload.ptyId!
    const tab = sessionStore.getTabByPtyId(ptyId)
    if (!tab || tab.status !== 'running') return

    if ((tab.cli ?? 'claude') !== 'claude' || tab.observerEnabled === false) return
    if (payload.observerSource === 'claude-hook') {
      // Generic hooks remain unordered. Only a bounded, first-invocation
      // subagent identity can establish the separate causal work hint.
      const event = fromClaudeHook(payload)
      if (!event || event.runId !== ptyId || event.generation !== 1) return
      let run = subagentRuns.get(tab.tabId)
      if (!run || run.ptyId !== ptyId) {
        run = { ptyId, reducer: createObservationReducer({ runId: ptyId, generation: 1 }) }
        subagentRuns.set(tab.tabId, run)
      }
      run.reducer.accept(event)
      const state = run.reducer.state()
      tab.observation = state.observation
      tab.activity = state.activity
      tab.working = state.activity === 'subagent_running'
      turnEnded.delete(tab.tabId)
      if (payload.detail.type === 'sessionStart' && payload.sessionId && !tab.sessionId) {
        const data = payload.detail.data as { model?: string }
        sessionStore.assignSessionIdByPtyId(ptyId, payload.sessionId, data.model)
      }
      return
    }

    // sessionStart：直接分配 session_id
    if (payload.detail.type === 'sessionStart') {
      const sessionId = payload.sessionId
      if (sessionId) {
        const data = payload.detail.data as { model?: string }
        sessionStore.assignSessionIdByPtyId(ptyId, sessionId, data.model)
      }
      return
    }

    // userPromptSubmit → 进入 working + 设置标题 + 开始新回合
    if (payload.detail.type === 'userPromptSubmit') {
      turnEnded.delete(tab.tabId)
      tab.working = true
      // 新回合:清所有 attention（含 error -- 用户开始新工作，旧 error 作废）
      attentionStore.ackPty(ptyId, { clearError: true })
      // 无自定义标题时，用首条用户消息作为标题
      if (tab.name === 'New Session' || tab.name === tab.sessionId?.slice(0, 8)) {
        const prompt = (payload.detail.data as { prompt?: string }).prompt?.trim()
        if (prompt) {
          sessionStore.updateTabName(tab.tabId, prompt.length > 50 ? prompt.slice(0, 50) + '…' : prompt)
        }
      }
      return
    }

    // 活跃事件：回合结束后忽略（防止 recap 等内部操作恢复 working）
    if (ACTIVITY_EVENTS.has(payload.detail.type)) {
      if (turnEnded.get(tab.tabId)) return
      tab.working = true
      tab.pending = false
      // 新回合:清所有 attention（含 error）
      attentionStore.ackPty(ptyId, { clearError: true })
      return
    }

    // notification：根据 notification_type 区分处理
    if (payload.detail.type === 'notification') {
      if (!tab.working) return

      const data = payload.detail.data as NotificationData
      const ntype = data.notificationType

      if (ntype === 'idle_prompt') {
        // 回合结束，等待用户下一条消息
        turnEnded.set(tab.tabId, true)
        tab.working = false
        setPendingWithAttention(tab, ptyId)
        return
      }

      if (ntype === 'permission_prompt' || ntype === 'worker_permission_prompt') {
        // 等待用户授权（回合未结束，不设 turnEnded，授权后 PostToolUse 恢复 working）
        tab.working = false
        setPendingWithAttention(tab, ptyId)
        return
      }

      // 其他通知类型不改变工作状态
      return
    }

    // stop/stopFailure/sessionEnd：仅在 working 时才处理
    if (!tab.working) return

    // stop/stopFailure 标记回合结束
    if (payload.detail.type !== 'sessionEnd') {
      turnEnded.set(tab.tabId, true)
    }

    tab.working = false

    // sessionEnd 不需要 pending/attention 提示
    if (payload.detail.type === 'sessionEnd') return

    setPendingWithAttention(tab, ptyId)
  }

  /** 设置 pending 状态：用户正在看时立即清除 + ack 非 error attention；否则触发任务栏跳动。
   *  error 粘性:ackPty 默认不清 error（CLI 异常需持续提示，看了不清）。 */
  function setPendingWithAttention(tab: { tabId: string; pending: boolean }, ptyId: string) {
    tab.pending = true
    // 用户正在看这个 tab → 不需要 pending 提示
    if (options.isFocused.value && options.isTerminalVisible.value && tab.tabId === sessionStore.activeTabId) {
      tab.pending = false
      attentionStore.ackPty(ptyId) // 清 permission/completed（error 保留）
      return
    }
    // 应用失焦时触发任务栏跳动
    if (!options.isFocused.value && options.requestWindowAttention !== false) {
      win.requestUserAttention(UserAttentionType.Critical).catch(() => {})
    }
  }

  // 聚焦 + 终端可见 + tab 激活 → 清除 pending
  watch(
    [() => options.isFocused.value, () => options.isTerminalVisible.value, () => sessionStore.activeTabId],
    ([focused, visible, activeTabId]) => {
      if (focused && visible && activeTabId) {
        const tab = sessionStore.tabs.get(activeTabId)
        if (tab) {
          tab.pending = false
          if (tab.ptyId) attentionStore.ackPty(tab.ptyId) // 清 permission/completed（error 保留）
        }
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
    turnEnded.clear()
    subagentRuns.clear()
  })
}
