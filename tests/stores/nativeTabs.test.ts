import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useNativeTabsStore } from '@/stores/nativeTabs'
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
})
