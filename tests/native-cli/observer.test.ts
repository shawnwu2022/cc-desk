import { describe, expect, it, vi } from 'vitest'
import {
  applyObservation,
  createObservationReducer as productionReducer,
  createObservationRegistry,
  type ObservationEvent,
  type RunRef,
} from '@/integrations/registry'
import { fromClaudeHook } from '@/integrations/claudeObserver'

// Test-only forbidden-port adapter. Production never receives a stop handle.
function createObservationReducer(run: RunRef, _forbiddenStopSpy?: () => unknown) {
  return productionReducer(run)
}

const current: RunRef = { runId: 'current', generation: 2 }

describe('D13 observer isolation', () => {
  it('D13_Observer_TimeoutNeverKillsRun_01', () => {
    const stop = vi.fn()
    const reducer = createObservationReducer(current, stop)
    reducer.accept({ kind: 'timeout', runId: 'current', generation: 2 })
    reducer.accept({ kind: 'working', runId: 'old', generation: 1 })
    expect(reducer.state()).toEqual({ observation: 'unavailable', activity: 'unknown' })
    expect(stop).not.toHaveBeenCalled()
  })

  it('D13_Observer_UnorderedActivityNeverBecomesFalsePrecision_02', () => {
    const reducer = createObservationReducer(current)
    reducer.accept({
      kind: 'working',
      runId: 'current',
      generation: 2,
      eventId: 'parallel-a',
    })
    reducer.accept({
      kind: 'waiting',
      runId: 'current',
      generation: 2,
      eventId: 'parallel-b',
    })
    expect(reducer.state()).toEqual({ observation: 'active', activity: 'unknown' })
  })

  it('D13_Observer_OrderedActivityRejectsDuplicateAndStaleSequence_03', () => {
    const reducer = createObservationReducer(current)
    reducer.accept({
      kind: 'working',
      runId: 'current',
      generation: 2,
      eventId: 'event-10',
      sourceSequence: '10',
    })
    expect(reducer.state()).toEqual({ observation: 'active', activity: 'working' })

    reducer.accept({
      kind: 'waiting',
      runId: 'current',
      generation: 2,
      eventId: 'event-10',
      sourceSequence: '11',
    })
    expect(reducer.state()).toEqual({ observation: 'active', activity: 'working' })

    reducer.accept({
      kind: 'waiting',
      runId: 'current',
      generation: 2,
      eventId: 'event-9',
      sourceSequence: '9',
    })
    expect(reducer.state()).toEqual({ observation: 'active', activity: 'unknown' })
  })

  it('D13_Observer_RegistryKeepsRunsAndGenerationsIndependent_04', () => {
    const registry = createObservationRegistry()
    const first = registry.attach({ runId: 'same', generation: 1 })
    const second = registry.attach({ runId: 'same', generation: 2 })

    first.accept({
      kind: 'working',
      runId: 'same',
      generation: 1,
      eventId: 'first',
      sourceSequence: '1',
    })
    second.accept({
      kind: 'waiting',
      runId: 'same',
      generation: 2,
      eventId: 'second',
      sourceSequence: '1',
    })

    expect(first.state().activity).toBe('working')
    expect(second.state().activity).toBe('waiting')
  })

  it('D13_Observer_ClaudeAdapterRequiresAuthenticatedRunMetadata_05', () => {
    const event = fromClaudeHook({
      ptyId: null,
      sessionId: 'native-session',
      eventName: 'UserPromptSubmit',
      state: 'thinking',
      timestamp: 1,
      runId: 'current',
      generation: 2,
      eventId: 'event-1',
      observerSource: 'claude-hook',
      detail: { type: 'userPromptSubmit', data: { prompt: 'secret prompt' } },
    })
    expect(event).toEqual({
      kind: 'working',
      runId: 'current',
      generation: 2,
      eventId: 'event-1',
    })

    const forged = fromClaudeHook({
      ptyId: 'legacy-pty',
      sessionId: 'native-session',
      eventName: 'UserPromptSubmit',
      state: 'thinking',
      timestamp: 1,
      detail: { type: 'userPromptSubmit', data: { prompt: 'secret prompt' } },
    })
    expect(forged).toBeNull()
  })

  it('D13_Observer_UnknownAndParallelClaudeEventsStayUnknown_06', () => {
    const events: ObservationEvent[] = [
      {
        kind: 'unknown',
        runId: 'current',
        generation: 2,
        eventId: 'unknown-1',
      },
      {
        kind: 'waiting',
        runId: 'current',
        generation: 2,
        eventId: 'waiting-1',
      },
    ]
    const reducer = createObservationReducer(current)
    for (const event of events) reducer.accept(event)
    expect(reducer.state()).toEqual({ observation: 'active', activity: 'unknown' })
  })

  it('D13_Observer_ApplyObservationIsRunScoped_07', () => {
    expect(
      applyObservation(current, {
        kind: 'timeout',
        runId: 'current',
        generation: 2,
      }),
    ).toEqual({ observation: 'unavailable', activity: 'unknown' })

    expect(
      applyObservation(current, {
        kind: 'working',
        runId: 'foreign',
        generation: 2,
        eventId: 'foreign-1',
        sourceSequence: '1',
      }),
    ).toEqual({ observation: 'off', activity: 'unknown' })

    expect(
      applyObservation(current, {
        kind: 'working',
        runId: 'current',
        generation: 2,
        eventId: 'current-1',
        sourceSequence: '1',
      }),
    ).toEqual({ observation: 'active', activity: 'working' })
  })

})

it('D13_Observer_ExplicitOffRejectsLateEvents_08', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'off', ...current })
  reducer.accept({ kind: 'working', ...current, eventId: 'late', sourceSequence: '1' })
  expect(reducer.state()).toEqual({ observation: 'off', activity: 'unknown' })
})
it('D13_Observer_DetachRevokesPreviouslyAcquiredReducer_09', () => {
  const registry = createObservationRegistry()
  const reducer = registry.attach(current)
  registry.detach(current)
  reducer.accept({ kind: 'working', ...current, eventId: 'late', sourceSequence: '1' })
  expect(reducer.state()).toEqual({ observation: 'off', activity: 'unknown' })
})
it('D13_Observer_MalformedWireCannotThrowThroughSubscribers_10', () => {
  const base = { runId: 'current', generation: 2, eventId: 'e', observerSource: 'claude-hook', detail: null }
  for (const bad of [null, [], base, { ...base, detail: { type: 'notification', data: null } }, { ...base, generation: 4294967296 }]) {
    expect(() => fromClaudeHook(bad as never)).not.toThrow()
    expect(fromClaudeHook(bad as never)).toBeNull()
  }
})
it('D13_Observer_MissingEventIdCannotInventWorking_11', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'working', ...current, sourceSequence: '1' })
  expect(reducer.state().activity).toBe('unknown')
})
it('D13_Observer_ReplayBudgetFailsClosed_12', () => {
  const reducer = createObservationReducer(current)
  for (let i = 0; i < 1025; i++) reducer.accept({ kind: 'working', ...current, eventId: `e${i}`, sourceSequence: String(i) })
  expect(reducer.state()).toEqual({ observation: 'unavailable', activity: 'unknown' })
})
it('D13_Observer_ReattachingRunPreservesDedupeAndState_13', () => {
  const registry = createObservationRegistry()
  const first = registry.attach(current)
  first.accept({ kind: 'working', ...current, eventId: 'a', sourceSequence: '1' })
  expect(registry.attach(current)).toBe(first)
  expect(registry.get(current)?.state().activity).toBe('working')
})
it('D13_Observer_RunMutationCannotRetargetExistingBinding_14', () => {
  const run = { ...current }
  const registry = createObservationRegistry()
  const first = registry.attach(run)
  run.runId = 'foreign'
  first.accept({ kind: 'working', ...run, eventId: 'a', sourceSequence: '1' })
  expect(first.state().activity).toBe('unknown')
})

// 真实鉴权事件保留 agent_id，首轮子代理活动不依赖全局到达顺序。
it('Subagent_FirstStart_001', () => {
  const reducer = createObservationReducer(current)
  const event = fromClaudeHook({ ptyId: null, sessionId: 'sid', eventName: 'SubagentStart',
    state: 'unknown', timestamp: 1, ...current, eventId: 'start-a', observerSource: 'claude-hook',
    detail: { type: 'subagentStart', data: { agentId: 'agent-a' } } })!
  reducer.accept(event)
  expect(reducer.state()).toEqual({ observation: 'active', activity: 'subagent_running' })
  reducer.accept({ kind: 'working', ...current, eventId: 'unordered-tool' })
  expect(reducer.state().activity).toBe('subagent_running')
})

// 同一 agent_id 的 Stop 先到时，迟到 Start 不恢复首轮工作提示。
it('Subagent_StopBeforeStart_002', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'subagent-stopped', ...current, eventId: 'stop-a', agentId: 'agent-a' } as ObservationEvent)
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'late-start-a', agentId: 'agent-a' } as ObservationEvent)
  expect(reducer.state()).toEqual({ observation: 'active', activity: 'unknown' })
})

// 两个子代理交错停止时，仅停止最后一个后撤销工作提示。
it('Subagent_InterleavedAgents_003', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'start-a', agentId: 'agent-a' } as ObservationEvent)
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'start-b', agentId: 'agent-b' } as ObservationEvent)
  reducer.accept({ kind: 'subagent-stopped', ...current, eventId: 'stop-b', agentId: 'agent-b' } as ObservationEvent)
  expect(reducer.state().activity).toBe('subagent_running')
  reducer.accept({ kind: 'subagent-stopped', ...current, eventId: 'stop-a', agentId: 'agent-a' } as ObservationEvent)
  expect(reducer.state().activity).toBe('unknown')
})

// 重复 Start/Stop 不制造额外活跃实例，停止后复用身份保持未知。
it('Subagent_DuplicateAndReuse_004', () => {
  const reducer = createObservationReducer(current)
  const start = { kind: 'subagent-started', ...current, eventId: 'start-a', agentId: 'agent-a' } as ObservationEvent
  reducer.accept(start); reducer.accept(start)
  reducer.accept({ ...start, eventId: 'duplicate-start-a' })
  expect(reducer.state().activity).toBe('subagent_running')
  const stop = { kind: 'subagent-stopped', ...current, eventId: 'stop-a', agentId: 'agent-a' } as ObservationEvent
  reducer.accept(stop); reducer.accept(stop)
  reducer.accept({ ...stop, eventId: 'duplicate-stop-a' })
  reducer.accept({ ...start, eventId: 'resume-same-agent-a' })
  expect(reducer.state().activity).toBe('unknown')
})

// 重启后的精确 generation 可重新使用 agent_id，旧 generation 不影响新运行。
it('Subagent_RestartIdentity_005', () => {
  const reducer = createObservationReducer({ runId: 'current', generation: 3 })
  reducer.accept({ kind: 'subagent-stopped', ...current, eventId: 'old-stop', agentId: 'agent-a' } as ObservationEvent)
  reducer.accept({ kind: 'subagent-started', runId: 'current', generation: 3,
    eventId: 'new-start', agentId: 'agent-a' } as ObservationEvent)
  expect(reducer.state().activity).toBe('subagent_running')
  reducer.accept({ kind: 'subagent-stopped', runId: 'foreign', generation: 3,
    eventId: 'foreign-stop', agentId: 'agent-a' } as ObservationEvent)
  expect(reducer.state().activity).toBe('subagent_running')
})

// 无序权限通知不能被随后到达的子代理或工具事件覆盖为工作中。
it('Subagent_PermissionBarrier_006', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'start-a', agentId: 'agent-a' } as ObservationEvent)
  expect(reducer.state().activity).toBe('subagent_running')
  reducer.accept({ kind: 'waiting', ...current, eventId: 'permission' })
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'start-b', agentId: 'agent-b' } as ObservationEvent)
  reducer.accept({ kind: 'working', ...current, eventId: 'unordered-tool' })
  expect(reducer.state().activity).toBe('unknown')
})

// 已验证的有序 waiting 状态在子代理生命周期事件期间保留。
it('Subagent_OrderedWaiting_007', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'waiting', ...current, eventId: 'permission', sourceSequence: '1' })
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'start-a', agentId: 'agent-a' } as ObservationEvent)
  expect(reducer.state().activity).toBe('waiting')
})

// 缺失身份的子代理信号不能撤销已有的有序 waiting 证明。
it('Subagent_MissingIdWaiting_015', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'waiting', ...current, eventId: 'permission', sourceSequence: '1' })
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'missing-id' } as ObservationEvent)
  expect(reducer.state().activity).toBe('waiting')
})

// 子代理事件携带源顺序时仍必须拒绝旧序号，不绕过已有有序门禁。
it('Subagent_RejectStaleOrder_016', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'unknown', ...current, eventId: 'latest', sourceSequence: '10' })
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'old-start',
    agentId: 'agent-a', sourceSequence: '9' } as ObservationEvent)
  expect(reducer.state().activity).toBe('unknown')
})

// 一个子代理的旧 Stop 序号使整体证据不可靠，不能保留另一个工作提示。
it('Subagent_RejectStaleStop_017', () => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'start-a',
    agentId: 'agent-a', sourceSequence: '10' } as ObservationEvent)
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'start-b',
    agentId: 'agent-b', sourceSequence: '11' } as ObservationEvent)
  expect(reducer.state().activity).toBe('subagent_running')
  reducer.accept({ kind: 'subagent-stopped', ...current, eventId: 'old-stop',
    agentId: 'agent-a', sourceSequence: '9' } as ObservationEvent)
  expect(reducer.state().activity).toBe('unknown')
})

// off/不可用/重新连接撤销旧活跃身份，迟到 Start 不复活旧工作提示。
it.each(['off', 'timeout', 'connecting'] as const)('Subagent_Invalidate_008 %s', kind => {
  const reducer = createObservationReducer(current)
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'start-a', agentId: 'agent-a' } as ObservationEvent)
  expect(reducer.state().activity).toBe('subagent_running')
  reducer.accept({ kind, ...current })
  reducer.accept({ kind: 'subagent-started', ...current, eventId: 'late-start-a', agentId: 'agent-a' } as ObservationEvent)
  expect(reducer.state().activity).toBe('unknown')
})

// 缺失或越界的 agent_id 不支持子代理活动结论。
it.each([undefined, '', 'x'.repeat(129), 'agent\nsecret'])('Subagent_RejectIdentity_009 %s', agentId => {
  const reducer = createObservationReducer(current)
  const event = fromClaudeHook({ ptyId: null, sessionId: 'sid', eventName: 'SubagentStart',
    state: 'unknown', timestamp: 1, ...current, eventId: 'start', observerSource: 'claude-hook',
    detail: { type: 'subagentStart', data: { agentId } } })!
  reducer.accept(event)
  expect(reducer.state().activity).toBe('unknown')
})
