import { beforeEach, describe, expect, it, vi } from 'vitest'

const writeInput = vi.fn()
const writeProtocol = vi.fn()
const ackOutput = vi.fn()

vi.mock('@/api/tauri', () => ({
  cliWriteInput: writeInput,
  cliWriteProtocol: writeProtocol,
  cliAckOutput: ackOutput,
}))

import { createDeskNativeTerminalBinding } from '@/terminal/deskNativeTerminal'

function emitter<T>() {
  const listeners = new Set<(value: T) => void>()
  return {
    event(listener: (value: T) => void) {
      listeners.add(listener)
      return { dispose: () => listeners.delete(listener) }
    },
    fire(value: T) {
      for (const listener of [...listeners]) listener(value)
    },
  }
}

function fakeXterm() {
  const data = emitter<string>()
  const binary = emitter<string>()
  const user = emitter<void>()
  const writeCallbacks: Array<() => void> = []
  const term: any = {
    onData: data.event,
    onBinary: binary.event,
    _core: { coreService: { onUserInput: user.event } },
    write(_bytes: Uint8Array, callback?: () => void) {
      if (callback) writeCallbacks.push(callback)
    },
  }
  return { term, data, binary, user, writeCallbacks }
}

describe('D19 production native terminal wiring', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    writeInput.mockImplementation(async (input: { runId: string; generation: number; inputSeq: string; modeEpoch: string; bytes: Uint8Array }) => ({
      runId: input.runId,
      generation: input.generation,
      inputSeq: input.inputSeq,
      modeEpoch: input.modeEpoch,
      state: 'host-written',
      confirmedBytes: String(input.bytes.length),
    }))
    writeProtocol.mockImplementation(async (_run: unknown, bytes: Uint8Array) => ({
      state: 'host-written',
      confirmedBytes: String(bytes.length),
    }))
    ackOutput.mockResolvedValue(undefined)
  })

  it('D19_Wiring_UserAndProtocolUseAuthenticatedNativeDocumentBridgeApis_016', async () => {
    const xterm = fakeXterm()
    const binding = createDeskNativeTerminalBinding({
      term: xterm.term,
      runId: 'run-a',
      generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch: '5' }),
    })

    xterm.user.fire()
    xterm.data.fire('hello')
    xterm.data.fire('\x1b[0n')
    await binding.drainInput()

    expect(writeInput).toHaveBeenCalledTimes(1)
    expect(writeInput.mock.calls[0][0]).toMatchObject({
      runId: 'run-a',
      generation: 2,
      inputSeq: '1',
      modeEpoch: '5',
    })
    expect(new TextDecoder().decode(writeInput.mock.calls[0][0].bytes)).toBe('hello')

    expect(writeProtocol).toHaveBeenCalledTimes(1)
    expect(writeProtocol.mock.calls[0][0]).toEqual({ runId: 'run-a', generation: 2 })
    expect(new TextDecoder().decode(writeProtocol.mock.calls[0][1])).toBe('\x1b[0n')
    binding.dispose()
  })

  it('D19_Wiring_OutputAckUsesAuthenticatedNativeDocumentBridgeApi_017', async () => {
    const xterm = fakeXterm()
    const binding = createDeskNativeTerminalBinding({
      term: xterm.term,
      runId: 'run-a',
      generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch: '5' }),
    })

    expect(binding.acceptOutput({
      runId: 'run-a',
      generation: 2,
      streamEpoch: '11',
      offset: '0',
      bytes: [65, 66],
    })).toBe(true)
    expect(ackOutput).not.toHaveBeenCalled()

    xterm.writeCallbacks[0]()
    await Promise.resolve()
    expect(ackOutput).toHaveBeenCalledWith({
      runId: 'run-a',
      generation: 2,
      streamEpoch: '11',
      throughOffset: '2',
    })
    binding.dispose()
  })
})
