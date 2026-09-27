import { describe, expect, it } from 'vitest'
import { createTerminalHostProtocol } from '@/terminal/nativeHostProtocol'

function receipt(
  runId: string,
  generation: number,
  inputSeq: string,
  modeEpoch: string,
  bytes: number,
) {
  return {
    runId,
    generation,
    inputSeq,
    modeEpoch,
    state: 'host-written' as const,
    confirmedBytes: String(bytes),
  }
}

describe('D19 native terminal host protocol', () => {
  it('D19_Protocol_ParserOriginatedOnDataUsesProtocolWriterWithoutContentGuessing_001', async () => {
    const events: string[] = []
    const host = createTerminalHostProtocol({
      runId: 'run-a',
      generation: 3,
      currentTarget: () => ({ runId: 'run-a', generation: 3, modeEpoch: '7' }),
      writeUser: async frame => receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length),
      writeProtocol: async (_run, bytes) => {
        events.push('protocol:' + Array.from(bytes).join(','))
        return { state: 'host-written', confirmedBytes: String(bytes.length) }
      },
    })

    const leaveParser = host.beginParserOutput()
    const pending = host.handleData('\x1b[0n')
    leaveParser()
    await pending

    expect(events).toEqual(['protocol:27,91,48,110'])
  })

  it('D19_Protocol_UserEventOnDataUsesOrderedUserWriter_002', async () => {
    const events: string[] = []
    const host = createTerminalHostProtocol({
      runId: 'run-a',
      generation: 3,
      currentTarget: () => ({ runId: 'run-a', generation: 3, modeEpoch: '7' }),
      writeUser: async frame => {
        events.push('user:' + frame.inputSeq + ':' + new TextDecoder().decode(frame.bytes))
        return receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length)
      },
      writeProtocol: async () => ({ state: 'host-written', confirmedBytes: '0' }),
    })

    const leaveUser = host.beginUserEvent()
    const pending = host.handleData('中')
    leaveUser()
    await pending

    expect(events).toEqual(['user:1:中'])
  })

  it('D19_Protocol_UnprovenancedOnDataFailsClosed_003', async () => {
    const host = createTerminalHostProtocol({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      writeUser: async frame => receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length),
      writeProtocol: async () => ({ state: 'host-written', confirmedBytes: '0' }),
    })

    await expect(host.handleData('\x1b[6n')).rejects.toThrow('AMBIGUOUS_XTERM_DATA_SOURCE')
  })

  it('D19_Protocol_OnBinaryPreservesRawEightBitBytes_004', async () => {
    const seen: number[][] = []
    const host = createTerminalHostProtocol({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      writeUser: async frame => receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length),
      writeProtocol: async (_run, bytes) => {
        seen.push(Array.from(bytes))
        return { state: 'host-written', confirmedBytes: String(bytes.length) }
      },
    })

    await host.handleBinary(String.fromCharCode(0, 127, 128, 255))
    expect(seen).toEqual([[0, 127, 128, 255]])
  })

  it('D19_Protocol_PartialUserReceiptFreezesLaterUserInput_005', async () => {
    const writes: string[] = []
    const host = createTerminalHostProtocol({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      writeUser: async frame => {
        writes.push(frame.inputSeq)
        if (frame.inputSeq === '1') {
          return {
            ...receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length),
            state: 'partial-or-unknown' as const,
            confirmedBytes: '1',
          }
        }
        return receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length)
      },
      writeProtocol: async () => ({ state: 'host-written', confirmedBytes: '0' }),
    })

    const leaveUser = host.beginUserEvent()
    const first = host.handleData('abc')
    const second = host.handleData('\r')
    leaveUser()
    await first
    await second

    expect(writes).toEqual(['1'])
    expect(host.snapshot()).toMatchObject({
      state: 'paused',
      blockedSeq: '1',
      reason: 'send-failed',
      queued: 2,
    })
  })

  it('D19_Protocol_HostWrittenRequiresExactConfirmedByteCount_006', async () => {
    const host = createTerminalHostProtocol({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      writeUser: async frame => ({
        ...receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length),
        confirmedBytes: '1',
      }),
      writeProtocol: async () => ({ state: 'host-written', confirmedBytes: '0' }),
    })

    const leaveUser = host.beginUserEvent()
    const pending = host.handleData('abcd')
    leaveUser()
    await pending

    expect(host.snapshot()).toMatchObject({ state: 'paused', reason: 'send-failed' })
  })

  it('D19_Protocol_ConflictingParserAndUserContextsFailClosed_007', async () => {
    const host = createTerminalHostProtocol({
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      writeUser: async frame => receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length),
      writeProtocol: async () => ({ state: 'host-written', confirmedBytes: '0' }),
    })

    const leaveUser = host.beginUserEvent()
    const leaveParser = host.beginParserOutput()
    const pending = host.handleData('x')
    leaveParser()
    leaveUser()

    await expect(pending).rejects.toThrow('CONFLICTING_XTERM_DATA_SOURCE')
  })
})
