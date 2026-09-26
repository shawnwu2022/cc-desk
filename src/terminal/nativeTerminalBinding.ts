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
      try {
        await host.handleData(data)
      } finally {
        leave()
      }
    },
    protocol: async data => {
      const leave = host.beginParserOutput()
      try {
        await host.handleData(data)
      } finally {
        leave()
      }
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
