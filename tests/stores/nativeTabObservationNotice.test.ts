import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { captureNativeAttempt, useNativeTabsStore, type NativeCliTab } from '@/stores/nativeTabs'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import type { NativeObservationNotice } from '@/types/nativeObservationNotice'

beforeEach(() => setActivePinia(createPinia()))
function status(tab: NativeCliTab, phase: LaunchStatus['phase'] = 'running'): LaunchStatus {
  return { instanceId: 'backend', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '2', phase, failure: null }
}
function setup(running = true, cli: 'claude' | 'codex' = 'claude', raw = false) {
  const store = useNativeTabsStore()
  const tab = store.create({ cli, projectId: 'p', projectPath: '/repo', profileId: cli, profileRevision: '1', action: raw ? { kind: 'raw', argv: [] } : { kind: 'new' } })
  store.markStarting(tab.tabId)
  if (running) store.applyLaunchStatus(tab.tabId, status(tab))
  const attempt = captureNativeAttempt(tab)
  const notice = (eventId = 'receipt-1', kind: NativeObservationNotice['kind'] = 'reply-ended'): NativeObservationNotice => ({ kind, eventId, receivedAt: 100, runId: tab.runId, generation: tab.generation })
  return { store, tab, attempt, notice }
}
function adapter(store: ReturnType<typeof useNativeTabsStore>) {
  return createNativeCliAdapter({ tabs: store, history: { all: () => [] }, runtime: {} as any,
    archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
}
function requireNoticeApi(store: ReturnType<typeof useNativeTabsStore>) { expect(typeof store.applyObservationNotice).toBe('function') }

// 原生事件通知只保留已认证发生事实，精确所有权、未读确认和启动生命周期互不混淆。
describe('Exact Native tab occurrence receipts', () => {
  it('Notice_SeparateReceiptUnreadAndDefensiveCopy_001', () => {
    const { store, tab, attempt, notice } = setup(); requireNoticeApi(store)
    const receipt = notice()
    expect(store.applyObservationNotice(tab.tabId, attempt, receipt)).toBe(true)
    receipt.eventId = 'mutated'
    expect(store.tab(tab.tabId)!.observationNotice!.unreadReplyEnd!.eventId).toBe('receipt-1')
    expect(store.applyObservationNotice(tab.tabId, attempt, notice('prompt', 'prompt-submitted'))).toBe(true)
    expect(store.tab(tab.tabId)).toMatchObject({ activityState: 'unknown', observationState: 'off', attentionState: 'none', observationNotice: { recent: { kind: 'prompt-submitted' }, unreadReplyEnd: { kind: 'reply-ended', eventId: 'receipt-1' } } })
  })
  it('Notice_PendingUntilExactRunningReceipt_002', () => {
    const { store, tab, attempt, notice } = setup(false); requireNoticeApi(store)
    expect(store.applyObservationNotice(tab.tabId, attempt, notice())).toBe(true)
    expect(store.applyObservationNotice(tab.tabId, attempt, notice('prompt', 'prompt-submitted'))).toBe(true)
    expect(store.tab(tab.tabId)!.observationNotice?.recent ?? null).toBeNull()
    expect(store.ackReplyEndNotice(tab.tabId, attempt, 'receipt-1')).toBe(false)
    expect(store.applyLaunchStatus(tab.tabId, { ...status(tab), requestId: 'foreign' })).toBe(false)
    expect(store.tab(tab.tabId)!.observationNotice?.recent ?? null).toBeNull()
    expect(store.applyLaunchStatus(tab.tabId, status(tab))).toBe(true)
    expect(store.tab(tab.tabId)!.observationNotice).toMatchObject({ recent: { eventId: 'prompt' }, unreadReplyEnd: { eventId: 'receipt-1' } })
    expect(store.hasOwnedObservationNotice(tab.tabId, attempt)).toBe(true)
  })
  it('Notice_InvalidOwnerMetadataAndSourceRejected_003', () => {
    const { store, tab, attempt, notice } = setup(); requireNoticeApi(store)
    for (const bad of [{ ...attempt, requestId: 'other' }, { ...attempt, runId: 'other' }, { ...attempt, generation: 2 }]) expect(store.applyObservationNotice(tab.tabId, bad, notice())).toBe(false)
    for (const bad of [{ runId: 'other' }, { generation: 2 }, { eventId: '' }, { eventId: 'content with spaces' }, { eventId: 'x'.repeat(129) }, { receivedAt: NaN }, { receivedAt: -1 }, { receivedAt: 0.5 }, { receivedAt: Number.MAX_SAFE_INTEGER + 1 }, { kind: 'unknown' }]) expect(store.applyObservationNotice(tab.tabId, attempt, { ...notice(), ...bad } as NativeObservationNotice)).toBe(false)
    for (const unsupported of [setup(true, 'codex'), setup(true, 'claude', true)]) expect(unsupported.store.applyObservationNotice(unsupported.tab.tabId, unsupported.attempt, unsupported.notice())).toBe(false)
    expect(store.hasOwnedObservationNotice(tab.tabId, attempt)).toBe(false)
  })
  it('Notice_ExactAckKeepsRecentAndRejectsReplay_004', () => {
    const { store, tab, attempt, notice } = setup(); requireNoticeApi(store)
    store.applyObservationNotice(tab.tabId, attempt, notice())
    expect(store.ackReplyEndNotice(tab.tabId, { ...attempt, requestId: 'other' }, 'receipt-1')).toBe(false)
    expect(store.ackReplyEndNotice(tab.tabId, attempt, 'other')).toBe(false)
    expect(store.ackReplyEndNotice(tab.tabId, attempt, 'receipt-1')).toBe(true)
    expect(store.tab(tab.tabId)!.observationNotice).toMatchObject({ recent: { eventId: 'receipt-1' }, unreadReplyEnd: null })
    expect(store.ackReplyEndNotice(tab.tabId, attempt, 'receipt-1')).toBe(false)
    expect(store.applyObservationNotice(tab.tabId, attempt, notice())).toBe(false)
  })
  it.each(['unknown', 'failed', 'exited', 'indeterminate', 'cancelled', 'restart', 'close', 'clear'] as const)('Notice_InvalidatedLifetimeClearsAndRejectsLate_005 %s', invalidate => {
    const { store, tab, attempt, notice } = setup(); requireNoticeApi(store)
    store.applyObservationNotice(tab.tabId, attempt, notice())
    if (invalidate === 'unknown') store.markUnknown(tab.tabId)
    else if (invalidate === 'failed') store.markError(tab.tabId, 'FAILED')
    else if (invalidate === 'restart') { store.applyLaunchStatus(tab.tabId, status(tab, 'exited')); store.restart(tab.tabId, { profileId: 'claude', profileRevision: '1' }) }
    else if (invalidate === 'close') store.close(tab.tabId)
    else if (invalidate === 'clear') store.clear()
    else store.applyLaunchStatus(tab.tabId, status(tab, invalidate))
    expect(store.tab(tab.tabId)?.observationNotice?.recent ?? null).toBeNull()
    expect(store.tab(tab.tabId)?.observationNotice?.unreadReplyEnd ?? null).toBeNull()
    expect(store.applyObservationNotice(tab.tabId, attempt, notice('late'))).toBe(false)
    expect(store.hasOwnedObservationNotice(tab.tabId, attempt)).toBe(false)
  })
  it('Notice_InvalidationKeepsBoundedReplayLedger_006', () => {
    const { store, tab, attempt, notice } = setup(false); requireNoticeApi(store)
    store.applyObservationNotice(tab.tabId, attempt, notice())
    store.markUnknown(tab.tabId); store.applyLaunchStatus(tab.tabId, status(tab))
    expect(store.applyObservationNotice(tab.tabId, attempt, notice())).toBe(false)
    expect(store.tab(tab.tabId)!.observationNotice?.recent ?? null).toBeNull()
    for (let n = 1; n < 1024; n++) expect(store.applyObservationNotice(tab.tabId, attempt, notice('event-' + n))).toBe(true)
    expect(store.applyObservationNotice(tab.tabId, attempt, notice('overflow'))).toBe(false)
    expect(store.applyObservationNotice(tab.tabId, attempt, notice('event-1'))).toBe(false)
    expect(store.tab(tab.tabId)!.observationNotice!.recent!.eventId).toBe('event-1023')
  })
  it('Notice_ProjectionUsesPrivateOwnerIndependentOfUnknownActivity_007', async () => {
    const { store, tab, attempt, notice } = setup(); requireNoticeApi(store)
    store.applyObservationNotice(tab.tabId, attempt, notice())
    const port = adapter(store), first = (await port.listSessions())[0]
    expect(first.activityState).toBe('unknown'); expect(first.attentionKind).toBeUndefined()
    expect(first.observationNotice!.unreadReplyEnd!.eventId).toBe('receipt-1')
    first.observationNotice!.recent!.eventId = 'changed'
    expect(store.tab(tab.tabId)!.observationNotice!.recent!.eventId).toBe('receipt-1')
    store.tab(tab.tabId)!.observationNotice!.unreadReplyEnd!.runId = 'foreign'
    expect((await port.listSessions())[0].observationNotice).toBeUndefined()
    store.tab(tab.tabId)!.observationNotice!.recent!.generation = 99
    expect((await port.listSessions())[0].observationNotice).toBeUndefined()
  })
  it('Notice_SameRunRequestReplacementCannotBorrowPrivateOwner_008', async () => {
    const { store, tab, attempt, notice } = setup(); requireNoticeApi(store)
    store.applyObservationNotice(tab.tabId, attempt, notice())
    store.tab(tab.tabId)!.requestId = 'replaced-request'
    const replacement = captureNativeAttempt(store.tab(tab.tabId)!)
    expect(store.hasOwnedObservationNotice(tab.tabId, replacement)).toBe(false)
    expect((await adapter(store).listSessions())[0].observationNotice).toBeUndefined()
    expect(store.applyObservationNotice(tab.tabId, replacement, notice('new'))).toBe(false)
    store.applyLaunchStatus(tab.tabId, status(store.tab(tab.tabId)!))
    expect(store.tab(tab.tabId)!.observationNotice?.recent ?? null).toBeNull()
  })
  // 公共可选投影不能伪造一条私有账本未接收过的通知。
  it('Notice_PublicMutationCannotFabricateReceiptProof_009', async () => {
    const { store, tab, attempt, notice } = setup(); requireNoticeApi(store)
    store.applyObservationNotice(tab.tabId, attempt, notice())
    store.tab(tab.tabId)!.observationNotice!.recent!.eventId = 'fabricated'
    store.tab(tab.tabId)!.observationNotice!.unreadReplyEnd!.eventId = 'fabricated'
    expect(store.hasOwnedObservationNotice(tab.tabId, attempt)).toBe(false)
    expect(store.ackReplyEndNotice(tab.tabId, attempt, 'fabricated')).toBe(false)
    expect((await adapter(store).listSessions())[0].observationNotice).toBeUndefined()
  })
})
