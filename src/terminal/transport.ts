import type { OutputAck, OutputFrame } from '@/types/terminal'

const MAX_FRAME_BYTES = 16 * 1024
const MAX_U64 = (1n << 64n) - 1n

type WriteTerminalBytes = (bytes: Uint8Array, parsed: () => void) => void
type AckOutput = (ack: OutputAck) => Promise<unknown>

export interface TerminalOutputTransportOptions {
  runId: string
  generation: number
  write: WriteTerminalBytes
  ack: AckOutput
  onDegraded?: (reason: string) => void
}

export interface TerminalOutputTransport {
  accept(frame: OutputFrame): boolean
  dispose(): void
}

interface PendingFrame {
  end: bigint
  parsed: boolean
}

function parseU64(value: unknown): bigint | null {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value)) return null
  try {
    const parsed = BigInt(value)
    return parsed <= MAX_U64 ? parsed : null
  } catch {
    return null
  }
}

function validBytes(value: unknown): value is number[] {
  return Array.isArray(value)
    && value.length > 0
    && value.length <= MAX_FRAME_BYTES
    && value.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255)
}

function exactFrameShape(value: unknown): value is OutputFrame {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const keys = Object.keys(value as Record<string, unknown>).sort()
  return keys.join(',') === 'bytes,generation,offset,runId,streamEpoch'
}

export function createTerminalOutputTransport(
  options: TerminalOutputTransportOptions,
): TerminalOutputTransport {
  let disposed = false
  let degraded = false
  let streamEpoch: string | null = null
  let nextOffset = 0n
  let parsedThrough = 0n
  const pending: PendingFrame[] = []
  const pendingAcks: OutputAck[] = []
  let ackInFlight = false

  const fail = (reason: string): false => {
    if (!degraded && !disposed) {
      degraded = true
      options.onDegraded?.(reason)
    }
    return false
  }

  const pumpAcks = () => {
    if (ackInFlight || disposed || degraded) return
    const value = pendingAcks.shift()
    if (!value) return
    ackInFlight = true

    let request: Promise<unknown>
    try {
      request = options.ack(value)
    } catch {
      ackInFlight = false
      pendingAcks.length = 0
      fail('OUTPUT_ACK_FAILED')
      return
    }

    Promise.resolve(request).then(
      () => {
        ackInFlight = false
        pumpAcks()
      },
      () => {
        ackInFlight = false
        pendingAcks.length = 0
        fail('OUTPUT_ACK_FAILED')
      },
    )
  }

  const enqueueAck = (through: bigint) => {
    const epoch = streamEpoch
    if (!epoch || disposed || degraded) return
    pendingAcks.push({
      runId: options.runId,
      generation: options.generation,
      streamEpoch: epoch,
      throughOffset: through.toString(),
    })
    pumpAcks()
  }

  const parsed = (entry: PendingFrame) => {
    if (disposed || degraded || entry.parsed) return
    entry.parsed = true

    let advanced = false
    while (pending[0]?.parsed) {
      const head = pending.shift()!
      parsedThrough = head.end
      advanced = true
    }
    if (advanced) enqueueAck(parsedThrough)
  }

  return {
    accept(frame: OutputFrame): boolean {
      if (disposed || degraded) return false
      if (!exactFrameShape(frame)
          || frame.runId !== options.runId
          || frame.generation !== options.generation
          || !Number.isInteger(frame.generation)
          || frame.generation <= 0) {
        return fail('OUTPUT_FRAME_IDENTITY')
      }

      const epoch = parseU64(frame.streamEpoch)
      const offset = parseU64(frame.offset)
      if (epoch === null || epoch === 0n || offset === null || !validBytes(frame.bytes)) {
        return fail('INVALID_OUTPUT_FRAME')
      }
      if (streamEpoch === null) streamEpoch = frame.streamEpoch
      if (streamEpoch !== frame.streamEpoch) return fail('STALE_OUTPUT_STREAM')
      if (offset !== nextOffset) return fail('OUTPUT_OFFSET_GAP')

      const end = offset + BigInt(frame.bytes.length)
      if (end > MAX_U64) return fail('OUTPUT_OFFSET_EXHAUSTED')
      const entry: PendingFrame = { end, parsed: false }
      pending.push(entry)
      nextOffset = end

      try {
        options.write(Uint8Array.from(frame.bytes), () => parsed(entry))
      } catch {
        return fail('OUTPUT_WRITE_FAILED')
      }
      return true
    },

    dispose() {
      disposed = true
      pending.length = 0
      pendingAcks.length = 0
    },
  }
}
