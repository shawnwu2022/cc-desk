import { describe, expect, it, vi } from 'vitest'
import { saveSessionOpenedAt, withSessionDisplayName } from '@/session/sessionMetadata'
import type { SessionMetadataPort } from '@/session/sessionMetadata'
import type { UnifiedSession } from '@/types/unifiedSession'

const row = (): UnifiedSession => ({ id: 'native-history:exact-context-and-root', runtime: 'native-cli', cli: 'codex',
  projectKey: '/repo', projectPath: '/repo', adapterSessionId: 'root-bound-source-key', nativeSessionId: 'native-id',
  title: 'Source summary', lastActivityAt: 0, lastOpenedAt: 0, processState: 'stopped', attentionState: 'none',
  activityState: 'unknown', observationState: 'off', archived: false, opened: false, resumable: true, renameState: 'idle' })
function metadata(identity: UnifiedSession, activity: number, mismatch?: string): SessionMetadataPort {
  return { sessionRecords: new Map([[identity.id, { runtime: identity.runtime, cli: identity.cli, projectPath: identity.projectPath,
    adapterSessionId: mismatch ?? identity.adapterSessionId, nativeSessionId: identity.nativeSessionId,
    title: 'Saved task', lastActivityAt: activity, lastOpenedAt: 50 }]]), upsertSessionRecord: vi.fn() }
}
describe('last-known display activity', () => {
  it.each([[0, 100, 100], [200, 100, 200], [40, 100, 100], [0, 0, 0], [0, -1, 0], [0, NaN, 0], [0, Infinity, 0]])(
    'retains finite exact-identity activity without asserting fresh source activity: %s/%s', (source, saved, expected) => {
      const current = { ...row(), lastActivityAt: source }
      expect(withSessionDisplayName(current, metadata(current, saved))).toMatchObject({ lastActivityAt: expected, activityState: 'unknown' })
    })
  it('does not transfer metadata across a changed source identity or catalog key', () => {
    const current = row(), saved = metadata(current, 100, 'another-root-key')
    expect(withSessionDisplayName(current, saved)).toEqual(current)
    expect(withSessionDisplayName({ ...current, id: 'another-profile-context' }, metadata(current, 100))).toMatchObject({ lastActivityAt: 0, title: 'Source summary' })
  })
})

// Open-order checkpoints are not manual renames. Persisting their source title
// as an override froze later native AI/first-user title observations.
it('Session_OpenCheckpointDoesNotFreezeAutomaticTitle_009', async () => {
  const identity = row()
  const records = new Map()
  const port: SessionMetadataPort = { sessionRecords: records, upsertSessionRecord: vi.fn(async (key, value) => { records.set(key, value) }) }
  await saveSessionOpenedAt(port, identity, 100, () => true)
  expect(records.get(identity.id).title).toBe('')
  expect(withSessionDisplayName({ ...identity, title: 'Fresh native AI title' }, port).title).toBe('Fresh native AI title')
  records.set(identity.id, { ...records.get(identity.id), title: 'User named task' })
  await saveSessionOpenedAt(port, identity, 200, () => true)
  expect(withSessionDisplayName({ ...identity, title: 'Another native title' }, port).title).toBe('User named task')
})
