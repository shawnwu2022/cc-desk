import type { SourceRef } from '@/types/nativeProjection'
import { projectionErrorCode, validateProjectionSource, type ProjectionBridge } from './nativeProjection'

export interface OpencodeArchiveRequest {
  operationId: string
  source: SourceRef
  sessionKey: string
  archived: boolean
  acknowledgeRecoverable: true
}
export interface OpencodeArchiveReceipt {
  operationId: string
  source: SourceRef
  sessionKey: string
  archived: boolean
  state: 'archived'
}
const archiveCodes = new Set(['OPENCODE_ARCHIVE_UNSUPPORTED', 'ARCHIVE_SCHEMA_UNSUPPORTED', 'ARCHIVE_SOURCE_UNAVAILABLE',
  'ARCHIVE_OUTCOME_UNKNOWN', 'ARCHIVE_OPERATION_CONFLICT', 'NODE_RUNTIME_UNAVAILABLE', 'SDK_HELPER_UNAVAILABLE'])
export function opencodeArchiveErrorCode(value: unknown): string {
  const code = value && typeof value === 'object' ? (value as { code?: unknown }).code : undefined
  if (typeof code === 'string' && archiveCodes.has(code)) return code
  const known = projectionErrorCode(value)
  return known === 'SOURCE_UNAVAILABLE' ? 'ARCHIVE_OUTCOME_UNKNOWN' : known
}
function failure(code: string): Error { return Object.assign(new Error(code), { code }) }
function exact(value: unknown, keys: string[]): boolean {
  return !!value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).sort().join() === [...keys].sort().join()
}
function request(value: OpencodeArchiveRequest, instance: string): OpencodeArchiveRequest {
  try {
    if (!exact(value, ['operationId', 'source', 'sessionKey', 'archived', 'acknowledgeRecoverable'])
      || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value.operationId)
      || typeof value.sessionKey !== 'string' || !value.sessionKey || new TextEncoder().encode(value.sessionKey).length > 8192
      || /[\p{Cc}\p{Cs}]/u.test(value.sessionKey) || typeof value.archived !== 'boolean' || value.acknowledgeRecoverable !== true) throw failure('INVALID_REQUEST')
    const source = validateProjectionSource(value.source)
    if (source.cli !== 'opencode' || source.instanceId !== instance || source.basis !== 'configured-profile' || source.target.kind !== 'profile') throw failure('INVALID_REQUEST')
    return { operationId: value.operationId, source, sessionKey: value.sessionKey, archived: value.archived, acknowledgeRecoverable: true }
  } catch { throw failure('INVALID_REQUEST') }
}
function receipt(value: unknown, sent: OpencodeArchiveRequest): OpencodeArchiveReceipt {
  try {
    if (!exact(value, ['operationId', 'source', 'sessionKey', 'archived', 'state'])) throw failure('ARCHIVE_OUTCOME_UNKNOWN')
    const result = value as OpencodeArchiveReceipt
    const source = validateProjectionSource(result.source)
    if (result.operationId !== sent.operationId || result.sessionKey !== sent.sessionKey || result.archived !== sent.archived
      || result.state !== 'archived' || JSON.stringify(source) !== JSON.stringify(sent.source)) throw failure('ARCHIVE_OUTCOME_UNKNOWN')
    return { operationId: result.operationId, source, sessionKey: result.sessionKey, archived: result.archived, state: 'archived' }
  } catch { throw failure('ARCHIVE_OUTCOME_UNKNOWN') }
}
/** 一次发送后不重放，也不把未知归档改为恢复。 */
export function createOpencodeArchiveClient(bridge: ProjectionBridge, current: () => boolean = () => true) {
  const instance = bridge.instanceId, issued = new Set<string>()
  function check() { if (bridge.instanceId !== instance || !current()) throw failure('BACKEND_INSTANCE_CHANGED') }
  return {
    async set(value: OpencodeArchiveRequest): Promise<OpencodeArchiveReceipt> {
      check()
      const sent = request(value, instance)
      if (issued.has(sent.operationId)) throw failure('ARCHIVE_OUTCOME_UNKNOWN')
      issued.add(sent.operationId)
      try {
        const result = await bridge.invoke('cli_set_opencode_archive', sent)
        check()
        return receipt(result, sent)
      } catch (error) { throw failure(opencodeArchiveErrorCode(error)) }
    },
  }
}
