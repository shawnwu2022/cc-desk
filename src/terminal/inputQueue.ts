import type { U64String } from '@/types/cli'

const MAX_U64 = (1n << 64n) - 1n

export const INPUT_ACTION_BYTES_MAX = 8 * 1024 * 1024
export const INPUT_RUN_QUEUE_BYTES_MAX = 16 * 1024 * 1024

export type UserInputSource = 'user-text' | 'user-paste'
export type InputPauseReason =
  | 'producer-failed'
  | 'mode-changed'
  | 'target-changed'
  | 'budget-exceeded'
  | 'send-failed'
export type InputRecovery = 'continue' | 'cancel'

export interface InputTarget {
  runId: string
  generation: number
  modeEpoch: U64String
}

export interface InputIntent {
  runId: string
  generation: number
  inputSeq: U64String
  modeEpoch: U64String
  source: UserInputSource
  bytes: Uint8Array
}

export interface ProtocolInput {
  runId: string
  generation: number
  modeEpoch: U64String
  bytes: Uint8Array
}

export interface InputQueueSnapshot {
  state: 'open' | 'paused'
  queued: number
  queuedBytes: number
  blockedSeq?: U64String
  reason?: InputPauseReason
}

export interface InputReservation {
  inputSeq: U64String
  settled: Promise<void>
}

export interface InputIntentQueue {
  enqueue(input: {
    source: UserInputSource
    modeEpoch: U64String
    bytes: Uint8Array
  }): { inputSeq: U64String }
  reserveAsync(input: {
    source: UserInputSource
    modeEpoch: U64String
    produce: () => Promise<Uint8Array>
  }): InputReservation
  flush(): Promise<void>
  sendProtocol(bytes: Uint8Array): Promise<void>
  recover(inputSeq: U64String, action: InputRecovery): boolean
  snapshot(): InputQueueSnapshot
}

export interface InputIntentQueueOptions {
  runId: string
  generation: number
  currentTarget: () => InputTarget
  limits?: {
    actionBytes?: number
    queuedBytes?: number
  }
  send: (intent: InputIntent) => Promise<unknown> | unknown
  sendProtocol?: (input: ProtocolInput) => Promise<unknown> | unknown
}

type ItemState = 'pending' | 'ready' | 'failed'

interface QueueItem {
  inputSeq: U64String
  modeEpoch: U64String
  source: UserInputSource
  state: ItemState
  failure?: InputPauseReason
  bytes?: Uint8Array
}

interface Pause {
  blockedSeq: U64String
  reason: InputPauseReason
}

function parseU64(value: string, field: string): bigint {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new Error(`INVALID_${field}`)
  const parsed = BigInt(value)
  if (parsed > MAX_U64) throw new Error(`INVALID_${field}`)
  return parsed
}

function copyBytes(value: Uint8Array): Uint8Array {
  return new Uint8Array(value)
}

function validateLimit(value: number, field: string): number {
  if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`INVALID_${field}`)
  return value
}

export function createInputIntentQueue(options: InputIntentQueueOptions): InputIntentQueue {
  if (!options.runId || options.runId.includes('\0')) throw new Error('INVALID_RUN_ID')
  if (!Number.isInteger(options.generation) || options.generation <= 0) {
    throw new Error('INVALID_GENERATION')
  }

  const actionBytesMax = validateLimit(
    options.limits?.actionBytes ?? INPUT_ACTION_BYTES_MAX,
    'INPUT_ACTION_BYTES',
  )
  const queuedBytesMax = validateLimit(
    options.limits?.queuedBytes ?? INPUT_RUN_QUEUE_BYTES_MAX,
    'INPUT_QUEUE_BYTES',
  )
  let nextSeq = 1n
  let queuedBytes = 0
  const items: QueueItem[] = []
  let pause: Pause | undefined
  let flushInFlight: Promise<void> | null = null

  // FIFO mutex around each actual host dispatch. Protocol traffic can bypass an
  // unresolved clipboard reservation because no dispatch is active yet. If a
  // user frame is already being dispatched, protocol waits for that frame and
  // is ordered before any later user frame that has not started.
  let dispatchTail: Promise<void> = Promise.resolve()
  const dispatchExclusive = async <T>(operation: () => Promise<T>): Promise<T> => {
    const previous = dispatchTail
    let release!: () => void
    dispatchTail = new Promise<void>(resolve => {
      release = resolve
    })
    await previous
    try {
      return await operation()
    } finally {
      release()
    }
  }

  const allocateSeq = (): U64String => {
    if (nextSeq > MAX_U64) throw new Error('INPUT_SEQ_EXHAUSTED')
    const value = nextSeq.toString() as U64String
    nextSeq += 1n
    return value
  }

  const validateModeEpoch = (value: U64String) => {
    if (parseU64(value, 'MODE_EPOCH') === 0n) throw new Error('INVALID_MODE_EPOCH')
  }

  const currentTarget = (): InputTarget => {
    const target = options.currentTarget()
    if (!target.runId || target.runId.includes('\0')) throw new Error('INVALID_CURRENT_RUN_ID')
    if (!Number.isInteger(target.generation) || target.generation <= 0) {
      throw new Error('INVALID_CURRENT_GENERATION')
    }
    validateModeEpoch(target.modeEpoch)
    return target
  }

  const checkBytes = (value: Uint8Array) => {
    if (value.byteLength > actionBytesMax) throw new Error('INPUT_ACTION_TOO_LARGE')
    if (queuedBytes + value.byteLength > queuedBytesMax) throw new Error('INPUT_QUEUE_BUDGET')
  }

  const account = (value: Uint8Array) => {
    checkBytes(value)
    queuedBytes += value.byteLength
  }

  const releaseItemBytes = (item: QueueItem) => {
    if (!item.bytes) return
    queuedBytes -= item.bytes.byteLength
    if (queuedBytes < 0) {
      queuedBytes = 0
      throw new Error('INPUT_QUEUE_ACCOUNTING')
    }
  }

  const pauseFor = (item: QueueItem, reason: InputPauseReason) => {
    item.failure = reason
    pause = { blockedSeq: item.inputSeq, reason }
  }

  const pushReady = (
    source: UserInputSource,
    modeEpoch: U64String,
    bytes: Uint8Array,
  ): { inputSeq: U64String } => {
    validateModeEpoch(modeEpoch)
    checkBytes(bytes)
    const inputSeq = allocateSeq()
    const stored = copyBytes(bytes)
    queuedBytes += stored.byteLength
    items.push({
      inputSeq,
      modeEpoch,
      source,
      state: 'ready',
      bytes: stored,
    })
    return { inputSeq }
  }

  const runFlush = async () => {
    while (!pause) {
      const head = items[0]
      if (!head || head.state === 'pending') return
      if (head.state === 'failed') {
        pauseFor(head, head.failure ?? 'producer-failed')
        return
      }

      const bytes = head.bytes
      if (!bytes) throw new Error('INPUT_BYTES_MISSING')

      let gateFailure: InputPauseReason | undefined
      try {
        await dispatchExclusive(async () => {
          const target = currentTarget()
          if (target.runId !== options.runId || target.generation !== options.generation) {
            gateFailure = 'target-changed'
            return
          }
          if (target.modeEpoch !== head.modeEpoch) {
            gateFailure = 'mode-changed'
            return
          }
          await options.send({
            runId: options.runId,
            generation: options.generation,
            inputSeq: head.inputSeq,
            modeEpoch: head.modeEpoch,
            source: head.source,
            bytes: copyBytes(bytes),
          })
        })
      } catch {
        head.state = 'failed'
        pauseFor(head, 'send-failed')
        return
      }

      if (gateFailure) {
        pauseFor(head, gateFailure)
        return
      }
      items.shift()
      releaseItemBytes(head)
    }
  }

  const flush = (): Promise<void> => {
    if (flushInFlight) return flushInFlight
    flushInFlight = runFlush().finally(() => {
      flushInFlight = null
    })
    return flushInFlight
  }

  return {
    enqueue(input) {
      return pushReady(input.source, input.modeEpoch, input.bytes)
    },

    reserveAsync(input) {
      validateModeEpoch(input.modeEpoch)
      const inputSeq = allocateSeq()
      const item: QueueItem = {
        inputSeq,
        modeEpoch: input.modeEpoch,
        source: input.source,
        state: 'pending',
      }
      items.push(item)

      const settled = Promise.resolve()
        .then(() => input.produce())
        .then(
          value => {
            if (item.state !== 'pending') return
            if (item.source === 'user-paste' && value.byteLength === 0) {
              item.state = 'failed'
              item.failure = 'producer-failed'
              if (items[0] === item && !pause) {
                pauseFor(item, 'producer-failed')
              }
              return
            }
            try {
              account(value)
              item.bytes = copyBytes(value)
              item.state = 'ready'
            } catch (error) {
              item.state = 'failed'
              item.failure = error instanceof Error && (
                error.message === 'INPUT_ACTION_TOO_LARGE'
                || error.message === 'INPUT_QUEUE_BUDGET'
              )
                ? 'budget-exceeded'
                : 'producer-failed'
              if (items[0] === item && !pause) {
                pauseFor(item, item.failure)
              }
            }
          },
          () => {
            if (item.state !== 'pending') return
            item.state = 'failed'
            item.failure = 'producer-failed'
            if (items[0] === item && !pause) {
              pauseFor(item, 'producer-failed')
            }
          },
        )

      return { inputSeq, settled }
    },

    flush,

    async sendProtocol(bytes) {
      const send = options.sendProtocol
      if (!send) throw new Error('PROTOCOL_SENDER_UNAVAILABLE')
      await dispatchExclusive(async () => {
        const target = currentTarget()
        if (target.runId !== options.runId || target.generation !== options.generation) {
          throw new Error('STALE_INPUT_TARGET')
        }
        await send({
          runId: options.runId,
          generation: options.generation,
          modeEpoch: target.modeEpoch,
          bytes: copyBytes(bytes),
        })
      })
    },

    recover(inputSeq, action) {
      if (!pause || pause.blockedSeq !== inputSeq) return false
      const index = items.findIndex(item => item.inputSeq === inputSeq)
      if (index < 0) return false

      if (action === 'cancel') {
        items.length = 0
        queuedBytes = 0
        pause = undefined
        return true
      }

      // Continue explicitly discards a failed action and retains later intents.
      // Target/mode changes cannot be rebased silently; those require cancellation
      // and a new user action against the new generation/epoch.
      if (pause.reason === 'mode-changed' || pause.reason === 'target-changed') return false
      const [removed] = items.splice(index, 1)
      releaseItemBytes(removed)
      pause = undefined
      return true
    },

    snapshot() {
      return {
        state: pause ? 'paused' : 'open',
        queued: items.length,
        queuedBytes,
        ...(pause ? { blockedSeq: pause.blockedSeq, reason: pause.reason } : {}),
      }
    },
  }
}
