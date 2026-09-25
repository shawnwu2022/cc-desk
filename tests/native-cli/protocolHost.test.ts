import { describe, expect, it, vi } from 'vitest'
import { bindTerminalProtocolHost } from '@/terminal/protocolHost'

type Disposable = { dispose(): void }

class FakeTerminal {
  private binary?: (data: string) => void

  onBinary(handler: (data: string) => void): Disposable {
    this.binary = handler
    return {
      dispose: () => {
        if (this.binary === handler) this.binary = undefined
      },
    }
  }

  emitBinary(data: string): void {
    this.binary?.(data)
  }
}

describe('D19 terminal protocol host', () => {
  it('D19_Protocol_OnBinaryPreservesRawEightBitBytes_001', async () => {
    const term = new FakeTerminal()
    const sendProtocol = vi.fn(async (_bytes: Uint8Array) => {})
    const binding = bindTerminalProtocolHost(term, sendProtocol)

    term.emitBinary(String.fromCharCode(0x00, 0x1b, 0x80, 0xff))

    await vi.waitFor(() => expect(sendProtocol).toHaveBeenCalledOnce())
    expect(Array.from(sendProtocol.mock.calls[0][0])).toEqual([0x00, 0x1b, 0x80, 0xff])

    binding.dispose()
  })

  it('D19_Protocol_DisposeStopsFurtherProtocolWrites_002', async () => {
    const term = new FakeTerminal()
    const sendProtocol = vi.fn(async (_bytes: Uint8Array) => {})
    const binding = bindTerminalProtocolHost(term, sendProtocol)

    term.emitBinary('A')
    await vi.waitFor(() => expect(sendProtocol).toHaveBeenCalledOnce())

    binding.dispose()
    term.emitBinary('B')
    await Promise.resolve()

    expect(sendProtocol).toHaveBeenCalledTimes(1)
  })

  it('D19_Protocol_SendFailureNeverFallsBackToTextOrReplay_003', async () => {
    const term = new FakeTerminal()
    const sendProtocol = vi.fn(async (_bytes: Uint8Array) => {
      throw new Error('writer failed')
    })
    const binding = bindTerminalProtocolHost(term, sendProtocol)

    term.emitBinary(String.fromCharCode(0x1b, 0x5b, 0x36, 0x6e))
    await vi.waitFor(() => expect(sendProtocol).toHaveBeenCalledOnce())
    await Promise.resolve()

    expect(sendProtocol).toHaveBeenCalledTimes(1)
    binding.dispose()
  })

  it('D19_Protocol_EmptyBinaryEventIsZeroWrite_004', async () => {
    const term = new FakeTerminal()
    const sendProtocol = vi.fn(async (_bytes: Uint8Array) => {})
    const binding = bindTerminalProtocolHost(term, sendProtocol)

    term.emitBinary('')
    await Promise.resolve()

    expect(sendProtocol).not.toHaveBeenCalled()
    binding.dispose()
  })
})
