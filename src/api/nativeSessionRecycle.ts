import type { ProjectionBridge } from './nativeProjection'
import { validateProjectionSource } from './nativeProjection'
import type { SourceRef } from '@/types/nativeProjection'
export interface RecycleRecord { operationId: string; nativeSessionId: string; projectPath: string; state: 'stored' | 'source' | 'unknown'; originalOccupied: boolean; scope: 'claude-root-transcript' }
export interface RecycleRequest { source: SourceRef; action: 'list' | 'remove' | 'restore'; operationId: string | null; sessionKey: string | null; acknowledgeRecoverable: boolean }
export interface RecycleResponse { records: RecycleRecord[]; unavailableCount: number }
const codes = new Set(['RECYCLE_UNSUPPORTED', 'RECYCLE_INVALID_REQUEST', 'RECYCLE_UNAVAILABLE', 'RECYCLE_OPERATION_CONFLICT', 'RECYCLE_CAPACITY', 'RECYCLE_RESTORE_CONFLICT', 'RECYCLE_OUTCOME_UNKNOWN', 'SOURCE_BUSY', 'SOURCE_CHANGED', 'SCOPE_STALE', 'SCOPE_REVOKED', 'SCOPE_UNKNOWN', 'FORBIDDEN', 'BACKEND_INSTANCE_CHANGED'])
export function recycleErrorCode(value: unknown): string { const code = value && typeof value === 'object' ? (value as { code?: unknown }).code : null; return typeof code === 'string' && codes.has(code) ? code : 'RECYCLE_OUTCOME_UNKNOWN' }
function failure(code: string): Error { return Object.assign(new Error(code), { code }) }
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value)
function invalid(): never { throw failure('RECYCLE_OUTCOME_UNKNOWN') }
function decode(value: unknown, operationId: string | null): RecycleResponse {
  if (!value || typeof value !== 'object' || Object.keys(value).sort().join() !== 'records,unavailableCount' || !Array.isArray((value as RecycleResponse).records)) return invalid()
  const { records, unavailableCount } = value as RecycleResponse
  if (!Number.isSafeInteger(unavailableCount) || unavailableCount < 0 || unavailableCount > 128) return invalid()
  if (records.length > 128 || operationId && (records.length !== 1 || records[0]?.operationId !== operationId)) return invalid()
  const seen = new Set<string>()
  for (const record of records) {
    if (!record || Object.keys(record).sort().join() !== 'nativeSessionId,operationId,originalOccupied,projectPath,scope,state' || !uuid(record.operationId) || !uuid(record.nativeSessionId)
      || typeof record.projectPath !== 'string' || !record.projectPath || record.projectPath.length > 32768 || /\p{Cc}/u.test(record.projectPath)
      || !['stored', 'source', 'unknown'].includes(record.state) || typeof record.originalOccupied !== 'boolean' || record.scope !== 'claude-root-transcript' || seen.has(record.operationId)) return invalid()
    seen.add(record.operationId)
  }
  return { records: records.map(record => ({ ...record })), unavailableCount }
}
/** 一次发送后不自动重放；重启后的恢复记录由后端持久日志只读发现。 */
export function createSessionRecycleClient(bridge: ProjectionBridge, current: () => boolean = () => true) {
  const instance = bridge.instanceId, issued = new Set<string>()
  function check() { if (bridge.instanceId !== instance || !current()) throw failure('BACKEND_INSTANCE_CHANGED') }
  return {
    async send(value: RecycleRequest): Promise<RecycleResponse> {
      check()
      const source = validateProjectionSource(value.source)
      if (source.instanceId !== instance || source.cli !== 'claude' || !['list', 'remove', 'restore'].includes(value.action)) throw failure('RECYCLE_INVALID_REQUEST')
      if (value.action === 'list' ? value.operationId !== null || value.sessionKey !== null || value.acknowledgeRecoverable !== false
        : !uuid(value.operationId) || value.acknowledgeRecoverable !== true || (value.action === 'remove' ? !value.sessionKey || value.sessionKey.length > 16384 : value.sessionKey !== null)) throw failure('RECYCLE_INVALID_REQUEST')
      const request = { source, action: value.action, operationId: value.operationId, sessionKey: value.sessionKey, acknowledgeRecoverable: value.acknowledgeRecoverable }
      const key = JSON.stringify([source.sourceRootKey, value.action, value.operationId])
      if (value.action !== 'list') { if (issued.has(key)) throw failure('RECYCLE_OUTCOME_UNKNOWN'); issued.add(key) }
      try { const result = await bridge.invoke('cli_recycle_session', request); check(); return decode(result, request.operationId) }
      catch (error) { throw failure(recycleErrorCode(error)) }
    },
  }
}
