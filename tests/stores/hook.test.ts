import { describe, it, expect, beforeEach, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useHookStore } from '@/stores/hook'
import type { HookEventPayload, HookEventDetail } from '@/types/hook'

let capturedNative: ((payload: HookEventPayload) => void) | null = null
vi.mock('@/api/observer', () => ({ onNativeObservation: vi.fn((handler: (payload: HookEventPayload) => void) => {
  capturedNative = handler
  return Promise.resolve(() => {})
}) }))
const observationTarget = { runId: 'run-claude', generation: 3, cli: 'claude' as const, enabled: true }

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

  it('D13_HookStore_AuthenticatedObservationBypassesLegacyPtySubscribers_001', () => {
    const store = useHookStore()
    store.init()

    const legacyHandler = vi.fn()
    const observationHandler = vi.fn()
    store.subscribe(['userPromptSubmit'], legacyHandler)
    store.subscribeObservation(observationTarget, observationHandler)

    capturedNative!({
      ptyId: 'legacy-looking-pty',
      sessionId: 'native-session',
      eventName: 'UserPromptSubmit',
      state: 'thinking',
      timestamp: 1,
      runId: 'run-claude',
      generation: 3,
      eventId: 'event-1',
      observerSource: 'claude-hook',
      detail: { type: 'userPromptSubmit', data: { prompt: 'must-not-route-by-pty' } },
    })

    expect(observationHandler).toHaveBeenCalledOnce()
    expect(observationHandler).toHaveBeenCalledWith({
      kind: 'working',
      runId: 'run-claude',
      generation: 3,
      eventId: 'event-1',
    }, { observation: 'active', activity: 'unknown' })
    expect(legacyHandler).not.toHaveBeenCalled()
  })

  it('D13_HookStore_ObservationUnsubscribeStopsOnlyObservationStream_002', () => {
    const store = useHookStore()
    store.init()

    const handler = vi.fn()
    const unsubscribe = store.subscribeObservation(observationTarget, handler)
    unsubscribe()

    capturedNative!({
      ptyId: null,
      sessionId: null,
      eventName: 'Notification',
      state: 'waiting_permission',
      timestamp: 1,
      runId: 'run-claude',
      generation: 3,
      eventId: 'event-2',
      observerSource: 'claude-hook',
      detail: {
        type: 'notification',
        data: { notificationType: 'permission_prompt' },
      },
    })

    expect(handler).not.toHaveBeenCalled()
  })

})

it('D13_HookStore_OneThrowingSubscriberCannotAbortOtherObservers_003', () => {
  setActivePinia(createPinia())
  const store = useHookStore()
  void store.init()
  const next = vi.fn()
  store.subscribe(['preToolUse'], () => { throw new Error('private-failure') })
  store.subscribe(['preToolUse'], next)
  expect(() => capturedOnHookEventCallback!(makePayload('preToolUse'))).not.toThrow()
  expect(next).toHaveBeenCalledOnce()
})

function nativePayload(eventId = 'event'): HookEventPayload {
  return { ptyId: null, sessionId: 'sid', eventName: 'UserPromptSubmit', state: 'unknown', timestamp: 1,
    runId: 'run-claude', generation: 3, eventId, observerSource: 'claude-hook', detail: { type: 'userPromptSubmit', data: {} } }
}
it('D13_HookStore_RunCliAndGenerationAreExactNotActiveTab', () => {
  setActivePinia(createPinia()); const store = useHookStore()
  const current = vi.fn(), foreign = vi.fn(), codex = vi.fn(), disabled = vi.fn()
  const stops = [
    store.subscribeObservation(observationTarget, current),
    store.subscribeObservation({ ...observationTarget, generation: 4 }, foreign),
    store.subscribeObservation({ ...observationTarget, cli: 'codex' }, codex),
    store.subscribeObservation({ ...observationTarget, enabled: false }, disabled),
  ]
  capturedNative!(nativePayload())
  expect(current).toHaveBeenCalledOnce()
  expect(store.observationFor(observationTarget)).toEqual({ observation: 'active', activity: 'unknown' })
  expect(foreign).not.toHaveBeenCalled(); expect(codex).not.toHaveBeenCalled(); expect(disabled).not.toHaveBeenCalled()
  stops.forEach(stop => stop()); store.$dispose()
})
it('D13_HookStore_ClearRejectsLateMetadataAndRevokesState', () => {
  setActivePinia(createPinia()); const store = useHookStore(); const handler = vi.fn()
  store.subscribeObservation(observationTarget, handler)
  store.clearSession(observationTarget.runId)
  capturedNative!(nativePayload())
  expect(handler).not.toHaveBeenCalled(); expect(store.observationFor(observationTarget)).toBeUndefined()
  store.$dispose()
})
it('D13_HookStore_MalformedEventsCannotEscapeDispatch', () => {
  setActivePinia(createPinia()); const store = useHookStore(); store.subscribeObservation(observationTarget, vi.fn())
  for (const bad of [null, [], {}, { ...nativePayload(), generation: 0 }, { ...nativePayload(), detail: null }]) {
    expect(() => capturedNative!(bad as never)).not.toThrow()
  }
  store.$dispose()
})
it('D13_HookStore_DisposeDuringListenerInstallationReleasesListener', async () => {
  const { onNativeObservation } = await import('@/api/observer')
  let release: ((stop: () => void) => void) | undefined
  vi.mocked(onNativeObservation).mockImplementationOnce(() => new Promise(resolve => { release = resolve }))
  setActivePinia(createPinia()); const store = useHookStore(); const stop = vi.fn()
  store.subscribeObservation(observationTarget, vi.fn()); store.$dispose()
  release!(stop); await Promise.resolve(); await Promise.resolve()
  expect(stop).toHaveBeenCalledOnce()
})
