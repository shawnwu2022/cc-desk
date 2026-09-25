import { describe, expect, it } from 'vitest'
import { createInputIntentQueue } from '@/terminal/inputQueue'
import {
  classifyExplicitUserText,
  classifyXtermBinary,
  classifyXtermData,
} from '@/terminal/hostAdapter'

const bytes = (...values: number[]) => Uint8Array.from(values)
const utf8 = (value: string) => new TextEncoder().encode(value)

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('D16 ordered input intent queue', () => {
  it('D16_Input_PasteBarrierPreventsLaterEnterOvertaking_001', async () => {
    const sent: Array<{ seq: string; text: string }> = []
    let modeEpoch = '7'
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch }),
      send: async intent => {
        sent.push({ seq: intent.inputSeq, text: new TextDecoder().decode(intent.bytes) })
      },
    })

    const clipboard = deferred<Uint8Array>()
    const paste = queue.reserveAsync({
      source: 'user-paste',
      modeEpoch: '7',
      produce: () => clipboard.promise,
    })
    const enter = queue.enqueue({
      source: 'user-text',
      modeEpoch: '7',
      bytes: utf8('\r'),
    })

    expect(paste.inputSeq).toBe('1')
    expect(enter.inputSeq).toBe('2')
    await queue.flush()
    expect(sent).toEqual([])

    clipboard.resolve(utf8('payload'))
    await paste.settled
    await queue.flush()
    expect(sent).toEqual([
      { seq: '1', text: 'payload' },
      { seq: '2', text: '\r' },
    ])

    modeEpoch = '8'
  })

  it('D16_Input_ClipboardFailurePausesLaterUserIntentUntilExplicitRecovery_002', async () => {
    const sent: string[] = []
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      send: async intent => sent.push(new TextDecoder().decode(intent.bytes)),
    })

    const clipboard = deferred<Uint8Array>()
    const paste = queue.reserveAsync({
      source: 'user-paste',
      modeEpoch: '1',
      produce: () => clipboard.promise,
    })
    queue.enqueue({ source: 'user-text', modeEpoch: '1', bytes: utf8('\r') })
    clipboard.reject(new Error('clipboard denied'))
    await paste.settled

    await queue.flush()
    expect(sent).toEqual([])
    expect(queue.snapshot()).toMatchObject({
      state: 'paused',
      blockedSeq: '1',
      queued: 2,
    })

    expect(queue.recover('1', 'continue')).toBe(true)
    await queue.flush()
    expect(sent).toEqual(['\r'])
  })

  it('D16_Input_IdenticalPasteBodiesRemainDistinctActions_003', async () => {
    const seqs: string[] = []
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '3' }),
      send: async intent => seqs.push(intent.inputSeq),
    })

    queue.enqueue({ source: 'user-paste', modeEpoch: '3', bytes: utf8('same') })
    queue.enqueue({ source: 'user-paste', modeEpoch: '3', bytes: utf8('same') })
    await queue.flush()

    expect(seqs).toEqual(['1', '2'])
  })

  it('D16_Input_ModeEpochChangePausesInsteadOfRetargeting_004', async () => {
    const sent: string[] = []
    let modeEpoch = '10'
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 4,
      currentTarget: () => ({ runId: 'run-a', generation: 4, modeEpoch }),
      send: async intent => sent.push(intent.inputSeq),
    })

    queue.enqueue({ source: 'user-text', modeEpoch: '10', bytes: utf8('x') })
    modeEpoch = '11'
    await queue.flush()

    expect(sent).toEqual([])
    expect(queue.snapshot()).toMatchObject({
      state: 'paused',
      reason: 'mode-changed',
      blockedSeq: '1',
    })
  })

  it('D16_Input_ProtocolMayCrossPendingClipboardButNeverActiveUserSend_005', async () => {
    const events: string[] = []
    const releaseSend = deferred<void>()
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      send: async intent => {
        events.push(`user:${intent.inputSeq}`)
        if (intent.inputSeq === '1') await releaseSend.promise
      },
      sendProtocol: async event => {
        events.push(`protocol:${Array.from(event.bytes).join(',')}`)
      },
    })

    const pending = deferred<Uint8Array>()
    const paste = queue.reserveAsync({
      source: 'user-paste',
      modeEpoch: '1',
      produce: () => pending.promise,
    })

    await queue.sendProtocol(bytes(27, 91, 48, 110))
    expect(events).toEqual(['protocol:27,91,48,110'])

    pending.resolve(utf8('paste'))
    await paste.settled
    const flushing = queue.flush()
    await Promise.resolve()
    expect(events).toEqual(['protocol:27,91,48,110', 'user:1'])

    const protocolDuringSend = queue.sendProtocol(bytes(27, 91, 49, 110))
    await Promise.resolve()
    expect(events).toEqual(['protocol:27,91,48,110', 'user:1'])

    releaseSend.resolve()
    await flushing
    await protocolDuringSend
    expect(events).toEqual([
      'protocol:27,91,48,110',
      'user:1',
      'protocol:27,91,49,110',
    ])
  })

  it('D16_Input_RunGenerationChangePausesOldQueue_009', async () => {
    const sent: string[] = []
    let generation = 5
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 5,
      currentTarget: () => ({ runId: 'run-a', generation, modeEpoch: '1' }),
      send: async intent => sent.push(intent.inputSeq),
    })

    queue.enqueue({ source: 'user-text', modeEpoch: '1', bytes: utf8('x') })
    generation = 6
    await queue.flush()

    expect(sent).toEqual([])
    expect(queue.snapshot()).toMatchObject({
      state: 'paused',
      reason: 'target-changed',
      blockedSeq: '1',
    })
  })

  it('D16_Input_BudgetsRejectWithoutTruncatingOrReordering_010', async () => {
    const sent: number[] = []
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      limits: {
        actionBytes: 4,
        queuedBytes: 6,
      },
      send: async intent => sent.push(intent.bytes.length),
    })

    queue.enqueue({ source: 'user-paste', modeEpoch: '1', bytes: bytes(1, 2, 3, 4) })
    expect(() => queue.enqueue({
      source: 'user-text',
      modeEpoch: '1',
      bytes: bytes(5, 6, 7),
    })).toThrow('INPUT_QUEUE_BUDGET')

    await queue.flush()
    expect(sent).toEqual([4])
    expect(queue.snapshot().queuedBytes).toBe(0)

    expect(() => queue.enqueue({
      source: 'user-paste',
      modeEpoch: '1',
      bytes: bytes(1, 2, 3, 4, 5),
    })).toThrow('INPUT_ACTION_TOO_LARGE')
  })

  it('D16_Input_RechecksTargetAfterWaitingForDispatchGate_011', async () => {
    const events: string[] = []
    const releaseFirst = deferred<void>()
    let generation = 1
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation, modeEpoch: '1' }),
      send: async intent => {
        events.push(`user:${intent.inputSeq}`)
        if (intent.inputSeq === '1') await releaseFirst.promise
      },
      sendProtocol: async () => {
        events.push('protocol')
      },
    })

    queue.enqueue({ source: 'user-text', modeEpoch: '1', bytes: utf8('a') })
    queue.enqueue({ source: 'user-text', modeEpoch: '1', bytes: utf8('b') })
    const flushing = queue.flush()
    await Promise.resolve()

    const protocol = queue.sendProtocol(bytes(27, 91, 53, 110))
    generation = 2
    releaseFirst.resolve()

    await flushing
    await expect(protocol).rejects.toThrow('STALE_INPUT_TARGET')
    expect(events).toEqual(['user:1'])
    expect(queue.snapshot()).toMatchObject({
      state: 'paused',
      reason: 'target-changed',
      blockedSeq: '2',
    })
  })

  it('D16_Input_EmptyPastePausesLaterEnter_012', async () => {
    const sent: string[] = []
    const queue = createInputIntentQueue({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      send: async intent => sent.push(new TextDecoder().decode(intent.bytes)),
    })

    const paste = queue.reserveAsync({
      source: 'user-paste',
      modeEpoch: '1',
      produce: async () => new Uint8Array(),
    })
    queue.enqueue({ source: 'user-text', modeEpoch: '1', bytes: utf8('\r') })
    await paste.settled
    await queue.flush()

    expect(sent).toEqual([])
    expect(queue.snapshot()).toMatchObject({
      state: 'paused',
      reason: 'producer-failed',
      blockedSeq: '1',
      queued: 2,
    })
  })
})

describe('D16 terminal host source classification', () => {
  it('D16_Host_DataThatLooksLikeProtocolIsNeverGuessedAsProtocol_006', () => {
    const event = classifyXtermData('\x1b[6n')
    expect(event.route).toBe('legacy-direct')
    expect(event.source).toBe('xterm-data-ambiguous')
    expect(Array.from(event.bytes)).toEqual(Array.from(utf8('\x1b[6n')))
  })

  it('D16_Host_XtermBinaryPreservesRawByteSemantics_007', () => {
    const event = classifyXtermBinary(String.fromCharCode(0, 127, 128, 255))
    expect(event.route).toBe('protocol-bypass')
    expect(event.source).toBe('xterm-binary')
    expect(Array.from(event.bytes)).toEqual([0, 127, 128, 255])
  })

  it('D16_Host_ExplicitUserTextUsesUtf8AndUserQueue_008', () => {
    const event = classifyExplicitUserText('中')
    expect(event.route).toBe('user-queue')
    expect(event.source).toBe('explicit-user')
    expect(Array.from(event.bytes)).toEqual(Array.from(utf8('中')))
  })
})
