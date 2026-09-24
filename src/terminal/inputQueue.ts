import type { U64String } from '@/types/cli'

const MAX_U64 = (1n << 64n) - 1n

export type UserInputSource = 'user-text' | 'user-paste'
export type InputPauseReason = 'producer-failed' | 'mode-changed' | 'send-failed'
export type InputRecovery = 'continue' | 'cancel'

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
  currentModeEpoch: () => U64String
  send: (intent: InputIntent) => Promise<unknown> | unknown
  sendProtocol?: (input: ProtocolInput) => Promise<unknown> | unknown
}

type ItemState = 'pending' | 'ready' | 'failed'

interface QueueItem {
  inputSeq: U64String
  modeEpoch: U64String
  source: UserInputSource
  state: ItemState
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

export function createInputIntentQueue(options: InputIntentQueueOptions): InputIntentQueue {
  if (!options.runId || options.runId.includes('\0')) throw new Error('INVALID_RUN_ID')
  if (!Number.isInteger(options.generation) || options.generation <= 0) {
    throw new Error('INVALID_GENERATION')
  }

  let nextSeq = 1n
  const items: QueueItem[] = []
  let pause: Pause | undefined
  let flushInFlight: Promise<void> | null = null

  // FIFO mutex around each actual host dispatch. A protocol event submitted
  // while a user frame is in flight queues immediately behind that frame; the
  // next user frame then queues behind the protocol event.
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

  const pushReady = (
    source: UserInputSource,
    modeEpoch: U64String,
    bytes: Uint8Array,
  ): { inputSeq: U64String } => {
    validateModeEpoch(modeEpoch)
    const inputSeq = allocateSeq()
    items.push({
      inputSeq,
      modeEpoch,
      source,
      state: 'ready',
      bytes: copyBytes(bytes),
    })
    return { inputSeq }
  }

  const runFlush = async () => {
    while (!pause) {
      const head = items[0]
      if (!head || head.state === 'pending') return
      if (head.state === 'failed') {
        pause = { blockedSeq: head.inputSeq, reason: 'producer-failed' }
        return
      }
      if (options.currentModeEpoch() !== head.modeEpoch) {
        pause = { blockedSeq: head.inputSeq, reason: 'mode-changed' }
        return
      }

      const bytes = head.bytes
      if (!bytes) throw new Error('INPUT_BYTES_MISSING')
      try {
        await dispatchExclusive(async () => {
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
        pause = { blockedSeq: head.inputSeq, reason: 'send-failed' }
        return
      }
      items.shift()
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
            item.bytes = copyBytes(value)
            item.state = 'ready'
          },
          () => {
            if (item.state !== 'pending') return
            item.state = 'failed'
            if (items[0] === item && !pause) {
              pause = { blockedSeq: item.inputSeq, reason: 'producer-failed' }
            }
          },
        )

      return { inputSeq, settled }
    },

    flush,

    async sendProtocol(bytes) {
      const send = options.sendProtocol
      if (!send) throw new Error('PROTOCOL_SENDER_UNAVAILABLE')
      const modeEpoch = options.currentModeEpoch()
      validateModeEpoch(modeEpoch)
      await dispatchExclusive(async () => {
        await send({
          runId: options.runId,
          generation: options.generation,
          modeEpoch,
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
        pause = undefined
        return true
      }

      // Continue is explicit permission to discard the failed action and retain
      // later user intents. A mode change is different: silently rebasing bytes
      // captured under an older terminal mode would violate the epoch contract.
      if (pause.reason === 'mode-changed') return false
      items.splice(index, 1)
      pause = undefined
      return true
    },

    snapshot() {
      return {
        state: pause ? 'paused' : 'open',
        queued: items.length,
        ...(pause ? { blockedSeq: pause.blockedSeq, reason: pause.reason } : {}),
      }
    },
  }
}
