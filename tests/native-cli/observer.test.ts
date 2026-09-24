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
