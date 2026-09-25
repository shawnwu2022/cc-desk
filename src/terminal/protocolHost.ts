import { classifyXtermBinary } from './hostAdapter'

export interface Disposable {
  dispose(): void
}

export interface BinaryProtocolTerminal {
  onBinary(handler: (data: string) => void): Disposable
}

export type ProtocolSender = (bytes: Uint8Array) => Promise<unknown>

/**
 * Bind the trustworthy xterm binary lane to the host protocol writer.
 *
 * This adapter deliberately does not inspect or subscribe to onData. xterm 5.5
 * does not expose reliable source provenance for onData, so D19 must not infer
 * terminal protocol replies from escape-looking text.
 *
 * Protocol sends are fire-and-forget from xterm's synchronous event callback.
 * A failed send is swallowed at this event boundary: retry/replay would be unsafe
 * because the host write outcome may already be partial or unknown.
 */
export function bindTerminalProtocolHost(
  terminal: BinaryProtocolTerminal,
  sendProtocol: ProtocolSender,
): Disposable {
  let disposed = false

  const subscription = terminal.onBinary(data => {
    if (disposed || data.length === 0) return

    const classified = classifyXtermBinary(data)
    if (classified.route !== 'protocol-bypass' || classified.bytes.byteLength === 0) return

    void sendProtocol(classified.bytes).catch(() => {
      // Never replay or reinterpret failed protocol bytes as user text.
    })
  })

  return {
    dispose() {
      if (disposed) return
      disposed = true
      subscription.dispose()
    },
  }
}
