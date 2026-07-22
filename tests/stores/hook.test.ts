import { describe, it, expect, beforeEach, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useHookStore } from '@/stores/hook'
import type { HookEventPayload, HookEventDetail } from '@/types/hook'

// Mock @/api/tauri to capture the onHookEvent callback
let capturedOnHookEventCallback: ((payload: HookEventPayload) => void) | null = null

vi.mock('@/api/tauri', () => ({
  onHookEvent: vi.fn((callback: (payload: HookEventPayload) => void) => {
    capturedOnHookEventCallback = callback
    return Promise.resolve(() => {})
  }),
}))

/** Build a minimal HookEventPayload for testing */
function makePayload(
  type: HookEventDetail['type'],
  ptyId: string | null = 'pty1',
): HookEventPayload {
  return {
    ptyId,
    sessionId: 'session1',
    eventName: type,
    state: 'thinking',
    timestamp: Date.now(),
    detail: { type, data: null } as unknown as HookEventDetail,
  }
}

describe('useHookStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    capturedOnHookEventCallback = null
  })

  // ---------- subscribe ----------

  // 注册 ["PreToolUse"] 后分发 PreToolUse 事件时 handler 被调用
  it('HookSubscribe_Receive_001', () => {
    const store = useHookStore()
    store.init()

    const handler = vi.fn()
    store.subscribe(['preToolUse'], handler)

    const payload = makePayload('preToolUse')
    capturedOnHookEventCallback!(payload)

    expect(handler).toHaveBeenCalledOnce()
    expect(handler).toHaveBeenCalledWith(payload)
  })

  // 调用 unsubscribe 后分发事件时 handler 不被调用
  it('HookSubscribe_Unsubscribe_001', () => {
    const store = useHookStore()
    store.init()

    const handler = vi.fn()
    const unsubscribe = store.subscribe(['preToolUse'], handler)

    unsubscribe()

    capturedOnHookEventCallback!(makePayload('preToolUse'))

    expect(handler).not.toHaveBeenCalled()
  })

  // 两个 handler 订阅同一事件类型时都被调用
  it('HookSubscribe_MultiHandler_001', () => {
    const store = useHookStore()
    store.init()

    const handler1 = vi.fn()
    const handler2 = vi.fn()
    store.subscribe(['preToolUse'], handler1)
    store.subscribe(['preToolUse'], handler2)

    const payload = makePayload('preToolUse')
    capturedOnHookEventCallback!(payload)

    expect(handler1).toHaveBeenCalledOnce()
    expect(handler2).toHaveBeenCalledOnce()
    expect(handler1).toHaveBeenCalledWith(payload)
    expect(handler2).toHaveBeenCalledWith(payload)
  })

  // 订阅 ["PreToolUse","PostToolUse"] 后分发每种事件时 handler 都被调用
  it('HookSubscribe_MultiType_001', () => {
    const store = useHookStore()
    store.init()

    const handler = vi.fn()
    store.subscribe(['preToolUse', 'postToolUse'], handler)

    const prePayload = makePayload('preToolUse')
    const postPayload = makePayload('postToolUse')
    capturedOnHookEventCallback!(prePayload)
    capturedOnHookEventCallback!(postPayload)

    expect(handler).toHaveBeenCalledTimes(2)
    expect(handler).toHaveBeenCalledWith(prePayload)
    expect(handler).toHaveBeenCalledWith(postPayload)
  })

  // ---------- dispatch ----------

  // 设置 ptyId="pty1" 分发 PreToolUse 事件时匹配的 handler 被调用
  it('HookDispatch_RouteByType_001', () => {
    const store = useHookStore()
    store.init()

    const handler = vi.fn()
    store.subscribe(['preToolUse'], handler)

    const payload = makePayload('preToolUse', 'pty1')
    capturedOnHookEventCallback!(payload)

    expect(handler).toHaveBeenCalledOnce()
    expect(handler).toHaveBeenCalledWith(payload)
  })

  // ptyId 为 null 时事件被静默丢弃，handler 不被调用
  it('HookDispatch_DropNullPtyId_001', () => {
    const store = useHookStore()
    store.init()

    const handler = vi.fn()
    store.subscribe(['preToolUse'], handler)

    const payload = makePayload('preToolUse', null)
    capturedOnHookEventCallback!(payload)

    expect(handler).not.toHaveBeenCalled()
  })

  // 分发的事件类型不在订阅列表时 handler 不被调用
  it('HookDispatch_NoMatchType_001', () => {
    const store = useHookStore()
    store.init()

    const handler = vi.fn()
    store.subscribe(['preToolUse'], handler)

    const payload = makePayload('postToolUse')
    capturedOnHookEventCallback!(payload)

    expect(handler).not.toHaveBeenCalled()
  })
})
