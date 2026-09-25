import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it, vi } from 'vitest'
import { Terminal } from '@xterm/xterm'
import { bindXterm55DataProvenance } from '@/terminal/protocolHost'

function write(term: Terminal, data: string): Promise<void> {
  return new Promise(resolve => term.write(data, resolve))
}

describe('D19 xterm 5.5 protocol source characterization', () => {
  it('D19_Xterm_DsrStatusReplyUsesDataNotBinary_005', async () => {
    const term = new Terminal()
    const data = vi.fn<(value: string) => void>()
    const binary = vi.fn<(value: string) => void>()
    const dataSub = term.onData(data)
    const binarySub = term.onBinary(binary)

    await write(term, '\x1b[5n')

    expect(data).toHaveBeenCalled()
    expect(data.mock.calls.map(call => call[0])).toContain('\x1b[0n')
    expect(binary).not.toHaveBeenCalled()

    dataSub.dispose()
    binarySub.dispose()
    term.dispose()
  })

  it('D19_Xterm_DsrCursorReplyUsesDataNotBinary_006', async () => {
    const term = new Terminal()
    const data = vi.fn<(value: string) => void>()
    const binary = vi.fn<(value: string) => void>()
    const dataSub = term.onData(data)
    const binarySub = term.onBinary(binary)

    await write(term, '\x1b[6n')

    expect(data).toHaveBeenCalled()
    expect(data.mock.calls.map(call => call[0])).toContain('\x1b[1;1R')
    expect(binary).not.toHaveBeenCalled()

    dataSub.dispose()
    binarySub.dispose()
    term.dispose()
  })
})


describe('D19 xterm 5.5 trusted data provenance', () => {
  it('D19_Xterm_UserInputSignalMarksOnlyTheImmediatelyFollowingData_007', async () => {
    const term = new Terminal()
    const user = vi.fn<(value: string) => void>()
    const protocol = vi.fn<(value: string) => void>()
    const binding = bindXterm55DataProvenance(term, {
      userData: user,
      protocolData: protocol,
    })

    term.input('user', true)
    term.input('protocol', false)

    expect(user).toHaveBeenCalledExactlyOnceWith('user')
    expect(protocol).toHaveBeenCalledExactlyOnceWith('protocol')

    binding.dispose()
    term.dispose()
  })

  it('D19_Xterm_DsrReplyIsTrustedProtocolWithoutByteGuessing_008', async () => {
    const term = new Terminal()
    const user = vi.fn<(value: string) => void>()
    const protocol = vi.fn<(value: string) => void>()
    const binding = bindXterm55DataProvenance(term, {
      userData: user,
      protocolData: protocol,
    })

    await write(term, '\x1b[5n')

    expect(protocol).toHaveBeenCalledExactlyOnceWith('\x1b[0n')
    expect(user).not.toHaveBeenCalled()

    binding.dispose()
    term.dispose()
  })

  it('D19_Xterm_UnsupportedPrivateShapeFailsClosed_009', () => {
    expect(() => bindXterm55DataProvenance(
      { onData: () => ({ dispose() {} }) },
      { userData: () => {}, protocolData: () => {} },
    )).toThrow('XTERM_55_PROVENANCE_UNAVAILABLE')
  })
})


it('D19_Xterm_PrivateProvenanceContractIsPinnedTo550_020', () => {
  const lockPath = fileURLToPath(new URL('../../package-lock.json', import.meta.url))
  const lock = JSON.parse(readFileSync(lockPath, 'utf8')) as {
    packages?: Record<string, { version?: string }>
  }
  expect(lock.packages?.['node_modules/@xterm/xterm']?.version).toBe('5.5.0')
})
