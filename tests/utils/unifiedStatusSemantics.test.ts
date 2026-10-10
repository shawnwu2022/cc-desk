import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { createObservationReducer } from '@/integrations/registry'
import { fromClaudeHook } from '@/integrations/claudeObserver'
import { captureNativeAttempt, useNativeTabsStore } from '@/stores/nativeTabs'
import { useAttentionStore } from '@/stores/attention'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { deriveSessionVisualState } from '@/utils/sessionPresentation'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import type { TerminalTab } from '@/stores/session'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { HookEventDetail } from '@/types/hook'

function row(extra: Record<string, unknown> = {}): UnifiedSession {
  return { id: 'one', projectKey: '/repo', projectPath: '/repo', runtime: 'legacy-claude', cli: 'claude',
    title: 'One', processState: 'running', attentionState: 'none', opened: true, archived: false,
    resumable: true, adapterSessionId: 'one', lastActivityAt: 1, ...extra }
}
function receipt(tab: ReturnType<ReturnType<typeof useNativeTabsStore>['create']>, phase: LaunchStatus['phase'] = 'running'): LaunchStatus {
  return { instanceId: 'test-host', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, phase,
    revision: '1', failure: null }
}
beforeEach(() => { setActivePinia(createPinia()) })

describe('Unified historical status semantics', () => {
  // 删除活动/关注原因投影会使这些边界断言失败，进程运行不能代替回合活动。
  it.each([
    ['working', 'working'], ['thinking', 'working'], ['tool_executing', 'working'],
    ['subagent_running', 'working'], ['compacting', 'working'], ['idle', 'running'],
    ['waiting_permission', 'permission'], ['waiting_input', 'needs-user'], ['waiting', 'needs-user'],
    ['error', 'error'], ['unknown', 'unknown'],
  ])('Activity_DetailMapping_001 %s', (activityState, expected) => {
    expect(deriveSessionVisualState(row({ activityState }))).toBe(expected)
  })
  it.each(['error', 'permission', 'completed'])('Attention_ExplicitReason_002 %s', attentionKind => {
    expect(deriveSessionVisualState(row({ activityState: 'unknown', attentionKind, attentionState: 'needs-user' }))).toBe(attentionKind)
  })
  it('Attention_HistoricalPriorityAndSelection_003', () => {
    expect(deriveSessionVisualState(row({ activityState: 'working', attentionKind: 'error' }))).toBe('working')
    expect(deriveSessionVisualState(row({ attentionKind: 'error' }), true)).toBe('error')
    expect(deriveSessionVisualState(row({ attentionKind: 'completed' }), true)).toBe('completed')
    expect(deriveSessionVisualState(row({ attentionKind: 'permission' }), true)).toBe('permission')
    expect(deriveSessionVisualState(row({ processState: 'stopped', attentionKind: 'completed' }), true)).toBe('stopped')
    expect(deriveSessionVisualState(row({ processState: 'stopped' }))).toBe('closed')
    expect(deriveSessionVisualState(row({ processState: 'stopped', opened: false }), true)).toBe('closed')
    expect(deriveSessionVisualState(row())).toBe('unknown')
  })
  // 所有已知运行态活动/关注原因保持同一显示，不从选择推断运行中。
  it.each([
    [{ activityState: 'waiting_permission' }, 'permission'],
    [{ activityState: 'waiting_input' }, 'needs-user'],
    [{ activityState: 'waiting' }, 'needs-user'],
    [{ attentionState: 'needs-user' }, 'needs-user'],
    [{ attentionKind: 'completed' }, 'completed'],
    [{ activityState: 'subagent_running' }, 'working'],
    [{ activityState: 'unknown' }, 'unknown'],
  ])('Activity_FocusInvariant_009 %j', (extra, expected) => {
    expect(deriveSessionVisualState(row(extra), true)).toBe(expected)
    expect(deriveSessionVisualState(row(extra), false)).toBe(expected)
  })
  // 子代理生命周期提示不能盖过真实权限请求或粘性错误。
  it.each(['permission', 'error'] as const)('Subagent_AttentionWins_010 %s', attentionKind => {
    const session = row({ activityState: 'subagent_running', attentionKind, attentionState: 'needs-user' })
    expect(deriveSessionVisualState(session, true)).toBe(attentionKind)
    expect(deriveSessionVisualState(session, false)).toBe(attentionKind)
  })
  it('Legacy_ExplicitAttentionAndWorkingProjection_004', async () => {
    const tab: TerminalTab = { tabId: 'tab', projectPath: '/repo', ptyId: 'pty', sessionId: 'saved', name: 'Legacy',
      status: 'running', createdAt: 1, lastActiveAt: 2, working: true, pending: false, isResume: false }
    const attention = useAttentionStore()
    const adapter = createLegacyClaudeAdapter({
      store: { tabs: new Map([['tab', tab]]), getCatalogHistoryFor: () => [], getArchivedSessions: () => [],
        createTab: () => 'unused', setActiveTab: () => {}, removeTab: () => {}, closeTab: async () => {},
        updateTabName: () => {}, archiveSession: async () => {}, restoreSession: async () => {} },
      runtime: { startTab: async () => {}, stopTab: async () => {}, restartTab: async () => {}, renameTab: async () => {} },
      projectPaths: () => ['/repo'], attention,
    })
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'working' })
    tab.working = false
    const event = { ptyId: 'pty', sessionId: 'saved', eventName: 'Notification', state: 'idle' as const, timestamp: 5,
      detail: { type: 'notification' as const, data: { notificationType: 'idle_prompt' } } }
    attention.ingestEvent({ ...event, detail: { type: 'stop', data: {} } })
    expect((await adapter.listSessions())[0]).not.toHaveProperty('attentionKind', 'completed')
    attention.ingestEvent(event)
    expect((await adapter.listSessions())[0]).toMatchObject({ attentionKind: 'completed', attentionState: 'needs-user' })
    attention.ackPty('pty')
    expect((await adapter.listSessions())[0].attentionKind).toBeUndefined()
    attention.ingestEvent({ ...event, detail: { type: 'stopFailure', data: {} } })
    attention.ackPty('pty')
    expect((await adapter.listSessions())[0].attentionKind).toBe('error')
    tab.activity = 'unknown'; tab.working = true
    expect((await adapter.listSessions())[0].activityState).toBe('unknown')
    tab.activity = 'working'; tab.observerEnabled = false
    expect((await adapter.listSessions())[0].activityState).toBe('unknown')
    tab.observerEnabled = true; tab.observation = 'unavailable'
    expect((await adapter.listSessions())[0].activityState).toBe('unknown')
    tab.observation = 'active'
    expect((await adapter.listSessions())[0].activityState).toBe('working')
    attention.ackPty('pty', { clearError: true })
    attention.ingestEvent({ ...event, sessionId: 'foreign' })
    expect((await adapter.listSessions())[0].attentionKind).toBeUndefined()
    tab.status = 'stopped'; tab.ptyId = null
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'unknown', attentionState: 'none' })
    expect((await adapter.listSessions())[0].attentionKind).toBeUndefined()
  })
  it('Native_OrderedActivitySurvivesProjection_005', async () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'profile', profileRevision: '1', action: { kind: 'new' } })
    const attempt = captureNativeAttempt(tab)
    const reducer = createObservationReducer(attempt)
    tabs.markStarting(tab.tabId)
    reducer.accept({ ...attempt, kind: 'working', eventId: 'work', sourceSequence: '1' })
    tabs.applyObservation(tab.tabId, attempt, reducer.state())
    tabs.applyLaunchStatus(tab.tabId, receipt(tab))
    const adapter = createNativeCliAdapter({ tabs, history: { all: () => [] },
      runtime: { createTab: () => tab, restartTab: () => tab, stopTab: async () => {} },
      archive: { getArchivedSessions: () => [], archiveSession: async () => {}, restoreSession: async () => {} } })
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'working', observationState: 'active' })
    expect(deriveSessionVisualState((await adapter.listSessions())[0])).toBe('working')
    reducer.accept({ ...attempt, kind: 'waiting', eventId: 'wait', sourceSequence: '2' })
    tabs.applyObservation(tab.tabId, attempt, reducer.state())
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'waiting', attentionState: 'needs-user' })
    expect((await adapter.listSessions())[0].attentionKind).toBeUndefined()
    reducer.accept({ ...attempt, kind: 'working', eventId: 'unordered' })
    tabs.applyObservation(tab.tabId, attempt, reducer.state())
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'unknown', attentionState: 'none' })
    tabs.applyLaunchStatus(tab.tabId, receipt(tab, 'exited'))
    expect((await adapter.listSessions())[0]).toMatchObject({ activityState: 'unknown', attentionState: 'none' })
    expect((await adapter.listSessions())[0].attentionKind).toBeUndefined()
  })
  it('Native_LatestPendingStateAndOwnerRejection_006', () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'profile', profileRevision: '1', action: { kind: 'new' } })
    const owner = captureNativeAttempt(tab)
    tabs.markStarting(tab.tabId)
    tabs.applyObservation(tab.tabId, owner, { observation: 'active', activity: 'working' })
    tabs.applyObservation(tab.tabId, owner, { observation: 'active', activity: 'unknown' })
    tabs.applyLaunchStatus(tab.tabId, receipt(tab))
    expect(tabs.tab(tab.tabId)).toMatchObject({ activityState: 'unknown' })
    tabs.applyObservation(tab.tabId, { ...owner, requestId: 'foreign' }, { observation: 'active', activity: 'working' })
    expect(tabs.tab(tab.tabId)).toMatchObject({ activityState: 'unknown' })
    tabs.applyObservation(tab.tabId, owner, { observation: 'unavailable', activity: 'working' })
    expect(tabs.tab(tab.tabId)).toMatchObject({ activityState: 'unknown', observationState: 'unavailable' })
    tabs.applyLaunchStatus(tab.tabId, receipt(tab, 'exited'))
    const next = tabs.restart(tab.tabId, { profileId: 'profile', profileRevision: '1' })
    tabs.applyLaunchStatus(tab.tabId, receipt(next))
    tabs.applyObservation(tab.tabId, owner, { observation: 'active', activity: 'working' })
    expect(tabs.tab(tab.tabId)).toMatchObject({ activityState: 'unknown', observationState: 'off' })
  })
  it('Native_CurrentClaudeHooksHaveNoCompletionProof_007', () => {
    const reducer = createObservationReducer({ runId: 'run', generation: 1 })
    const details: HookEventDetail[] = [{ type: 'stop', data: {} }, { type: 'notification', data: { notificationType: 'idle_prompt' } }]
    for (const detail of details) {
      const event = fromClaudeHook({ ptyId: null, sessionId: null, eventName: 'Hook', timestamp: 1, state: 'idle',
        runId: 'run', generation: 1, eventId: detail.type, observerSource: 'claude-hook', detail })!
      reducer.accept(event)
      expect(reducer.state().activity).toBe('unknown')
    }
  })
  it('Project_SeparateCauseCountsExcludeArchives_008', () => {
    const catalog = useUnifiedSessionsStore()
    catalog.sessions = [row({ id: 'error', attentionKind: 'error', attentionState: 'needs-user' }),
      row({ id: 'permission', attentionKind: 'permission', attentionState: 'needs-user' }),
      row({ id: 'done', attentionKind: 'completed', attentionState: 'needs-user' }),
      row({ id: 'work', activityState: 'working' }), row({ id: 'idle', activityState: 'idle' }),
      row({ id: 'archived', attentionKind: 'error', attentionState: 'needs-user', archived: true })]
    expect(catalog.projectGroups[0]).toMatchObject({ runningCount: 5, workingCount: 1,
      errorCount: 1, permissionCount: 1, completedCount: 1, needsUserCount: 3 })
  })
})
