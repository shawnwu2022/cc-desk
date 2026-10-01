import type { SessionUiRecord } from '@/types/app'
import type { UnifiedSession } from '@/types/unifiedSession'
import { sameProjectPath } from '@/utils/path'

/** Display metadata never authorizes a runtime action or discovers a session. */
export interface SessionMetadataPort {
  readonly sessionRecords: ReadonlyMap<string, SessionUiRecord>
  upsertSessionRecord(key: string, record: SessionUiRecord, beforeMutation?: () => void): Promise<unknown>
}

export function withSessionDisplayName(row: UnifiedSession, metadata: SessionMetadataPort | undefined, identity = row): UnifiedSession {
  const saved = metadata?.sessionRecords.get(identity.id)
  if (!saved || saved.runtime !== identity.runtime || saved.cli !== identity.cli
    || !sameProjectPath(saved.projectPath, identity.projectPath) || saved.adapterSessionId !== identity.adapterSessionId
    || (saved.nativeSessionId ?? null) !== (identity.nativeSessionId ?? null)) return row
  return { ...row, title: saved.title || row.title }
}

export async function saveSessionDisplayName(metadata: SessionMetadataPort, identity: UnifiedSession, title: string, owns: () => boolean): Promise<void> {
  const value = title.trim()
  if (!value || /[\x00-\x1f\x7f]/.test(value) || [...value].length > 200) throw new Error('SESSION_TITLE_REQUIRED')
  const requireCurrent = () => { if (!owns()) throw new Error('STALE_SESSION_ATTEMPT') }
  requireCurrent()
  await metadata.upsertSessionRecord(identity.id, {
    runtime: identity.runtime, cli: identity.cli, projectPath: identity.projectPath,
    adapterSessionId: identity.adapterSessionId, nativeSessionId: identity.nativeSessionId,
    title: value, lastActivityAt: identity.lastActivityAt,
  }, requireCurrent)
  requireCurrent()
}
