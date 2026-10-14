import { afterEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises } from '@vue/test-utils'
import { webcrypto } from 'node:crypto'
import { createNativeTerminalBinding } from '@/terminal/nativeTerminalBinding'
import { createLegacyInputQueue } from '@/utils/pasteText'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { createLegacyClaudeAdapter } from '@/session/adapters/legacyClaudeAdapter'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useSessionStore } from '@/stores/session'
import type { NativeCliTab } from '@/stores/nativeTabs'
import type { NativeInputFrame, InputWriteReceipt } from '@/types/terminal'

type Admission = { beforeDispatch?: () => void; signal?: AbortSignal }
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: Error) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b }); return { promise, resolve, reject } }
const cleanup: (() => void)[] = []
afterEach(() => { cleanup.splice(0).forEach(fn => fn()); vi.unstubAllGlobals() })
const exact = (f: NativeInputFrame): InputWriteReceipt => ({ runId: f.runId, generation: f.generation, inputSeq: f.inputSeq, modeEpoch: f.modeEpoch, state: 'host-written', confirmedBytes: String(f.bytes.length) })
async function harness(kind: 'native' | 'legacy', write?: (f: NativeInputFrame) => Promise<InputWriteReceipt>, beforeSave?: () => Promise<void>) {
  setActivePinia(createPinia())
  vi.stubGlobal('crypto', webcrypto)
  const writes: string[] = []
  const records = new Map()
  const metadata = { sessionRecords: records, upsertSessionRecord: async (id: string, record: any, before?: () => void) => { await beforeSave?.(); before?.(); records.set(id, record) } }
  const term = { onData: () => ({ dispose() {} }), onBinary: () => ({ dispose() {} }), _core: { coreService: { onUserInput: () => ({ dispose() {} }) } }, write() {} }
  const binding = createNativeTerminalBinding({ term, runId: 'run', generation: 1, currentTarget: () => ({ runId: 'run', generation: 1, modeEpoch: '1' }), isUserInputAllowed: () => true,
    writeUser: async f => { writes.push(new TextDecoder().decode(f.bytes)); return write ? write(f) : exact(f) }, writeProtocol: vi.fn(), ackOutput: vi.fn() })
  const legacyStore = useSessionStore()
  const legacyId = legacyStore.createTab('/repo', { name: 'Original CLI title' })
  legacyStore.setTabPty(legacyId, 'pty')
  const legacyQueue = createLegacyInputQueue({ owns: () => legacyStore.tabs.get(legacyId)?.ptyId === 'pty', write: async payload => { writes.push(payload); if (write) await write({ runId: 'run', generation: 1, inputSeq: '1', modeEpoch: '1', bytes: new TextEncoder().encode(payload) }) } })
  cleanup.push(() => { binding.dispose(); legacyQueue.dispose() })
  const tab: NativeCliTab = { tabId: 'cc', cli: 'claude', projectId: 'p', projectPath: '/repo', profileId: 'claude', profileRevision: '7', requestId: 'req', runId: 'run', generation: 1, action: { kind: 'new' }, status: 'running', errorCode: null, launchRevision: '7', title: 'Original CLI title', createdAt: 1, lastActivityAt: 30 }
  const tabs = new Map([['cc', tab]])
  const native = createNativeCliAdapter({ tabs: { tabs, activeTabId: 'cc', setActive() {}, close() {}, rename: (_id, title) => { tab.title = title } }, history: { all: () => [] }, metadata,
    runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn(), renameTab: async (_id, title, admission?: Admission) => { await binding.sendUserText(`/rename ${title}\r`, admission) } }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() } })
  const legacy = createLegacyClaudeAdapter({ store: legacyStore, metadata, projectPaths: () => ['/repo'], runtime: { startTab: vi.fn(), stopTab: vi.fn(), restartTab: vi.fn(), renameTab: async (_id, title, admission?: Admission) => { await legacyQueue.enqueue(`/rename ${title}\r`, undefined, admission) } } })
  const adapter = kind === 'native' ? native : legacy
  const catalog = useUnifiedSessionsStore(); catalog.configureAdapters([adapter]); await catalog.refresh()
  const row = catalog.sessions.find(row => row.opened)!
  records.set(row.id, { runtime: row.runtime, cli: row.cli, projectPath: row.projectPath, adapterSessionId: row.adapterSessionId, nativeSessionId: row.nativeSessionId, title: 'Old GUI override', lastActivityAt: 30 })
  catalog.activeSessionId = row.id
  return { binding, legacyQueue, writes, records, catalog, adapter, id: row.id,
    reserve: (producer: () => Promise<Uint8Array>) => kind === 'native' ? binding.reserveUserPaste(producer) : legacyQueue.reserve(async () => new TextDecoder().decode(await producer())),
    invalidate: () => { if (kind === 'native') tabs.delete('cc'); else legacyStore.removeTab(legacyId) },
    replace: () => { if (kind === 'native') tabs.set('cc', { ...tab, generation: 2, title: 'Replacement' }); else legacyStore.tabs.get(legacyId)!.ptyId = 'replacement' },
    name: () => kind === 'native' ? tabs.get('cc')?.title : legacyStore.tabs.get(legacyId)?.name,
    rename: () => { catalog.beginRename(row.id); return catalog.renameSession(row.id, 'New name', true) } }
}

for (const kind of ['native', 'legacy'] as const) {
  // 已有 prompt 仍属于 CLI；GUI 改名不能追加任何字符或回车。
  it(`Rename_ExistingDraftUsesMetadata_001 ${kind}`, async () => {
    const h = await harness(kind)
    if (kind === 'native') await h.binding.sendUserText('unfinished private draft')
    else await h.legacyQueue.enqueue('unfinished private draft')
    expect(await h.rename()).toBe('display-name-saved')
    expect(h.writes).toEqual(['unfinished private draft'])
    expect(h.records.get(h.id).title).toBe('New name'); expect(h.name()).toBe('New name')
  })
  // 剪贴板 reservation 与 metadata 是独立通道；改名不排队注入 CLI。
  it(`Rename_PendingPasteDoesNotOwnDisplayName_002 ${kind}`, async () => {
    const h = await harness(kind), paste = deferred<Uint8Array>(), reserved = h.reserve(() => paste.promise)
    try { expect(await h.rename()).toBe('display-name-saved'); expect(h.writes).toEqual([]); expect(h.records.get(h.id).title).toBe('New name') }
    finally { paste.resolve(new TextEncoder().encode('paste')); await reserved.settled }
    expect(h.writes).toEqual(['paste'])
  })
  // 真实 metadata writer 边界之前切换选择，必须拒绝旧编辑 owner。
  it(`Rename_SelectionBeforeMetadataRejects_003 ${kind}`, async () => {
    const gate = deferred<void>(), h = await harness(kind, undefined, () => gate.promise)
    const result = h.rename().catch(error => error); await flushPromises(); h.catalog.activeSessionId = null
    gate.resolve(); expect(await result).toBeInstanceOf(Error)
    expect(h.writes).toEqual([]); expect(h.records.get(h.id).title).toBe('Old GUI override')
  })
  it.each(['invalidate', 'replace'] as const)(`Rename_OwnerInvalidBeforeMetadata_004 ${kind} %s`, async action => {
    const gate = deferred<void>(), h = await harness(kind, undefined, () => gate.promise)
    const result = h.rename().catch(error => error); await flushPromises(); h[action](); await h.catalog.refresh()
    gate.resolve(); expect(await result).toBeInstanceOf(Error)
    expect(h.writes).toEqual([]); expect(h.records.get(h.id).title).toBe('Old GUI override')
  })
  it(`Rename_MetadataFailureKeepsOverride_005 ${kind}`, async () => {
    const h = await harness(kind, undefined, async () => { throw new Error('REVISION_CONFLICT') })
    await expect(h.rename()).rejects.toThrow('REVISION_CONFLICT')
    expect(h.writes).toEqual([]); expect(h.records.get(h.id).title).toBe('Old GUI override'); expect(h.name()).toBe('Original CLI title')
  })
  it(`Rename_AbortedAdmissionCannotWriteMetadata_006 ${kind}`, async () => {
    const h = await harness(kind), controller = new AbortController(); controller.abort()
    await expect(h.adapter.renameSession(h.id, 'Cancelled', () => true, undefined, controller.signal)).rejects.toThrow('STALE_SESSION_ATTEMPT')
    expect(h.writes).toEqual([]); expect(h.records.get(h.id).title).toBe('Old GUI override')
  })
}
for (const fault of ['reject', 'partial', 'mismatch', 'short'] as const) {
  // metadata 改名不接触 writer；坏回执不能被当作空编辑器证明。
  it(`Rename_NativeReceiptDoesNotAuthorizeEditor_007 ${fault}`, async () => {
    const h = await harness('native', async f => {
      if (fault === 'reject') throw new Error('WRITE_DENIED')
      return fault === 'partial' ? { ...exact(f), state: 'partial-or-unknown', confirmedBytes: '0' } : fault === 'mismatch' ? { ...exact(f), inputSeq: '99' } : { ...exact(f), confirmedBytes: '1' }
    })
    expect(await h.rename()).toBe('display-name-saved'); expect(h.writes).toEqual([]); expect(h.records.get(h.id).title).toBe('New name')
  })
}
it('Rename_NativePausedQueueStillSavesDisplayName_008', async () => {
  const h = await harness('native', async () => { throw new Error('WRITE_DENIED') })
  await h.binding.sendUserText('previous input').catch(() => {})
  expect(await h.rename()).toBe('display-name-saved'); expect(h.writes).toEqual(['previous input']); expect(h.records.get(h.id).title).toBe('New name')
})
