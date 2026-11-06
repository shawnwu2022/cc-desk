import type { SessionDeleteClient, SessionDeletePreview, SessionDeleteReceipt } from '@/types/nativeSessionDelete'
import type { ProjectionBridge } from './nativeProjection'
import { validateProjectionSource } from './nativeProjection'

const safeCodes = new Set(['NATIVE_DELETE_UNSUPPORTED', 'DELETE_UNSUPPORTED', 'DELETE_FAILED', 'DELETE_OUTCOME_UNKNOWN', 'DELETE_PREVIEW_CHANGED', 'DELETE_PREVIEW_EXPIRED', 'DELETE_PREVIEW_REQUIRED', 'DELETE_CONFIRMATION_REQUIRED', 'DELETE_OPERATION_CONFLICT', 'DELETE_NOT_ARCHIVED', 'DELETE_SOURCE_UNVERIFIED', 'DELETE_CONFIGURATION_UNSUPPORTED', 'DELETE_SOURCE_BUSY', 'DELETE_IN_PROGRESS', 'SESSION_NOT_FOUND', 'SESSION_IDENTITY_UNKNOWN', 'SESSION_ORIGIN_AMBIGUOUS', 'PROGRAM_UNAVAILABLE', 'RUNNER_UNAVAILABLE', 'SOURCE_BUSY', 'SOURCE_CHANGED', 'SOURCE_UNAVAILABLE', 'SOURCE_TOO_LARGE', 'SOURCE_BUDGET_EXCEEDED', 'SOURCE_AMBIGUOUS', 'SCOPE_STALE', 'SCOPE_REVOKED', 'SCOPE_UNKNOWN', 'REVISION_CONFLICT', 'PROJECT_IDENTITY_CHANGED', 'FORBIDDEN', 'INVALID_REQUEST', 'BACKEND_INSTANCE_CHANGED', 'DELETE_INVALID_RESPONSE'])
for (const code of ['DELETE_SOURCE_UNAVAILABLE', 'DELETE_ROOT_NOT_ARCHIVED', 'DELETE_ARGUMENTS_UNSUPPORTED', 'DELETE_OWNER_PENDING', 'DELETE_NOT_ACKNOWLEDGED', 'DELETE_ROOT_PRESENT', 'DELETE_OPERATION_UNKNOWN', 'DELETE_UNAVAILABLE', 'DELETE_CONFIG_UNPROVEN', 'DELETE_CONTAINER_UNSAFE', 'DOCUMENT_BRIDGE_UNAVAILABLE']) safeCodes.add(code)
function error(code: string): Error & { code: string; retryable: false } { return Object.assign(new Error(code), { code, retryable: false as const }) }
export function sessionDeleteErrorCode(value: unknown): string {
  const code = value && typeof value === 'object' && 'code' in value ? (value as { code: unknown }).code : value instanceof Error ? value.message : null
  return typeof code === 'string' && safeCodes.has(code) ? code : 'DELETE_OUTCOME_UNKNOWN'
}
function invalid(): never { throw error('DELETE_INVALID_RESPONSE') }
function object(value: unknown, fields: string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalid()
  const record = value as Record<string, unknown>
  if (Object.keys(record).length !== fields.length || fields.some(key => !Object.prototype.hasOwnProperty.call(record, key))) return invalid()
  return record
}
function uuid(value: unknown): string { if (typeof value !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value)) return invalid(); return value }
function reason(value: unknown): string | null { if (value === null) return null; if (typeof value !== 'string' || !safeCodes.has(value)) return invalid(); return value }
function nullableBoolean(value: unknown): boolean | null { if (value !== null && typeof value !== 'boolean') return invalid(); return value }
function decodePreview(value: unknown, operationId: string, cli: string): SessionDeletePreview {
  const r = object(value, ['operationId', 'previewId', 'nativeSessionId', 'cli', 'state', 'rootArchived', 'scope', 'knownDescendantCount', 'reason'])
  if (r.operationId !== operationId || r.cli !== cli || !['ready', 'blocked', 'unsupported'].includes(r.state as string) || r.scope !== 'native-subtree' || r.knownDescendantCount !== null) return invalid()
  const result = { operationId, previewId: r.previewId === null ? null : uuid(r.previewId), nativeSessionId: r.nativeSessionId === null ? null : uuid(r.nativeSessionId), cli, state: r.state, rootArchived: nullableBoolean(r.rootArchived), scope: 'native-subtree', knownDescendantCount: null, reason: reason(r.reason) } as SessionDeletePreview
  if (result.state === 'ready' && (cli !== 'codex' || !result.previewId || !result.nativeSessionId || result.rootArchived !== true || result.reason !== null)) return invalid()
  return result
}
function decodeReceipt(value: unknown, operationId: string, cli: string): SessionDeleteReceipt {
  const r = object(value, ['operationId', 'cli', 'state', 'rootPresent', 'descendants', 'reason'])
  if (r.operationId !== operationId || r.cli !== cli || !['acknowledged', 'partial', 'unknown', 'blocked', 'unsupported'].includes(r.state as string) || r.descendants !== 'unverified') return invalid()
  return { operationId, cli, state: r.state, rootPresent: nullableBoolean(r.rootPresent), descendants: 'unverified', reason: reason(r.reason) } as SessionDeleteReceipt
}

/** 固定文档桥；一旦发送确认，未知或无效回执只能检查，不能重放。 */
export function createSessionDeleteClient(bridge: ProjectionBridge, current: () => boolean = () => true): SessionDeleteClient {
  const instance = bridge.instanceId
  const operations = new Map<string, { payload: string; cli: string; preview: SessionDeletePreview; issued: boolean; receipt?: SessionDeleteReceipt; flight?: Promise<SessionDeleteReceipt> }>()
  function assertCurrent() {
    let valid = false
    try { valid = bridge.instanceId === instance && current() } catch { /* 文档已失去授权。 */ }
    if (!valid) throw error('BACKEND_INSTANCE_CHANGED')
  }
  return {
    async preview(value) {
      assertCurrent()
      object(value, ['operationId', 'source', 'sessionKey'])
      const operationId = uuid(value.operationId), source = validateProjectionSource(value.source)
      if (source.instanceId !== instance || typeof value.sessionKey !== 'string' || !value.sessionKey || new TextEncoder().encode(value.sessionKey).length > 16384 || /\p{Cc}/u.test(value.sessionKey)) return invalid()
      const request = { operationId, source, sessionKey: value.sessionKey }, payload = JSON.stringify(request), old = operations.get(operationId)
      if (old) { if (old.payload !== payload) throw error('DELETE_OPERATION_CONFLICT'); if (old.issued) throw error('DELETE_OUTCOME_UNKNOWN'); return old.preview }
      try {
        const response = await bridge.invoke('cli_preview_session_delete', request)
        assertCurrent()
        const preview = decodePreview(response, operationId, source.cli)
        operations.set(operationId, { payload, cli: source.cli, preview, issued: false })
        return preview
      } catch (failure) { throw error(sessionDeleteErrorCode(failure)) }
    },
    async confirm(value) {
      assertCurrent()
      object(value, ['operationId', 'previewId', 'acknowledgePermanentDelete', 'acknowledgeNativeSubtree'])
      const operationId = uuid(value.operationId), previewId = uuid(value.previewId), entry = operations.get(operationId)
      if (value.acknowledgePermanentDelete !== true || value.acknowledgeNativeSubtree !== true) throw error('DELETE_CONFIRMATION_REQUIRED')
      if (!entry || entry.preview.state !== 'ready' || entry.preview.previewId !== previewId) throw error('DELETE_PREVIEW_REQUIRED')
      if (entry.flight) return entry.flight
      if (entry.issued) { if (entry.receipt?.state === 'acknowledged') return entry.receipt; throw error('DELETE_OUTCOME_UNKNOWN') }
      entry.issued = true
      entry.flight = Promise.resolve().then(async () => {
        try {
          assertCurrent()
          const response = await bridge.invoke('cli_confirm_session_delete', { operationId, previewId, acknowledgePermanentDelete: true, acknowledgeNativeSubtree: true })
          assertCurrent()
          const receipt = decodeReceipt(response, operationId, entry.cli)
          entry.receipt = receipt
          return receipt
        } catch { throw error('DELETE_OUTCOME_UNKNOWN') }
        finally { entry.flight = undefined }
      })
      return entry.flight
    },
    async inspect(value) {
      assertCurrent()
      object(value, ['operationId'])
      const operationId = uuid(value.operationId), entry = operations.get(operationId)
      if (!entry?.issued || entry.flight) throw error('DELETE_PREVIEW_REQUIRED')
      try {
        const response = await bridge.invoke('cli_inspect_session_delete', { operationId })
        assertCurrent()
        const receipt = decodeReceipt(response, operationId, entry.cli)
        entry.receipt = receipt
        return receipt
      } catch (failure) { throw error(sessionDeleteErrorCode(failure)) }
    },
  }
}
