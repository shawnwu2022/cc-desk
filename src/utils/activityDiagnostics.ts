const operations = ['archive', 'restore', 'rename', 'delete', 'refresh', 'recycleList', 'recycleRecheck', 'recycleRestore'] as const
const providers = ['codex', 'claude', 'opencode', 'antigravity', 'unknown'] as const
const stages = ['ui', 'adapter', 'backend', 'provider', 'list', 'preview', 'confirmation', 'reconcile', 'recycle', 'history'] as const
const results = ['started', 'returned', 'applied', 'unchanged', 'ok', 'acknowledged', 'partial', 'failed', 'unknown', 'unsupported', 'cancelled', 'blocked', 'stale', 'skipped', 'sourceObserved', 'storedObserved', 'conflict'] as const

type Operation = typeof operations[number]
type Provider = typeof providers[number]
type Stage = typeof stages[number]
type Result = typeof results[number]

interface ActivityEvent {
  id: string
  operation: Operation
  cli: Provider
  stage: Stage
  result: Result
  ms: number
}

const limit = 256

export function createActivityDiagnostics() {
  let enabled = false
  let grant: string | undefined
  let epoch = 0
  let dropped = 0
  let next = 0
  const events: ActivityEvent[] = []

  function enable(token?: string) {
    if (enabled) return
    epoch++
    dropped = 0
    next = 0
    events.length = 0
    grant = token
    enabled = true
  }

  function disable() {
    enabled = false
    grant = undefined
  }

  function start(operation: Operation, cli: Provider) {
    if (!enabled || !operations.includes(operation) || !providers.includes(cli)) return
    let id: string
    let started: number
    try {
      id = Array.from(globalThis.crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join('')
      started = performance.now()
      if (!Number.isFinite(started)) return
    } catch { return }
    const ownedEpoch = epoch
    function record(stage: Stage, result: Result) {
      if (!enabled || ownedEpoch !== epoch || !stages.includes(stage) || !results.includes(result)) return
      try {
        const elapsed = performance.now() - started
        if (!Number.isFinite(elapsed)) return
        if (events.length === limit) dropped = Math.min(0xffffffff, dropped + 1)
        events[next] = { id, operation, cli, stage, result, ms: Math.min(0xffffffff, Math.max(0, Math.floor(elapsed))) }
        next = (next + 1) % limit
      } catch { /* 诊断失败不改变操作结果。 */ }
    }
    record('ui', 'started')
    async function observe<T>(stage: Stage, run: () => Promise<T>): Promise<T> {
      try {
        const value = await run()
        record(stage, stage === 'list' ? 'returned' : 'ok')
        return value
      } catch (error) {
        record(stage, 'failed')
        throw error
      }
    }
    function traceHeader() {
      return enabled && ownedEpoch === epoch && grant ? grant + ':' + id : undefined
    }
    return { record, observe, traceHeader }
  }

  function snapshot() {
    const offset = events.length === limit ? next : 0
    return { schemaVersion: 1, dropped, events: [...events.slice(offset), ...events.slice(0, offset)].map(event => ({ ...event })) }
  }

  return { enable, disable, start, snapshot, isEnabled: () => enabled }
}

export const activityDiagnostics = createActivityDiagnostics()

export type DiagnosticHeader = () => string | undefined
export type ActivityHandle = NonNullable<ReturnType<typeof activityDiagnostics.start>>
