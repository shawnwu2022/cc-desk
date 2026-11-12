import type { UnifiedSession } from '@/types/unifiedSession'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'
import { validateProjectionSource } from '@/api/nativeProjection'
import { normalizePath } from './path'

export function opencodeArchiveBinding(row: UnifiedSession, entry: NativeHistoryEntry | undefined) {
  const origin = row.nativeOrigin
  if (row.cli !== 'opencode' || row.runtime !== 'native-cli' || !row.archived || row.opened !== false
    || row.resumePending || row.preparationState || !['stopped', 'failed'].includes(row.processState)
    || !origin || origin.cli !== 'opencode' || !row.nativeHistorySessionKey || !row.nativeSessionId || !row.nativeRootKey
    || normalizePath(origin.projectPath) !== normalizePath(row.projectPath)
    || !entry?.loaded || entry.loading || entry.error || !entry.source) return null
  let source
  try { source = validateProjectionSource(entry.source) } catch { return null }
  if (source.cli !== 'opencode' || source.basis !== 'configured-profile' || source.target.kind !== 'profile'
    || !source.target.projectIds.includes(origin.projectId) || source.sourceRootKey !== row.nativeRootKey
    || source.profileId !== origin.profileId || source.profileRevision !== origin.profileRevision) return null
  const matches = entry.sessions.filter(item => item.sessionKey === row.nativeHistorySessionKey && item.nativeSessionId === row.nativeSessionId)
  if (matches.length !== 1 || matches[0]!.readFailure || matches[0]!.projectId !== origin.projectId || typeof matches[0]!.archived !== 'boolean') return null
  return { rowId: row.id, title: row.title, nativeSessionId: row.nativeSessionId, sessionKey: row.nativeHistorySessionKey,
    source, origin: { ...origin }, nativeArchived: matches[0]!.archived }
}
export type OpencodeArchiveBinding = NonNullable<ReturnType<typeof opencodeArchiveBinding>>
