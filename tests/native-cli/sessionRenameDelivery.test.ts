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
async function harness(kind: 'native' | 'legacy', write?: (f: NativeInputFrame) => Promise<InputWriteReceipt>) {
  setActivePinia(createPinia())
  vi.stubGlobal('crypto', webcrypto)
  const writes: string[] = []
  const records = new Map()
  const metadata = { sessionRecords: records, upsertSessionRecord: async (id: string, record: any, before?: () => void) => { before?.(); records.set(id, record) } }
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
  // 显示名只能在自身写入确认后清除，不能拿别的队列操作完成充当确认。
  it(`Rename_PasteBarrierWaitsForOwnReceipt_001 ${kind}`, async () => {
    const h = await harness(kind); const paste = deferred<Uint8Array>();
    const reserved = h.reserve(() => paste.promise)
    let done = false; const pending = h.rename().then(() => { done = true })
    await flushPromises()
    try { expect(done).toBe(false); expect(h.records.get(h.id).title).toBe('Old GUI override'); expect(h.writes).toEqual([]) }
    finally { paste.resolve(new TextEncoder().encode('paste')); await reserved.settled; await pending }
    expect(h.writes).toEqual(['paste', '/rename New name\r']); expect(h.records.get(h.id).title).toBe('')
  })
  // 已开始 writer 的改名不再属于可撤销草稿，切换选择仍须等待其回执再收尾。
  it(`Rename_SelectionAfterDispatchKeepsCleanup_002 ${kind}`, async () => {
    const receipt = deferred<InputWriteReceipt>(); const h = await harness(kind, () => receipt.promise)
    const result = h.rename().catch(error => error)
    await vi.waitFor(() => expect(h.writes).toEqual(['/rename New name\r']))
    h.catalog.activeSessionId = null
    receipt.resolve({ runId: 'run', generation: 1, inputSeq: '1', modeEpoch: '1', state: 'host-written', confirmedBytes: String(new TextEncoder().encode('/rename New name\r').length) })
    expect(await result).toBe('input-written'); expect(h.records.get(h.id).title).toBe(''); expect(h.name()).toBe('New name')
  })
  // FIFO 内未提交的改名撤销后必须拒绝，后续剪贴板完成不能把它再写出去。
  it(`Rename_SelectionBeforeDispatchCancels_003 ${kind}`, async () => {
    const h = await harness(kind); const paste = deferred<Uint8Array>(); const reserved = h.reserve(() => paste.promise)
    const result = h.rename().catch(error => error); await flushPromises(); h.catalog.activeSessionId = null
    paste.resolve(new TextEncoder().encode('paste')); await reserved.settled
    expect(await result).toBeInstanceOf(Error); expect(h.writes).toEqual(['paste']); expect(h.records.get(h.id).title).toBe('Old GUI override')
  })
  // 会话删除或新代次替代都不能把旧改名写入或改名结果转交给新 owner。
  it.each(['invalidate', 'replace'] as const)(`Rename_OwnerInvalidBeforeDispatch_004 ${kind} %s`, async action => {
    const h = await harness(kind); const paste = deferred<Uint8Array>(); const reserved = h.reserve(() => paste.promise)
    const result = h.rename().catch(error => error); await flushPromises(); h[action](); await h.catalog.refresh()
    paste.resolve(new TextEncoder().encode('paste')); await reserved.settled
    expect(await result).toBeInstanceOf(Error); expect(h.writes).not.toContain('/rename New name\r'); expect(h.records.get(h.id).title).toBe('Old GUI override')
  })
  it(`Rename_OwnerInvalidAfterDispatchKeepsOverride_005 ${kind}`, async () => {
    const receipt = deferred<InputWriteReceipt>(); const h = await harness(kind, () => receipt.promise)
    const result = h.rename().catch(error => error); await vi.waitFor(() => expect(h.writes).toHaveLength(1))
    h.invalidate(); await h.catalog.refresh()
    receipt.resolve({ runId: 'run', generation: 1, inputSeq: '1', modeEpoch: '1', state: 'host-written', confirmedBytes: '17' })
    expect(await result).toBeInstanceOf(Error); expect(h.records.get(h.id).title).toBe('Old GUI override')
  })
}
for (const fault of ['reject', 'partial', 'mismatch', 'short'] as const) {
  it(`Rename_NativeFailurePreservesOverride_006 ${fault}`, async () => {
    const h = await harness('native', async f => {
      if (fault === 'reject') throw new Error('SECRET_ERROR_MUST_NOT_ESCAPE')
      return fault === 'partial' ? { ...exact(f), state: 'partial-or-unknown', confirmedBytes: '0' } : fault === 'mismatch' ? { ...exact(f), inputSeq: '99' } : { ...exact(f), confirmedBytes: '1' }
    })
    await expect(h.rename()).rejects.toThrow(); expect(h.records.get(h.id).title).toBe('Old GUI override'); expect(h.name()).toBe('Original CLI title')
  })
}
it('Rename_NativePausedQueueRejectsWithoutNewWrite_007', async () => {
  const h = await harness('native', async () => { throw new Error('WRITE_DENIED') })
  await h.binding.sendUserText('previous input').catch(() => {})
  await expect(h.rename()).rejects.toThrow(); expect(h.writes).toEqual(['previous input']); expect(h.records.get(h.id).title).toBe('Old GUI override')
})
it('Rename_NativeClipboardFailureRejects_008', async () => {
  const h = await harness('native'); const paste = deferred<Uint8Array>(); const reserved = h.reserve(() => paste.promise)
  const result = h.rename().catch(error => error); await flushPromises(); paste.reject(new Error('CLIPBOARD_DENIED')); await reserved.settled
  expect(await result).toBeInstanceOf(Error); expect(h.writes).toEqual([]); expect(h.records.get(h.id).title).toBe('Old GUI override')
})

for (const kind of ['native', 'legacy'] as const) {
  // 撤销不依赖剪贴板或前一条 writer 恢复，原始 pending reservation 独立保留。
  it(`Rename_CancelSettlesBeforeClipboard_009 ${kind}`, async () => {
    const h = await harness(kind); const paste = deferred<Uint8Array>(); const reserved = h.reserve(() => paste.promise)
    const result = h.rename().catch(error => error); await flushPromises(); h.catalog.activeSessionId = null
    try { expect(await result).toBeInstanceOf(Error); expect(h.writes).toEqual([]); expect(h.records.get(h.id).title).toBe('Old GUI override') }
    finally { paste.resolve(new TextEncoder().encode('paste')); await reserved.settled }
    expect(h.writes).toEqual(['paste'])
  })
  it(`Rename_DisposeUnsubmittedKeepsOverride_010 ${kind}`, async () => {
    const h = await harness(kind); const paste = deferred<Uint8Array>(); const reserved = h.reserve(() => paste.promise)
    const result = h.rename().catch(error => error); await flushPromises()
    if (kind === 'native') h.binding.dispose(); else h.legacyQueue.dispose()
    expect(await result).toBeInstanceOf(Error); expect(h.records.get(h.id).title).toBe('Old GUI override')
    paste.resolve(new TextEncoder().encode('paste')); await reserved.settled; expect(h.writes).toEqual([])
  })
  it(`Rename_WriteFailureAfterSelectionKeepsOverride_011 ${kind}`, async () => {
    const receipt = deferred<InputWriteReceipt>(); const h = await harness(kind, () => receipt.promise)
    const result = h.rename().catch(error => error); await vi.waitFor(() => expect(h.writes).toHaveLength(1))
    h.catalog.activeSessionId = null; receipt.reject(new Error('WRITE_DENIED'))
    expect(await result).toBeInstanceOf(Error); expect(h.records.get(h.id).title).toBe('Old GUI override'); expect(h.name()).toBe('Original CLI title')
  })
}
