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
  isUserInputAllowed?: () => boolean
  writeUser: (frame: NativeInputFrame) => Promise<InputWriteReceipt>
  writeProtocol: (run: RunKey, bytes: Uint8Array) => Promise<ProtocolWriteReceipt>
  ackOutput: (ack: OutputAck) => Promise<unknown>
  onDegraded?: (reason: string) => void
  onActivity?: () => void
}

export interface NativeTerminalBinding {
  acceptOutput(frame: OutputFrame): boolean
  sendUserText(data: string): Promise<void>
  reserveUserPaste(produce: () => Promise<Uint8Array>): { inputSeq: string; settled: Promise<void> }
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

  let disposed = false
  let inputPauseReported = false
  const completeUserOperation = (hasBytes: boolean) => {
    if (disposed) return
    // flush() resolves when the queue pauses, so promise rejection alone cannot
    // report failed input. Keep feedback bounded to the first pause of this run.
    if (host.snapshot().state === 'paused') {
      if (!inputPauseReported) {
        inputPauseReported = true
        options.onDegraded?.('NATIVE_INPUT_PAUSED')
      }
      return
    }
    if (hasBytes) options.onActivity?.()
  }

  const provenance = bindXtermInputProvenance(options.term, {
    user: async data => {
      if (options.isUserInputAllowed?.() === false) return
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
      completeUserOperation(data.length > 0)
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
      options.onActivity?.()
    },
    ack: options.ackOutput,
    onDegraded: options.onDegraded,
  })

  return {
    acceptOutput(frame) {
      if (disposed) return false
      return output.accept(frame)
    },

    sendUserText(data) {
      if (disposed) return Promise.reject(new Error('NATIVE_TERMINAL_DISPOSED'))
      if (options.isUserInputAllowed?.() === false) return Promise.reject(new Error('NATIVE_TERMINAL_HIDDEN'))
      return host.sendUserText(data).then(() => completeUserOperation(data.length > 0))
    },

    reserveUserPaste(produce) {
      if (disposed || options.isUserInputAllowed?.() === false) {
        return {
          inputSeq: '0',
          settled: Promise.reject(new Error('NATIVE_TERMINAL_DISPOSED')),
        }
      }
      let hasBytes = false
      const reserved = host.reserveUserPaste(async () => {
        const bytes = await produce(); hasBytes = bytes.length > 0; return bytes
      })
      return { inputSeq: reserved.inputSeq, settled: reserved.settled.then(() => completeUserOperation(hasBytes)) }
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
