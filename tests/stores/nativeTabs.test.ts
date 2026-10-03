import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import {
  captureNativeAttempt,
  matchesNativeAttempt,
  useNativeTabsStore,
} from '@/stores/nativeTabs'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'

function status(tab: any, phase: LaunchStatus['phase'] = 'running'): LaunchStatus {
  return {
    instanceId: 'backend-tabs',
    requestId: tab.requestId,
    run: { runId: tab.runId, generation: tab.generation },
    revision: '2',
    phase,
    failure: phase === 'indeterminate'
      ? 'outcome-unknown'
      : phase === 'failed'
        ? 'process-start-failed'
        : null,
  }
}

beforeEach(() => setActivePinia(createPinia()))
afterEach(() => vi.restoreAllMocks())

describe('D22 native dual-CLI tab store', () => {
  it('D22_Tabs_SameProjectClaudeAndCodexRemainIndependent_08', () => {
    const store = useNativeTabsStore()
    const claude = store.create({
      cli: 'claude', projectId: 'p1', projectPath: '/repo',
      profileId: 'claude-main', profileRevision: '3', action: { kind: 'new' },
    })
    const codex = store.create({
      cli: 'codex', projectId: 'p1', projectPath: '/repo',
      profileId: 'codex-main', profileRevision: '7', action: { kind: 'new' },
    })

    expect(claude.tabId).not.toBe(codex.tabId)
    expect(claude.runId).not.toBe(codex.runId)
    expect(store.byProject('/repo').map(t => t.cli)).toEqual(['claude', 'codex'])
  })

  it('D22_Tabs_LaunchStatusOnlyMutatesExactRunGeneration_09', () => {
    const store = useNativeTabsStore()
    const tab = store.create({
      cli: 'codex', projectId: 'p1', projectPath: '/repo',
      profileId: 'codex-main', profileRevision: '7', action: { kind: 'new' },
    })

    expect(store.applyLaunchStatus(tab.tabId, status(tab))).toBe(true)
    expect(store.tab(tab.tabId)?.status).toBe('running')
    expect(store.applyLaunchStatus(tab.tabId, {
      ...status(tab),
      run: { runId: tab.runId, generation: tab.generation + 1 },
    })).toBe(false)
    expect(store.tab(tab.tabId)?.status).toBe('running')
  })

  it('D22_Tabs_UnknownOutcomeCannotRestartOrAllocateReplacementRun_10', () => {
    const store = useNativeTabsStore()
    const tab = store.create({
      cli: 'claude', projectId: 'p1', projectPath: '/repo',
      profileId: 'claude-main', profileRevision: '1', action: { kind: 'new' },
    })
    store.markUnknown(tab.tabId)

    const before = store.tab(tab.tabId)!
    expect(() => store.restart(tab.tabId, {
      profileId: 'claude-main', profileRevision: '2',
    })).toThrowError('LAUNCH_STATE_UNKNOWN')
    expect(store.tab(tab.tabId)?.runId).toBe(before.runId)
    expect(store.tab(tab.tabId)?.generation).toBe(before.generation)
  })

  it('D22_Tabs_ExplicitRestartAllocatesFreshRunAndUsesNextProfileRevision_11', () => {
    const store = useNativeTabsStore()
    const created = store.create({
      cli: 'codex', projectId: 'p1', projectPath: '/repo',
      profileId: 'codex-main', profileRevision: '1', action: { kind: 'new' },
    })
    store.applyLaunchStatus(created.tabId, status(created, 'exited'))

    const restarted = store.restart(created.tabId, {
      profileId: 'codex-main', profileRevision: '9',
    })
    expect(restarted.generation).toBe(2)
    expect(restarted.runId).not.toBe(created.runId)
    expect(restarted.requestId).not.toBe(created.requestId)
    expect(restarted.profileRevision).toBe('9')
    expect(restarted.status).toBe('stopped')
  })

  it('D22_Tabs_RestartRejectsCrossCliProfile_12', () => {
    const store = useNativeTabsStore()
    const tab = store.create({
      cli: 'claude', projectId: 'p1', projectPath: '/repo',
      profileId: 'claude-main', profileRevision: '1', action: { kind: 'new' },
    })
    store.applyLaunchStatus(tab.tabId, status(tab, 'exited'))

    expect(() => store.restart(tab.tabId, {
      profileId: 'codex-main', profileRevision: '2', cli: 'codex',
    })).toThrowError('PROFILE_CLI_MISMATCH')
  })

  it('D22_Tabs_CloseDoesNotAffectSiblingCliTab_13', () => {
    const store = useNativeTabsStore()
    const claude = store.create({
      cli: 'claude', projectId: 'p1', projectPath: '/repo',
      profileId: 'claude-main', profileRevision: '1', action: { kind: 'new' },
    })
    const codex = store.create({
      cli: 'codex', projectId: 'p1', projectPath: '/repo',
      profileId: 'codex-main', profileRevision: '1', action: { kind: 'new' },
    })
    store.close(claude.tabId)
    expect(store.tab(claude.tabId)).toBeUndefined()
    expect(store.tab(codex.tabId)?.cli).toBe('codex')
  })

  it('D24_Tabs_TerminalDegradedDiagnosticDoesNotFakeProcessFailure_14', () => {
    const store = useNativeTabsStore()
    const tab = store.create({
      cli: 'codex', projectId: 'p1', projectPath: '/repo',
      profileId: 'codex-main', profileRevision: '1', action: { kind: 'new' },
    })
    store.applyLaunchStatus(tab.tabId, status(tab, 'running'))

    store.setDiagnostic(tab.tabId, 'NATIVE_OUTPUT_DEGRADED')

    expect(store.tab(tab.tabId)?.status).toBe('running')
    expect(store.tab(tab.tabId)?.errorCode).toBe('NATIVE_OUTPUT_DEGRADED')
  })

  it('D28_Tabs_AsyncCompletionCannotCrossExplicitRestart_15', () => {
    const store = useNativeTabsStore()
    const created = store.create({
      cli: 'codex', projectId: 'p1', projectPath: '/repo',
      profileId: 'codex-main', profileRevision: '1', action: { kind: 'new' },
    })
    const oldAttempt = captureNativeAttempt(created)
    store.applyLaunchStatus(created.tabId, status(created, 'exited'))
    const restarted = store.restart(created.tabId, {
      profileId: 'codex-main', profileRevision: '2',
    })

    expect(matchesNativeAttempt(store.tab(created.tabId), oldAttempt)).toBe(false)
    expect(matchesNativeAttempt(store.tab(created.tabId), captureNativeAttempt(restarted))).toBe(true)
    expect(store.applyLaunchStatus(created.tabId, status(created, 'running'))).toBe(false)
    expect(store.tab(created.tabId)?.status).toBe('stopped')
  })
  // 只有 create/restart 的新 attempt 有未开始证明，失败不能撤销既有准备证据。
  it('Tabs_ResourceAttemptProof_016', () => {
    const store = useNativeTabsStore()
    const created = store.create({ cli: 'codex', projectId: 'p1', projectPath: '/repo', profileId: 'config', profileRevision: '1', action: { kind: 'new' } })
    expect(store.hasUnstartedAttempt(created.tabId)).toBe(true)
    store.markStarting(created.tabId); store.markError(created.tabId, 'INVALID_LAUNCH_RESPONSE')
    expect(store.hasUnstartedAttempt(created.tabId)).toBe(false)
    expect(store.tab(created.tabId)).toMatchObject({ status: 'failed', launchRevision: null })
    const restarted = store.restart(created.tabId, { profileId: 'config', profileRevision: '1' })
    expect(store.hasUnstartedAttempt(restarted.tabId)).toBe(true)
    expect(restarted).toMatchObject({ generation: 2 })
    expect(restarted.requestId).not.toBe(created.requestId)
  })
  // 未分类错误同样撤销正向未开始证明，不从 failed/null 推断没有提交。
  it('Tabs_ErrorRevokesAttemptProof_017', () => {
    const store = useNativeTabsStore()
    const tab = store.create({ cli: 'codex', projectId: 'p1', projectPath: '/repo', profileId: 'config', profileRevision: '1', action: { kind: 'new' } })
    store.markError(tab.tabId, 'INVALID_LAUNCH_RESPONSE')
    expect(store.hasUnstartedAttempt(tab.tabId)).toBe(false)
  })
  // 状态真正变化与新的诊断需要更新；相同诊断与旧代次必须保持时间。
  it('Activity_ExactOwnerAndState_018', () => {
    const now = vi.spyOn(Date, 'now').mockReturnValue(1000)
    const store = useNativeTabsStore()
    const tab = store.create({ cli: 'codex', projectId: 'p1', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    store.applyLaunchStatus(tab.tabId, status(tab))
    now.mockReturnValue(2000); store.setDiagnostic(tab.tabId, 'NATIVE_INPUT_PAUSED')
    expect(store.tab(tab.tabId)!.lastActivityAt).toBe(2000)
    now.mockReturnValue(3000); store.setDiagnostic(tab.tabId, 'NATIVE_INPUT_PAUSED')
    expect(store.tab(tab.tabId)!.lastActivityAt).toBe(2000)
    now.mockReturnValue(4000); store.applyLaunchStatus(tab.tabId, status(tab, 'exited'))
    expect(store.tab(tab.tabId)!.lastActivityAt).toBe(4000)
    const newer = store.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
    now.mockReturnValue(8000); store.touch(tab.tabId, captureNativeAttempt(tab))
    expect(store.tab(tab.tabId)!.lastActivityAt).toBe(4000)
    store.touch(tab.tabId, captureNativeAttempt(newer))
    expect(store.tab(tab.tabId)!.lastActivityAt).toBe(8000)
  })

  // 状态不明不得保留之前 observer 投影的明确回复状态。
  it('Activity_UnknownClearsAttention_019', () => {
    const store = useNativeTabsStore()
    const tab = store.create({ cli: 'claude', projectId: 'p1', projectPath: '/repo', profileId: 'cc', profileRevision: '7', action: { kind: 'new' } })
    store.applyLaunchStatus(tab.tabId, status(tab))
    store.applyObservation(tab.tabId, captureNativeAttempt(tab), { observation: 'active', activity: 'waiting' })
    expect(store.tab(tab.tabId)!.attentionState).toBe('needs-user')
    store.markUnknown(tab.tabId)
    expect(store.tab(tab.tabId)!.attentionState).toBe('none')
  })

  // 进程不明、失败、结束或新代次必须丢弃等待中的注意力投影。
  it.each(['unknown', 'failed', 'exited', 'restart'] as const)('Attention_PendingInvalidation_020 %s', invalidation => {
    const store = useNativeTabsStore()
    const tab = store.create({ cli: 'claude', projectId: 'p1', projectPath: '/repo', profileId: 'cc', profileRevision: '7', action: { kind: 'new' } })
    store.markStarting(tab.tabId)
    store.applyObservation(tab.tabId, captureNativeAttempt(tab), { observation: 'active', activity: 'waiting' })
    if (invalidation === 'unknown') store.markUnknown(tab.tabId)
    else if (invalidation === 'failed') store.markError(tab.tabId, 'LAUNCH_FAILED')
    else store.applyLaunchStatus(tab.tabId, status(tab, 'exited'))
    const current = invalidation === 'restart' ? store.restart(tab.tabId, { profileId: 'cc', profileRevision: '7' }) : tab
    store.applyLaunchStatus(tab.tabId, status(current))
    expect(store.tab(tab.tabId)!.attentionState).toBe('none')
  })

})
