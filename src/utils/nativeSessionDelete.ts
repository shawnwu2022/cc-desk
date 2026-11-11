import { normalizePath } from './path'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import type { SourceRef } from '@/types/nativeProjection'

export interface NativeDeleteBinding {
  rowId: string
  source: SourceRef
  sessionKey: string
  nativeSessionId: string
  origin: NonNullable<UnifiedSession['nativeOrigin']>
  title: string
  signature: string
  physicalKey: string
}
/** 显示层资格不替代后端预览；单项和批量共享同一严格来源边界。 */
export function nativeDeleteBinding(row: UnifiedSession, entry: NativeHistoryEntry | undefined): NativeDeleteBinding | null {
  const origin = row.nativeOrigin
  if (!row.archived || row.runtime !== 'native-cli' || row.cli !== 'codex' || !origin || origin.cli !== 'codex'
    || row.opened !== false || row.resumePending || row.preparationState || !['stopped', 'failed'].includes(row.processState)
    || !row.nativeHistorySessionKey || !row.nativeSessionId || !row.nativeRootKey
    || normalizePath(origin.projectPath) !== normalizePath(row.projectPath)) return null
  if (!entry?.loaded || entry.loading || entry.error || !entry.source || entry.source.basis !== 'configured-profile'
    || entry.source.target.kind !== 'profile' || !entry.source.target.projectIds.includes(origin.projectId)
    || entry.source.sourceRootKey !== row.nativeRootKey || entry.source.cli !== row.cli
    || entry.source.profileId !== origin.profileId || entry.source.profileRevision !== origin.profileRevision) return null
  const matches = entry.sessions.filter(item => item.sessionKey === row.nativeHistorySessionKey && item.nativeSessionId === row.nativeSessionId)
  if (matches.length !== 1 || matches[0]!.archived !== true || matches[0]!.truncated || matches[0]!.readFailure
    || matches[0]!.projectId && matches[0]!.projectId !== origin.projectId) return null
  return {
    rowId: row.id, source: entry.source, sessionKey: row.nativeHistorySessionKey, nativeSessionId: row.nativeSessionId,
    origin, title: row.title,
    signature: JSON.stringify([row.id, origin, entry.source, row.nativeHistorySessionKey, row.nativeSessionId]),
    physicalKey: JSON.stringify([row.cli, row.nativeRootKey, row.nativeSessionId.toLowerCase()]),
  }
}
