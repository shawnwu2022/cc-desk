import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import NativeSessionDeletePanel from '@/components/sessions/NativeSessionDeletePanel.vue'
import { useNativeHistoryStore, nativeHistoryContextKey } from '@/stores/nativeHistory'
import { useNativeSessionDeletesStore } from '@/stores/nativeSessionDeletes'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useCliSupportStore } from '@/stores/cliSupport'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { SourceRef } from '@/types/nativeProjection'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
const io = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
const nativeId = '40000000-0000-4000-8000-000000000001'
const previewId = '40000000-0000-4000-8000-000000000002'
const context = { cli: 'codex' as const, profileId: 'profile', profileRevision: '1', projectId: 'project', projectPath: '/synthetic/project' }
const source: SourceRef = { scopeId: 'scope', instanceId: 'instance', cli: 'codex', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '1', projectIds: ['project'] } }
const target = (): UnifiedSession => ({ id: 'desk', projectKey: 'project', projectPath: context.projectPath, cli: 'codex', runtime: 'native-cli', title: 'Synthetic archived session', processState: 'stopped', attentionState: 'none', lastActivityAt: 1, archived: true, opened: false, resumable: true, adapterSessionId: 'adapter', nativeSessionId: nativeId, nativeRootKey: 'root', nativeHistorySessionKey: 'key', nativeOrigin: context })
let view: VueWrapper | undefined
beforeEach(() => {
  setActivePinia(createPinia()); vi.resetAllMocks()
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke: io.invoke } })
  useNativeHistoryStore().entries.set(nativeHistoryContextKey(context), { key: nativeHistoryContextKey(context), context, source, sourceRootKey: 'root', loaded: true, loading: false, error: null, requestEpoch: '1', sessions: [{ type: 'session', sessionKey: 'key', nativeSessionId: nativeId, projectId: 'project', title: 'Synthetic archived session', truncated: false, archived: true, cwd: context.projectPath, updatedAt: null }] })
  io.invoke.mockImplementation(async (command, request) => {
    if (command === 'cli_preview_session_delete') return { operationId: request.operationId, previewId, nativeSessionId: nativeId, cli: 'codex', state: 'ready', rootArchived: true, scope: 'native-subtree', knownDescendantCount: null, reason: null }
    if (command === 'native_get_scope') return source
    if (command === 'native_list_resources') return { source: request.source, resourceKind: 'history', requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: [], hasMore: false }
    return { operationId: request.operationId, cli: 'codex', state: 'acknowledged', rootPresent: false, descendants: 'unverified', reason: null }
  })
})
afterEach(() => { view?.unmount(); view = undefined; delete (window as any).__CC_DESK_DOCUMENT__; document.body.innerHTML = '' })
function render() { view = mount(NativeSessionDeletePanel, { props: { target: target(), active: true }, attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } }); return view }
const element = (selector: string) => document.querySelector<HTMLElement>(selector)!
async function review() { const w = render(); await w.get('[data-native-cleanup-delete]').trigger('click'); await flushPromises(); return w }
async function acknowledge(selector: string) { const checkbox = element(selector) as HTMLInputElement; checkbox.checked = true; checkbox.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises() }

describe('native permanent deletion review and second confirmation', () => {
  it('PermanentDeleteUi_CancelBeforeFinalConfirmationDoesNotMutate_001', async () => {
    await review()
    expect(element('[data-delete-dialog]').textContent).toContain(nativeId)
    expect(element('[data-delete-dialog]').textContent).toContain('not a frozen list')
    element('[data-delete-next-confirm]').click(); await flushPromises()
    expect((element('[data-delete-final]') as HTMLButtonElement).disabled).toBe(true)
    await acknowledge('[data-delete-ack-permanent]')
    expect((element('[data-delete-final]') as HTMLButtonElement).disabled).toBe(true)
    element('[data-delete-cancel]').click(); await flushPromises()
    expect(io.invoke.mock.calls.map(([command]) => command)).toEqual(['cli_preview_session_delete'])
  })
  it('PermanentDeleteUi_SecondConfirmationIssuesOneNativeMutation_002', async () => {
    await review(); element('[data-delete-next-confirm]').click(); await flushPromises()
    await acknowledge('[data-delete-ack-permanent]'); await acknowledge('[data-delete-ack-subtree]')
    const button = element('[data-delete-final]'); button.click(); button.click(); await flushPromises()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cli_confirm_session_delete')).toHaveLength(1)
    expect(element('[data-delete-result]').textContent).toContain('acknowledged')
    expect(element('[data-delete-result]').textContent).toContain('not independently verified')
    expect(io.invoke.mock.calls.filter(([command]) => command === 'native_list_resources')).toHaveLength(1)
  })
  it.each([false, true])('PermanentDeleteUi_ReReadsHistoryWithoutDiscardingOtherRows_010 (read failure=%s)', async readFailure => {
    const history = useNativeHistoryStore(), catalog = useUnifiedSessionsStore()
    useCliSupportStore().setCliEnabled('codex', true)
    const controlId = '40000000-0000-4000-8000-000000000010'
    const control = { ...history.get(context)!.sessions[0]!, sessionKey: JSON.stringify(['local', 'codex', 'root', controlId]), nativeSessionId: controlId, title: 'Unrelated synthetic session' }
    history.get(context)!.sessions.push(control)
    const unused = () => { throw new Error('unrelated native action') }
    catalog.configureAdapters([createNativeCliAdapter({
      tabs: { tabs: new Map(), activeTabId: null, setActive: unused, close: unused, rename: unused },
      history,
      runtime: { createTab: unused, restartTab: unused, stopTab: async () => unused(), renameTab: async () => unused() },
      archive: { getArchivedSessions: () => [], archiveSession: async () => unused(), restoreSession: async () => unused() },
    })])
    await catalog.refresh(context.projectPath, { cli: 'codex' })
    expect(catalog.sessions.map(row => row.nativeSessionId)).toContain(controlId)
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation(async (command, request) => {
      if (command === 'native_get_scope' && readFailure) throw { code: 'SOURCE_UNAVAILABLE' }
      if (command === 'native_list_resources') return { source: request.source, resourceKind: 'history', requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: [control], hasMore: false }
      return previous(command, request)
    })
    await review(); element('[data-delete-next-confirm]').click(); await flushPromises()
    await acknowledge('[data-delete-ack-permanent]'); await acknowledge('[data-delete-ack-subtree]')
    element('[data-delete-final]').click(); await flushPromises()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'native_get_scope')).toHaveLength(1)
    expect(catalog.sessions.map(row => row.nativeSessionId)).toContain(controlId)
    if (!readFailure) expect(catalog.sessions.map(row => row.nativeSessionId)).not.toContain(nativeId)
    else expect(history.get(context)?.error).toBeTruthy()
    expect(element('[data-delete-result]').textContent).toContain('acknowledged')
  })
  it('PermanentDeleteUi_SourceOrActivityChangeInvalidatesReview_003', async () => {
    const w = await review()
    element('[data-delete-next-confirm]').click(); await flushPromises()
    await acknowledge('[data-delete-ack-permanent]'); await acknowledge('[data-delete-ack-subtree]')
    await w.setProps({ target: { ...target(), processState: 'running' } }); await flushPromises()
    expect(document.querySelector('[data-delete-final]')).toBeNull()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cli_confirm_session_delete')).toHaveLength(0)
  })
  it('PermanentDeleteUi_UnknownOnlyOffersReadOnlyInspection_004', async () => {
    io.invoke.mockImplementation(async (command, request) => command === 'cli_preview_session_delete' ? { operationId: request.operationId, previewId, nativeSessionId: nativeId, cli: 'codex', state: 'ready', rootArchived: true, scope: 'native-subtree', knownDescendantCount: null, reason: null } : { operationId: request.operationId, cli: 'codex', state: 'unknown', rootPresent: false, descendants: 'unverified', reason: 'DELETE_OUTCOME_UNKNOWN' })
    await review(); element('[data-delete-next-confirm]').click(); await flushPromises()
    await acknowledge('[data-delete-ack-permanent]'); await acknowledge('[data-delete-ack-subtree]')
    element('[data-delete-final]').click(); await flushPromises()
    expect(document.querySelector('[data-delete-final]')).toBeNull()
    expect(element('[data-delete-result]').textContent).toContain('unknown')
    element('[data-delete-inspect]').click(); await flushPromises()
    expect(io.invoke.mock.calls.map(([command]) => command)).toEqual(['cli_preview_session_delete', 'cli_confirm_session_delete', 'cli_inspect_session_delete'])
    expect(element('[data-delete-result]').textContent).toContain('unknown')
  })
  it.each([false, true])('PermanentDeleteUi_InspectedSuccessRefreshesOriginalSource_011 (read failure=%s)', async readFailure => {
    const history = useNativeHistoryStore(), catalog = useUnifiedSessionsStore()
    const controlId = '40000000-0000-4000-8000-000000000011'
    const control = { ...history.get(context)!.sessions[0]!, sessionKey: JSON.stringify(['local', 'codex', 'root', controlId]), nativeSessionId: controlId }
    history.get(context)!.sessions.push(control)
    const refresh = vi.spyOn(catalog, 'refresh').mockResolvedValue(undefined)
    const previous = io.invoke.getMockImplementation()!
    let resolveInspect!: (value: unknown) => void
    io.invoke.mockImplementation(async (command, request) => {
      if (command === 'cli_confirm_session_delete') return { operationId: request.operationId, cli: 'codex', state: 'unknown', rootPresent: null, descendants: 'unverified', reason: 'DELETE_OWNER_PENDING' }
      if (command === 'cli_inspect_session_delete') return new Promise(done => { resolveInspect = done })
      if (command === 'native_get_scope' && readFailure) throw { code: 'SOURCE_UNAVAILABLE' }
      if (command === 'native_list_resources') return { source: request.source, resourceKind: 'history', requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: [control], hasMore: false }
      return previous(command, request)
    })
    const w = await review(); element('[data-delete-next-confirm]').click(); await flushPromises()
    await acknowledge('[data-delete-ack-permanent]'); await acknowledge('[data-delete-ack-subtree]')
    element('[data-delete-final]').click(); await flushPromises()
    const operationId = io.invoke.mock.calls.find(([command]) => command === 'cli_confirm_session_delete')![1].operationId
    element('[data-delete-inspect]').click(); await flushPromises()
    await w.setProps({ target: { ...target(), id: 'different-target', projectPath: '/synthetic/other', nativeOrigin: { ...context, projectId: 'other', projectPath: '/synthetic/other', profileId: 'other-profile' } } })
    resolveInspect({ operationId, cli: 'codex', state: 'acknowledged', rootPresent: false, descendants: 'unverified', reason: null }); await flushPromises()
    expect(refresh).toHaveBeenCalledExactlyOnceWith(context.projectPath, { cli: 'codex' })
    expect(io.invoke.mock.calls.find(([command]) => command === 'native_get_scope')?.[1]).toMatchObject({ profileId: context.profileId, projectIds: [context.projectId] })
    expect(history.get(context)!.sessions.map(row => row.nativeSessionId)).toContain(controlId)
    if (readFailure) expect(history.get(context)?.error).toBeTruthy()
    else expect(history.get(context)!.sessions.map(row => row.nativeSessionId)).not.toContain(nativeId)
    expect(element('[data-delete-result]').textContent).toContain('acknowledged')
    io.invoke.mockImplementation(async (command, request) => command === 'cli_inspect_session_delete'
      ? { operationId, cli: 'codex', state: 'acknowledged', rootPresent: false, descendants: 'unverified', reason: null }
      : previous(command, request))
    element('[data-delete-inspect]').click(); await flushPromises()
    expect(refresh).toHaveBeenCalledTimes(1)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cli_confirm_session_delete')).toHaveLength(1)
  })
  it('PermanentDeleteUi_DeskOnlyArchiveCannotAuthorizeNativeDeletion_005', async () => {
    useNativeHistoryStore().entries.get(nativeHistoryContextKey(context))!.sessions[0]!.archived = false
    const w = render()
    expect(w.get('[data-native-cleanup-delete]').attributes('disabled')).toBeDefined()
    await w.get('[data-native-cleanup-delete]').trigger('click')
    expect(io.invoke).not.toHaveBeenCalled()
  })
  it('PermanentDeleteUi_BlockedPreviewRequiresFreshTwoStepReview_012', async () => {
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation(async (command, request) => ['cli_confirm_session_delete', 'cli_inspect_session_delete'].includes(command)
      ? { operationId: request.operationId, cli: 'codex', state: 'blocked', rootPresent: null, descendants: 'unverified', reason: 'DELETE_PREVIEW_CHANGED' }
      : previous(command, request))
    await review(); element('[data-delete-next-confirm]').click(); await flushPromises()
    await acknowledge('[data-delete-ack-permanent]'); await acknowledge('[data-delete-ack-subtree]')
    element('[data-delete-final]').click(); await flushPromises()
    expect(element('[data-delete-result]').textContent).toContain('blocked')
    element('[data-delete-inspect]').click(); await flushPromises()
    element('[data-delete-review-again]').click(); await flushPromises()
    const previews = io.invoke.mock.calls.filter(([command]) => command === 'cli_preview_session_delete')
    expect(previews).toHaveLength(2)
    expect(previews[0]![1].operationId).not.toBe(previews[1]![1].operationId)
    expect(document.querySelector('[data-delete-final]')).toBeNull()
    element('[data-delete-next-confirm]').click(); await flushPromises()
    expect((element('[data-delete-final]') as HTMLButtonElement).disabled).toBe(true)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cli_confirm_session_delete')).toHaveLength(1)
  })
  it('PermanentDeleteUi_MissingBridgeFailsBeforeMutation_006', async () => {
    delete (window as any).__CC_DESK_DOCUMENT__
    const w = render()
    await w.get('[data-native-cleanup-delete]').trigger('click'); await flushPromises()
    expect(document.querySelector('[data-delete-final]')).toBeNull()
    expect(element('[data-delete-result]').textContent).toContain('blocked')
    expect(io.invoke).not.toHaveBeenCalled()
  })
  it('PermanentDeleteUi_LatePreviewCannotCloseNewTarget_007', async () => {
    const secondId = '40000000-0000-4000-8000-000000000003'
    const entry = useNativeHistoryStore().entries.get(nativeHistoryContextKey(context))!
    entry.sessions.push({ ...entry.sessions[0]!, sessionKey: 'key-b', nativeSessionId: secondId })
    let resolveFirst!: (value: unknown) => void, firstOperation = ''
    io.invoke.mockImplementation(async (_command, request) => {
      if (request.sessionKey === 'key') { firstOperation = request.operationId; return new Promise(done => { resolveFirst = done }) }
      return { operationId: request.operationId, previewId, nativeSessionId: secondId, cli: 'codex', state: 'ready', rootArchived: true, scope: 'native-subtree', knownDescendantCount: null, reason: null }
    })
    const w = render(); await w.get('[data-native-cleanup-delete]').trigger('click')
    await w.setProps({ target: { ...target(), id: 'desk-b', nativeSessionId: secondId, nativeHistorySessionKey: 'key-b' } })
    await w.get('[data-native-cleanup-delete]').trigger('click'); await flushPromises()
    resolveFirst({ operationId: firstOperation, previewId, nativeSessionId: nativeId, cli: 'codex', state: 'ready', rootArchived: true, scope: 'native-subtree', knownDescendantCount: null, reason: null }); await flushPromises()
    expect(element('[data-delete-dialog]').textContent).toContain(secondId)
    expect(document.querySelector('[data-delete-next-confirm]')).not.toBeNull()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cli_confirm_session_delete')).toHaveLength(0)
  })
  it('PermanentDeleteUi_OtherTargetsReviewCannotBorrowCurrentOwner_008', async () => {
    const secondId = '40000000-0000-4000-8000-000000000003'
    const store = useNativeSessionDeletesStore()
    await store.prepare({ operationId: '40000000-0000-4000-8000-000000000004', source, sessionKey: 'key' })
    const entry = useNativeHistoryStore().entries.get(nativeHistoryContextKey(context))!
    entry.sessions.push({ ...entry.sessions[0]!, sessionKey: 'key-b', nativeSessionId: secondId })
    const w = render(); await w.setProps({ target: { ...target(), id: 'desk-b', nativeSessionId: secondId, nativeHistorySessionKey: 'key-b' } })
    await w.get('[data-delete-existing]').trigger('click'); await flushPromises()
    element('[data-delete-next-confirm]').click(); await flushPromises()
    await acknowledge('[data-delete-ack-permanent]'); await acknowledge('[data-delete-ack-subtree]')
    expect((element('[data-delete-final]') as HTMLButtonElement).disabled).toBe(true)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cli_confirm_session_delete')).toHaveLength(0)
  })
  it('PermanentDeleteUi_ReReviewCannotDiscardAnotherTargetsResult_009', async () => {
    const secondId = '40000000-0000-4000-8000-000000000003'
    io.invoke.mockImplementation(async (command, request) => command === 'cli_preview_session_delete'
      ? { operationId: request.operationId, previewId, nativeSessionId: nativeId, cli: 'codex', state: 'ready', rootArchived: true, scope: 'native-subtree', knownDescendantCount: null, reason: null }
      : { operationId: request.operationId, cli: 'codex', state: 'blocked', rootPresent: true, descendants: 'unverified', reason: 'DELETE_PREVIEW_CHANGED' })
    const store = useNativeSessionDeletesStore()
    await store.prepare({ operationId: '40000000-0000-4000-8000-000000000004', source, sessionKey: 'key' })
    const key = [...store.records.keys()][0]!
    store.beginConfirmation(key); await store.execute(key, true, true, () => true)
    const entry = useNativeHistoryStore().entries.get(nativeHistoryContextKey(context))!
    entry.sessions.push({ ...entry.sessions[0]!, sessionKey: 'key-b', nativeSessionId: secondId })
    const w = render(); await w.setProps({ target: { ...target(), id: 'desk-b', nativeSessionId: secondId, nativeHistorySessionKey: 'key-b' } })
    await w.get('[data-delete-existing]').trigger('click'); await flushPromises()
    expect(document.querySelector('[data-delete-review-again]')).toBeNull()
    expect(store.records.get(key)?.receipt?.state).toBe('blocked')
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cli_preview_session_delete')).toHaveLength(1)
  })
})
