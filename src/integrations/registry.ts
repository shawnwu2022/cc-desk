export interface RunRef {
  runId: string
  generation: number
}

export type ObservationStatus = 'off' | 'connecting' | 'active' | 'unavailable'
export type ActivityStatus = 'unknown' | 'working' | 'waiting' | 'subagent_running' | 'idle' | 'waiting_permission' | 'waiting_input' | 'error'
export type OpencodeStatus = 'idle' | 'busy' | 'retry' | 'permission' | 'question' | 'error' | 'unknown'

export interface ObservationState {
  observation: ObservationStatus
  activity: ActivityStatus
}

type ObservationEventBase = RunRef & {
  eventId?: string
  sourceSequence?: string
}

export type ObservationEvent =
  | (ObservationEventBase & { kind: 'opencode-status'; status: OpencodeStatus })
  | (ObservationEventBase & { kind: 'off' })
  | (ObservationEventBase & { kind: 'connecting' })
  | (ObservationEventBase & { kind: 'timeout' })
  | (ObservationEventBase & { kind: 'working' })
  | (ObservationEventBase & { kind: 'waiting' })
  | (ObservationEventBase & { kind: 'unknown' })
  | (ObservationEventBase & { kind: 'subagent-started' | 'subagent-stopped'; agentId?: string })
  | (ObservationEventBase & { kind: 'turn-end' })
  | (ObservationEventBase & { kind: 'turn-idle-timeout' })

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

/** Match the authenticated backend's opaque UTF-8 metadata bound. */
export function validObservationAgentId(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && value.length <= 128
    && !/[\u0000-\u001f\u007f-\u009f]/.test(value) && new TextEncoder().encode(value).length <= 128
}

class RunObservationReducer implements ObservationReducer {
  private current: ObservationState = { observation: 'off', activity: 'unknown' }
  private readonly seen = new Set<string>()
  private lastSourceSequence: string | null = null
  private closed = false
  // Each ID denotes only its first observed invocation. Stops may precede
  // starts in delivery, or be vetoed by other hooks. Never revive a stopped ID
  // without a provider invocation epoch; never infer completion from this set.
  private readonly activeAgents = new Set<string>()
  private readonly retiredAgents = new Set<string>()

  private invalidateAgents(): void {
    for (const id of this.activeAgents) this.retiredAgents.add(id)
    this.activeAgents.clear()
  }

  constructor(private readonly run: RunRef) {}

  accept(event: ObservationEvent): void {
    if (!sameRun(this.run, event)) return

    if (event.kind === 'off') {
      this.closed = true
      this.current = { observation: 'off', activity: 'unknown' }
      this.seen.clear()
      this.lastSourceSequence = null
      this.activeAgents.clear()
      this.retiredAgents.clear()
      return
    }
    if (this.closed) return
    if (event.kind === 'connecting') {
      this.invalidateAgents()
      this.current = { observation: 'connecting', activity: 'unknown' }
      return
    }
    if (event.kind === 'timeout') {
      this.invalidateAgents()
      this.current = { observation: 'unavailable', activity: 'unknown' }
      return
    }
    if (event.kind === 'turn-idle-timeout') {
      // 粗粒度到达顺序证据的静默上界：只有宣称中的活动回落，waiting/unknown 不受影响。
      if (this.current.activity === 'working' || this.current.activity === 'subagent_running') {
        this.current.activity = 'idle'
      }
      return
    }

    if (!event.eventId || event.eventId.length > 128 || !/^[A-Za-z0-9._:-]+$/.test(event.eventId)) return
    if (event.kind === 'opencode-status') {
      const sequence=event.sourceSequence
      if (sequence === undefined || !canonicalU64(sequence) || this.lastSourceSequence !== null && compareU64(sequence,this.lastSourceSequence)<=0) return
      this.lastSourceSequence=sequence
      const activity: Record<OpencodeStatus,ActivityStatus>={idle:'idle',busy:'working',retry:'working',permission:'waiting_permission',question:'waiting_input',error:'error',unknown:'unknown'}
      this.current={observation:'active',activity:activity[event.status] ?? 'unknown'}
      return
    }
    if (this.seen.has(event.eventId)) return
    if (this.seen.size >= 1024) {
      this.invalidateAgents()
      this.current = { observation: 'unavailable', activity: 'unknown' }
      this.closed = true
      return
    }
    this.seen.add(event.eventId)

    this.current.observation = 'active'

    // Preserve the existing ordering fence whenever a source supplies one;
    // causal subagent evidence is only an alternative when no sequence exists.
    const sequence = event.sourceSequence
    if (sequence !== undefined) {
      if (typeof sequence !== 'string' || !canonicalU64(sequence)
        || this.lastSourceSequence !== null && compareU64(sequence, this.lastSourceSequence) <= 0) {
        this.invalidateAgents()
        this.current.activity = 'unknown'
        return
      }
      this.lastSourceSequence = sequence
    }

    if (event.kind === 'subagent-started' || event.kind === 'subagent-stopped') {
      if (!validObservationAgentId(event.agentId)) {
        this.invalidateAgents()
        if (this.current.activity !== 'waiting') this.current.activity = 'unknown'
        return
      }
      if (event.kind === 'subagent-stopped') {
        this.activeAgents.delete(event.agentId)
        this.retiredAgents.add(event.agentId)
        // turn 已结束（idle）时迟到的停止回执只退役 agent，不把行改回 unknown。
        if (this.current.activity === 'subagent_running') {
          this.current.activity = this.activeAgents.size > 0 ? 'subagent_running' : 'unknown'
        }
      } else if (!this.retiredAgents.has(event.agentId)) {
        this.activeAgents.add(event.agentId)
        // 等待用户是比子代理提示更高价值的信号，started 不覆盖它。
        if (this.current.activity !== 'waiting') this.current.activity = 'subagent_running'
      }
      return
    }

    if (sequence === undefined) {
      // 无可信顺序时的粗粒度证据：按到达顺序宣称 working/waiting/turn 结束，
      // 迟到乱序的误报由宿主静默超时（turn-idle-timeout）约束上界。
      if (event.kind === 'turn-end') {
        this.invalidateAgents()
        this.current.activity = 'idle'
        return
      }
      if (event.kind === 'unknown') this.invalidateAgents()
      if (event.kind === 'working') {
        this.current.activity = this.activeAgents.size > 0 ? 'subagent_running' : 'working'
      } else if (event.kind === 'waiting') {
        this.current.activity = 'waiting'
      } else {
        this.current.activity = this.activeAgents.size > 0 ? 'subagent_running' : 'unknown'
      }
      return
    }
    if (event.kind === 'turn-end') {
      this.invalidateAgents()
      this.current.activity = 'idle'
      return
    }
    if (event.kind === 'working' || event.kind === 'waiting') {
      this.current.activity = event.kind === 'working' && this.activeAgents.size > 0 ? 'subagent_running' : event.kind
    } else {
      this.invalidateAgents()
      this.current.activity = this.activeAgents.size > 0 ? 'subagent_running' : 'unknown'
    }
  }

  state(): ObservationState {
    return { ...this.current }
  }
}

/** Pure projection; deliberately has no terminal/process-control dependency. */
export function createObservationReducer(run: RunRef): ObservationReducer {
  if (!run.runId || run.runId.length > 128 || /[\u0000-\u001f\u007f]/.test(run.runId) ||
      !Number.isInteger(run.generation) || run.generation < 1 || run.generation > 0xffffffff) {
    throw new Error('INVALID_OBSERVER_RUN')
  }
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
      const existing = reducers.get(runKey(run))
      if (existing) return existing
      if (reducers.size >= 256) throw new Error('OBSERVER_CAPACITY')
      const reducer = createObservationReducer(run)
      reducers.set(runKey(run), reducer)
      return reducer
    },
    get(run) {
      return reducers.get(runKey(run))
    },
    detach(run) {
      reducers.get(runKey(run))?.accept({ kind: 'off', ...run })
      reducers.delete(runKey(run))
    },
  }
}

/** One-shot projection. Stateful streams must retain one reducer per binding. */
export function applyObservation(run: RunRef, event: ObservationEvent): ObservationState {
  const reducer = createObservationReducer(run)
  reducer.accept(event)
  return reducer.state()
}
