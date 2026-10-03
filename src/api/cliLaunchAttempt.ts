import type { LaunchRequest, U64String } from '@/types/cli'
import { parseU64, validateLaunchRequest } from '@/utils/nativeIdentity'

export type LaunchPhase =
  | 'reserved' | 'starting' | 'running' | 'failed'
  | 'cancelled' | 'indeterminate' | 'exited'

export interface LaunchStatus {
  instanceId: string
  requestId: string
  run: { runId: string; generation: number }
  revision: U64String
  phase: LaunchPhase
  failure: 'route-unavailable' | 'process-start-failed' | 'aborted' | 'outcome-unknown' | null
}

// This is a transport boundary, not authorization. Bind it to authenticated IPC
// only after D11's actual WebView document-lifetime integration is verified.
export interface LaunchAttemptTransport {
  start(request: LaunchRequest): Promise<unknown>
  status(requestId: string): Promise<unknown>
  cancel?(request: LaunchRequest): Promise<unknown>
}

export interface LaunchAttempt {
  start(): Promise<LaunchStatus>
  recover(): Promise<LaunchStatus>
  cancel(): Promise<LaunchStatus>
  latest(): LaunchStatus | undefined
}

function invalidResponse(): never {
  throw new Error('INVALID_LAUNCH_RESPONSE')
}

function exactRecord(value: unknown, fields: readonly string[]): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) invalidResponse()
  const keys = Object.keys(value)
  if (keys.length !== fields.length || keys.some((key) => !fields.includes(key))) invalidResponse()
  return value as Record<string, unknown>
}

function parseReceipt(value: unknown, request: LaunchRequest): LaunchStatus {
  try {
    const object = exactRecord(value, ['instanceId', 'requestId', 'run', 'revision', 'phase', 'failure'])
    const run = exactRecord(object.run, ['runId', 'generation'])
    if (
      typeof object.instanceId !== 'string' || object.instanceId.length === 0
      || object.instanceId.length > 128 || /[\u0000-\u001f\u007f-\u009f]/.test(object.instanceId)
      || object.requestId !== request.requestId || run.runId !== request.runId
      || run.generation !== request.generation || typeof object.revision !== 'string'
      || object.revision.length > 20
    ) invalidResponse()
    parseU64(object.revision)
    const phases: readonly unknown[] = [
      'reserved', 'starting', 'running', 'failed', 'cancelled', 'indeterminate', 'exited',
    ]
    if (!phases.includes(object.phase)) invalidResponse()
    if (object.phase === 'failed') {
      if (!['route-unavailable', 'process-start-failed', 'aborted'].includes(String(object.failure))) {
        invalidResponse()
      }
      if (typeof object.failure !== 'string') invalidResponse()
    } else if (object.phase === 'indeterminate') {
      if (object.failure !== 'outcome-unknown') invalidResponse()
    } else if (object.failure !== null) invalidResponse()
    return Object.freeze({
      instanceId: object.instanceId,
      requestId: request.requestId,
      run: Object.freeze({ runId: request.runId, generation: request.generation }),
      revision: object.revision,
      phase: object.phase as LaunchPhase,
      failure: object.failure as LaunchStatus['failure'],
    })
  } catch {
    // Never echo parser/getter errors or the untrusted response body.
    return invalidResponse()
  }
}

function canAdvance(previous: LaunchPhase, next: LaunchPhase): boolean {
  if (previous === next) return true
  switch (previous) {
    case 'reserved': return true
    case 'starting': return ['running', 'failed', 'indeterminate', 'exited'].includes(next)
    case 'running':
    case 'indeterminate': return next === 'exited'
    default: return false
  }
}

// Only structured backend diagnostics survive this boundary. They describe the
// rejection, not proof of process absence; cleanup still needs an exact receipt.
const START_DENIAL_CODES = new Set([
  'FORBIDDEN', 'DOCUMENT_BRIDGE_UNAVAILABLE', 'INVALID_REQUEST',
  'PROGRAM_TRUST_REQUIRED', 'PROGRAM_UNAVAILABLE', 'WORKING_DIRECTORY_UNAVAILABLE',
  'PROFILE_NOT_FOUND', 'PROFILE_CLI_MISMATCH', 'PROFILE_MISMATCH', 'REVISION_CONFLICT',
  'NATIVE_RUNTIME_NOT_READY', 'RUN_SUPERVISOR_STOPPING',
])

function startRejection(value: unknown): never {
  let code = 'LAUNCH_STATE_UNKNOWN'
  try {
    if (value && typeof value === 'object' && !(value instanceof Error)) {
      const fields = Object.getOwnPropertyDescriptors(value)
      const candidate = fields.code?.value
      if (Reflect.ownKeys(fields).every(key => ['code', 'retryable', 'field', 'index'].includes(String(key)))
        && Object.values(fields).every(field => 'value' in field)
        && typeof candidate === 'string' && START_DENIAL_CODES.has(candidate)
        && (fields.field === undefined || typeof fields.field.value === 'string' && fields.field.value.length <= 128)
        && (fields.index === undefined || Number.isSafeInteger(fields.index.value) && fields.index.value >= 0)
        && (fields.retryable === undefined || fields.retryable.value === false)) {
        code = candidate
      }
    }
  } catch { /* Hostile property/proxy errors cannot supply a diagnostic code. */ }
  throw new Error(code)
}

export function createLaunchAttempt(
  input: LaunchRequest,
  instanceId: string,
  transport: LaunchAttemptTransport,
): LaunchAttempt {
  if (!instanceId || instanceId.length > 128) throw new Error('INVALID_REQUEST:instanceId')
  // Validation creates owned objects/arrays; later UI edits cannot change this attempt.
  const request = validateLaunchRequest(input)
  if (request.action.kind === 'raw') Object.freeze(request.action.argv)
  Object.freeze(request.action)
  Object.freeze(request.extraArgs)
  Object.freeze(request)

  let started: Promise<LaunchStatus> | undefined
  let current: LaunchStatus | undefined

  function accept(value: unknown): LaunchStatus {
    const next = parseReceipt(value, request)
    if (next.instanceId !== instanceId) throw new Error('BACKEND_INSTANCE_CHANGED')
    if (current) {
      const nextRevision = parseU64(next.revision)
      const currentRevision = parseU64(current.revision)
      if (nextRevision < currentRevision) return current
      if (nextRevision === currentRevision) {
        if (next.phase !== current.phase || next.failure !== current.failure) invalidResponse()
        return current
      }
      if (!canAdvance(current.phase, next.phase)) invalidResponse()
      if (current.phase === next.phase && current.failure !== next.failure) invalidResponse()
    }
    current = next
    return next
  }

  function unknownOutcome(): never {
    // A rejected transport can still have executed the backend operation.
    throw new Error('LAUNCH_STATE_UNKNOWN')
  }

  return Object.freeze({
    start(): Promise<LaunchStatus> {
      // Assign before invoking the transport, including synchronous re-entry.
      if (!started) {
        started = Promise.resolve().then(() => transport.start(request)).then(accept, startRejection)
      }
      return started
    },
    recover(): Promise<LaunchStatus> {
      if (!started) return Promise.reject(new Error('LAUNCH_NOT_STARTED'))
      // Never resend start, even if status is missing or the backend restarted.
      return Promise.resolve().then(() => transport.status(request.requestId)).then(accept, unknownOutcome)
    },
    cancel(): Promise<LaunchStatus> {
      if (!started) return Promise.reject(new Error('LAUNCH_NOT_STARTED'))
      if (!transport.cancel) return Promise.reject(new Error('LAUNCH_CANCEL_UNAVAILABLE'))
      return Promise.resolve().then(() => transport.cancel!(request)).then(accept, unknownOutcome)
    },
    latest(): LaunchStatus | undefined {
      return current
    },
  })
}
