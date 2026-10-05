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

// 成功回执必须匹配发出的四项身份；错回执暂停队列且不发送第二条。
it.each([
  ['runId', 'run-b'], ['generation', 2], ['inputSeq', '2'], ['modeEpoch', '8'],
])('Native_ReceiptIdentity_008 %s', async (field, value) => {
  const writes: string[] = []
  const host = createTerminalHostProtocol({
    runId: 'run-a', generation: 1,
    currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '7' }),
    writeUser: async frame => {
      writes.push(frame.inputSeq)
      await Promise.resolve()
      return { ...receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length), [field]: value }
    },
    writeProtocol: async (_run, bytes) => ({ state: 'host-written', confirmedBytes: String(bytes.length) }),
  })
  await Promise.all([host.sendUserText('first'), host.sendUserText('second')])
  expect(writes).toEqual(['1'])
  expect(host.snapshot()).toMatchObject({ state: 'paused', reason: 'send-failed', blockedSeq: '1', queued: 2 })
})

// 数字、空值、前导零、负数、溢出和不完整回执不能冒充精确成功。
it.each([1, null, undefined, '01', '-1', '18446744073709551616', '0', '2'])('Native_ReceiptBytes_009 %s', async confirmedBytes => {
  const writes: string[] = []
  const host = createTerminalHostProtocol({
    runId: 'run-a', generation: 1,
    currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '7' }),
    writeUser: async frame => {
      writes.push(frame.inputSeq)
      return { ...receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length), confirmedBytes } as any
    },
    writeProtocol: async (_run, bytes) => ({ state: 'host-written', confirmedBytes: String(bytes.length) }),
  })
  await Promise.all([host.sendUserText('x'), host.sendUserText('y')])
  expect(writes).toEqual(['1'])
  expect(host.snapshot()).toMatchObject({ state: 'paused', reason: 'send-failed' })
})

// writer 持有的参数发生变化也不能改变 await 前冻结的成功关联身份。
it('Native_FreezeReceiptIdentity_010', async () => {
  const writes: string[] = []
  const host = createTerminalHostProtocol({
    runId: 'run-a', generation: 1,
    currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '7' }),
    writeUser: async frame => {
      writes.push(frame.inputSeq)
      frame.runId = 'mutated-run'; frame.generation = 3; frame.inputSeq = '9'; frame.modeEpoch = '10'
      await Promise.resolve()
      return receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length)
    },
    writeProtocol: async (_run, bytes) => ({ state: 'host-written', confirmedBytes: String(bytes.length) }),
  })
  await Promise.all([host.sendUserText('x'), host.sendUserText('y')])
  expect(writes).toEqual(['1'])
  expect(host.snapshot()).toMatchObject({ state: 'paused', reason: 'send-failed' })
})

// 协议回执沿用真实的两字段 schema，不凭空增加 user 身份字段。
it('Native_ProtocolReceiptShape_011', async () => {
  const protocol: number[][] = []
  const host = createTerminalHostProtocol({
    runId: 'run-a', generation: 1,
    currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '7' }),
    writeUser: async frame => receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length),
    writeProtocol: async (_run, bytes) => {
      protocol.push([...bytes])
      return { state: 'host-written', confirmedBytes: String(bytes.length) }
    },
  })
  await host.handleBinary(String.fromCharCode(0, 128, 255))
  expect(protocol).toEqual([[0, 128, 255]])
  expect(host.snapshot().state).toBe('open')
})

// 协议坏回执保持固定错误码，不能把原始回执或 payload 带进错误。
it.each([3, '03', null, '18446744073709551616'])('Native_ProtocolBadBytes_012 %s', async confirmedBytes => {
  const host = createTerminalHostProtocol({
    runId: 'run-a', generation: 1,
    currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '7' }),
    writeUser: async frame => receipt(frame.runId, frame.generation, frame.inputSeq, frame.modeEpoch, frame.bytes.length),
    writeProtocol: async () => ({ state: 'host-written', confirmedBytes } as any),
  })
  await expect(host.handleBinary('abc')).rejects.toThrow(/^(INVALID_CONFIRMED_BYTES|NATIVE_INPUT_WRITE_INCOMPLETE)$/)
})
