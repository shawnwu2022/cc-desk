import type { U64String } from '@/types/cli'
import type { InputWriteAdmission } from './inputAdmission'

const MAX_U64 = BigInt('18446744073709551615')

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
  cancel(): boolean
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
  enqueueConfirmed(input: { source: UserInputSource; modeEpoch: U64String; bytes: Uint8Array; admission?: InputWriteAdmission }): { inputSeq: U64String; settled: Promise<void> }
  dispose(): void
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

// cancelled：已被 recover('cancel') 清出队列；producer 迟到的 settle 不得为其入账
type ItemState = 'pending' | 'ready' | 'failed' | 'cancelled'

interface QueueItem {
  inputSeq: U64String
  modeEpoch: U64String
  source: UserInputSource
  state: ItemState
  failure?: InputPauseReason
  bytes?: Uint8Array
  confirmation?: { resolve: () => void; reject: (error: Error) => void; admission?: InputWriteAdmission; dispatched: boolean; settled: boolean; detach: () => void }
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
  let nextSeq = BigInt(1)
  let queuedBytes = 0
  const items: QueueItem[] = []
  let pause: Pause | undefined
  let flushInFlight: Promise<void> | null = null
  let disposed = false

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
    nextSeq += BigInt(1)
    return value
  }

  const validateModeEpoch = (value: U64String) => {
    if (parseU64(value, 'MODE_EPOCH') === BigInt(0)) throw new Error('INVALID_MODE_EPOCH')
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

  const finishConfirmation = (item: QueueItem, error?: Error) => {
    const receipt = item.confirmation
    if (!receipt || receipt.settled) return
    receipt.settled = true
    receipt.detach()
    if (error) receipt.reject(error)
    else receipt.resolve()
  }
  const pauseFor = (item: QueueItem, reason: InputPauseReason) => {
    item.failure = reason
    pause = { blockedSeq: item.inputSeq, reason }
    // 保留阻塞项与既有恢复边界，拒绝其后未发送的 confirmed 输入，绝不重放。
    for (const queued of [...items]) {
      if (!queued.confirmation) continue
      finishConfirmation(queued, new Error('NATIVE_INPUT_PAUSED'))
      if (queued !== item && !queued.confirmation.dispatched) {
        queued.state = 'cancelled'
        items.splice(items.indexOf(queued), 1)
        releaseItemBytes(queued)
      }
    }
  }

  const pushReady = (
    source: UserInputSource,
    modeEpoch: U64String,
    bytes: Uint8Array,
  ): { inputSeq: U64String } => {
    if (disposed) throw new Error('NATIVE_TERMINAL_DISPOSED')
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
    while (!pause && !disposed) {
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
          if (disposed || head.state === 'cancelled') return
          const target = currentTarget()
          if (target.runId !== options.runId || target.generation !== options.generation) {
            gateFailure = 'target-changed'
            return
          }
          if (target.modeEpoch !== head.modeEpoch) {
            gateFailure = 'mode-changed'
            return
          }
          if (head.confirmation) {
            try { head.confirmation.admission?.beforeDispatch?.() }
            catch {
              head.state = 'cancelled'
              finishConfirmation(head, new Error('STALE_SESSION_ATTEMPT'))
              return
            }
            head.confirmation.dispatched = true
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
        if (disposed) return
        head.state = 'failed'
        pauseFor(head, 'send-failed')
        return
      }

      if (disposed) return
      if (head.state === 'cancelled') {
        if (items[0] === head) { items.shift(); releaseItemBytes(head) }
        continue
      }
      if (gateFailure) {
        pauseFor(head, gateFailure)
        return
      }
      items.shift()
      releaseItemBytes(head)
      finishConfirmation(head)
    }
  }

  const flush = (): Promise<void> => {
    if (flushInFlight) return flushInFlight
    flushInFlight = runFlush().finally(() => {
      flushInFlight = null
      if (!disposed && !pause && items[0]?.state === 'ready') void flush()
    })
    return flushInFlight
  }

  return {
    enqueue(input) {
      return pushReady(input.source, input.modeEpoch, input.bytes)
    },

    enqueueConfirmed(input) {
      if (disposed) throw new Error('NATIVE_TERMINAL_DISPOSED')
      if (pause) throw new Error('NATIVE_INPUT_PAUSED')
      if (input.admission?.signal?.aborted) throw new Error('STALE_SESSION_ATTEMPT')
      const allocated = pushReady(input.source, input.modeEpoch, input.bytes)
      const item = items[items.length - 1]
      let resolve!: () => void, reject!: (error: Error) => void
      const settled = new Promise<void>((res, rej) => { resolve = res; reject = rej })
      const signal = input.admission?.signal
      const cancel = () => {
        if (item.confirmation?.dispatched || item.confirmation?.settled) return
        item.state = 'cancelled'
        const index = items.indexOf(item)
        if (index >= 0) { items.splice(index, 1); releaseItemBytes(item) }
        finishConfirmation(item, new Error('STALE_SESSION_ATTEMPT'))
        void flush()
      }
      item.confirmation = { resolve, reject, admission: input.admission, dispatched: false, settled: false, detach: () => signal?.removeEventListener('abort', cancel) }
      signal?.addEventListener('abort', cancel, { once: true })
      void flush()
      return { ...allocated, settled }
    },

    dispose() {
      if (disposed) return
      disposed = true
      for (const item of items) { item.state = 'cancelled'; finishConfirmation(item, new Error('NATIVE_TERMINAL_DISPOSED')) }
      items.length = 0
      queuedBytes = 0
    },

    reserveAsync(input) {
      if (disposed) throw new Error('NATIVE_TERMINAL_DISPOSED')
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

      return {
        inputSeq,
        settled,
        cancel() {
          // 仅撤销尚未产出字节的单个 intent；ready/dispatch/失败状态保留原冻结边界。
          if (item.state !== 'pending') return false
          const index = items.indexOf(item)
          if (index < 0) return false
          item.state = 'cancelled'
          items.splice(index, 1)
          void flush()
          return true
        },
      }
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
        // 标记 cancelled：迟到的 producer settle 据此跳过入账，否则字节会记到
        // 已不在 items 的条目上且永不释放，累计触发误报 INPUT_QUEUE_BUDGET。
        for (const item of items) item.state = 'cancelled'
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
