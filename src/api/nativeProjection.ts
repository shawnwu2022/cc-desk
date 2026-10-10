import type { HistoryReadFailure, ProjectionResult, ReadRequest, ResourceItem, ResourceKind, ScopeTarget, SourceRef } from '@/types/nativeProjection'
export interface ProjectionBridge { readonly instanceId: string; invoke(command: string, payload: unknown): Promise<unknown> }
export interface ProjectionClient { scope(target: ScopeTarget): Promise<SourceRef>; read(request: ReadRequest): Promise<ProjectionResult> }
const kinds: ResourceKind[] = ['history', 'messages', 'search', 'config', 'mcp', 'skills', 'agents', 'plugins', 'instructions']
const diagnosticStages = ['frontend-bridge', 'frontend-serialization',
  'scope-invoke', 'scope-document-admission', 'scope-request-decode', 'scope-request-validation', 'scope-profile-validation',
  'scope-environment', 'scope-project-registration', 'scope-source-selection', 'scope-source-root', 'scope-capability', 'scope-task', 'scope-response-admission', 'scope-response-validation',
  'read-invoke', 'read-document-admission', 'read-request-decode', 'read-request-validation', 'read-capability', 'read-source-enumeration', 'read-task', 'read-response-admission', 'read-response-validation'] as const
export type ProjectionStage = typeof diagnosticStages[number]
export function projectionErrorStage(value: unknown): ProjectionStage | undefined {
  const stage = value && typeof value === 'object' && Object.prototype.hasOwnProperty.call(value, 'stage') ? (value as { stage: unknown }).stage : undefined
  return diagnosticStages.includes(stage as ProjectionStage) ? stage as ProjectionStage : undefined
}
const diagnosticCodes = new Set(['INVALID_PROJECTION', 'RAW_BODY_REQUIRED', 'REQUEST_TOO_LARGE', 'CLOCK_UNAVAILABLE',
  'WORKSPACE_INVALID', 'PROFILE_INVALID', 'ENV_SOURCE_MISSING', 'LEGACY_INVALID', 'LEGACY_READ_FAILED', 'LEGACY_TOO_LARGE',
  'STORAGE_IO', 'STORAGE_BUSY', 'WORKSPACE_TOO_LARGE', 'UNSUPPORTED_SCHEMA', 'UNSAFE_WORKSPACE_PATH', 'INVALID_PATH',
  'RUN_NOT_FOUND', 'RUN_NOT_READY', 'STALE_GENERATION'])
export function projectionFailure(value: unknown, fallback: ProjectionStage): Error & { code: string; stage: ProjectionStage; retryable: boolean } {
  const code = projectionErrorCode(value)
  // Construct a fresh bounded error; never retain raw fields, values or parser messages.
  return Object.assign(new Error(code), { code, stage: projectionErrorStage(value) ?? fallback, retryable: code === 'SOURCE_BUSY' })
}
const reasons = new Set(['SCOPE_UNKNOWN', 'SCOPE_STALE', 'SCOPE_REVOKED', 'SCOPE_CAPACITY', 'SCOPE_EPOCH_EXHAUSTED', 'SCOPE_UNAVAILABLE',
  'SOURCE_UNSUPPORTED', 'SOURCE_INVALID', 'SOURCE_INVALID_TEXT', 'SOURCE_PATH_REJECTED', 'SOURCE_CHANGED', 'SOURCE_NOT_REGULAR',
  'SOURCE_TOO_LARGE', 'SOURCE_TOO_MANY_ENTRIES', 'SOURCE_BUDGET_EXCEEDED', 'SOURCE_READ_FAILED', 'SOURCE_READ_FORBIDDEN',
  'SOURCE_RESPONSE_TOO_LARGE', 'SOURCE_AMBIGUOUS', 'SOURCE_BUSY', 'SOURCE_TASK_FAILED', 'PROJECT_NOT_FOUND', 'PROFILE_NOT_FOUND',
  'PROJECT_IDENTITY_CHANGED', 'REVISION_CONFLICT', 'FORBIDDEN', 'INVALID_REQUEST', 'DOCUMENT_BRIDGE_UNAVAILABLE', 'BACKEND_INSTANCE_CHANGED'])
function invalid(): never { throw new Error('INVALID_PROJECTION') }
function object(value: unknown, required: string[], optional: string[] = []): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalid()
  const r = value as Record<string, unknown>
  if (required.some(k => !Object.prototype.hasOwnProperty.call(r, k)) || Object.keys(r).some(k => !required.includes(k) && !optional.includes(k))) return invalid()
  return r
}
function text(v: unknown, maximum = 4096, empty = true): string {
  if (typeof v !== 'string' || (!empty && !v) || v.length > maximum || v.includes('\0') || /[\uD800-\uDFFF]/u.test(v)) return invalid()
  if (new TextEncoder().encode(v).length > maximum) return invalid()
  return v
}
function id(v: unknown): string { const s = text(v, 128, false); if (!/^[A-Za-z0-9_-]+$/.test(s)) return invalid(); return s }
function u64(v: unknown): string {
  const s = text(v, 20, false)
  if (!/^(0|[1-9][0-9]*)$/.test(s) || BigInt(s) > BigInt('18446744073709551615')) return invalid()
  return s
}
function integer(v: unknown, min: number, max: number): number {
  if (typeof v !== 'number' || !Number.isInteger(v) || v < min || v > max) return invalid(); return v
}
function bool(v: unknown): boolean { if (typeof v !== 'boolean') return invalid(); return v }
function optionalText(v: unknown, max = 4096): string | null { return v === null ? null : text(v, max) }
function optionalBool(v: unknown): boolean | null { return v === null ? null : bool(v) }
function target(v: unknown): ScopeTarget {
  const r = v as Record<string, unknown> | null
  if (r?.kind === 'profile') {
    object(r, ['kind', 'profileId', 'expectedProfileRevision'], ['projectId'])
    return { kind: 'profile', profileId: id(r.profileId), expectedProfileRevision: u64(r.expectedProfileRevision), projectId: r.projectId == null ? null : id(r.projectId) }
  }
  const q = object(v, ['kind', 'runId', 'generation']); if (q.kind !== 'run') return invalid()
  return { kind: 'run', runId: id(q.runId), generation: integer(q.generation, 1, 4294967295) }
}
function source(v: unknown): SourceRef {
  const r = object(v, ['scopeId', 'instanceId', 'cli', 'sourceRootKey', 'identityEpoch', 'profileId', 'profileRevision', 'target', 'basis'])
  if (r.cli !== 'claude' && r.cli !== 'codex') return invalid()
  if (r.basis !== 'configured-profile' && r.basis !== 'launch-environment') return invalid()
  const s: SourceRef = { scopeId: id(r.scopeId), instanceId: id(r.instanceId), cli: r.cli, sourceRootKey: text(r.sourceRootKey, 4096, false),
    identityEpoch: u64(r.identityEpoch), profileId: id(r.profileId), profileRevision: u64(r.profileRevision), target: target(r.target), basis: r.basis }
  if (s.identityEpoch === '0') return invalid()
  if (s.target.kind === 'profile' && (s.basis !== 'configured-profile' || s.profileId !== s.target.profileId || s.profileRevision !== s.target.expectedProfileRevision)) return invalid()
  if (s.target.kind === 'run' && s.basis !== 'launch-environment') return invalid()
  return s
}
function kind(v: unknown): ResourceKind { if (!kinds.includes(v as ResourceKind)) return invalid(); return v as ResourceKind }
function readRequest(v: ReadRequest): Required<ReadRequest> {
  const r = object(v, ['source', 'resourceKind', 'requestEpoch'], ['query', 'sessionId', 'limit', 'offset'])
  const k = kind(r.resourceKind)
  const q = r.query == null ? null : text(r.query, 1024, false)
  const sessionId = r.sessionId == null ? null : text(r.sessionId, 256, false)
  if ((k === 'search') !== (q !== null) || q !== null && !q.trim()) return invalid()
  if ((k === 'messages') !== (sessionId !== null) || sessionId !== null && /\p{Cc}/u.test(sessionId)) return invalid()
  return { source: source(r.source), resourceKind: k, requestEpoch: u64(r.requestEpoch), query: q, sessionId,
    limit: integer(r.limit ?? 100, 1, 200), offset: integer(r.offset ?? 0, 0, 1_000_000) }
}
function item(v: unknown, request: ReadRequest): ResourceItem {
  const r = v as Record<string, unknown> | null
  if (!r || typeof r.type !== 'string') return invalid()
  const allowed: Record<ResourceKind, string> = { history: 'session', messages: 'message', search: 'message', config: 'setting', mcp: 'mcp', skills: 'skill', agents: 'agent', plugins: 'plugin', instructions: 'document' }
  if (r.type !== allowed[request.resourceKind]) return invalid()
  if (r.type === 'session' || r.type === 'message') {
    const common = ['type', 'sessionKey', 'nativeSessionId', 'truncated']
    object(r, [...common, ...(r.type === 'session' ? ['title', 'cwd', 'updatedAt'] : ['role', 'text'])])
    const nativeSessionId = text(r.nativeSessionId, 256, false)
    const sessionKey = text(r.sessionKey, 8192, false)
    if (/\p{Cc}/u.test(nativeSessionId) || sessionKey !== JSON.stringify(['local', request.source.cli, request.source.sourceRootKey, nativeSessionId])) return invalid()
    const truncated = bool(r.truncated)
    if (r.type === 'session') return { type: 'session', sessionKey, nativeSessionId, truncated, title: text(r.title, 512), cwd: optionalText(r.cwd, 32768), updatedAt: optionalText(r.updatedAt, 64) }
    if (r.role !== 'user' && r.role !== 'assistant') return invalid()
    return { type: 'message', sessionKey, nativeSessionId, truncated, role: r.role, text: text(r.text, 16384) }
  }
  const origin = text(r.origin, 4096, false)
  const name = text(r.name, 4096)
  switch (r.type) {
    case 'setting':
      object(r, ['type', 'name', 'value', 'origin'])
      if (!['model', 'language', 'outputStyle', 'model_reasoning_effort', 'approval_policy', 'sandbox_mode'].includes(name)) return invalid()
      return { type: 'setting', name, value: text(r.value, 1024), origin }
    case 'mcp':
      object(r, ['type', 'name', 'transport', 'origin'])
      if (!['stdio', 'http', 'sse', 'unknown'].includes(r.transport as string)) return invalid()
      return { type: 'mcp', name, transport: r.transport as string, origin }
    case 'skill':
      object(r, ['type', 'name', 'description', 'origin']); return { type: 'skill', name, description: text(r.description, 2048), origin }
    case 'agent':
      object(r, ['type', 'name', 'description', 'model', 'origin']); return { type: 'agent', name, description: text(r.description, 2048), model: optionalText(r.model, 1024), origin }
    case 'plugin':
      object(r, ['type', 'id', 'name', 'version', 'enabled', 'installed', 'origin'])
      return { type: 'plugin', id: text(r.id, 4096, false), name, version: optionalText(r.version, 1024), enabled: optionalBool(r.enabled), installed: optionalBool(r.installed), origin }
    case 'document':
      object(r, ['type', 'name', 'text', 'truncated', 'origin']); return { type: 'document', name, text: text(r.text, 16384), truncated: bool(r.truncated), origin }
    default: return invalid()
  }
}
function result(v: unknown, request: Required<ReadRequest>): ProjectionResult {
  const r = object(v, ['source', 'resourceKind', 'requestEpoch', 'observedAt', 'state', 'reason', 'items', 'hasMore'], ['historyMetadataIncomplete', 'historyReadFailures'])
  const s = source(r.source)
  if (JSON.stringify(s) !== JSON.stringify(request.source) || kind(r.resourceKind) !== request.resourceKind || u64(r.requestEpoch) !== request.requestEpoch) return invalid()
  if (r.state !== 'ready' && r.state !== 'unavailable') return invalid()
  const metadata = Object.prototype.hasOwnProperty.call(r, 'historyMetadataIncomplete')
    ? { historyMetadataIncomplete: bool(r.historyMetadataIncomplete) } : {}
  if ('historyMetadataIncomplete' in metadata && (request.resourceKind !== 'history' || r.state !== 'ready')) return invalid()
  let failures: { historyReadFailures?: HistoryReadFailure[] } = {}
  if (Object.prototype.hasOwnProperty.call(r, 'historyReadFailures')) {
    const codes = r.historyReadFailures
    const allowed: HistoryReadFailure[] = ['SOURCE_UNSUPPORTED', 'SOURCE_INVALID', 'SOURCE_INVALID_TEXT', 'SOURCE_TOO_LARGE', 'SOURCE_READ_FAILED']
    if (request.resourceKind !== 'history' || r.state !== 'ready' || metadata.historyMetadataIncomplete !== true
      || !Array.isArray(codes) || !codes.length || codes.length > allowed.length || new Set(codes).size !== codes.length
      || codes.some(code => !allowed.includes(code))) return invalid()
    failures = { historyReadFailures: [...codes] }
  }
  if (!Array.isArray(r.items) || r.items.length > request.limit) return invalid()
  const reason = optionalText(r.reason, 128)
  if (r.state === 'ready' ? reason !== null : !reason || !reasons.has(reason) || r.items.length !== 0 || r.hasMore !== false) return invalid()
  if (new TextEncoder().encode(JSON.stringify(r)).length > 2 * 1024 * 1024) return invalid()
  return { source: s, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch, observedAt: u64(r.observedAt),
    state: r.state, reason, items: r.items.map(v => item(v, request)), hasMore: bool(r.hasMore), ...metadata, ...failures }
}
const readQueues = new Map<string, ReturnType<typeof createReadQueue>>()
/** All projection clients for this admitted backend instance share its two
 * reader slots. Pending work has both a count and a wait limit; never replay. */
function createReadQueue(onIdle: () => void) {
  let active = 0
  const waiting: Array<{ start: () => void; timer: ReturnType<typeof setTimeout> }> = []
  return (operation: () => Promise<unknown>): Promise<unknown> => new Promise((resolve, reject) => {
    const complete = () => {
      --active
      const next = waiting.shift()
      if (next) { clearTimeout(next.timer); next.start() }
      if (!active && !waiting.length) onIdle()
    }
    const start = () => {
      ++active
      let result: Promise<unknown>
      try { result = operation() } catch (failure) { reject(failure); complete(); return }
      result.then(value => { resolve(value); complete() }, failure => { reject(failure); complete() })
    }
    if (active < 2) { start(); return }
    const busy = () => projectionFailure({ code: 'SOURCE_BUSY' }, 'read-invoke')
    if (waiting.length >= 32) { reject(busy()); return }
    const pending = { start, timer: setTimeout(() => {
      const index = waiting.indexOf(pending)
      if (index !== -1) { waiting.splice(index, 1); reject(busy()) }
    }, 5000) }
    waiting.push(pending)
  })
}
function scheduleRead(instance: string, operation: () => Promise<unknown>): Promise<unknown> {
  let queue = readQueues.get(instance)
  if (!queue) {
    queue = createReadQueue(() => { if (readQueues.get(instance) === queue) readQueues.delete(instance) })
    readQueues.set(instance, queue)
  }
  return queue(operation)
}

/** Pins the admitted document transport. No retry or ambient/default-root invoke is allowed. */
export function createProjectionClient(bridge: ProjectionBridge, current: () => boolean = () => true): ProjectionClient {
  const instance = id(bridge.instanceId)
  const assertCurrent = () => {
    let valid = false
    try { valid = bridge.instanceId === instance && current() } catch { /* Lost document authority. */ }
    if (!valid) throw new Error('BACKEND_INSTANCE_CHANGED')
  }
  const invoke = async (command: string, payload: unknown) => {
    assertCurrent()
    const response = await bridge.invoke(command, payload)
    assertCurrent()
    return response
  }
  return {
    async scope(value) {
      let stage: ProjectionStage = 'scope-request-validation'
      try {
        const query = target(value)
        stage = 'scope-invoke'
        const response = await invoke('native_get_scope', query)
        assertCurrent()
        stage = 'scope-response-validation'
        const received = source(response)
        if (received.instanceId !== instance || JSON.stringify(received.target) !== JSON.stringify(query)) return invalid()
        return received
      } catch (failure) { throw projectionFailure(failure, stage) }
    },
    async read(value) {
      let stage: ProjectionStage = 'read-request-validation'
      try {
        const query = readRequest(value)
        if (query.source.instanceId !== instance) return invalid()
        stage = 'read-invoke'
        assertCurrent()
        const response = await scheduleRead(instance, () => invoke('native_list_resources', query))
        assertCurrent()
        stage = 'read-response-validation'
        return result(response, query)
      } catch (failure) { throw projectionFailure(failure, stage) }
    },
  }
}
/** Never reflect arbitrary transport exception messages (which may contain native values). */
export function projectionErrorCode(value: unknown): string {
  const code = value && typeof value === 'object' && 'code' in value ? (value as { code: unknown }).code : value instanceof Error ? value.message : null
  return typeof code === 'string' && (reasons.has(code) || diagnosticCodes.has(code)) ? code : 'SOURCE_UNAVAILABLE'
}
