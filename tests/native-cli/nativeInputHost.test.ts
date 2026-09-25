import { describe, expect, it, vi } from 'vitest'
import { Terminal } from '@xterm/xterm'
import { createNativeTerminalInputHost } from '@/terminal/nativeInputHost'
import type { InputWriteReceipt, ProtocolWriteReceipt } from '@/types/terminal'

function hostWritten(input: {
  runId: string
  generation: number
  inputSeq: string
  modeEpoch: string
  bytes: Uint8Array
}): InputWriteReceipt {
  return {
    runId: input.runId,
    generation: input.generation,
    inputSeq: input.inputSeq,
    modeEpoch: input.modeEpoch,
    state: 'host-written',
    confirmedBytes: String(input.bytes.byteLength),
  }
}

describe('D19 native terminal input host', () => {
  it('D19_Host_UserDataAndDsrUseDifferentAuthenticatedLanes_010', async () => {
    const term = new Terminal()
    const userWrites: number[][] = []
    const protocolWrites: number[][] = []
    const host = createNativeTerminalInputHost({
      terminal: term,
      runId: 'run-a',
      generation: 3,
      currentTarget: () => ({ runId: 'run-a', generation: 3, modeEpoch: '7' }),
      writeUser: async input => {
        userWrites.push(Array.from(input.bytes))
        return hostWritten(input)
      },
      writeProtocol: async (_run, bytes): Promise<ProtocolWriteReceipt> => {
        protocolWrites.push(Array.from(bytes))
        return { state: 'host-written', confirmedBytes: String(bytes.byteLength) }
      },
    })

    term.input('A', true)
    await host.flush()
    await new Promise<void>(resolve => term.write('\x1b[5n', resolve))
    await vi.waitFor(() => expect(protocolWrites).toHaveLength(1))

    expect(new TextDecoder().decode(Uint8Array.from(userWrites[0]))).toBe('A')
    expect(new TextDecoder().decode(Uint8Array.from(protocolWrites[0]))).toBe('\x1b[0n')

    host.dispose()
    term.dispose()
  })

  it('D19_Host_ProtocolCrossesPendingPasteButNotActiveUserFrame_011', async () => {
    const term = new Terminal()
    const events: string[] = []
    let releaseUser!: () => void
    const userGate = new Promise<void>(resolve => { releaseUser = resolve })
    let resolvePaste!: (value: Uint8Array) => void
    const pasteBytes = new Promise<Uint8Array>(resolve => { resolvePaste = resolve })

    const host = createNativeTerminalInputHost({
      terminal: term,
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      writeUser: async input => {
        events.push('user-start')
        await userGate
        events.push('user-end')
        return hostWritten(input)
      },
      writeProtocol: async (_run, bytes) => {
        events.push('protocol:' + new TextDecoder().decode(bytes))
        return { state: 'host-written', confirmedBytes: String(bytes.byteLength) }
      },
    })

    host.reservePaste(() => pasteBytes)
    await new Promise<void>(resolve => term.write('\x1b[5n', resolve))
    await vi.waitFor(() => expect(events).toEqual(['protocol:\x1b[0n']))

    resolvePaste(new TextEncoder().encode('payload'))
    const activeFlush = host.flush()
    await vi.waitFor(() => expect(events).toContain('user-start'))

    await new Promise<void>(resolve => term.write('\x1b[5n', resolve))
    await Promise.resolve()
    expect(events).toEqual(['protocol:\x1b[0n', 'user-start'])

    releaseUser()
    await activeFlush
    await vi.waitFor(() => expect(events).toEqual([
      'protocol:\x1b[0n',
      'user-start',
      'user-end',
      'protocol:\x1b[0n',
    ]))

    host.dispose()
    term.dispose()
  })

  it('D19_Host_PartialReceiptPausesFailedSeqAndNeverSendsLaterEnter_012', async () => {
    const term = new Terminal()
    const writes: string[] = []
    const host = createNativeTerminalInputHost({
      terminal: term,
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '2' }),
      writeUser: async input => {
        writes.push(new TextDecoder().decode(input.bytes))
        return {
          ...hostWritten(input),
          state: 'partial-or-unknown',
          confirmedBytes: '1',
        }
      },
      writeProtocol: async (_run, bytes) => ({
        state: 'host-written',
        confirmedBytes: String(bytes.byteLength),
      }),
    })

    term.input('paste-frame', true)
    term.input('\r', true)
    await host.flush()

    expect(writes).toEqual(['paste-frame'])
    expect(host.snapshot()).toMatchObject({
      state: 'paused',
      blockedSeq: '1',
      reason: 'send-failed',
      queued: 2,
    })

    host.dispose()
    term.dispose()
  })

  it('D19_Host_DisposeStopsDataAndBinaryRouting_013', async () => {
    const term = new Terminal()
    const writeUser = vi.fn(async input => hostWritten(input))
    const writeProtocol = vi.fn(async (_run, bytes: Uint8Array) => ({
      state: 'host-written' as const,
      confirmedBytes: String(bytes.byteLength),
    }))
    const host = createNativeTerminalInputHost({
      terminal: term,
      runId: 'run-a',
      generation: 1,
      currentTarget: () => ({ runId: 'run-a', generation: 1, modeEpoch: '1' }),
      writeUser,
      writeProtocol,
    })

    host.dispose()
    term.input('A', true)
    ;(term as any)._core.coreService.triggerBinaryEvent(String.fromCharCode(0xff))
    await Promise.resolve()

    expect(writeUser).not.toHaveBeenCalled()
    expect(writeProtocol).not.toHaveBeenCalled()
    term.dispose()
  })
})
