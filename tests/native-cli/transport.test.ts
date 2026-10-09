import { describe, expect, it, vi } from 'vitest'
import { createTerminalOutputTransport } from '@/terminal/transport'
import type { OutputFrame } from '@/types/terminal'

function frame(offset: string, bytes: number[], epoch = '9'): OutputFrame {
  return { runId: 'run-a', generation: 2, streamEpoch: epoch, offset, bytes }
}

describe('D14 native terminal output transport', () => {
  it('D14_Frontend_AcksOnlyFromTheMatchingWriteCallback_001', async () => {
    const callbacks: Array<() => void> = []
    const writes: number[][] = []
    const acks: unknown[] = []
    const transport = createTerminalOutputTransport({
      runId: 'run-a',
      generation: 2,
      write(bytes, done) {
        writes.push(Array.from(bytes))
        callbacks.push(done)
      },
      async ack(value) { acks.push(value) },
    })

    expect(transport.accept(frame('0', [0, 255, 27, 65]))).toBe(true)
    expect(transport.accept(frame('4', [66]))).toBe(true)
    expect(writes).toEqual([[0, 255, 27, 65], [66]])
    expect(acks).toEqual([])

    callbacks[0]()
    await Promise.resolve()
    expect(acks).toEqual([
      { runId: 'run-a', generation: 2, streamEpoch: '9', throughOffset: '4' },
    ])
    callbacks[1]()
    await Promise.resolve()
    expect(acks).toEqual([
      { runId: 'run-a', generation: 2, streamEpoch: '9', throughOffset: '4' },
      { runId: 'run-a', generation: 2, streamEpoch: '9', throughOffset: '5' },
    ])
  })

  it('D14_Frontend_OutOfOrderCallbacksNeverAckAcrossAnUnparsedGap_002', async () => {
    const callbacks: Array<() => void> = []
    const acks: Array<{ throughOffset: string }> = []
    const transport = createTerminalOutputTransport({
      runId: 'run-a',
      generation: 2,
      write(_bytes, done) { callbacks.push(done) },
      async ack(value) { acks.push(value) },
    })
    transport.accept(frame('0', [1, 2, 3, 4]))
    transport.accept(frame('4', [5, 6]))

    callbacks[1]()
    await Promise.resolve()
    expect(acks).toEqual([])
    callbacks[0]()
    await Promise.resolve()
    expect(acks.map(value => value.throughOffset)).toEqual(['6'])
  })

  it('D14_Frontend_InvalidIdentityEpochOffsetOrBytesDegradesWithoutWriting_003', () => {
    const write = vi.fn()
    const degraded: string[] = []
    const transport = createTerminalOutputTransport({
      runId: 'run-a',
      generation: 2,
      write,
      async ack() {},
      onDegraded: reason => degraded.push(reason),
    })

    expect(transport.accept(frame('0', [1]))).toBe(true)
    expect(transport.accept(frame('1', [2], '10'))).toBe(false)
    expect(write).toHaveBeenCalledTimes(1)
    expect(degraded).toEqual(['STALE_OUTPUT_STREAM'])

    for (const bad of [
      { ...frame('01', [3]), streamEpoch: '9' },
      { ...frame('2', [3]), streamEpoch: '9' },
      { ...frame('1', [-1]), streamEpoch: '9' },
      { ...frame('1', new Array(16 * 1024 + 1).fill(1)), streamEpoch: '9' },
      { ...frame('1', [3]), runId: 'peer' },
      { ...frame('1', [3]), generation: 3 },
    ] as unknown as OutputFrame[]) {
      expect(transport.accept(bad)).toBe(false)
    }
    expect(write).toHaveBeenCalledTimes(1)
  })

  it('D14_Frontend_DisposeOrAckFailureCannotCreateAFalseParsedAck_004', async () => {
    const callbacks: Array<() => void> = []
    const degraded: string[] = []
    const ack = vi.fn(async () => { throw new Error('transport lost') })
    const transport = createTerminalOutputTransport({
      runId: 'run-a',
      generation: 2,
      write(_bytes, done) { callbacks.push(done) },
      ack,
      onDegraded: reason => degraded.push(reason),
    })
    transport.accept(frame('0', [1]))
    callbacks[0]()
    await Promise.resolve()
    await Promise.resolve()
    expect(ack).toHaveBeenCalledTimes(1)
    expect(degraded).toEqual(['OUTPUT_ACK_FAILED'])

    const later = createTerminalOutputTransport({
      runId: 'run-a',
      generation: 2,
      write(_bytes, done) { callbacks.push(done) },
      async ack() { throw new Error('must not run') },
    })
    later.accept(frame('0', [2]))
    later.dispose()
    callbacks[1]()
    await Promise.resolve()
  })
  it('D27_Frontend_HundredsOfParsedCallbacksNeverAckAcrossGap_005', async () => {
    const callbacks: Array<() => void> = []
    const acks: string[] = []
    const transport = createTerminalOutputTransport({
      runId: 'run-a',
      generation: 2,
      write(_bytes, done) { callbacks.push(done) },
      async ack(value) { acks.push(value.throughOffset) },
    })

    for (let index = 0; index < 256; index += 1) {
      expect(transport.accept(frame(String(index), [index & 0xff]))).toBe(true)
    }
    for (let index = callbacks.length - 1; index >= 1; index -= 1) {
      callbacks[index]()
    }
    await Promise.resolve()
    expect(acks).toEqual([])

    callbacks[0]()
    await Promise.resolve()
    expect(acks).toEqual(['256'])
  })

  it('D27_Frontend_OneDegradedRunCannotPoisonPeerTransport_006', async () => {
    const peerCallbacks: Array<() => void> = []
    const peerAcks: string[] = []
    const degraded: string[] = []

    const broken = createTerminalOutputTransport({
      runId: 'run-a',
      generation: 2,
      write(_bytes, done) { done() },
      async ack() { throw new Error('channel lost') },
      onDegraded: reason => degraded.push(reason),
    })
    const peer = createTerminalOutputTransport({
      runId: 'run-b',
      generation: 1,
      write(_bytes, done) { peerCallbacks.push(done) },
      async ack(value) { peerAcks.push(value.throughOffset) },
    })

    expect(broken.accept(frame('0', [1]))).toBe(true)
    await Promise.resolve()
    await Promise.resolve()
    expect(degraded).toEqual(['OUTPUT_ACK_FAILED'])
    expect(broken.accept(frame('1', [2]))).toBe(false)

    expect(peer.accept({
      runId: 'run-b',
      generation: 1,
      streamEpoch: '3',
      offset: '0',
      bytes: [9, 8, 7],
    })).toBe(true)
    peerCallbacks[0]()
    await Promise.resolve()
    expect(peerAcks).toEqual(['3'])
  })

})
