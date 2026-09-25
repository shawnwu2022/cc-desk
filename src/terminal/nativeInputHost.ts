import type { U64String } from '@/types/cli'
import type {
  InputWriteReceipt,
  NativeInputFrame,
  ProtocolWriteReceipt,
  RunKey,
} from '@/types/terminal'
import {
  createInputIntentQueue,
  type InputQueueSnapshot,
  type InputRecovery,
  type InputReservation,
  type InputTarget,
} from './inputQueue'
import {
  bindTerminalProtocolHost,
  bindXterm55DataProvenance,
  type BinaryProtocolTerminal,
  type Disposable,
} from './protocolHost'

const encoder = new TextEncoder()

export interface NativeTerminalInputHostOptions {
  terminal: unknown
  runId: string
  generation: number
  currentTarget: () => InputTarget
  writeUser: (input: NativeInputFrame) => Promise<InputWriteReceipt>
  writeProtocol: (run: RunKey, bytes: Uint8Array) => Promise<ProtocolWriteReceipt>
  onInputError?: (error: unknown) => void
  onProtocolError?: (error: unknown) => void
}

export interface NativeTerminalInputHost extends Disposable {
  reservePaste(produce: () => Promise<Uint8Array>): InputReservation
  flush(): Promise<void>
  recover(inputSeq: U64String, action: InputRecovery): boolean
  snapshot(): InputQueueSnapshot
}

function exactConfirmedBytes(value: string, expected: number): boolean {
  return /^(0|[1-9][0-9]*)$/.test(value) && BigInt(value) === BigInt(expected)
}

function requireUserReceipt(
  input: NativeInputFrame,
  receipt: InputWriteReceipt,
): void {
  if (receipt.runId !== input.runId
      || receipt.generation !== input.generation
      || receipt.inputSeq !== input.inputSeq
      || receipt.modeEpoch !== input.modeEpoch
      || receipt.state !== 'host-written'
      || !exactConfirmedBytes(receipt.confirmedBytes, input.bytes.byteLength)) {
    throw new Error('INPUT_WRITE_NOT_CONFIRMED')
  }
}

function requireProtocolReceipt(
  bytes: Uint8Array,
  receipt: ProtocolWriteReceipt,
): void {
  if (receipt.state !== 'host-written'
      || !exactConfirmedBytes(receipt.confirmedBytes, bytes.byteLength)) {
    throw new Error('PROTOCOL_WRITE_NOT_CONFIRMED')
  }
}

/**
 * Compose D16 ordering/source semantics with D17 authenticated byte writers.
 *
 * xterm 5.5 public onData has no source bit. bindXterm55DataProvenance recovers
 * the exact internal onUserInput marker and fails closed if that locked private
 * shape is absent. User data enters the ordered queue; terminal-generated data
 * and raw onBinary bytes enter the protocol lane. Both lanes share D16's
 * dispatch critical section and therefore cannot interleave inside an active
 * user frame.
 */
export function createNativeTerminalInputHost(
  options: NativeTerminalInputHostOptions,
): NativeTerminalInputHost {
  let disposed = false

  const queue = createInputIntentQueue({
    runId: options.runId,
    generation: options.generation,
    currentTarget: options.currentTarget,
    send: async intent => {
      const input: NativeInputFrame = {
        runId: intent.runId,
        generation: intent.generation,
        inputSeq: intent.inputSeq,
        modeEpoch: intent.modeEpoch,
        bytes: intent.bytes,
      }
      const receipt = await options.writeUser(input)
      requireUserReceipt(input, receipt)
    },
    sendProtocol: async input => {
      const run: RunKey = {
        runId: input.runId,
        generation: input.generation,
      }
      const receipt = await options.writeProtocol(run, input.bytes)
      requireProtocolReceipt(input.bytes, receipt)
    },
  })

  const reportInputError = (error: unknown) => {
    if (!disposed) options.onInputError?.(error)
  }
  const reportProtocolError = (error: unknown) => {
    if (!disposed) options.onProtocolError?.(error)
  }

  const flush = (): Promise<void> => queue.flush().catch(error => {
    reportInputError(error)
    throw error
  })

  const provenance = bindXterm55DataProvenance(options.terminal, {
    userData(data) {
      if (disposed) return
      try {
        const target = options.currentTarget()
        queue.enqueue({
          source: 'user-text',
          modeEpoch: target.modeEpoch,
          bytes: encoder.encode(data),
        })
        void flush().catch(() => {})
      } catch (error) {
        reportInputError(error)
      }
    },
    protocolData(data) {
      if (disposed || data.length === 0) return
      void queue.sendProtocol(encoder.encode(data)).catch(reportProtocolError)
    },
  })

  const binary = bindTerminalProtocolHost(
    options.terminal as BinaryProtocolTerminal,
    bytes => queue.sendProtocol(bytes).catch(error => {
      reportProtocolError(error)
      throw error
    }),
  )

  return {
    reservePaste(produce) {
      if (disposed) throw new Error('INPUT_HOST_DISPOSED')
      const target = options.currentTarget()
      const reservation = queue.reserveAsync({
        source: 'user-paste',
        modeEpoch: target.modeEpoch,
        produce,
      })
      void reservation.settled.then(
        () => flush().catch(() => {}),
        error => reportInputError(error),
      )
      return reservation
    },

    flush,

    recover(inputSeq, action) {
      if (disposed) return false
      return queue.recover(inputSeq, action)
    },

    snapshot() {
      return queue.snapshot()
    },

    dispose() {
      if (disposed) return
      disposed = true
      binary.dispose()
      provenance.dispose()
    },
  }
}
