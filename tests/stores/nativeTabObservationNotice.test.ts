import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { captureNativeAttempt, useNativeTabsStore, type NativeCliTab } from '@/stores/nativeTabs'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import type { NativeObservationNotice } from '@/types/nativeObservationNotice'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'

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
  // 关闭缓存仅命中完整来源键，恢复的新运行不借用旧通知，显式清除不会影响其他来源。
  it('Notice_CloseResumeExactSource_011', async () => {
    const { store, tab, attempt, notice } = setup()
    const sessionKey = JSON.stringify(['local', 'claude', 'root-one', 'session'])
    store.tab(tab.tabId)!.sourceSessionKey = sessionKey
    store.applyObservationIdentity(tab.tabId, attempt, 'session')
    const entry: NativeHistoryEntry = { key: 'one', context: { cli: 'claude', projectId: 'p', projectPath: '/repo', profileId: 'claude', profileRevision: '1' }, sessions: [{ type: 'session', sessionKey, nativeSessionId: 'session', title: 'One', cwd: '/repo', updatedAt: '2026-10-12T00:00:00Z', truncated: false }], loaded: true, loading: false, error: null, requestEpoch: '1' }
    const other = { ...entry, key: 'other', sessions: [{ ...entry.sessions[0], sessionKey: JSON.stringify(['local', 'claude', 'root-two', 'session']) }] }
    const port = createNativeCliAdapter({ tabs: store, history: { all: () => [entry, other] }, runtime: {
      stopTab: async value => { store.applyLaunchStatus(value.tabId, status(value, 'exited')) },
      restartTab: () => { throw new Error('unexpected restart') },
      createTab: input => { const created = store.create({ cli: 'claude', projectId: 'p', projectPath: '/repo', profileId: 'claude', profileRevision: '1', sourceSessionKey: input.sourceSessionKey, action: input.action }); store.markStarting(created.tabId); return created },
    }, archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
    store.applyObservationNotice(tab.tabId, attempt, notice())
    expect(store.applyObservationNotice(tab.tabId, attempt, notice())).toBe(false)
    await port.closeSession('native-tab:' + tab.tabId)
    const closed = (await port.listSessions()).find(row => row.adapterSessionId === sessionKey)!
    expect(closed.observationNotice?.unreadReplyEnd?.eventId).toBe('receipt-1')
    expect((await port.listSessions()).find(row => row.adapterSessionId === other.sessions[0].sessionKey)!.observationNotice).toBeUndefined()
    const resumed = await port.resumeSession({ ...closed, nativeOrigin: entry.context })
    expect(resumed.observationNotice).toBeUndefined()
    const newTab = store.tab(resumed.adapterSessionId)!
    store.applyLaunchStatus(newTab.tabId, status(newTab))
    const newAttempt = captureNativeAttempt(newTab)
    store.applyObservationNotice(newTab.tabId, newAttempt, { ...notice('receipt-2'), runId: newTab.runId, generation: newTab.generation, receivedAt: 200 })
    expect(store.applyObservationNotice(newTab.tabId, attempt, notice('late-old'))).toBe(false)
    await port.closeSession(resumed.id)
    const latest = (await port.listSessions()).find(row => row.id === closed.id)!
    expect(latest.observationNotice?.unreadReplyEnd?.eventId).toBe('receipt-2')
    expect(() => port.clearUnread!(closed.id, closed)).toThrow('STALE_SESSION_ATTEMPT')
    expect((await port.listSessions()).find(row => row.id === closed.id)!.observationNotice?.unreadReplyEnd?.eventId).toBe('receipt-2')
    await port.clearUnread!(closed.id, latest)
    expect((await port.listSessions()).find(row => row.id === closed.id)!.observationNotice).toBeUndefined()
  })
  // 已结束进程保留本次未读事实，但拒绝迟到事件，重新启动清除旧代次。
  it('Notice_ExitKeepsUnread_010', async () => {
    const { store, tab, attempt, notice } = setup()
    store.applyObservationNotice(tab.tabId, attempt, notice())
    store.applyLaunchStatus(tab.tabId, status(tab, 'exited'))
    expect(store.hasOwnedObservationNotice(tab.tabId, attempt)).toBe(true)
    expect((await adapter(store).listSessions())[0].observationNotice?.unreadReplyEnd?.eventId).toBe('receipt-1')
    expect(store.applyObservationNotice(tab.tabId, attempt, notice('late'))).toBe(false)
    store.restart(tab.tabId, { profileId: 'claude', profileRevision: '1' })
    expect(store.hasOwnedObservationNotice(tab.tabId, attempt)).toBe(false)
    expect(store.tab(tab.tabId)?.observationNotice?.unreadReplyEnd).toBeNull()
  })
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
  it('Notice_PendingReceiptSurvivesDirectExit_012', async () => {
    const { store, tab, attempt, notice } = setup(false)
    expect(store.applyObservationNotice(tab.tabId, attempt, notice())).toBe(true)
    expect(store.applyLaunchStatus(tab.tabId, status(tab, 'exited'))).toBe(true)
    expect(store.hasOwnedObservationNotice(tab.tabId, attempt)).toBe(true)
    expect((await adapter(store).listSessions())[0].observationNotice?.unreadReplyEnd?.eventId).toBe('receipt-1')
    expect(store.applyObservationNotice(tab.tabId, attempt, notice('late'))).toBe(false)
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
  it.each(['unknown', 'failed', 'indeterminate', 'cancelled', 'restart', 'close', 'clear'] as const)('Notice_InvalidatedLifetimeClearsAndRejectsLate_005 %s', invalidate => {
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
