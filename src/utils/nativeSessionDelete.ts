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
export type NativeDeleteBlockedReason = 'cleanupClaudeUnsupported' | 'cleanupOpencodeUnsafe' | 'deleteEligibility'
  | 'deleteBlockedActive' | 'deleteSourceLoading' | 'deleteSourceUnavailable' | 'deleteHistoryReadFailed' | 'deleteRequiresNativeArchive'
export type NativeDeleteEligibility = { binding: NativeDeleteBinding; reason: null } | { binding: null; reason: NativeDeleteBlockedReason }

/** 显示层资格不替代后端预览；图标提示、单项和批量共享同一来源边界。 */
export function nativeDeleteEligibility(row: UnifiedSession, entry: NativeHistoryEntry | undefined): NativeDeleteEligibility {
  const origin = row.nativeOrigin
  if (row.cli === 'claude') return { binding: null, reason: 'cleanupClaudeUnsupported' }
  if (row.cli === 'opencode') return { binding: null, reason: 'cleanupOpencodeUnsafe' }
  if (!row.archived) return { binding: null, reason: 'deleteEligibility' }
  if (row.opened !== false || row.resumePending || row.preparationState || !['stopped', 'failed'].includes(row.processState))
    return { binding: null, reason: 'deleteBlockedActive' }
  if (row.runtime !== 'native-cli' || row.cli !== 'codex' || !origin || origin.cli !== 'codex'
    || !row.nativeHistorySessionKey || !row.nativeSessionId || !row.nativeRootKey
    || normalizePath(origin.projectPath) !== normalizePath(row.projectPath)) return { binding: null, reason: 'deleteSourceUnavailable' }
  if (entry?.loading) return { binding: null, reason: 'deleteSourceLoading' }
  if (!entry?.loaded || entry.error || !entry.source || entry.source.basis !== 'configured-profile'
    || entry.source.target.kind !== 'profile' || !entry.source.target.projectIds.includes(origin.projectId)
    || entry.source.sourceRootKey !== row.nativeRootKey || entry.source.cli !== row.cli
    || entry.source.profileId !== origin.profileId || entry.source.profileRevision !== origin.profileRevision)
    return { binding: null, reason: 'deleteSourceUnavailable' }
  const matches = entry.sessions.filter(item => item.sessionKey === row.nativeHistorySessionKey && item.nativeSessionId === row.nativeSessionId)
  if (matches.length !== 1 || matches[0]!.projectId && matches[0]!.projectId !== origin.projectId)
    return { binding: null, reason: 'deleteSourceUnavailable' }
  if (matches[0]!.readFailure) return { binding: null, reason: 'deleteHistoryReadFailed' }
  if (matches[0]!.archived !== true) return { binding: null, reason: 'deleteRequiresNativeArchive' }
  // truncated 包含标题截断与正常有界元数据读取，不代表身份或原生归档证据失效。
  // 这里只允许进入只读预览；完整目标观察、授权与活动保护仍由后端重新核验。
  return { binding: {
    rowId: row.id, source: entry.source, sessionKey: row.nativeHistorySessionKey, nativeSessionId: row.nativeSessionId,
    origin, title: row.title,
    signature: JSON.stringify([row.id, origin, entry.source, row.nativeHistorySessionKey, row.nativeSessionId]),
    physicalKey: JSON.stringify([row.cli, row.nativeRootKey, row.nativeSessionId.toLowerCase()]),
  }, reason: null }
}

export function nativeDeleteBinding(row: UnifiedSession, entry: NativeHistoryEntry | undefined): NativeDeleteBinding | null {
  return nativeDeleteEligibility(row, entry).binding
}
