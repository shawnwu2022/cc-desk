export interface RunRef {
  runId: string
  generation: number
}

export type ObservationStatus = 'off' | 'connecting' | 'active' | 'unavailable'
export type ActivityStatus = 'unknown' | 'working' | 'waiting'

export interface ObservationState {
  observation: ObservationStatus
  activity: ActivityStatus
}

type ObservationEventBase = RunRef & {
  eventId?: string
  sourceSequence?: string
}

export type ObservationEvent =
  | (ObservationEventBase & { kind: 'off' })
  | (ObservationEventBase & { kind: 'connecting' })
  | (ObservationEventBase & { kind: 'timeout' })
  | (ObservationEventBase & { kind: 'working' })
  | (ObservationEventBase & { kind: 'waiting' })
  | (ObservationEventBase & { kind: 'unknown' })

export interface ObservationReducer {
  accept(event: ObservationEvent): void
  state(): ObservationState
}

const MAX_U64 = '18446744073709551615'

function sameRun(left: RunRef, right: RunRef): boolean {
  return left.runId === right.runId && left.generation === right.generation
}

function canonicalU64(value: string): boolean {
  if (!/^(0|[1-9]\d*)$/.test(value)) return false
  return value.length < MAX_U64.length || (value.length === MAX_U64.length && value <= MAX_U64)
}

function compareU64(left: string, right: string): number {
  if (left.length !== right.length) return left.length < right.length ? -1 : 1
  return left === right ? 0 : left < right ? -1 : 1
}

class RunObservationReducer implements ObservationReducer {
  private current: ObservationState = { observation: 'off', activity: 'unknown' }
  private readonly seen = new Set<string>()
  private lastSourceSequence: string | null = null

  constructor(private readonly run: RunRef) {}

  accept(event: ObservationEvent): void {
    if (!sameRun(this.run, event)) return

    if (event.kind === 'off') {
      this.current = { observation: 'off', activity: 'unknown' }
      this.seen.clear()
      this.lastSourceSequence = null
      return
    }
    if (event.kind === 'connecting') {
      this.current = { observation: 'connecting', activity: 'unknown' }
      return
    }
    if (event.kind === 'timeout') {
      this.current = { observation: 'unavailable', activity: 'unknown' }
      return
    }

    if (event.eventId) {
      if (this.seen.has(event.eventId)) return
      this.seen.add(event.eventId)
    }

    this.current.observation = 'active'

    const sequence = event.sourceSequence
    if (!sequence || !canonicalU64(sequence)) {
      this.current.activity = 'unknown'
      return
    }
    if (this.lastSourceSequence !== null && compareU64(sequence, this.lastSourceSequence) <= 0) {
      this.current.activity = 'unknown'
      return
    }
    this.lastSourceSequence = sequence
    this.current.activity =
      event.kind === 'working' ? 'working' : event.kind === 'waiting' ? 'waiting' : 'unknown'
  }

  state(): ObservationState {
    return { ...this.current }
  }
}

/**
 * The second argument exists only as a test guard: the reducer deliberately
 * has no process-control dependency, so observer failure can never stop a run.
 */
export function createObservationReducer(
  run: RunRef,
  forbiddenStopSpy?: () => unknown,
): ObservationReducer {
  void forbiddenStopSpy
  return new RunObservationReducer({ ...run })
}

export interface ObservationRegistry {
  attach(run: RunRef): ObservationReducer
  get(run: RunRef): ObservationReducer | undefined
  detach(run: RunRef): void
}

function runKey(run: RunRef): string {
  return `${run.runId}\u0000${run.generation}`
}

export function createObservationRegistry(): ObservationRegistry {
  const reducers = new Map<string, ObservationReducer>()
  return {
    attach(run) {
      const reducer = createObservationReducer(run)
      reducers.set(runKey(run), reducer)
      return reducer
    },
    get(run) {
      return reducers.get(runKey(run))
    },
    detach(run) {
      reducers.delete(runKey(run))
    },
  }
}
