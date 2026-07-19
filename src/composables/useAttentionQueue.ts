import type { HookEventPayload } from '@/types/hook'

/**
 * 关注项分类。
 * 严重度升序：completed < permission < error（焦点队列按此排序）。
 */
export type AttentionKind = 'error' | 'permission' | 'completed'

/**
 * 单个未确认的关注项。
 *
 * 独立于 tab.working / tab.pending -- 后者是「用户是否已看到」的注意力确认位，
 * 不是业务状态（codex 对抗审查指出：把等权限/出错/完成塞进 pending 会污染语义）。
 * 关注原因用本结构单独承载，working/pending 退为兼容派生值。
 */
export interface AttentionItem {
  kind: AttentionKind
  ptyId: string
  sessionId?: string
  createdAt: number
  /** 用户确认时间；undefined 表示尚未确认（仍留在焦点队列里） */
  acknowledgedAt?: number
}

/**
 * hook 事件 -> AttentionItem | null 的纯函数。
 *
 * 基于事件 detail.type 的「转变」判定，**不基于 ClaudeState 当前值** --
 * derive_state 把 Stop / idle_prompt / SessionStart / SessionEnd 都归为 idle，
 * 用 state 会把 SessionStart 误报为「完成」。
 *
 * kind 来源（经 codex 对抗审查 + P1 复审验证）：
 * - error: 仅 StopFailure（PostToolUseFailure 被 derive_state 推为 thinking，不计 error）
 * - permission: notification 的 permission_prompt / worker_permission_prompt
 * - completed: notification 的 idle_prompt（**回合结束稳定信号**）
 *   注意：Stop 不直接生成 completed -- stop hook 可能阻止停止让 Claude 继续，且并行 hook
 *   决策 reporter 无法得知，会产生虚假完成项。用 idle_prompt 确认真结束。
 * - 其余事件（sessionStart / sessionEnd / preToolUse / postToolUse / postToolUseFailure /
 *   stop / userPromptSubmit / subagent* / *Compact / unknown）不生成关注项
 */
export function attentionFromEvent(payload: HookEventPayload): AttentionItem | null {
  const { ptyId, sessionId, timestamp, detail } = payload
  // 无法关联终端 tab -> 不生成关注项
  if (!ptyId) return null

  let kind: AttentionKind | null = null

  switch (detail.type) {
    case 'stopFailure':
      kind = 'error'
      break
    case 'notification': {
      const ntype = detail.data.notificationType
      if (ntype === 'permission_prompt' || ntype === 'worker_permission_prompt') {
        kind = 'permission'
      } else if (ntype === 'idle_prompt') {
        kind = 'completed'
      }
      break
    }
    default:
      kind = null
  }

  if (!kind) return null

  return {
    kind,
    ptyId,
    sessionId: sessionId ?? undefined,
    createdAt: timestamp,
  }
}

/**
 * 严重度数值，用于焦点队列排序：error(3) > permission(2) > completed(1)。
 */
export function severityRank(kind: AttentionKind): number {
  switch (kind) {
    case 'error':
      return 3
    case 'permission':
      return 2
    case 'completed':
      return 1
  }
}

/**
 * 构建焦点队列：去重（同 ptyId 取最严重；同严重度取最新）+ 排序
 * （严重度降序，同严重度 createdAt 降序--新的在前）。
 *
 * 输入是 reducer 维护的全部未确认 AttentionItem（可能含同会话多个历史项），
 * 输出是可直接渲染的有序队列：错误 > 等权限 > 新完成。
 *
 * 去重语义：一个会话在队列里只占一个位置，体现其「当前最需关注」的状态--
 * 紧急的（error）不被后续非紧急（completed）覆盖。
 */
export function buildAttentionQueue(items: AttentionItem[]): AttentionItem[] {
  // 去重：同 ptyId 保留 severityRank 最高；同 rank 保留最新 createdAt
  const byPty = new Map<string, AttentionItem>()
  for (const item of items) {
    const existing = byPty.get(item.ptyId)
    if (!existing) {
      byPty.set(item.ptyId, item)
      continue
    }
    const existingRank = severityRank(existing.kind)
    const itemRank = severityRank(item.kind)
    if (itemRank > existingRank || (itemRank === existingRank && item.createdAt >= existing.createdAt)) {
      byPty.set(item.ptyId, item)
    }
  }

  // 排序：严重度降序，同严重度 createdAt 降序
  return [...byPty.values()].sort((a, b) => {
    const rankDiff = severityRank(b.kind) - severityRank(a.kind)
    if (rankDiff !== 0) return rankDiff
    return b.createdAt - a.createdAt
  })
}
