import { watch, onMounted, onUnmounted, type Ref } from 'vue'
import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window'
import { useHookStore, type HookEventType, type HookEventHandler } from '@/stores/hook'
import { useSessionStore } from '@/stores/session'
import { useAttentionStore } from '@/stores/attention'
import type { HookEventPayload, NotificationData } from '@/types/hook'
import { fromClaudeHook } from '@/integrations/claudeObserver'
import { createObservationReducer, type ObservationReducer, type ActivityStatus } from '@/integrations/registry'

function legacyActivity(activity: ActivityStatus) {
  return activity === 'waiting_permission' || activity === 'waiting_input' || activity === 'error' ? 'unknown' : activity
}

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

/** 子代理提示与 working 的陈旧上限：终止类 hook 的 POST 丢失（curl 超时静默失败）时
 * 按超时收敛，与 hook store 原生观察路径的 30s 上限保持同一量级。 */
const SUBAGENT_STALE_MS = 30_000

interface SubagentRun {
  ptyId: string
  reducer: ObservationReducer
  lastSeenAt: number
  staleTimer: ReturnType<typeof setTimeout> | null
  turnActive: boolean
}

export function useStatusMonitor(options: { isFocused: Ref<boolean>; isTerminalVisible: Ref<boolean>; requestWindowAttention?: boolean; now?: () => number }) {
  const hookStore = useHookStore()
  const sessionStore = useSessionStore()
  const attentionStore = useAttentionStore()
  const win = getCurrentWindow()
  // 可注入时钟：staleness 判定需要可控时间源（测试用 fake timers 覆盖 Date.now）
  const now = options.now ?? (() => Date.now())

  let unsubscribe: (() => void) | null = null

  /** 记录每个 tab 已结束回合的精确 PTY，避免旧 recap 污染状态或阻塞重启后的新 PTY。 */
  const turnEnded = new Map<string, string>()
  const subagentRuns = new Map<string, SubagentRun>()

  function clearRunTimer(run: SubagentRun) {
    if (run.staleTimer) {
      clearTimeout(run.staleTimer)
      run.staleTimer = null
    }
  }

  /** 提示过期只撤销活动证据，保留真实终止/权限事件的处理资格。 */
  function expireStaleRun(tabId: string) {
    const run = subagentRuns.get(tabId)
    if (!run || now() - run.lastSeenAt < SUBAGENT_STALE_MS) return
    const tab = sessionStore.tabs.get(tabId)
    if (!tab) {
      subagentRuns.delete(tabId)
      return
    }
    // 仅提示仍活跃时让 reducer 进入 unavailable，正常结束的 run 不补打 unavailable
    if (['working', 'subagent_running'].includes(run.reducer.state().activity)) {
      run.reducer.accept({ kind: 'timeout', runId: run.ptyId, generation: 1 })
      tab.activity = legacyActivity(run.reducer.state().activity)
    }
    tab.working = false
  }

  function armStaleTimer(tabId: string, run: SubagentRun) {
    clearRunTimer(run)
    run.lastSeenAt = now()
    run.staleTimer = setTimeout(() => expireStaleRun(tabId), SUBAGENT_STALE_MS)
  }

  // A replaced/ended PTY cannot carry a previous run's causal identities.
  watch(() => [...sessionStore.tabs.values()].map(tab => [tab.tabId, tab.ptyId, tab.status,
    tab.cli, tab.observerEnabled, tab.observation]), () => {
    for (const [tabId, run] of subagentRuns) {
      const tab = sessionStore.tabs.get(tabId)
      if (!tab || tab.ptyId !== run.ptyId || tab.status !== 'running') {
        clearRunTimer(run)
        run.reducer.accept({ kind: 'off', runId: run.ptyId, generation: 1 })
        subagentRuns.delete(tabId)
      } else if ((tab.cli ?? 'claude') !== 'claude' || tab.observerEnabled === false
        || tab.observation === 'off' || tab.observation === 'unavailable') {
        // Keep dedupe and invalidated IDs for the same PTY if observation resumes.
        clearRunTimer(run)
        run.turnActive = false
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
    // 后端对所有 hook 事件恒带 claude-hook 鉴权标签（hook_events.rs），无标签 payload 没有生产来源。
    if (payload.observerSource !== 'claude-hook') return
    const event = fromClaudeHook(payload)
    if (!event || event.runId !== ptyId || event.generation !== 1) return
    const type = payload.detail.type
    // 已结束回合的迟到活动不能先污染 reducer、投影或陈旧计时；新 prompt 和新 PTY 会解除该边界。
    if (ACTIVITY_EVENTS.has(type) && turnEnded.get(tab.tabId) === ptyId) return
    let run = subagentRuns.get(tab.tabId)
    if (!run || run.ptyId !== ptyId) {
      run = { ptyId, reducer: createObservationReducer({ runId: ptyId, generation: 1 }), lastSeenAt: now(), staleTimer: null, turnActive: false }
      subagentRuns.set(tab.tabId, run)
    }
    run.reducer.accept(event)
    const state = run.reducer.state()
    tab.observation = state.observation
    tab.activity = legacyActivity(state.activity)
    armStaleTimer(tab.tabId, run)

    // 回合状态机自原通用分支逐事件迁移：通用分支因后端恒打标签在生产不可达。
    // sessionStart：直接分配 session_id
    if (type === 'sessionStart') {
      if (payload.sessionId && !tab.sessionId) {
        const data = payload.detail.data as { model?: string }
        sessionStore.assignSessionIdByPtyId(ptyId, payload.sessionId, data.model)
      }
      return
    }

    // userPromptSubmit → 进入 working + 设置标题 + 开始新回合
    if (type === 'userPromptSubmit') {
      turnEnded.delete(tab.tabId)
      run.turnActive = true
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
    if (ACTIVITY_EVENTS.has(type)) {
      if (turnEnded.get(tab.tabId) === ptyId) return
      run.turnActive = true
      tab.working = true
      tab.pending = false
      // 新回合:清所有 attention（含 error）
      attentionStore.ackPty(ptyId, { clearError: true })
      return
    }

    // notification：根据 notification_type 区分处理
    if (type === 'notification') {
      if (!tab.working && !run.turnActive) return

      const data = payload.detail.data as NotificationData
      const ntype = data.notificationType

      if (ntype === 'idle_prompt') {
        // 回合结束，等待用户下一条消息
        turnEnded.set(tab.tabId, ptyId)
        run.turnActive = false
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

    // 已知回合可在提示过期或等待权限后收到真实终止；空闲/重复事件仍不发布提醒。
    if (!tab.working && !run.turnActive) return
    run.turnActive = false

    // stop/stopFailure 标记回合结束
    if (type !== 'sessionEnd') {
      turnEnded.set(tab.tabId, ptyId)
    }

    tab.working = false

    // sessionEnd 不需要 pending/attention 提示
    if (type === 'sessionEnd') return

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
    for (const run of subagentRuns.values()) clearRunTimer(run)
    subagentRuns.clear()
  })
}
