import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { useHookStore } from './hook'
import type { HookEventType } from './hook'
import type { HookEventPayload } from '@/types/hook'
import {
  attentionFromEvent,
  buildAttentionQueue,
  severityRank,
  type AttentionItem,
} from '@/composables/useAttentionQueue'

// 订阅可能产生关注项的事件类型（attentionFromEvent 内部决定哪些真正生成 AttentionItem）
// 不含 stop：stop hook 可能阻止停止让 Claude 继续，用 idle_prompt（notification 子类型）作回合结束稳定信号
const ATTENTION_EVENTS: HookEventType[] = ['stopFailure', 'notification']

/**
 * 焦点队列 store：维护未确认的 AttentionItem，提供有序队列。
 *
 * 独立于 useStatusMonitor 的 working/pending——那是「用户是否已看到」的注意力确认位，
 * 关注原因（错误 / 等权限 / 完成）用 AttentionItem 单独承载，与确认位解耦。
 *
 * 订阅 hook 事件总线，每 ptyId 维护一个 item（ingestEvent 按严重度 upsert），
 * queue getter 输出有序焦点队列（错误 > 等权限 > 新完成）。
 *
 * tombstone 门禁：clearPty（PTY 退出/关 tab）后该 ptyId 标记已关闭，迟到的在途 Hook
 * 事件直接拒绝，防「找不到 tab 的幽灵关注项」。
 */
export const useAttentionStore = defineStore('attention', () => {
  // ptyId -> 未确认 AttentionItem（reducer 维护，每 ptyId 一个）
  const items = ref(new Map<string, AttentionItem>())
  // tombstone：已关闭 PTY 的 ptyId 集合，永久拒绝其迟到事件（防 clearPty 后在途 Hook 复活幽灵项）。
  // ptyId 为 randomUUID 不复用，但 hook 事件可能因 server 拥塞/进程挂起等异常路径延迟到达，
  // 永久保留墓碑保证迟到事件始终被拒；ptyId 字符串占用极小，无需 TTL 清理。
  const closedPtyIds = ref(new Set<string>())

  // 有序焦点队列：buildAttentionQueue 去重（防御）+ 严重度排序
  const queue = computed(() => buildAttentionQueue([...items.value.values()]))

  /** 单点查询：某 ptyId 当前的未确认 AttentionItem（供 SessionList/ProjectNode reactive 消费） */
  function getItem(ptyId: string): AttentionItem | undefined {
    return items.value.get(ptyId)
  }

  let initialized = false
  let unsubscribe: (() => void) | null = null

  /** hook 事件 -> AttentionItem upsert（同 ptyId 取严重度更高者；同严重度取更新者） */
  function ingestEvent(payload: HookEventPayload) {
    const item = attentionFromEvent(payload)
    if (!item) return
    // tombstone 门禁：PTY 已关闭（clearPty）后迟到的在途事件直接拒绝，防幽灵关注项
    if (closedPtyIds.value.has(item.ptyId)) return
    const existing = items.value.get(item.ptyId)
    if (!existing) {
      items.value.set(item.ptyId, item)
      return
    }
    // upsert 规则与 buildAttentionQueue 去重同语义（后续可抽 mergeAttention 共用）
    const existingRank = severityRank(existing.kind)
    const itemRank = severityRank(item.kind)
    if (itemRank > existingRank || (itemRank === existingRank && item.createdAt >= existing.createdAt)) {
      items.value.set(item.ptyId, item)
    }
  }

  /** 用户确认某会话的关注（打开会话 + 窗口聚焦时调）。
   *  error 粘性：默认保留 error item（CLI 异常需持续提示，看了不清），
   *  只在 clearError:true（新回合）或 clearPty（会话结束）时清。 */
  function ackPty(ptyId: string, opts?: { clearError?: boolean }) {
    const existing = items.value.get(ptyId)
    if (!existing) return
    if (existing.kind === 'error' && !opts?.clearError) return
    items.value.delete(ptyId)
  }

  /** PTY 退出 / 关 tab 时清理 + 标 tombstone（拒绝后续迟到事件） */
  function clearPty(ptyId: string) {
    items.value.delete(ptyId)
    closedPtyIds.value.add(ptyId)
  }

  /** 订阅 hook 事件总线（App 启动时调一次） */
  function init() {
    if (initialized) return
    initialized = true
    const hookStore = useHookStore()
    unsubscribe = hookStore.subscribe(ATTENTION_EVENTS, ingestEvent)
  }

  /** 取消订阅（测试 / 卸载用） */
  function dispose() {
    unsubscribe?.()
    unsubscribe = null
    initialized = false
  }

  return { queue, getItem, ingestEvent, ackPty, clearPty, init, dispose }
})
