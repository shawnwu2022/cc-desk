import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useNativeSessionDeletesStore, sessionDeleteTargetKey } from '@/stores/nativeSessionDeletes'
import type { SessionDeletePreviewRequest } from '@/types/nativeSessionDelete'
const io = vi.hoisted(() => ({ preview: vi.fn(), confirm: vi.fn(), inspect: vi.fn() }))
vi.mock('@/api/tauri', () => ({ createNativeSessionDeleteClient: () => io }))
const request = (): SessionDeletePreviewRequest => ({ operationId: '30000000-0000-4000-8000-000000000001', sessionKey: 'key', source: { scopeId: 'scope', instanceId: 'instance', cli: 'codex', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '1', projectIds: ['project'] } } })
const preview = () => ({ operationId: request().operationId, previewId: '30000000-0000-4000-8000-000000000002', nativeSessionId: '30000000-0000-4000-8000-000000000003', cli: 'codex', state: 'ready', rootArchived: true, scope: 'native-subtree', knownDescendantCount: null, reason: null })
const receipt = () => ({ operationId: request().operationId, cli: 'codex', state: 'acknowledged', rootPresent: false, descendants: 'unverified', reason: null })
beforeEach(() => { setActivePinia(createPinia()); vi.resetAllMocks(); io.preview.mockResolvedValue(preview()); io.confirm.mockResolvedValue(receipt()); io.inspect.mockResolvedValue({ ...receipt(), state: 'unknown', reason: 'DELETE_OUTCOME_UNKNOWN' }) })

describe('permanent deletion operation ownership', () => {
  it('PermanentDeleteStore_SeparateReviewAndConfirmation_001', async () => {
    const store = useNativeSessionDeletesStore(), input = request(), key = sessionDeleteTargetKey(input)
    await store.prepare(input)
    expect(store.records.get(key)?.phase).toBe('review')
    await expect(store.execute(key, true, true, () => true)).rejects.toThrow()
    expect(io.confirm).not.toHaveBeenCalled()
    store.beginConfirmation(key)
    await expect(store.execute(key, false, true, () => true)).rejects.toThrow()
    store.cancel(key)
    expect(store.records.has(key)).toBe(false)
    expect(io.confirm).not.toHaveBeenCalled()
  })
  it('PermanentDeleteStore_DuplicateClickOwnsOneMutation_002', async () => {
    let resolve!: (value: unknown) => void
    io.confirm.mockReturnValue(new Promise(done => { resolve = done }))
    const store = useNativeSessionDeletesStore(), key = sessionDeleteTargetKey(request())
    await store.prepare(request()); store.beginConfirmation(key)
    const first = store.execute(key, true, true, () => true)
    const second = store.execute(key, true, true, () => true)
    store.cancel(key)
    expect(store.records.get(key)?.phase).toBe('executing')
    resolve(receipt()); await Promise.all([first, second])
    expect(io.confirm).toHaveBeenCalledTimes(1)
    expect(store.records.get(key)?.receipt).toMatchObject({ state: 'acknowledged', descendants: 'unverified' })
  })
  it('PermanentDeleteStore_UnknownRetainsOriginAndOnlyInspects_003', async () => {
    io.confirm.mockRejectedValue(new Error('private transport payload'))
    const store = useNativeSessionDeletesStore(), input = request(), key = sessionDeleteTargetKey(input)
    await store.prepare(input); store.beginConfirmation(key); await store.execute(key, true, true, () => true)
    expect(store.records.get(key)?.receipt).toMatchObject({ state: 'unknown', rootPresent: null, reason: 'DELETE_OUTCOME_UNKNOWN' })
    const replaced = { ...input, operationId: '30000000-0000-4000-8000-000000000004', source: { ...input.source, scopeId: 'replacement' } }
    await store.prepare(replaced)
    expect(io.preview).toHaveBeenCalledTimes(1)
    expect(store.records.get(key)?.request.operationId).toBe(input.operationId)
    await store.inspect(key)
    expect(store.records.get(key)?.receipt?.state).toBe('unknown')
    expect(io.confirm).toHaveBeenCalledTimes(1)
    expect(io.inspect).toHaveBeenCalledWith({ operationId: input.operationId })
    expect(JSON.stringify(store.records.get(key))).not.toContain('private transport')
  })
  it('PermanentDeleteStore_CancelledPreviewCannotPublishLate_004', async () => {
    let resolve!: (value: unknown) => void
    io.preview.mockReturnValue(new Promise(done => { resolve = done }))
    const store = useNativeSessionDeletesStore(), key = sessionDeleteTargetKey(request())
    const task = store.prepare(request()); store.cancel(key); resolve(preview()); await task
    expect(store.records.has(key)).toBe(false)
    expect(io.confirm).not.toHaveBeenCalled()
  })
  it('PermanentDeleteStore_FinalOwnerLossPreventsMutation_005', async () => {
    const store = useNativeSessionDeletesStore(), key = sessionDeleteTargetKey(request())
    await store.prepare(request()); store.beginConfirmation(key)
    await expect(store.execute(key, true, true, () => false)).rejects.toThrow()
    expect(io.confirm).not.toHaveBeenCalled()
  })
  it('PermanentDeleteStore_ChangedPreviewNeedsExplicitNewReview_006', async () => {
    io.confirm.mockResolvedValue({ ...receipt(), state: 'blocked', rootPresent: true, reason: 'DELETE_PREVIEW_CHANGED' })
    const store = useNativeSessionDeletesStore(), input = request(), key = sessionDeleteTargetKey(input)
    await store.prepare(input); store.beginConfirmation(key); await store.execute(key, true, true, () => true)
    expect(store.canReviewAgain(key)).toBe(true)
    store.restartReview(key)
    await store.prepare({ ...input, operationId: '30000000-0000-4000-8000-000000000004' })
    expect(store.records.get(key)?.phase).toBe('review')
    expect(io.preview).toHaveBeenCalledTimes(2)
    expect(io.confirm).toHaveBeenCalledTimes(1)
  })
  it('PermanentDeleteStore_OwnerLostBeforeDispatchCannotSend_007', async () => {
    let owns = true
    const store = useNativeSessionDeletesStore(), key = sessionDeleteTargetKey(request())
    await store.prepare(request()); store.beginConfirmation(key)
    const task = store.execute(key, true, true, () => owns); owns = false
    await task
    expect(io.confirm).not.toHaveBeenCalled()
    expect(store.records.get(key)?.receipt?.state).toBe('blocked')
  })
})
