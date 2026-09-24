import { afterEach, describe, expect, it, vi } from 'vitest'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { cliAckOutput } from '@/api/tauri'
import {
  createTerminalOutputConsumer,
  type NativeOutputAck,
  type NativeOutputFrame,
} from '@/integrations/terminalTransport'

class FakeTerminal {
  readonly writes: Uint8Array[] = []
  readonly callbacks: Array<() => void> = []

  write(data: Uint8Array, callback?: () => void) {
    this.writes.push(new Uint8Array(data))
    if (callback) this.callbacks.push(callback)
  }
}

const run = { runId: 'run-a', generation: 2 }
const frame = (offset: string, bytes: number[], streamEpoch = '9'): NativeOutputFrame => ({
  ...run,
  streamEpoch,
  offset,
  bytes,
})

afterEach(() => {
  clearMocks()
  delete (window as Window & { __CC_DESK_DOCUMENT__?: unknown }).__CC_DESK_DOCUMENT__
})

describe('D14 native terminal transport', () => {
  it('D14_Frontend_AcksOnlyAfterTermWriteCallback_001', async () => {
    const term = new FakeTerminal()
    const acks: NativeOutputAck[] = []
    const consumer = createTerminalOutputConsumer(run, term, async ack => { acks.push(ack) })

    consumer.accept(frame('0', [0, 255, 27, 91]))
    expect(term.writes.map(value => [...value])).toEqual([[0, 255, 27, 91]])
    expect(acks).toEqual([])

    term.callbacks[0]()
    await Promise.resolve()
    expect(acks).toEqual([{ ...run, streamEpoch: '9', throughOffset: '4' }])
  })

  it('D14_Frontend_CumulativeAckWaitsForContiguousCallbacks_002', async () => {
    const term = new FakeTerminal()
    const acks: NativeOutputAck[] = []
    const consumer = createTerminalOutputConsumer(run, term, async ack => { acks.push(ack) })

    consumer.accept(frame('0', [1, 2, 3]))
    consumer.accept(frame('3', [4, 5]))

    term.callbacks[1]()
    await Promise.resolve()
    expect(acks).toEqual([])

    term.callbacks[0]()
    await Promise.resolve()
    expect(acks).toEqual([{ ...run, streamEpoch: '9', throughOffset: '5' }])
  })

  it('D14_Frontend_RejectsWrongRunEpochGapAndOversize_003', () => {
    const term = new FakeTerminal()
    const consumer = createTerminalOutputConsumer(run, term, async () => {})

    expect(() => consumer.accept({ ...frame('0', [1]), runId: 'other' })).toThrow('OUTPUT_RUN_MISMATCH')
    consumer.accept(frame('0', [1]))
    expect(() => consumer.accept(frame('1', [2], '10'))).toThrow('OUTPUT_STREAM_EPOCH_MISMATCH')
    expect(() => consumer.accept(frame('2', [2]))).toThrow('OUTPUT_OFFSET_GAP')
    expect(() => consumer.accept(frame('1', new Array(16 * 1024 + 1).fill(1)))).toThrow('OUTPUT_FRAME_TOO_LARGE')
  })

  it('D14_Frontend_DisposeMakesLateWriteCallbacksInert_004', async () => {
    const term = new FakeTerminal()
    const sendAck = vi.fn(async () => {})
    const consumer = createTerminalOutputConsumer(run, term, sendAck)

    consumer.accept(frame('0', [1, 2]))
    consumer.dispose()
    term.callbacks[0]()
    await Promise.resolve()

    expect(sendAck).not.toHaveBeenCalled()
    expect(() => consumer.accept(frame('2', [3]))).toThrow('OUTPUT_CONSUMER_CLOSED')
  })

  it('D14_Api_AckUsesPinnedDocumentBridgeAndNeverGenericInvoke_005', async () => {
    mockIPC(() => { throw new Error('unguarded invoke') })
    const invoke = vi.fn(async () => undefined)
    Object.defineProperty(window, '__CC_DESK_DOCUMENT__', {
      configurable: true,
      value: { instanceId: 'backend-d14', invoke },
    })
    const ack: NativeOutputAck = { ...run, streamEpoch: '9', throughOffset: '5' }

    await cliAckOutput(ack)

    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith('cli_ack_output', ack)
  })
})
