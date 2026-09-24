export interface NativeRunRef {
  runId: string
  generation: number
}

export interface NativeOutputFrame extends NativeRunRef {
  streamEpoch: string
  offset: string
  bytes: number[]
}

export interface NativeOutputAck extends NativeRunRef {
  streamEpoch: string
  throughOffset: string
}

interface TerminalWriter {
  write(data: Uint8Array, callback?: () => void): void
}

export interface TerminalOutputConsumer {
  accept(frame: NativeOutputFrame): void
  dispose(): void
}

const MAX_FRAME_BYTES = 16 * 1024
const MAX_U64 = '18446744073709551615'

function canonicalU64(value: unknown): value is string {
  if (typeof value !== 'string' || !/^(0|[1-9]\d*)$/.test(value)) return false
  return value.length < MAX_U64.length || (value.length === MAX_U64.length && value <= MAX_U64)
}

function addDecimal(value: string, delta: number): string {
  if (!canonicalU64(value) || !Number.isSafeInteger(delta) || delta < 0) {
    throw new Error('OUTPUT_OFFSET_INVALID')
  }
  const digits = value.split('').map(Number)
  let carry = delta
  for (let index = digits.length - 1; index >= 0 && carry > 0; index--) {
    const add = carry % 10
    carry = Math.floor(carry / 10)
    const sum = digits[index] + add
    digits[index] = sum % 10
    carry += Math.floor(sum / 10)
  }
  while (carry > 0) {
    digits.unshift(carry % 10)
    carry = Math.floor(carry / 10)
  }
  const result = digits.join('')
  if (!canonicalU64(result)) throw new Error('OUTPUT_OFFSET_EXHAUSTED')
  return result
}

function validRun(run: NativeRunRef): boolean {
  return typeof run.runId === 'string'
    && run.runId.length > 0
    && run.runId.length <= 128
    && !/[\u0000-\u001f\u007f]/.test(run.runId)
    && Number.isInteger(run.generation)
    && run.generation >= 1
    && run.generation <= 0xffffffff
}

function validateBytes(bytes: unknown): asserts bytes is number[] {
  if (!Array.isArray(bytes)) throw new Error('OUTPUT_BYTES_INVALID')
  if (bytes.length === 0) throw new Error('OUTPUT_FRAME_EMPTY')
  if (bytes.length > MAX_FRAME_BYTES) throw new Error('OUTPUT_FRAME_TOO_LARGE')
  if (!bytes.every(value => Number.isInteger(value) && value >= 0 && value <= 255)) {
    throw new Error('OUTPUT_BYTES_INVALID')
  }
}

type PendingWrite = {
  end: string
  done: boolean
}

export function createTerminalOutputConsumer(
  run: NativeRunRef,
  term: TerminalWriter,
  sendAck: (ack: NativeOutputAck) => Promise<void>,
): TerminalOutputConsumer {
  if (!validRun(run)) throw new Error('OUTPUT_RUN_INVALID')
  const pinnedRun = { ...run }
  let closed = false
  let epoch: string | null = null
  let expectedOffset = '0'
  const pending: PendingWrite[] = []
  let ackTail = Promise.resolve()

  const flushAck = () => {
    if (closed) return
    let through: string | null = null
    while (pending[0]?.done) {
      through = pending.shift()!.end
    }
    if (through === null || epoch === null) return
    const ack: NativeOutputAck = {
      ...pinnedRun,
      streamEpoch: epoch,
      throughOffset: through,
    }
    // Preserve cumulative ACK ordering even if the bridge Promise is asynchronous.
    ackTail = ackTail
      .then(() => closed ? undefined : sendAck(ack))
      .catch(() => {
        // No retry: an uncertain/stale document cannot safely acknowledge again.
        closed = true
      })
  }

  return {
    accept(frame) {
      if (closed) throw new Error('OUTPUT_CONSUMER_CLOSED')
      if (!validRun(frame) || frame.runId !== pinnedRun.runId || frame.generation !== pinnedRun.generation) {
        throw new Error('OUTPUT_RUN_MISMATCH')
      }
      if (!canonicalU64(frame.streamEpoch)) throw new Error('OUTPUT_STREAM_EPOCH_INVALID')
      if (!canonicalU64(frame.offset)) throw new Error('OUTPUT_OFFSET_INVALID')
      validateBytes(frame.bytes)

      if (epoch === null) epoch = frame.streamEpoch
      else if (frame.streamEpoch !== epoch) throw new Error('OUTPUT_STREAM_EPOCH_MISMATCH')
      if (frame.offset !== expectedOffset) throw new Error('OUTPUT_OFFSET_GAP')

      const end = addDecimal(frame.offset, frame.bytes.length)
      expectedOffset = end
      const write: PendingWrite = { end, done: false }
      pending.push(write)
      // xterm accepts Uint8Array and invokes this callback only after this write
      // has been parsed. Channel receipt itself never advances credit.
      term.write(Uint8Array.from(frame.bytes), () => {
        if (closed) return
        write.done = true
        flushAck()
      })
    },
    dispose() {
      closed = true
      pending.length = 0
    },
  }
}
