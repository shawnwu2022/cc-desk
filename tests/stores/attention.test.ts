import { describe, it, expect, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useAttentionStore } from '@/stores/attention'
import type { HookEventPayload } from '@/types/hook'

/** 构造 hook 事件 payload，省略字段用合理默认 */
function makePayload(
  detail: HookEventPayload['detail'],
  overrides: Partial<HookEventPayload> = {}
): HookEventPayload {
  return {
    ptyId: overrides.ptyId ?? 'pty-1',
    sessionId: overrides.sessionId ?? 'sess-1',
    eventName: overrides.eventName ?? 'Test',
    state: overrides.state ?? 'idle',
    timestamp: overrides.timestamp ?? 1000,
    detail,
  }
}

/** 构造 idle_prompt（回合结束稳定信号）payload */
function idlePrompt(overrides: Partial<HookEventPayload> = {}): HookEventPayload {
  return makePayload({ type: 'notification', data: { notificationType: 'idle_prompt' } }, overrides)
}

describe('useAttentionStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  // ingestEvent(idle_prompt) -> queue 含 completed
  it('AttentionStore_Ingest_AddItem_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(idlePrompt())
    expect(store.queue).toHaveLength(1)
    expect(store.queue[0].kind).toBe('completed')
  })

  // 非关注事件（preToolUse）-> queue 空
  it('AttentionStore_Ingest_NullEvent_NoOp_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(makePayload({ type: 'preToolUse', data: { toolName: 'Bash' } }))
    expect(store.queue).toHaveLength(0)
  })

  // 同 ptyId 先 completed 后 error -> merge 取 error（更严重，即使时间更早）
  it('AttentionStore_Ingest_MergeSevere_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(idlePrompt({ timestamp: 100 }))
    store.ingestEvent(makePayload({ type: 'stopFailure', data: { error: 'x' } }, { timestamp: 50 }))
    expect(store.queue).toHaveLength(1)
    expect(store.queue[0].kind).toBe('error')
  })

  // 同 ptyId 先 error 后 completed -> 保留 error（严重的不被非紧急覆盖）
  it('AttentionStore_Ingest_KeepSevere_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(makePayload({ type: 'stopFailure', data: { error: 'x' } }, { timestamp: 100 }))
    store.ingestEvent(idlePrompt({ timestamp: 200 }))
    expect(store.queue).toHaveLength(1)
    expect(store.queue[0].kind).toBe('error')
  })

  // ackPty -> 移除该会话关注（PTY 仍活着，不加 tombstone）
  it('AttentionStore_Ack_RemoveItem_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(idlePrompt({ ptyId: 'p1' }))
    store.ackPty('p1')
    expect(store.queue).toHaveLength(0)
  })

  // ack 后该 PTY 仍接受新事件（ack 不 tombstone，PTY 还活着）
  it('AttentionStore_Ack_StillAcceptsEvents_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(idlePrompt({ ptyId: 'p1' }))
    store.ackPty('p1')
    store.ingestEvent(idlePrompt({ ptyId: 'p1' }))
    expect(store.queue).toHaveLength(1)
  })

  // clearPty -> 移除（PTY 退出 / 关 tab 清理 + tombstone）
  it('AttentionStore_ClearPty_RemoveItem_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(idlePrompt({ ptyId: 'p1' }))
    store.clearPty('p1')
    expect(store.queue).toHaveLength(0)
  })

  // clearPty 后迟到的在途事件不复活（tombstone 门禁，防幽灵关注项，codex P2 复审）
  it('AttentionStore_ClearPty_LateEventRejected_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(
      makePayload({ type: 'notification', data: { notificationType: 'permission_prompt' } }, { ptyId: 'p1' })
    )
    expect(store.queue).toHaveLength(1)
    store.clearPty('p1') // PTY 退出/关 tab
    // 模拟关 tab 后迟到的在途事件（HTTP/前端事件迟到）
    store.ingestEvent(
      makePayload({ type: 'notification', data: { notificationType: 'permission_prompt' } }, { ptyId: 'p1' })
    )
    store.ingestEvent(makePayload({ type: 'stopFailure', data: { error: 'late' } }, { ptyId: 'p1' }))
    expect(store.queue).toHaveLength(0) // 幽灵项未复活
  })

  // 多 ptyId 不同 kind -> queue 按严重度排序（error > permission > completed）
  it('AttentionStore_Queue_SeveritySort_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(idlePrompt({ ptyId: 'p1', timestamp: 100 }))
    store.ingestEvent(makePayload({ type: 'stopFailure', data: { error: 'x' } }, { ptyId: 'p2', timestamp: 200 }))
    store.ingestEvent(
      makePayload({ type: 'notification', data: { notificationType: 'permission_prompt' } }, { ptyId: 'p3', timestamp: 300 })
    )
    expect(store.queue.map((i) => i.kind)).toEqual(['error', 'permission', 'completed'])
  })

  // ack 一个不影响其他会话的关注
  it('AttentionStore_Ack_OthersKept_001', () => {
    const store = useAttentionStore()
    store.ingestEvent(idlePrompt({ ptyId: 'p1' }))
    store.ingestEvent(makePayload({ type: 'stopFailure', data: { error: 'x' } }, { ptyId: 'p2' }))
    store.ackPty('p1')
    expect(store.queue).toHaveLength(1)
    expect(store.queue[0].ptyId).toBe('p2')
  })
})
