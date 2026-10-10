import type { SessionUiRecord } from '@/types/app'
import type { UnifiedSession } from '@/types/unifiedSession'
import { sameProjectPath } from '@/utils/path'
import { useNotificationsStore } from '@/stores/notifications'

/** Display metadata never authorizes a runtime action or discovers a session. */
export interface SessionMetadataPort {
  readonly sessionRecords: ReadonlyMap<string, SessionUiRecord>
  upsertSessionRecord(key: string, record: SessionUiRecord, beforeMutation?: () => void): Promise<unknown>
  recordSessionOpened?(key: string, record: SessionUiRecord, beforeMutation?: () => void): Promise<unknown>
}

export function matchesSessionMetadata(saved: SessionUiRecord | undefined, identity: UnifiedSession): saved is SessionUiRecord {
  return !!saved && saved.runtime === identity.runtime && saved.cli === identity.cli
    && sameProjectPath(saved.projectPath, identity.projectPath) && saved.adapterSessionId === identity.adapterSessionId
    && (saved.nativeSessionId ?? null) === (identity.nativeSessionId ?? null)
}

export function withSessionDisplayName(row: UnifiedSession, metadata: SessionMetadataPort | undefined, identity = row): UnifiedSession {
  const saved = metadata?.sessionRecords.get(identity.id)
  if (!matchesSessionMetadata(saved, identity)) return row
  return { ...row, title: saved.title || row.title,
    lastOpenedAt: Math.max(row.lastOpenedAt ?? 0, saved.lastOpenedAt ?? 0) }
}

/** Save a proven accepted open (or transfer its existing time to its history key). */
export async function saveSessionOpenedAt(metadata: SessionMetadataPort | undefined, identity: UnifiedSession, at: number, owns: () => boolean): Promise<void> {
  if (!metadata) return
  const saved = metadata.sessionRecords.get(identity.id)
  if (matchesSessionMetadata(saved, identity) && (saved.lastOpenedAt ?? 0) >= at) return
  const record: SessionUiRecord = {
    runtime: identity.runtime, cli: identity.cli, projectPath: identity.projectPath,
    adapterSessionId: identity.adapterSessionId, nativeSessionId: identity.nativeSessionId,
    title: identity.title, lastActivityAt: identity.lastActivityAt, lastOpenedAt: at,
  }
  const requireCurrent = () => { if (!owns()) throw new Error('STALE_SESSION_ATTEMPT') }
  requireCurrent()
  const save = metadata.recordSessionOpened?.bind(metadata) ?? metadata.upsertSessionRecord.bind(metadata)
  try {
    await save(identity.id, record, requireCurrent)
  } catch {
    if (owns()) useNotificationsStore().pushToast({ kind: 'warning', messageKey: 'feedbackSessionOpenTimeNotSaved', dedupeKey: 'session-open-time-save' })
  }
}

export async function saveSessionDisplayName(metadata: SessionMetadataPort, identity: UnifiedSession, title: string, owns: () => boolean, onIssued?: () => void): Promise<void> {
  const value = title.trim()
  if (!value || /[\x00-\x1f\x7f]/.test(value) || [...value].length > 200) throw new Error('SESSION_TITLE_REQUIRED')
  const requireCurrent = () => { if (!owns()) throw new Error('STALE_SESSION_ATTEMPT') }
  requireCurrent()
  await metadata.upsertSessionRecord(identity.id, {
    runtime: identity.runtime, cli: identity.cli, projectPath: identity.projectPath,
    adapterSessionId: identity.adapterSessionId, nativeSessionId: identity.nativeSessionId,
    title: value, lastActivityAt: identity.lastActivityAt,
  }, () => {
    requireCurrent()
    // Called inside the canonical writer queue, immediately before the IPC.
    // Selection changes after this boundary cannot undo an issued save.
    onIssued?.()
  })
  requireCurrent()
}
