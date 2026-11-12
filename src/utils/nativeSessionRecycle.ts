import type { UnifiedSession } from '@/types/unifiedSession'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import { normalizePath } from './path'
export function nativeRecycleBinding(row: UnifiedSession, entry: NativeHistoryEntry | undefined) {
  const origin = row.nativeOrigin
  if (row.cli !== 'claude' || row.runtime !== 'native-cli' || !row.archived || row.opened !== false || row.resumePending || row.preparationState || !['stopped', 'failed'].includes(row.processState)
    || !origin || origin.cli !== 'claude' || !row.nativeHistorySessionKey || !row.nativeSessionId || !row.nativeRootKey || normalizePath(origin.projectPath) !== normalizePath(row.projectPath)
    || !entry?.loaded || entry.loading || entry.error || !entry.source || entry.source.cli !== 'claude' || entry.source.basis !== 'configured-profile' || entry.source.target.kind !== 'profile'
    || !entry.source.target.projectIds.includes(origin.projectId) || entry.source.sourceRootKey !== row.nativeRootKey || entry.source.profileId !== origin.profileId || entry.source.profileRevision !== origin.profileRevision) return null
  const matches = entry.sessions.filter(item => item.sessionKey === row.nativeHistorySessionKey && item.nativeSessionId === row.nativeSessionId)
  if (matches.length !== 1 || matches[0]!.readFailure || matches[0]!.projectId !== origin.projectId) return null
  return { rowId: row.id, title: row.title, nativeSessionId: row.nativeSessionId, sessionKey: row.nativeHistorySessionKey, source: entry.source, origin }
}
