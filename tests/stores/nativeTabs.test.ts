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

it('CodexAdapter_ObserverTimeoutNeverInventsTuiActivity_110', () => {
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'p', projectPath: '/repo', profileId: 'cx', profileRevision: '1', action: { kind: 'new' } })
  tabs.applyLaunchStatus(tab.tabId, status(tab))
  const owner = captureNativeAttempt(tab)
  tabs.applyCodexProgramStatus(tab.tabId, owner, 'idle')
  tabs.applyObservation(tab.tabId, owner, { observation: 'active', activity: 'waiting_permission' })
  expect(tabs.tab(tab.tabId)).toMatchObject({ codexProgramStatus: 'idle', activityState: 'unknown', attentionState: 'none', observationState: 'active' })
  tabs.applyObservation(tab.tabId, owner, { observation: 'unavailable', activity: 'working' })
  expect(tabs.tab(tab.tabId)).toMatchObject({ codexProgramStatus: 'idle', activityState: 'unknown', attentionState: 'none', observationState: 'unavailable' })
  tabs.applyObservation(tab.tabId, { ...owner, generation: owner.generation + 1 }, { observation: 'active', activity: 'working' })
  expect(tabs.tab(tab.tabId)?.observationState).toBe('unavailable')
})

it.each(['new','resume-id'] as const)('Opencode_StatusAndIdentityKeepExactAttempt_030 %s',kind=>{
  const tabs=useNativeTabsStore(),tab=tabs.create({cli:'opencode',projectId:'p',projectPath:'/repo',profileId:'oc',profileRevision:'1',action:kind==='new'?{kind}:{kind,nativeSessionId:'ses_main'}})
  const owner=captureNativeAttempt(tab)
  tabs.markStarting(tab.tabId)
  expect(tabs.applyObservationIdentity(tab.tabId,owner,'ses_main','notification')).toBe(false)
  expect(tabs.applyObservationIdentity(tab.tabId,owner,'ses_main','sessionStart')).toBe(true)
  tabs.applyObservation(tab.tabId,owner,{observation:'active',activity:'waiting_permission'})
  tabs.applyLaunchStatus(tab.tabId,status(tab))
  expect(tabs.tab(tab.tabId)).toMatchObject({status:'running',activityState:'waiting_permission',attentionState:'needs-user'})
  tabs.applyObservation(tab.tabId,owner,{observation:'active',activity:'idle'})
  expect(tabs.tab(tab.tabId)).toMatchObject({activityState:'idle',attentionState:'none'})
  if(kind==='resume-id')expect(tabs.applyObservationIdentity(tab.tabId,owner,'ses_foreign','sessionStart')).toBe(false)
  tabs.applyLaunchStatus(tab.tabId,status(tab,'exited'))
  tabs.restart(tab.tabId,{profileId:'oc',profileRevision:'1',cli:'opencode'})
  tabs.applyObservation(tab.tabId,owner,{observation:'active',activity:'working'})
  expect(tabs.applyObservationIdentity(tab.tabId,owner,'ses_main','sessionStart')).toBe(false)
  expect(tabs.tab(tab.tabId)).toMatchObject({activityState:'unknown',observationState:'off'})
  tabs.close(tab.tabId)
  tabs.applyObservation(tab.tabId,owner,{observation:'active',activity:'error'})
  expect(tabs.tab(tab.tabId)).toBeUndefined()
})
afterEach(() => vi.restoreAllMocks())

// 丢失 SessionStart 时只允许主线程提示建立初始身份，普通工具/结束事件不建立或切换身份。
it('ClaudeIdentity_OnlyExplicitStartChangesObservedSession_026', () => {
  const tabs = useNativeTabsStore()
  const tab = tabs.create({ cli: 'claude', projectId: 'p', projectPath: '/repo',
    profileId: 'cc', profileRevision: '1', action: { kind: 'new' } })
  tabs.applyLaunchStatus(tab.tabId, status(tab))
  const attempt = captureNativeAttempt(tab)
  expect(tabs.applyObservationIdentity(tab.tabId, attempt, 'first', 'stop')).toBe(false)
  expect(tabs.applyObservationIdentity(tab.tabId, attempt, 'first', 'postToolUse')).toBe(false)
  expect(tabs.applyObservationIdentity(tab.tabId, attempt, 'first', 'userPromptSubmit')).toBe(true)
  expect(tabs.applyObservationIdentity(tab.tabId, attempt, 'second', 'userPromptSubmit')).toBe(false)
  expect(tabs.applyObservationIdentity(tab.tabId, attempt, 'second', 'sessionStart')).toBe(true)
  expect(tabs.applyObservationIdentity(tab.tabId, attempt, 'first', 'stop')).toBe(false)
  expect(tabs.tab(tab.tabId)?.observedSessionId).toBe('second')
  expect(tabs.applyObservationIdentity(tab.tabId, attempt, 'second', 'stop')).toBe(true)
})

describe('D22 native dual-CLI tab store', () => {
  // 终端视觉提示按完整 attempt 绑定，启动未确认时不改变进程状态，失效边界丢弃提示。
  it('Codex_ProgramStatusLifetime_023', () => {
    const tabs = useNativeTabsStore()
    const tab = tabs.create({ cli: 'codex', projectId: 'p', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    const owner = captureNativeAttempt(tab)
    tabs.applyCodexProgramStatus(tab.tabId, owner, 'working')
    expect(tabs.tab(tab.tabId)?.codexProgramStatus).toBeUndefined()
    tabs.markStarting(tab.tabId)
    tabs.applyCodexProgramStatus(tab.tabId, owner, 'blocked')
    expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'starting', codexProgramStatus: 'blocked', attentionState: 'none' })
    tabs.applyLaunchStatus(tab.tabId, status(tab))
    for (const foreign of [{ ...owner, requestId: 'other' }, { ...owner, runId: 'other' }, { ...owner, generation: 2 }]) {
      tabs.applyCodexProgramStatus(tab.tabId, foreign, 'idle')
      expect(tabs.tab(tab.tabId)?.codexProgramStatus).toBe('blocked')
    }
    tabs.markUnknown(tab.tabId)
    tabs.applyCodexProgramStatus(tab.tabId, owner, 'working')
    expect(tabs.tab(tab.tabId)?.codexProgramStatus).toBeUndefined()
    tabs.applyLaunchStatus(tab.tabId, status(tab))
    tabs.applyCodexProgramStatus(tab.tabId, owner, 'working')
    tabs.applyLaunchStatus(tab.tabId, status(tab, 'exited'))
    expect(tabs.tab(tab.tabId)?.codexProgramStatus).toBeUndefined()
    const next = tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
    tabs.markStarting(next.tabId)
    tabs.applyCodexProgramStatus(next.tabId, owner, 'blocked')
    expect(tabs.tab(next.tabId)?.codexProgramStatus).toBeUndefined()
    tabs.applyCodexProgramStatus(next.tabId, captureNativeAttempt(next), 'idle')
    tabs.markError(next.tabId, 'NATIVE_LAUNCH_FAILED')
    expect(tabs.tab(next.tabId)?.codexProgramStatus).toBeUndefined()
    tabs.close(next.tabId)
    tabs.applyCodexProgramStatus(next.tabId, captureNativeAttempt(next), 'working')
    expect(tabs.tab(next.tabId)).toBeUndefined()
  })
  it('Observer_InstallationReceiptCannotOverwriteActivityOrNewAttempt_022', () => {
    const store=useNativeTabsStore(),tab=store.create({cli:'claude',projectId:'p',projectPath:'/repo',profileId:'cc',profileRevision:'7',action:{kind:'new'}})
    const owner=captureNativeAttempt(tab)
    store.applyLaunchStatus(tab.tabId,status(tab))
    const receipt={requestId:owner.requestId,run:{runId:owner.runId,generation:owner.generation},state:'unavailable' as const,reasonCode:'OBSERVER_PLUGIN_UNAVAILABLE' as const}
    store.applyObserverInstallation(tab.tabId,owner,receipt)
    expect(store.tab(tab.tabId)).toMatchObject({observationState:'unavailable',observerReasonCode:'OBSERVER_PLUGIN_UNAVAILABLE'})
    store.applyObservation(tab.tabId,owner,{observation:'active',activity:'working'})
    store.applyObserverInstallation(tab.tabId,owner,receipt)
    expect(store.tab(tab.tabId)).toMatchObject({observationState:'active',activityState:'working',observerReasonCode:undefined})
    store.applyObservation(tab.tabId,owner,{observation:'unavailable',activity:'unknown'})
    store.applyLaunchStatus(tab.tabId,status(tab,'exited'))
    store.restart(tab.tabId,{profileId:'cc',profileRevision:'7'})
    store.applyObserverInstallation(tab.tabId,owner,receipt)
    expect(store.tab(tab.tabId)?.observerReasonCode).toBeUndefined()
  })
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

  // 认证 hook 回填的运行中会话身份：claude 非 raw 专属、随 attempt 精确校验、同 run 幂等。
  it('D22_Tabs_ObservedIdentityBinding_021', () => {
    const store = useNativeTabsStore()
    const claude = store.create({ cli: 'claude', projectId: 'p1', projectPath: '/repo', profileId: 'cc', profileRevision: '7', action: { kind: 'new' } })
    store.applyLaunchStatus(claude.tabId, status(claude))
    expect(store.applyObservationIdentity(claude.tabId, captureNativeAttempt(claude), ' session-9 ')).toBe(true)
    expect(store.tab(claude.tabId)?.observedSessionId).toBe('session-9')
    // 幂等覆盖不刷新活动时间
    const at = store.tab(claude.tabId)!.lastActivityAt
    expect(store.applyObservationIdentity(claude.tabId, captureNativeAttempt(claude), 'session-9')).toBe(true)
    expect(store.tab(claude.tabId)!.lastActivityAt).toBe(at)
    // 旧 attempt 迟到事件不得覆盖当前身份
    expect(store.applyObservationIdentity(claude.tabId, { ...captureNativeAttempt(claude), generation: 99 }, 'stale')).toBe(false)
    expect(store.tab(claude.tabId)?.observedSessionId).toBe('session-9')
    // codex 与 raw 动作没有认证观察源，拒绝绑定
    const codex = store.create({ cli: 'codex', projectId: 'p1', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    expect(store.applyObservationIdentity(codex.tabId, captureNativeAttempt(codex), 'session-x')).toBe(false)
    const raw = store.create({ cli: 'claude', projectId: 'p1', projectPath: '/repo', profileId: 'cc', profileRevision: '7', action: { kind: 'raw', argv: ['sh'] } })
    expect(store.applyObservationIdentity(raw.tabId, captureNativeAttempt(raw), 'session-r')).toBe(false)
    expect(store.applyObservationIdentity(claude.tabId, captureNativeAttempt(claude), '  ')).toBe(false)
  })

})

// Codex 新TUI只能接收本attempt的认证主会话身份；旧线程Stop和关闭后事件不回绑。
it('CodexIdentity_ExactAttemptAndRetirement_031', () => {
  const store = useNativeTabsStore()
  const a = store.create({ cli: 'codex', projectId: 'p', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  const b = store.create({ cli: 'codex', projectId: 'p', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  store.applyLaunchStatus(a.tabId, status(a)); store.applyLaunchStatus(b.tabId, status(b))
  const first = '00000000-0000-4000-8000-000000000031', second = '00000000-0000-4000-8000-000000000032'
  expect(store.applyObservationIdentity(a.tabId, captureNativeAttempt(a), first, 'stop')).toBe(false)
  expect(store.applyObservationIdentity(a.tabId, captureNativeAttempt(a), first)).toBe(true)
  expect(store.applyObservationIdentity(a.tabId, captureNativeAttempt(a), second, 'stop')).toBe(false)
  expect(store.applyObservationIdentity(b.tabId, captureNativeAttempt(a), first)).toBe(false)
  expect(store.tab(b.tabId)?.observedSessionId).toBeUndefined()
  expect(store.applyObservationIdentity(b.tabId, captureNativeAttempt(b), second)).toBe(true)
  store.applyLaunchStatus(a.tabId, status(a, 'exited'))
  expect(store.applyObservationIdentity(a.tabId, captureNativeAttempt(a), second)).toBe(false)
  expect(store.tab(a.tabId)?.observedSessionId).toBe(first)
})
