import { describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { resolve } from 'node:path'
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

// 内部 provenance tap 的锁文件与实际运行依赖都必须保持已验证的 xterm 5.5.0。
it('Native_PinXtermProvenance_014', () => {
  const lock = JSON.parse(readFileSync(resolve('package-lock.json'), 'utf8'))
  const require = createRequire(resolve('package.json'))
  const installed = JSON.parse(readFileSync(require.resolve('@xterm/xterm/package.json'), 'utf8'))
  expect(lock.packages['node_modules/@xterm/xterm'].version).toBe('5.5.0')
  expect(installed.version).toBe('5.5.0')
})

// 已安装真实 xterm 的 input、解析器 DSR 和 8-bit 输出仍通过本项目 provenance tap 正确分流。
it('Native_RealXtermProvenance_015', async () => {
  const canvas = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  const { Terminal } = await import('@xterm/xterm')
  canvas.mockRestore()
  const term = new Terminal({ allowProposedApi: true })
  const users: string[] = []; const protocol: string[] = []; const binary: number[][] = []
  const binding = bindXtermInputProvenance(term as any, {
    user: value => { users.push(value) },
    protocol: value => { protocol.push(value) },
    binary: value => { binary.push([...value].map(char => char.charCodeAt(0))) },
  })
  try {
    term.input('typed', true)
    await new Promise<void>(resolve => term.write('\x1b[6n', resolve))
    ;(term as any)._core.coreService.triggerBinaryEvent(String.fromCharCode(0, 128, 255))
    await binding.drain()
    expect(users).toEqual(['typed'])
    expect(protocol).toEqual(['\x1b[1;1R'])
    expect(binary).toEqual([[0, 128, 255]])
  } finally { binding.dispose(); term.dispose() }
})
