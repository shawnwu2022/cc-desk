import { describe, it, expect } from 'vitest'
import {
  attentionFromEvent,
  buildAttentionQueue,
  severityRank,
  type AttentionItem,
} from '@/composables/useAttentionQueue'
import type { HookEventPayload } from '@/types/hook'

/** 构造 hook 事件 payload，省略字段用合理默认 */
function makePayload(
  detail: HookEventPayload['detail'],
  overrides: Partial<HookEventPayload> = {}
): HookEventPayload {
  return {
    ptyId: overrides.ptyId === undefined ? 'pty-1' : overrides.ptyId,
    sessionId: overrides.sessionId ?? 'sess-1',
    eventName: overrides.eventName ?? 'Test',
    state: overrides.state ?? 'idle',
    timestamp: overrides.timestamp ?? 1000,
    detail,
  }
}

/** 构造 AttentionItem，省略字段用合理默认 */
function makeItem(
  overrides: Partial<AttentionItem> & { ptyId: string; kind: AttentionItem['kind'] }
): AttentionItem {
  return {
    kind: overrides.kind,
    ptyId: overrides.ptyId,
    sessionId: overrides.sessionId,
    createdAt: overrides.createdAt ?? 1000,
    acknowledgedAt: overrides.acknowledgedAt,
  }
}

describe('attentionFromEvent', () => {
  // StopFailure -> 错误（唯一进入 error 的事件）
  it('Attention_StopFailure_Error_001', () => {
    const out = attentionFromEvent(makePayload({ type: 'stopFailure', data: { error: 'boom' } }))
    expect(out?.kind).toBe('error')
  })

  // PostToolUseFailure 不算 error（derive_state 推为 thinking，codex 对抗审查反驳）
  it('Attention_PostToolUseFailure_NotError_001', () => {
    const out = attentionFromEvent(
      makePayload({
        type: 'postToolUseFailure',
        data: { toolName: 'Bash', error: 'x', isInterrupt: true },
      })
    )
    expect(out).toBeNull()
  })

  // 权限请求 -> permission
  it('Attention_PermissionPrompt_Permission_001', () => {
    const out = attentionFromEvent(
      makePayload({ type: 'notification', data: { notificationType: 'permission_prompt' } })
    )
    expect(out?.kind).toBe('permission')
  })

  // worker 权限请求 -> permission
  it('Attention_WorkerPermission_Permission_001', () => {
    const out = attentionFromEvent(
      makePayload({ type: 'notification', data: { notificationType: 'worker_permission_prompt' } })
    )
    expect(out?.kind).toBe('permission')
  })

  // Stop 不直接生成 completed--stop hook 可能阻止停止让 Claude 继续，且并行 hook 决策
  // reporter 无法得知（codex P1 复审）；用 idle_prompt 作回合结束稳定信号
  it('Attention_Stop_NotCompleted_001', () => {
    const out = attentionFromEvent(makePayload({ type: 'stop', data: { stopHookActive: true } }))
    expect(out).toBeNull()
  })

  // Stop 即使 stopHookActive=false 也不直接 completed（统一由 idle_prompt 确认，避免虚假完成项）
  it('Attention_Stop_NoHookActive_NotCompleted_001', () => {
    const out = attentionFromEvent(makePayload({ type: 'stop', data: { stopHookActive: false } }))
    expect(out).toBeNull()
  })

  // idle_prompt -> 完成（回合结束转变，与 Stop 等价的另一信号）
  it('Attention_IdlePrompt_Completed_001', () => {
    const out = attentionFromEvent(
      makePayload({ type: 'notification', data: { notificationType: 'idle_prompt' } })
    )
    expect(out?.kind).toBe('completed')
  })

  // SessionStart -> null（codex 阻塞级反驳：SessionStart 也→idle，用 state 会误报完成）
  it('Attention_SessionStart_Null_001', () => {
    const out = attentionFromEvent(
      makePayload({ type: 'sessionStart', data: { cwd: '/p', source: 'startup' } })
    )
    expect(out).toBeNull()
  })

  // 其余事件不应生成关注项（default 分支）
  const nullCases: Array<[string, HookEventPayload['detail']]> = [
    ['SessionEnd', { type: 'sessionEnd', data: {} }],
    ['PreToolUse', { type: 'preToolUse', data: { toolName: 'Bash' } }],
    ['PostToolUse', { type: 'postToolUse', data: { toolName: 'Bash' } }],
    ['UserPromptSubmit', { type: 'userPromptSubmit', data: { prompt: 'hi' } }],
    ['SubagentStart', { type: 'subagentStart', data: { agentType: 'x' } }],
    ['SubagentStop', { type: 'subagentStop', data: { agentType: 'x' } }],
    ['PreCompact', { type: 'preCompact', data: {} }],
    ['PostCompact', { type: 'postCompact', data: {} }],
  ]
  it.each(nullCases)('Attention_%s_Null_001', (_name, detail) => {
    const out = attentionFromEvent(makePayload(detail))
    expect(out).toBeNull()
  })

  // notification 其他类型 -> null（既非权限也非 idle_prompt）
  it('Attention_NotificationOther_Null_001', () => {
    const out = attentionFromEvent(
      makePayload({ type: 'notification', data: { notificationType: 'something_else' } })
    )
    expect(out).toBeNull()
  })

  // ptyId 缺失 -> null（无法关联终端 tab）
  it('Attention_NoPtyId_Null_001', () => {
    const out = attentionFromEvent(
      makePayload({ type: 'stopFailure', data: { error: 'x' } }, { ptyId: null })
    )
    expect(out).toBeNull()
  })

  // 生成的 AttentionItem 携带 ptyId / sessionId / createdAt（焦点队列跳转 + 去重要用）
  it('Attention_ItemCarriesIdentity_001', () => {
    const out = attentionFromEvent(
      makePayload({ type: 'notification', data: { notificationType: 'idle_prompt' } }, { ptyId: 'pty-9', sessionId: 'sess-9', timestamp: 5555 })
    )
    expect(out).toEqual({
      kind: 'completed',
      ptyId: 'pty-9',
      sessionId: 'sess-9',
      createdAt: 5555,
    } satisfies AttentionItem)
  })
})

describe('severityRank', () => {
  // 严重度顺序：error > permission > completed（焦点队列排序依据）
  it('Queue_SeverityRank_Order_001', () => {
    expect(severityRank('error')).toBeGreaterThan(severityRank('permission'))
    expect(severityRank('permission')).toBeGreaterThan(severityRank('completed'))
  })
})

describe('buildAttentionQueue', () => {
  // 空输入 -> 空队列
  it('Queue_Empty_001', () => {
    expect(buildAttentionQueue([])).toEqual([])
  })

  // 单项 -> 原样返回
  it('Queue_Single_001', () => {
    const item = makeItem({ ptyId: 'p1', kind: 'permission' })
    expect(buildAttentionQueue([item])).toEqual([item])
  })

  // 多严重度 -> error > permission > completed
  it('Queue_SeveritySort_001', () => {
    const items = [
      makeItem({ ptyId: 'p1', kind: 'completed', createdAt: 100 }),
      makeItem({ ptyId: 'p2', kind: 'error', createdAt: 200 }),
      makeItem({ ptyId: 'p3', kind: 'permission', createdAt: 300 }),
    ]
    expect(buildAttentionQueue(items).map((i) => i.kind)).toEqual(['error', 'permission', 'completed'])
  })

  // 同严重度 -> 新 createdAt 在前
  it('Queue_SameSeverity_NewestFirst_001', () => {
    const items = [
      makeItem({ ptyId: 'p1', kind: 'completed', createdAt: 100 }),
      makeItem({ ptyId: 'p2', kind: 'completed', createdAt: 300 }),
      makeItem({ ptyId: 'p3', kind: 'completed', createdAt: 200 }),
    ]
    expect(buildAttentionQueue(items).map((i) => i.createdAt)).toEqual([300, 200, 100])
  })

  // 去重：同 ptyId 取最严重（error 胜 completed，即使 error 时间更早）
  it('Queue_Dedupe_MostSevereWins_001', () => {
    const items = [
      makeItem({ ptyId: 'p1', kind: 'completed', createdAt: 100 }),
      makeItem({ ptyId: 'p1', kind: 'error', createdAt: 50 }),
    ]
    const out = buildAttentionQueue(items)
    expect(out).toHaveLength(1)
    expect(out[0].kind).toBe('error')
  })

  // 去重：同 ptyId 同严重度 -> 取最新 createdAt
  it('Queue_Dedupe_SameSeverity_Newest_001', () => {
    const items = [
      makeItem({ ptyId: 'p1', kind: 'permission', createdAt: 100 }),
      makeItem({ ptyId: 'p1', kind: 'permission', createdAt: 300 }),
    ]
    const out = buildAttentionQueue(items)
    expect(out).toHaveLength(1)
    expect(out[0].createdAt).toBe(300)
  })

  // 不同 ptyId 都保留
  it('Queue_DifferentPty_AllKept_001', () => {
    const items = [
      makeItem({ ptyId: 'p1', kind: 'completed' }),
      makeItem({ ptyId: 'p2', kind: 'permission' }),
    ]
    expect(buildAttentionQueue(items)).toHaveLength(2)
  })
})
