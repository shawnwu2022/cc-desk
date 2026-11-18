import { createTerminalHostProtocol } from './nativeHostProtocol'
import { createTerminalOutputTransport } from './transport'
import {
  bindXtermInputProvenance,
  type XtermProvenanceSource,
} from './xtermProvenance'
import type { InputReservation, InputTarget } from './inputQueue'
import type { InputWriteAdmission } from './inputAdmission'
import type {
  InputWriteReceipt,
  NativeInputFrame,
  OutputAck,
  OutputFrame,
  ProtocolWriteReceipt,
  ProtocolSource,
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
  writeProtocol: (run: RunKey, bytes: Uint8Array, source?: ProtocolSource) => Promise<ProtocolWriteReceipt>
  ackOutput: (ack: OutputAck) => Promise<unknown>
  onDegraded?: (reason: string) => void
  onActivity?: () => void
}

export interface NativeTerminalBinding {
  acceptOutput(frame: OutputFrame): boolean
  sendUserText(data: string, admission?: InputWriteAdmission): Promise<void>
  reserveUserPaste(produce: () => Promise<Uint8Array>): InputReservation
  drainInput(): Promise<void>
  dispose(): void
}

export function createNativeTerminalBinding(
  options: NativeTerminalBindingOptions,
): NativeTerminalBinding {
  let parserSource: ProtocolSource | undefined
  const host = createTerminalHostProtocol({
    runId: options.runId,
    generation: options.generation,
    currentTarget: options.currentTarget,
    writeUser: options.writeUser,
    writeProtocol: options.writeProtocol,
    currentProtocolSource: () => parserSource,
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
    binary: async data => {
      if (options.isUserInputAllowed?.() === false) return
      try { await host.handleBinary(data) }
      finally { completeUserOperation(data.length > 0) }
    },
  })

  const parserWrites: Array<{ bytes: Uint8Array; parsed: () => void; source: ProtocolSource }> = []
  let parserBusy = false
  const pumpParser = () => {
    if (disposed || parserBusy) return
    const next = parserWrites.shift()
    if (!next) return
    parserBusy = true
    parserSource = next.source
    options.term.write(next.bytes, () => {
      if (disposed) return
      parserBusy = false
      parserSource = undefined
      next.parsed()
      pumpParser()
    })
    options.onActivity?.()
  }
  const output = createTerminalOutputTransport({
    runId: options.runId,
    generation: options.generation,
    write(bytes, parsed, source) {
      // The existing bounded native output credit owns these queued bytes.
      // One active parse binds every synchronous reply to its ORIGINAL frame,
      // even if user IO delays the later protocol dispatch.
      parserWrites.push({ bytes, parsed, source })
      pumpParser()
    },
    ack: options.ackOutput,
    onDegraded: options.onDegraded,
  })

  return {
    acceptOutput(frame) {
      if (disposed) return false
      return output.accept(frame)
    },

    sendUserText(data, admission) {
      if (disposed) return Promise.reject(new Error('NATIVE_TERMINAL_DISPOSED'))
      if (options.isUserInputAllowed?.() === false) return Promise.reject(new Error('NATIVE_TERMINAL_HIDDEN'))
      return host.sendUserText(data, admission).then(() => completeUserOperation(data.length > 0), error => {
        completeUserOperation(false)
        throw error
      })
    },

    reserveUserPaste(produce) {
      if (disposed || options.isUserInputAllowed?.() === false) {
        return {
          inputSeq: '0',
          settled: Promise.reject(new Error('NATIVE_TERMINAL_DISPOSED')),
          cancel: () => false,
        }
      }
      let hasBytes = false
      const reserved = host.reserveUserPaste(async () => {
        const bytes = await produce(); hasBytes = bytes.length > 0; return bytes
      })
      return {
        inputSeq: reserved.inputSeq,
        settled: reserved.settled.then(() => completeUserOperation(hasBytes)),
        cancel: reserved.cancel,
      }
    },

    drainInput() {
      return provenance.drain()
    },

    dispose() {
      if (disposed) return
      disposed = true
      parserWrites.length = 0
      parserSource = undefined
      host.dispose()
      provenance.dispose()
      output.dispose()
    },
  }
}
