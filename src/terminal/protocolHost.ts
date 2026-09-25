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


export interface Xterm55DataHandlers {
  userData(data: string): void
  protocolData(data: string): void
}

interface Xterm55CoreServiceLike {
  onUserInput(handler: () => void): Disposable
}

interface Xterm55CoreLike {
  coreService?: Xterm55CoreServiceLike
}

interface Xterm55PublicLike {
  onData?(handler: (data: string) => void): Disposable
  _core?: Xterm55CoreLike
}

/**
 * Recover the source bit that xterm 5.5 keeps internally but drops from its
 * public onData callback.
 *
 * Exact 5.5 CoreService semantics are:
 *   wasUserInput=true -> onUserInput.fire() -> onData.fire(data)
 *   wasUserInput=false ->                    onData.fire(data)
 *
 * The marker is consumed by the immediately following synchronous onData event.
 * If the locked private shape is unavailable, fail closed. Never classify by
 * escape-sequence contents.
 */
export function bindXterm55DataProvenance(
  terminal: unknown,
  handlers: Xterm55DataHandlers,
): Disposable {
  const candidate = terminal as Xterm55PublicLike
  const onData = candidate?.onData
  const onUserInput = candidate?._core?.coreService?.onUserInput
  if (typeof onData !== 'function' || typeof onUserInput !== 'function') {
    throw new Error('XTERM_55_PROVENANCE_UNAVAILABLE')
  }

  let disposed = false
  let nextDataIsUser = false

  const userInputSub = onUserInput.call(candidate._core!.coreService, () => {
    if (!disposed) nextDataIsUser = true
  })

  const dataSub = onData.call(candidate, data => {
    if (disposed) return
    const isUser = nextDataIsUser
    nextDataIsUser = false
    if (isUser) handlers.userData(data)
    else handlers.protocolData(data)
  })

  return {
    dispose() {
      if (disposed) return
      disposed = true
      nextDataIsUser = false
      dataSub.dispose()
      userInputSub.dispose()
    },
  }
}
