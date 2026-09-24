import { describe, expect, it } from 'vitest'
import { createRunLifecycle } from '@/terminal/runLifecycle'

describe('D15 run lifecycle', () => {
  it('D15_Frontend_ExitBeforeFinalWriteCallbackStaysDraining_001', () => {
    const lifecycle = createRunLifecycle({ runId: 'run-a', generation: 1, cli: 'codex' })
    lifecycle.processRunning()
    lifecycle.outputStarted('7')
    lifecycle.sentThrough('8')
    lifecycle.processExited()
    lifecycle.outputEnd('8')

    expect(lifecycle.snapshot()).toMatchObject({ process: 'exited', output: 'draining' })

    lifecycle.parsedThrough('4')
    expect(lifecycle.snapshot().output).toBe('draining')

    lifecycle.parsedThrough('8')
    expect(lifecycle.snapshot()).toMatchObject({
      process: 'exited',
      output: 'drained',
      finalOffset: '8',
      parsedThrough: '8',
    })
  })

  it('D15_Frontend_OldGenerationAndEpochCannotFinishCurrentRun_002', () => {
    const lifecycle = createRunLifecycle({ runId: 'run-a', generation: 2, cli: 'claude' })
    lifecycle.processRunning()
    lifecycle.outputStarted('11')
    lifecycle.sentThrough('5')

    expect(lifecycle.accept({
      type: 'output-end',
      runId: 'run-a',
      generation: 1,
      streamEpoch: '11',
      finalOffset: '5',
    })).toBe(false)
    expect(lifecycle.accept({
      type: 'output-end',
      runId: 'run-a',
      generation: 2,
      streamEpoch: '10',
      finalOffset: '5',
    })).toBe(false)
    expect(lifecycle.snapshot().output).toBe('open')
  })

  it('D15_Frontend_DegradedOrIncompleteIsFinalNotDrained_003', () => {
    const degraded = createRunLifecycle({ runId: 'a', generation: 1, cli: 'codex' })
    degraded.processRunning()
    degraded.outputStarted('1')
    degraded.degraded()
    degraded.processExited()
    expect(degraded.snapshot().output).toBe('degraded')

    const incomplete = createRunLifecycle({ runId: 'b', generation: 1, cli: 'claude' })
    incomplete.processRunning()
    incomplete.outputStarted('2')
    incomplete.incomplete()
    incomplete.processExited()
    expect(incomplete.snapshot().output).toBe('incomplete')
  })
})
