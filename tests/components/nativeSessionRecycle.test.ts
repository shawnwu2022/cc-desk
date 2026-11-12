import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import NativeSessionRecyclePanel from '@/components/sessions/NativeSessionRecyclePanel.vue'
import { useNativeHistoryStore, nativeHistoryContextKey } from '@/stores/nativeHistory'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { SourceRef } from '@/types/nativeProjection'
import type { RecycleRecord } from '@/api/nativeSessionRecycle'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
const io = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
const nativeId = '70000000-0000-4000-8000-000000000001'
const context = { cli: 'claude' as const, profileId: 'profile', profileRevision: '1', projectId: 'project', projectPath: '/synthetic/project' }
const source: SourceRef = { scopeId: 'scope', instanceId: 'instance', cli: 'claude', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '1', projectIds: ['project'] } }
const target = (): UnifiedSession => ({ id: 'desk', projectKey: 'project', projectPath: context.projectPath, cli: 'claude', runtime: 'native-cli', title: 'Synthetic archived session', processState: 'stopped', attentionState: 'none', lastActivityAt: 1, archived: true, opened: false, resumable: true, adapterSessionId: 'adapter', nativeSessionId: nativeId, nativeRootKey: 'root', nativeHistorySessionKey: 'key', nativeOrigin: context })
let view: VueWrapper | undefined, saved: RecycleRecord[]
beforeEach(() => {
  setActivePinia(createPinia()); vi.resetAllMocks(); saved = []
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke: io.invoke } })
  const history = useNativeHistoryStore()
  history.entries.set(nativeHistoryContextKey(context), { key: nativeHistoryContextKey(context), context, source, sourceRootKey: 'root', loaded: true, loading: false, error: null, requestEpoch: '1', sessions: [{ type: 'session', sessionKey: 'key', nativeSessionId: nativeId, projectId: 'project', title: 'Synthetic archived session', truncated: false, archived: false, cwd: context.projectPath, updatedAt: null }] })
  vi.spyOn(history, 'load').mockImplementation(async () => history.get(context)!)
  vi.spyOn(useUnifiedSessionsStore(), 'refresh').mockResolvedValue(undefined)
  io.invoke.mockImplementation(async (command, request) => {
    if (command !== 'cli_recycle_session') throw new Error('unexpected command')
    if (request.action === 'remove') saved = [{ operationId: request.operationId, nativeSessionId: nativeId, projectPath: context.projectPath, state: 'stored', originalOccupied: false, scope: 'claude-root-transcript' }]
    if (request.action === 'restore') saved = saved.map(item => ({ ...item, state: 'source', originalOccupied: true }))
    return { records: saved, unavailableCount: 0 }
  })
})
afterEach(() => { view?.unmount(); view = undefined; delete (window as any).__CC_DESK_DOCUMENT__; document.body.innerHTML = '' })
function render(sessions = [target()]) { view = mount(NativeSessionRecyclePanel, { props: { sessions, active: true }, attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } }); return view }
const element = (selector: string) => document.querySelector<HTMLElement>(selector)!
async function show(sessions = [target()]) { const w = render(sessions); await flushPromises(); return w }
const mutations = () => io.invoke.mock.calls.filter(([, request]) => request.action !== 'list')
const retained = (extra: Partial<RecycleRecord> = {}): RecycleRecord => ({ operationId: '70000000-0000-4000-8000-000000000002', nativeSessionId: nativeId, projectPath: context.projectPath, state: 'stored', originalOccupied: false, scope: 'claude-root-transcript', ...extra })

describe('legacy Claude recovery compatibility', () => {
  it('NativeRecycleUi_EmptyHistoryOffersNoNewRemoval_001', async () => {
    await show()
    expect(element('[data-recycle-open]')).toBeNull()
    expect(element('[data-recycle-select]')).toBeNull()
    expect(element('[data-recycle-remove]')).toBeNull()
    expect(element('[data-recycle-legacy]')).toBeNull()
    expect(mutations()).toHaveLength(0)
  })
  it('NativeRecycleUi_RetainedOriginalCanStillBeRestoredOnce_002', async () => {
    saved = [retained()]; await show([])
    expect(element('[data-recycle-records]').textContent).toContain(nativeId)
    const restore = element('[data-recycle-restore]'); restore.click(); restore.click(); await flushPromises()
    expect(mutations()).toHaveLength(1)
    expect(mutations()[0]![1]).toMatchObject({ source, action: 'restore', operationId: saved[0]!.operationId, sessionKey: null, acknowledgeRecoverable: true })
    expect(element('[data-recycle-legacy]')).toBeNull()
    expect(useUnifiedSessionsStore().refresh).toHaveBeenCalledWith(context.projectPath, { cli: 'claude' })
  })
  it('NativeRecycleUi_ConflictAndUnknownKeepOriginalUntouched_003', async () => {
    saved = [retained({ originalOccupied: true })]; await show([])
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true)
    saved = [retained({ state: 'unknown' })]
    element('[data-recycle-refresh]').click(); await flushPromises()
    expect(element('[data-recycle-restore]')).toBeNull()
    expect(element('[data-recycle-records]').textContent).toContain(en.recycleState_unknown)
    expect(mutations()).toHaveLength(0)
  })
  it('NativeRecycleUi_CloseOrChangedSourceCannotRestoreStaleRecord_004', async () => {
    saved = [retained()]; const w = await show([])
    const button = element('[data-recycle-restore]')
    useNativeHistoryStore().get(context)!.source = { ...source, profileRevision: '2' }
    button.click(); await flushPromises()
    expect(mutations()).toHaveLength(0)
    await w.setProps({ active: false }); await flushPromises()
    expect(element('[data-recycle-legacy]')).toBeNull()
  })
  it('NativeRecycleUi_LateReadCannotLeakAcrossProjectOrReopen_005', async () => {
    let finish!: (value: unknown) => void
    io.invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    const w = render([]); await flushPromises()
    await w.setProps({ active: false, projectPath: '/other/project' })
    finish({ records: [retained()], unavailableCount: 0 }); await flushPromises()
    await w.setProps({ active: true }); await flushPromises()
    expect(element('[data-recycle-legacy]')).toBeNull()
    expect(mutations()).toHaveLength(0)
  })
  it('NativeRecycleUi_UnknownRestoreMustBeReadBeforeRetry_006', async () => {
    saved = [retained()]; await show([])
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation(async (command, request) => { if (request.action === 'restore') throw { code: 'RECYCLE_OUTCOME_UNKNOWN', privatePath: 'secret' }; return previous(command, request) })
    element('[data-recycle-restore]').click(); await flushPromises()
    expect(element('[data-recycle-error]').textContent).toBe('RECYCLE_OUTCOME_UNKNOWN')
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true)
    element('[data-recycle-restore]').click(); await flushPromises()
    expect(mutations()).toHaveLength(1)
    saved = [retained({ state: 'source' })]
    element('[data-recycle-refresh]').click(); await flushPromises()
    expect(element('[data-recycle-restore]')).toBeNull()
    expect(mutations()).toHaveLength(1)
  })
  it('NativeRecycleUi_DamagedRecordsAndReadFailureRemainVisible_007', async () => {
    io.invoke.mockResolvedValueOnce({ records: [], unavailableCount: 1 }); await show([])
    expect(element('[data-recycle-legacy]').textContent).toContain('1 recovery records')
    io.invoke.mockRejectedValueOnce({ code: 'RECYCLE_UNAVAILABLE', privatePath: 'secret' })
    element('[data-recycle-refresh]').click(); await flushPromises()
    expect(element('[data-recycle-error]').textContent).toBe('RECYCLE_UNAVAILABLE')
    expect(mutations()).toHaveLength(0)
  })
  it('NativeRecycleUi_ReopenDuringRestoreKeepsInFlightBarrier_008', async () => {
    saved = [retained()]; await show([])
    let reject!: (error: unknown) => void
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => request.action === 'restore'
      ? new Promise((_, failure) => { reject = failure }) : previous(command, request))
    element('[data-recycle-restore]').click(); await flushPromises()
    view!.unmount(); view = undefined; await show([])
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true)
    element('[data-recycle-restore]').click(); await flushPromises()
    expect(mutations()).toHaveLength(1)
    reject({ code: 'RECYCLE_OUTCOME_UNKNOWN' }); await flushPromises()
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true)
    element('[data-recycle-refresh]').click(); await flushPromises()
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(false)
    expect(mutations()).toHaveLength(1)
  })

  it('NativeRecycleUi_HistoryReadFailureRetainsDisabledRecoveryAndCanRecheck_009', async () => {
    saved = [retained()]; await show([])
    const entry = useNativeHistoryStore().get(context)!
    entry.loading = true; await flushPromises()
    expect(element('[data-recycle-records]').textContent).toContain(nativeId)
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true)
    entry.loading = false; entry.error = 'SOURCE_UNAVAILABLE'; await flushPromises()
    expect(element('[data-recycle-records]').textContent).toContain(nativeId)
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true)
    vi.mocked(useNativeHistoryStore().load).mockImplementation(async () => { entry.error = null; return entry })
    element('[data-recycle-refresh]').click(); await flushPromises()
    expect(useNativeHistoryStore().load).toHaveBeenCalledWith(expect.objectContaining({ ...context, force: true, retainOnFailure: true }))
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(false)
    expect(mutations()).toHaveLength(0)
  })

  it('NativeRecycleUi_RestoreReadFailureCannotResendAndRecheckFindsOriginal_010', async () => {
    saved = [retained()]; await show([])
    const entry = useNativeHistoryStore().get(context)!
    vi.mocked(useNativeHistoryStore().load).mockImplementation(async () => { entry.error = 'SOURCE_UNAVAILABLE'; throw { code: 'SOURCE_UNAVAILABLE' } })
    element('[data-recycle-restore]').click(); await flushPromises()
    expect(mutations()).toHaveLength(1)
    expect(element('[data-recycle-source-unavailable]')).not.toBeNull()
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true)
    vi.mocked(useNativeHistoryStore().load).mockImplementation(async () => { entry.error = null; return entry })
    element('[data-recycle-refresh]').click(); await flushPromises()
    expect(element('[data-recycle-legacy]')).toBeNull()
    expect(mutations()).toHaveLength(1)
  })

  it('NativeRecycleUi_UnavailableRootCountSurvivesOtherRootRefresh_011', async () => {
    const history = useNativeHistoryStore(), first = history.get(context)!
    const otherContext = { ...context, profileId: 'other-profile', projectId: 'other-project', projectPath: '/other/project' }
    const otherSource: SourceRef = { ...source, scopeId: 'other-scope', sourceRootKey: 'other-root', profileId: 'other-profile', target: { kind: 'profile', profileId: 'other-profile', expectedProfileRevision: '1', projectIds: ['other-project'] } }
    history.entries.set(nativeHistoryContextKey(otherContext), { ...first, key: nativeHistoryContextKey(otherContext), context: otherContext, source: otherSource, sourceRootKey: 'other-root', sessions: [] })
    let damaged = 1
    io.invoke.mockImplementation(async (_command, request) => ({ records: [], unavailableCount: request.source.sourceRootKey === 'root' ? damaged : 0 }))
    await show([])
    expect(element('[data-recycle-legacy]').textContent).toContain('1 recovery records')
    first.loading = true; await flushPromises()
    expect(element('[data-recycle-legacy]').textContent).toContain('1 recovery records')
    first.error = 'SOURCE_UNAVAILABLE'; first.loading = false; await flushPromises()
    expect(element('[data-recycle-refresh]')).not.toBeNull()
    vi.mocked(history.load).mockImplementation(async () => { first.error = null; return first })
    damaged = 0
    element('[data-recycle-refresh]').click(); await flushPromises()
    expect(element('[data-recycle-legacy]')).toBeNull()
    expect(mutations()).toHaveLength(0)
  })

  it('NativeRecycleUi_UnrelatedSourceChangeKeepsUnavailableRoot_012', async () => {
    const history = useNativeHistoryStore(), first = history.get(context)!
    const otherContext = { ...context, profileId: 'other-profile', projectId: 'other-project', projectPath: '/other/project' }
    const otherSource: SourceRef = { ...source, scopeId: 'other-scope', sourceRootKey: 'other-root', profileId: 'other-profile', target: { kind: 'profile', profileId: 'other-profile', expectedProfileRevision: '1', projectIds: ['other-project'] } }
    history.entries.set(nativeHistoryContextKey(otherContext), { ...first, key: nativeHistoryContextKey(otherContext), context: otherContext, source: otherSource, sourceRootKey: 'other-root', sessions: [] })
    io.invoke.mockImplementation(async (_command, request) => ({ records: request.source.sourceRootKey === 'root' ? [retained()] : [], unavailableCount: request.source.sourceRootKey === 'root' ? 1 : 0 }))
    await show([])
    first.error = 'SOURCE_UNAVAILABLE'; await flushPromises()
    history.entries.delete(nativeHistoryContextKey(otherContext)); await flushPromises()
    expect(element('[data-recycle-records]').textContent).toContain(nativeId)
    expect(element('[data-recycle-legacy]').textContent).toContain('1 recovery records')
    expect(element('[data-recycle-refresh]')).not.toBeNull()
    expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true)
    expect(mutations()).toHaveLength(0)
  })

})
