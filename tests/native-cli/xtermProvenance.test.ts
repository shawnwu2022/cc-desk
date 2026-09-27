import { describe, expect, it } from 'vitest'
import { bindXtermInputProvenance } from '@/terminal/xtermProvenance'

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
    size: () => listeners.size,
  }
}

function fakeXterm(withUserHook = true) {
  const data = emitter<string>()
  const binary = emitter<string>()
  const user = emitter<void>()
  const term: any = {
    onData: data.event,
    onBinary: binary.event,
    _core: withUserHook ? { coreService: { onUserInput: user.event } } : {},
  }
  return { term, data, binary, user }
}

describe('D19 xterm provenance bridge', () => {
  it('D19_Xterm_UserSignalTagsExactlyTheFollowingOnDataAsUser_008', async () => {
    const xterm = fakeXterm()
    const events: string[] = []
    const binding = bindXtermInputProvenance(xterm.term, {
      user: async value => { events.push('user:' + value) },
      protocol: async value => { events.push('protocol:' + value) },
      binary: async value => { events.push('binary:' + value) },
    })

    xterm.user.fire()
    xterm.data.fire('a')
    xterm.data.fire('\x1b[0n')
    await binding.drain()

    expect(events).toEqual(['user:a', 'protocol:\x1b[0n'])
    binding.dispose()
  })

  it('D19_Xterm_MultipleUserSignalsAreConsumedOneForOne_009', async () => {
    const xterm = fakeXterm()
    const events: string[] = []
    const binding = bindXtermInputProvenance(xterm.term, {
      user: async value => { events.push('user:' + value) },
      protocol: async value => { events.push('protocol:' + value) },
      binary: async () => {},
    })

    xterm.user.fire()
    xterm.user.fire()
    xterm.data.fire('a')
    xterm.data.fire('b')
    xterm.data.fire('c')
    await binding.drain()

    expect(events).toEqual(['user:a', 'user:b', 'protocol:c'])
    binding.dispose()
  })

  it('D19_Xterm_OnBinaryKeepsSeparateRawProtocolLane_010', async () => {
    const xterm = fakeXterm()
    const binaries: string[] = []
    const binding = bindXtermInputProvenance(xterm.term, {
      user: async () => {},
      protocol: async () => {},
      binary: async value => { binaries.push(value) },
    })

    xterm.binary.fire(String.fromCharCode(0, 128, 255))
    await binding.drain()
    expect(binaries[0].split('').map(ch => ch.charCodeAt(0))).toEqual([0, 128, 255])
    binding.dispose()
  })

  it('D19_Xterm_MissingUserProvenanceHookFailsClosedBeforeSubscriptions_011', () => {
    const xterm = fakeXterm(false)
    expect(() => bindXtermInputProvenance(xterm.term, {
      user: async () => {},
      protocol: async () => {},
      binary: async () => {},
    })).toThrow('XTERM_USER_INPUT_PROVENANCE_UNAVAILABLE')
    expect(xterm.data.size()).toBe(0)
    expect(xterm.binary.size()).toBe(0)
  })

  it('D19_Xterm_DisposeRemovesAllSubscriptions_012', () => {
    const xterm = fakeXterm()
    const binding = bindXtermInputProvenance(xterm.term, {
      user: async () => {},
      protocol: async () => {},
      binary: async () => {},
    })
    expect(xterm.data.size()).toBe(1)
    expect(xterm.binary.size()).toBe(1)
    expect(xterm.user.size()).toBe(1)

    binding.dispose()
    expect(xterm.data.size()).toBe(0)
    expect(xterm.binary.size()).toBe(0)
    expect(xterm.user.size()).toBe(0)
  })
  it('D19_Xterm_AsyncRouteFailureRemainsObservableAtDrain_013', async () => {
    const xterm = fakeXterm()
    const binding = bindXtermInputProvenance(xterm.term, {
      user: async () => { throw new Error('native writer failed') },
      protocol: async () => {},
      binary: async () => {},
    })

    xterm.user.fire()
    xterm.data.fire('x')
    await Promise.resolve()
    await expect(binding.drain()).rejects.toThrow('native writer failed')
    binding.dispose()
  })

})
