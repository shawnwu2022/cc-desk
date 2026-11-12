import { selectSessionPrimaryAction, selectSessionMenuActions } from '@/utils/sessionPresentation'
import { expect, it, vi } from 'vitest'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { NativeHistoryEntry } from '@/stores/nativeHistory'

function fixture() {
  const source = { scopeId: 'scope', instanceId: 'instance', cli: 'codex' as const, sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', basis: 'configured-profile' as const, target: { kind: 'profile' as const, profileId: 'profile', expectedProfileRevision: '1', projectIds: ['project'] } }
  const entry: NativeHistoryEntry = { key: 'history', context: { cli: 'codex', profileId: 'profile', profileRevision: '1', projectId: 'project', projectPath: '/repo' }, source, sourceRootKey: 'root', loaded: true, loading: false, error: null, requestEpoch: '1', sessions: [{ type: 'session', sessionKey: 'key', nativeSessionId: '00000000-0000-4000-8000-000000000001', title: 'Fixture', archived: false, truncated: false, cwd: '/repo', updatedAt: null }] }
  let archivedByCli = false
  const nativeArchive = { archive: vi.fn(async () => { archivedByCli = true }) }
  const entries = [entry]
  const load = vi.fn(async (context: { projectId: string }) => { entry.sessions[0].archived = archivedByCli; return entries.find(value => value.context.projectId === context.projectId)! })
  const cliUnarchive = { unarchive: vi.fn(async () => { archivedByCli = false }) }
  const archive = { getArchivedSessions: () => [] as string[], archiveSession: vi.fn(async () => {}), restoreSession: vi.fn(async () => {}) }
  const adapter = createNativeCliAdapter({ tabs: { tabs: new Map(), activeTabId: null, setActive: vi.fn(), close: vi.fn(), rename: vi.fn() }, history: { all: () => entries, load }, runtime: {} as any, archive, nativeArchive, cliUnarchive })
  return { adapter, entry, entries, archive, nativeArchive, cliUnarchive, load, setArchived: (value: boolean) => { archivedByCli = value } }
}

it('CodexArchive_UsesNativeMutationAndVerifiedReadback_001', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  await f.adapter.archiveSession(row.id)
  expect(f.nativeArchive.archive).toHaveBeenCalledOnce()
  expect(f.nativeArchive.archive).toHaveBeenCalledWith(expect.objectContaining({ source: f.entry.source, sessionKey: 'key' }))
  expect(f.archive.archiveSession).not.toHaveBeenCalled()
  expect((await f.adapter.listSessions())[0].archived).toBe(true)
})
it('CodexArchive_FailureNeverCreatesDeskOnlyArchive_002', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.nativeArchive.archive.mockRejectedValueOnce(new Error('ARCHIVE_OUTCOME_UNKNOWN'))
  await expect(f.adapter.archiveSession(row.id)).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
  expect(f.archive.archiveSession).not.toHaveBeenCalled()
  expect((await f.adapter.listSessions())[0].archived).toBe(false)
})
it('CodexArchive_ReadFailurePreventsNativeWrite_003', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.entry.error = 'SOURCE_UNAVAILABLE'
  await expect(f.adapter.archiveSession(row.id)).rejects.toThrow('SOURCE_UNAVAILABLE')
  expect(f.nativeArchive.archive).not.toHaveBeenCalled()
  expect(f.archive.archiveSession).not.toHaveBeenCalled()
})
it('CodexArchive_ReadbackMustProveNativeArchive_004', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.nativeArchive.archive.mockImplementationOnce(async () => {})
  await expect(f.adapter.archiveSession(row.id)).rejects.toThrow('ARCHIVE_DISPLAY_SYNC_FAILED')
  expect(f.archive.archiveSession).not.toHaveBeenCalled()
})
it('CodexArchive_UnknownOutcomeCannotReplay_005', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.nativeArchive.archive.mockRejectedValueOnce(new Error('ARCHIVE_OUTCOME_UNKNOWN'))
  await expect(f.adapter.archiveSession(row.id)).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
  await expect(f.adapter.archiveSession(row.id)).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
  expect(f.nativeArchive.archive).toHaveBeenCalledOnce()
})
it('CodexArchive_SourceMismatchNeverWrites_006', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.entry.sourceRootKey = 'different'
  await expect(f.adapter.archiveSession(row.id)).rejects.toThrow('SOURCE_CHANGED')
  expect(f.nativeArchive.archive).not.toHaveBeenCalled()
})
it('CodexArchive_ListingOldDeskArchiveNeverMigrates_007', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.archive.getArchivedSessions = () => [row.id]
  expect((await f.adapter.listSessions())[0].archived).toBe(true)
  expect(f.nativeArchive.archive).not.toHaveBeenCalled()
  expect(f.load).not.toHaveBeenCalled()
})

it('CodexArchive_RefreshesSameRootAcrossProjectsOnly_008', async () => {
  const f = fixture()
  for (const [projectId, root] of [['other-project', 'root'], ['foreign-project', 'foreign']]) f.entries.push({ ...f.entry, context: { ...f.entry.context, projectId, projectPath: '/' + projectId }, sourceRootKey: root, sessions: [] })
  const [row] = await f.adapter.listSessions()
  await f.adapter.archiveSession(row.id)
  expect(f.load.mock.calls.map(([context]) => context.projectId)).toEqual(['project', 'other-project'])
})
it('CodexArchive_LostAckThenRestoreClearsLocalBarrier_009', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.nativeArchive.archive.mockImplementationOnce(async () => { f.setArchived(true); throw new Error('ARCHIVE_OUTCOME_UNKNOWN') })
  await expect(f.adapter.archiveSession(row.id)).rejects.toThrow('ARCHIVE_OUTCOME_UNKNOWN')
  expect((await f.adapter.listSessions())[0].archived).toBe(true)
  await f.adapter.restoreArchivedSession(row.id)
  expect(f.cliUnarchive.unarchive).toHaveBeenCalledOnce()
  await f.adapter.archiveSession(row.id)
  expect(f.nativeArchive.archive).toHaveBeenCalledTimes(2)
})
it('CodexArchive_RestoreRequiresFreshRootBeforeUnarchive_010', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.setArchived(true); await f.load(f.entry.context)
  f.load.mockImplementationOnce(async () => { f.entry.sourceRootKey = 'different'; return f.entry })
  await expect(f.adapter.restoreArchivedSession(row.id)).rejects.toThrow('SOURCE_CHANGED')
  expect(f.cliUnarchive.unarchive).not.toHaveBeenCalled()
})

// 后端会重新授权并读取目标，已验证历史不必先全量扫描一次才获得忙碌反馈。
it('CodexArchive_BusyResponseDoesNotWaitForRedundantHistoryScan_011', async () => {
  const f = fixture(), [row] = await f.adapter.listSessions()
  f.load.mockRejectedValueOnce(new Error('REDUNDANT_HISTORY_SCAN'))
  f.nativeArchive.archive.mockRejectedValueOnce(new Error('ARCHIVE_BUSY'))
  await expect(f.adapter.archiveSession(row.id)).rejects.toThrow('ARCHIVE_BUSY')
  expect(f.load).not.toHaveBeenCalled()
  expect(f.archive.archiveSession).not.toHaveBeenCalled()
})

it('OpenCodeArchive_NativeHistoryRequiresSeparateRestore_012', async () => {
  const f = fixture()
  f.entry.context.cli = 'opencode'
  f.entry.source = { ...f.entry.source!, cli: 'opencode' }
  f.entry.sessions[0].archived = true
  const [row] = await f.adapter.listSessions()
  f.archive.getArchivedSessions = () => [row.id]
  expect(selectSessionPrimaryAction(row)).toBeNull()
  expect(selectSessionMenuActions(row).some(action => action.id === 'restore-archive')).toBe(false)
  await expect(f.adapter.restoreArchivedSession(row.id)).rejects.toThrow('OPENCODE_NATIVE_ARCHIVE_RESTORE_REQUIRED')
  expect(f.cliUnarchive.unarchive).not.toHaveBeenCalled()
  expect(f.archive.restoreSession).not.toHaveBeenCalled()
  f.entry.sessions[0].archived = false
  const [restored] = await f.adapter.listSessions()
  expect(restored.archived).toBe(true)
  expect(selectSessionPrimaryAction(restored)).toBe('restore-archive')
  await f.adapter.restoreArchivedSession(restored.id)
  expect(f.cliUnarchive.unarchive).not.toHaveBeenCalled()
  expect(f.archive.restoreSession).toHaveBeenCalledOnce()
})