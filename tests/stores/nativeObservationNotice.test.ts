import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { useHookStore } from '@/stores/hook'
import type { HookEventPayload } from '@/types/hook'

const io = vi.hoisted(() => ({ listener: null as ((payload: HookEventPayload) => void) | null, stop: vi.fn() }))
vi.mock('@/api/observer', () => ({ onNativeObservation: async (listener: typeof io.listener) => { io.listener = listener; return io.stop } }))
vi.mock('@/api/tauri', () => ({ onHookEvent: async () => () => {} }))
let store: ReturnType<typeof useHookStore>
beforeEach(() => { setActivePinia(createPinia()); vi.clearAllMocks(); io.listener = null; store = useHookStore() })
afterEach(() => { store.$dispose(); vi.restoreAllMocks() })
function payload(eventId = 'receipt-1', generation = 1): HookEventPayload {
  return { ptyId: null, sessionId: 'provider', eventName: 'Stop', state: 'unknown', timestamp: 1,
    runId: 'owned-run', generation, eventId, observerSource: 'claude-hook',
    detail: { type: 'stop', data: { lastAssistantMessage: 'SECRET' } } }
}
function target(generation = 1) { return { cli: 'claude' as const, enabled: true, runId: 'owned-run', generation } }
describe('Native receipt subscription lifetime', () => {
  it('Notice_BusPreservesReceiptWithoutChangingActivity_001', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(1234)
    const observe = vi.fn(), onNotice = vi.fn()
    store.subscribeObservation(target(), observe, { onNotice }); await flushPromises()
    io.listener!(payload())
    expect(onNotice).toHaveBeenCalledOnce()
    expect(onNotice).toHaveBeenCalledWith({ kind: 'reply-ended', eventId: 'receipt-1', receivedAt: 1234, runId: 'owned-run', generation: 1 })
    expect(observe.mock.calls[0][1]).toEqual({ observation: 'active', activity: 'unknown' })
    expect(JSON.stringify(onNotice.mock.calls)).not.toContain('SECRET')
  })
  it('Notice_DuplicateAndReattachedOwnerCannotReplay_002', async () => {
    const first = vi.fn(), second = vi.fn()
    store.subscribeObservation(target(), vi.fn(), { onNotice: first }); await flushPromises()
    io.listener!(payload())
    store.subscribeObservation(target(), vi.fn(), { onNotice: second })
    io.listener!(payload())
    expect(first).toHaveBeenCalledOnce(); expect(second).not.toHaveBeenCalled()
    io.listener!(payload('receipt-2'))
    expect(first).toHaveBeenCalledTimes(2); expect(second).toHaveBeenCalledOnce()
  })
  it('Notice_InvalidMetadataOldRunAndDisabledCliAreRejected_003', async () => {
    const active = vi.fn(), codex = vi.fn(), off = vi.fn()
    store.subscribeObservation(target(), vi.fn(), { onNotice: active })
    store.subscribeObservation({ ...target(), cli: 'codex' }, vi.fn(), { onNotice: codex })
    store.subscribeObservation({ ...target(2), enabled: false }, vi.fn(), { onNotice: off }); await flushPromises()
    io.listener!(payload('old', 2))
    io.listener!({ ...payload(), runId: 'other-run' })
    io.listener!({ ...payload(), observerSource: 'unknown' } as unknown as HookEventPayload)
    io.listener!({ ...payload(), eventId: '' })
    expect(active).not.toHaveBeenCalled(); expect(codex).not.toHaveBeenCalled(); expect(off).not.toHaveBeenCalled()
  })
  it('Notice_UnsubscribeAndGenerationRetirementRevokeDelivery_004', async () => {
    const old = vi.fn(), current = vi.fn()
    const stop = store.subscribeObservation(target(), vi.fn(), { onNotice: old }); await flushPromises()
    stop()
    store.subscribeObservation(target(2), vi.fn(), { onNotice: current }); await flushPromises()
    io.listener!(payload('late-old'))
    io.listener!(payload('new', 2))
    expect(old).not.toHaveBeenCalled(); expect(current).toHaveBeenCalledOnce()
    store.clearSession('owned-run')
    io.listener!(payload('retired', 2)); expect(current).toHaveBeenCalledOnce()
  })
  it('Notice_BoundedReceiptIdsNeverEvictAndReplay_005', async () => {
    const onNotice = vi.fn()
    store.subscribeObservation(target(), vi.fn(), { onNotice }); await flushPromises()
    for (let n = 0; n < 1025; n++) io.listener!(payload('event-' + n))
    expect(onNotice).toHaveBeenCalledTimes(1024)
    io.listener!(payload('event-0')); expect(onNotice).toHaveBeenCalledTimes(1024)
  })
  it('Notice_ConsumerDetachAndThrowCannotAffectActivity_006', async () => {
    const second = vi.fn(), observation = vi.fn()
    let detachSecond!: () => void
    store.subscribeObservation(target(), observation, { onNotice: () => { detachSecond(); throw new Error('SECRET') } })
    detachSecond = store.subscribeObservation(target(), vi.fn(), { onNotice: second }); await flushPromises()
    expect(() => io.listener!(payload())).not.toThrow()
    expect(second).not.toHaveBeenCalled()
    expect(observation.mock.calls[0][1]).toEqual({ observation: 'active', activity: 'unknown' })
  })
})
