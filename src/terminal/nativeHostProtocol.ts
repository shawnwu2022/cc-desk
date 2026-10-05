import { classifyExplicitUserText, classifyXtermBinary } from './hostAdapter'
import {
  createInputIntentQueue,
  type InputQueueSnapshot,
  type InputTarget,
} from './inputQueue'
import type {
  InputWriteReceipt,
  NativeInputFrame,
  ProtocolWriteReceipt,
  RunKey,
} from '@/types/terminal'

export interface NativeTerminalHostProtocolOptions extends RunKey {
  currentTarget: () => InputTarget
  writeUser: (frame: NativeInputFrame) => Promise<InputWriteReceipt>
  writeProtocol: (run: RunKey, bytes: Uint8Array) => Promise<ProtocolWriteReceipt>
}

export interface NativeTerminalHostProtocol {
  beginUserEvent(): () => void
  beginParserOutput(): () => void
  handleData(data: string): Promise<void>
  handleBinary(data: string): Promise<void>
  sendUserText(data: string): Promise<void>
  reserveUserPaste(produce: () => Promise<Uint8Array>): { inputSeq: string; settled: Promise<void> }
  snapshot(): InputQueueSnapshot
}

const MAX_U64 = BigInt('18446744073709551615')

function parseConfirmedBytes(value: unknown): bigint {
  if (typeof value !== 'string' || value.length > 20 || !/^(0|[1-9][0-9]*)$/.test(value)) {
    throw new Error('INVALID_CONFIRMED_BYTES')
  }
  const parsed = BigInt(value)
  if (parsed > MAX_U64) throw new Error('INVALID_CONFIRMED_BYTES')
  return parsed
}

function requireExactHostWrite(
  receipt: { state: 'host-written' | 'partial-or-unknown'; confirmedBytes: string },
  expectedBytes: number,
): void {
  if (
    !receipt || receipt.state !== 'host-written'
    || parseConfirmedBytes(receipt.confirmedBytes) !== BigInt(expectedBytes)
  ) {
    throw new Error('NATIVE_INPUT_WRITE_INCOMPLETE')
  }
}

function scopedDepth(
  increment: () => void,
  decrement: () => void,
): () => void {
  increment()
  let active = true
  return () => {
    if (!active) return
    active = false
    decrement()
  }
}

/**
 * Provenance-safe xterm host adapter.
 *
 * xterm's public onData event does not identify whether bytes came from a user
 * action or from terminal protocol processing. D19 therefore refuses to infer a
 * source from the payload itself. Callers must bracket the synchronous operation
 * that can emit onData with either beginUserEvent() or beginParserOutput().
 *
 * onBinary is documented raw 8-bit terminal traffic and remains a protocol lane.
 */
export function createTerminalHostProtocol(
  options: NativeTerminalHostProtocolOptions,
): NativeTerminalHostProtocol {
  let userDepth = 0
  let parserDepth = 0

  const queue = createInputIntentQueue({
    runId: options.runId,
    generation: options.generation,
    currentTarget: options.currentTarget,
    send: async intent => {
      // Freeze the actual dispatch identity before handing the frame to the
      // asynchronous writer. A receipt cannot acknowledge a different intent.
      const { runId, generation, inputSeq, modeEpoch } = intent
      const expectedBytes = intent.bytes.byteLength
      const receipt = await options.writeUser({
        runId, generation, inputSeq, modeEpoch, bytes: intent.bytes,
      })
      if (!receipt || receipt.runId !== runId || receipt.generation !== generation
        || receipt.inputSeq !== inputSeq || receipt.modeEpoch !== modeEpoch) {
        throw new Error('NATIVE_INPUT_RECEIPT_MISMATCH')
      }
      requireExactHostWrite(receipt, expectedBytes)
    },
    sendProtocol: async input => {
      const expectedBytes = input.bytes.byteLength
      const receipt = await options.writeProtocol(
        { runId: input.runId, generation: input.generation },
        input.bytes,
      )
      requireExactHostWrite(receipt, expectedBytes)
    },
  })

  return {
    beginUserEvent() {
      return scopedDepth(
        () => { userDepth += 1 },
        () => { userDepth = Math.max(0, userDepth - 1) },
      )
    },

    beginParserOutput() {
      return scopedDepth(
        () => { parserDepth += 1 },
        () => { parserDepth = Math.max(0, parserDepth - 1) },
      )
    },

    async handleData(data) {
      const user = userDepth > 0
      const parser = parserDepth > 0

      if (user && parser) throw new Error('CONFLICTING_XTERM_DATA_SOURCE')
      if (!user && !parser) throw new Error('AMBIGUOUS_XTERM_DATA_SOURCE')

      if (parser) {
        await queue.sendProtocol(new TextEncoder().encode(data))
        return
      }

      const classified = classifyExplicitUserText(data)
      const target = options.currentTarget()
      queue.enqueue({
        source: 'user-text',
        modeEpoch: target.modeEpoch,
        bytes: classified.bytes,
      })
      await queue.flush()
    },

    async handleBinary(data) {
      const classified = classifyXtermBinary(data)
      await queue.sendProtocol(classified.bytes)
    },

    async sendUserText(data) {
      const classified = classifyExplicitUserText(data)
      const target = options.currentTarget()
      queue.enqueue({
        source: 'user-text',
        modeEpoch: target.modeEpoch,
        bytes: classified.bytes,
      })
      await queue.flush()
    },

    reserveUserPaste(produce) {
      const target = options.currentTarget()
      const reservation = queue.reserveAsync({
        source: 'user-paste',
        modeEpoch: target.modeEpoch,
        produce,
      })
      return {
        inputSeq: reservation.inputSeq,
        settled: reservation.settled.then(() => queue.flush()),
      }
    },

    snapshot() {
      return queue.snapshot()
    },
  }
}
