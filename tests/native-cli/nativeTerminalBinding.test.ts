import { describe, expect, it } from 'vitest'
import { createNativeTerminalBinding } from '@/terminal/nativeTerminalBinding'
import type { OutputFrame } from '@/types/terminal'

type Listener<T> = (value: T) => void

function emitter<T>() {
  const listeners = new Set<Listener<T>>()
  return {
    event(listener: Listener<T>) {
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

function frame(bytes: number[], offset = '0'): OutputFrame {
  return {
    runId: 'run-a',
    generation: 2,
    streamEpoch: '9',
    offset,
    bytes,
  }
}

describe('D19 native terminal binding', () => {
  it('D19_Binding_OutputParserReplyUsesProtocolWriterAndAckWaitsForParsedCallback_013', async () => {
    const xterm = fakeXterm()
    const protocol: number[][] = []
    const acks: string[] = []
    const binding = createNativeTerminalBinding({
      term: xterm.term,
      runId: 'run-a',
      generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch: '4' }),
      writeUser: async input => ({
        runId: input.runId,
        generation: input.generation,
        inputSeq: input.inputSeq,
        modeEpoch: input.modeEpoch,
        state: 'host-written',
        confirmedBytes: String(input.bytes.length),
      }),
      writeProtocol: async (_run, bytes) => {
        protocol.push(Array.from(bytes))
        return { state: 'host-written', confirmedBytes: String(bytes.length) }
      },
      ackOutput: async ack => { acks.push(ack.throughOffset) },
    })

    expect(binding.acceptOutput(frame([27, 91, 54, 110]))).toBe(true)
    xterm.data.fire('\x1b[1;1R')
    await binding.drainInput()
    expect(protocol).toEqual([[27, 91, 49, 59, 49, 82]])
    expect(acks).toEqual([])

    xterm.writeCallbacks[0]()
    await Promise.resolve()
    expect(acks).toEqual(['4'])
    binding.dispose()
  })

  it('D19_Binding_UserSignalRoutesKeyboardDataThroughOrderedNativeWriter_014', async () => {
    const xterm = fakeXterm()
    const users: string[] = []
    const protocol: string[] = []
    const binding = createNativeTerminalBinding({
      term: xterm.term,
      runId: 'run-a',
      generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch: '4' }),
      writeUser: async input => {
        users.push(input.inputSeq + ':' + new TextDecoder().decode(input.bytes))
        return {
          runId: input.runId,
          generation: input.generation,
          inputSeq: input.inputSeq,
          modeEpoch: input.modeEpoch,
          state: 'host-written',
          confirmedBytes: String(input.bytes.length),
        }
      },
      writeProtocol: async (_run, bytes) => {
        protocol.push(new TextDecoder().decode(bytes))
        return { state: 'host-written', confirmedBytes: String(bytes.length) }
      },
      ackOutput: async () => {},
    })

    xterm.user.fire()
    xterm.data.fire('a')
    await binding.drainInput()

    expect(users).toEqual(['1:a'])
    expect(protocol).toEqual([])
    binding.dispose()
  })

  it('D19_Binding_ProtocolAndUserTrafficShareTheD16DispatchGate_015', async () => {
    const xterm = fakeXterm()
    const events: string[] = []
    let releaseUser!: () => void
    const userBlocked = new Promise<void>(resolve => { releaseUser = resolve })
    const binding = createNativeTerminalBinding({
      term: xterm.term,
      runId: 'run-a',
      generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch: '4' }),
      writeUser: async input => {
        events.push('user')
        await userBlocked
        return {
          runId: input.runId,
          generation: input.generation,
          inputSeq: input.inputSeq,
          modeEpoch: input.modeEpoch,
          state: 'host-written',
          confirmedBytes: String(input.bytes.length),
        }
      },
      writeProtocol: async (_run, bytes) => {
        events.push('protocol:' + new TextDecoder().decode(bytes))
        return { state: 'host-written', confirmedBytes: String(bytes.length) }
      },
      ackOutput: async () => {},
    })

    xterm.user.fire()
    xterm.data.fire('x')
    await Promise.resolve()
    xterm.data.fire('\x1b[0n')
    await Promise.resolve()
    expect(events).toEqual(['user'])

    releaseUser()
    await binding.drainInput()
    expect(events).toEqual(['user', 'protocol:\x1b[0n'])
    binding.dispose()
  })

  it('D23_Binding_ExplicitUserTextUsesSameOrderedWriter_016', async () => {
    const xterm = fakeXterm()
    const users: string[] = []
    const binding = createNativeTerminalBinding({
      term: xterm.term,
      runId: 'run-a',
      generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch: '4' }),
      writeUser: async input => {
        users.push(input.inputSeq + ':' + new TextDecoder().decode(input.bytes))
        return {
          runId: input.runId,
          generation: input.generation,
          inputSeq: input.inputSeq,
          modeEpoch: input.modeEpoch,
          state: 'host-written',
          confirmedBytes: String(input.bytes.length),
        }
      },
      writeProtocol: async (_run, bytes) => ({
        state: 'host-written',
        confirmedBytes: String(bytes.length),
      }),
      ackOutput: async () => {},
    })

    await binding.sendUserText('中文')
    expect(users).toEqual(['1:中文'])
    binding.dispose()
  })

  it('D23_Binding_AsyncPasteReservesSequenceBeforeLaterEnter_017', async () => {
    const xterm = fakeXterm()
    const users: string[] = []
    let releasePaste!: (value: Uint8Array) => void
    const paste = new Promise<Uint8Array>(resolve => { releasePaste = resolve })
    const binding = createNativeTerminalBinding({
      term: xterm.term,
      runId: 'run-a',
      generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch: '4' }),
      writeUser: async input => {
        users.push(input.inputSeq + ':' + new TextDecoder().decode(input.bytes))
        return {
          runId: input.runId,
          generation: input.generation,
          inputSeq: input.inputSeq,
          modeEpoch: input.modeEpoch,
          state: 'host-written',
          confirmedBytes: String(input.bytes.length),
        }
      },
      writeProtocol: async (_run, bytes) => ({
        state: 'host-written',
        confirmedBytes: String(bytes.length),
      }),
      ackOutput: async () => {},
    })

    const reserved = binding.reserveUserPaste(() => paste)
    const enter = binding.sendUserText('\r')
    await Promise.resolve()
    expect(users).toEqual([])

    releasePaste(new TextEncoder().encode('paste-body'))
    await reserved.settled
    await enter

    expect(reserved.inputSeq).toBe('1')
    expect(users).toEqual(['1:paste-body', '2:\r'])
    binding.dispose()
  })

  // 隐藏Native终端拒绝用户键盘/粘贴，后台协议应答和输出ACK继续执行。
  it('Unified_HiddenInputKeepsProtocol_001', async () => {
    const xterm = fakeXterm()
    const users: string[] = []; const protocol: string[] = []; const acks: string[] = []
    let active = false
    const binding = createNativeTerminalBinding({
      term: xterm.term, runId: 'run-a', generation: 2,
      currentTarget: () => ({ runId: 'run-a', generation: 2, modeEpoch: '4' }),
      isUserInputAllowed: () => active,
      writeUser: async input => {
        users.push(new TextDecoder().decode(input.bytes))
        return { ...input, state: 'host-written', confirmedBytes: String(input.bytes.length) }
      },
      writeProtocol: async (_run, bytes) => {
        protocol.push(new TextDecoder().decode(bytes))
        return { state: 'host-written', confirmedBytes: String(bytes.length) }
      },
      ackOutput: async ack => { acks.push(ack.throughOffset) },
    })
    xterm.user.fire(); xterm.data.fire('hidden keyboard'); await binding.drainInput()
    await expect(binding.sendUserText('hidden command')).rejects.toThrow()
    await expect(binding.reserveUserPaste(async () => new TextEncoder().encode('hidden paste')).settled).rejects.toThrow()
    expect(binding.acceptOutput(frame([65]))).toBe(true)
    xterm.data.fire('protocol reply'); xterm.writeCallbacks[0](); await binding.drainInput(); await Promise.resolve()
    expect(users).toEqual([]); expect(protocol).toEqual(['protocol reply']); expect(acks).toEqual(['1'])
    active = true; await binding.sendUserText('visible keyboard')
    expect(users).toEqual(['visible keyboard']); binding.dispose()
  })

})
