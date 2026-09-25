import { describe, expect, it, vi } from 'vitest'
import { Terminal } from '@xterm/xterm'

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
