import type { ProjectionResult, ReadRequest, ResourceItem, ResourceKind, ScopeTarget, SourceRef } from '@/types/nativeProjection'
export interface ProjectionBridge { readonly instanceId: string; invoke(command: string, payload: unknown): Promise<unknown> }
export interface ProjectionClient { scope(target: ScopeTarget): Promise<SourceRef>; read(request: ReadRequest): Promise<ProjectionResult> }
const kinds: ResourceKind[] = ['history', 'messages', 'search', 'config', 'mcp', 'skills', 'agents', 'plugins', 'instructions']
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
  const r = object(v, ['source', 'resourceKind', 'requestEpoch', 'observedAt', 'state', 'reason', 'items', 'hasMore'])
  const s = source(r.source)
  if (JSON.stringify(s) !== JSON.stringify(request.source) || kind(r.resourceKind) !== request.resourceKind || u64(r.requestEpoch) !== request.requestEpoch) return invalid()
  if (r.state !== 'ready' && r.state !== 'unavailable') return invalid()
  if (!Array.isArray(r.items) || r.items.length > request.limit) return invalid()
  const reason = optionalText(r.reason, 128)
  if (r.state === 'ready' ? reason !== null : !reason || !reasons.has(reason) || r.items.length !== 0 || r.hasMore !== false) return invalid()
  if (new TextEncoder().encode(JSON.stringify(r)).length > 2 * 1024 * 1024) return invalid()
  return { source: s, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch, observedAt: u64(r.observedAt),
    state: r.state, reason, items: r.items.map(v => item(v, request)), hasMore: bool(r.hasMore) }
}
/** Pins the admitted document transport. No retry or ambient/default-root invoke is allowed. */
export function createProjectionClient(bridge: ProjectionBridge): ProjectionClient {
  const instance = id(bridge.instanceId)
  return {
    async scope(value) {
      const query = target(value)
      const received = source(await bridge.invoke('native_get_scope', query))
      if (received.instanceId !== instance || JSON.stringify(received.target) !== JSON.stringify(query)) return invalid()
      return received
    },
    async read(value) {
      const query = readRequest(value)
      if (query.source.instanceId !== instance) return invalid()
      return result(await bridge.invoke('native_list_resources', query), query)
    },
  }
}
/** Never reflect arbitrary transport exception messages (which may contain native values). */
export function projectionErrorCode(value: unknown): string {
  const code = value && typeof value === 'object' && 'code' in value ? (value as { code: unknown }).code : null
  return typeof code === 'string' && reasons.has(code) ? code : 'SOURCE_UNAVAILABLE'
}
