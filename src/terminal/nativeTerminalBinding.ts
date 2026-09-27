import { createTerminalHostProtocol } from './nativeHostProtocol'
import { createTerminalOutputTransport } from './transport'
import {
  bindXtermInputProvenance,
  type XtermProvenanceSource,
} from './xtermProvenance'
import type { InputTarget } from './inputQueue'
import type {
  InputWriteReceipt,
  NativeInputFrame,
  OutputAck,
  OutputFrame,
  ProtocolWriteReceipt,
  RunKey,
} from '@/types/terminal'

export interface NativeTerminalLike extends XtermProvenanceSource {
  write(data: string | Uint8Array, callback?: () => void): void
}

export interface NativeTerminalBindingOptions extends RunKey {
  term: NativeTerminalLike
  currentTarget: () => InputTarget
  writeUser: (frame: NativeInputFrame) => Promise<InputWriteReceipt>
  writeProtocol: (run: RunKey, bytes: Uint8Array) => Promise<ProtocolWriteReceipt>
  ackOutput: (ack: OutputAck) => Promise<unknown>
  onDegraded?: (reason: string) => void
}

export interface NativeTerminalBinding {
  acceptOutput(frame: OutputFrame): boolean
  drainInput(): Promise<void>
  dispose(): void
}

export function createNativeTerminalBinding(
  options: NativeTerminalBindingOptions,
): NativeTerminalBinding {
  const host = createTerminalHostProtocol({
    runId: options.runId,
    generation: options.generation,
    currentTarget: options.currentTarget,
    writeUser: options.writeUser,
    writeProtocol: options.writeProtocol,
  })

  const provenance = bindXtermInputProvenance(options.term, {
    user: async data => {
      const leave = host.beginUserEvent()
      let operation: Promise<void>
      try {
        // Provenance is a property of this synchronous xterm emission, not of
        // the potentially long host write. Release it before awaiting so a
        // protocol reply can queue behind the active frame without becoming
        // spuriously "conflicting" source context.
        operation = host.handleData(data)
      } finally {
        leave()
      }
      await operation
    },
    protocol: async data => {
      const leave = host.beginParserOutput()
      let operation: Promise<void>
      try {
        operation = host.handleData(data)
      } finally {
        leave()
      }
      await operation
    },
    binary: data => host.handleBinary(data),
  })

  const output = createTerminalOutputTransport({
    runId: options.runId,
    generation: options.generation,
    write(bytes, parsed) {
      options.term.write(bytes, parsed)
    },
    ack: options.ackOutput,
    onDegraded: options.onDegraded,
  })

  let disposed = false
  return {
    acceptOutput(frame) {
      if (disposed) return false
      return output.accept(frame)
    },

    drainInput() {
      return provenance.drain()
    },

    dispose() {
      if (disposed) return
      disposed = true
      provenance.dispose()
      output.dispose()
    },
  }
}
