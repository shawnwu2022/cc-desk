import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import NativeSessionBatchDeletePanel from '@/components/sessions/NativeSessionBatchDeletePanel.vue'
import { nativeHistoryContextKey, useNativeHistoryStore } from '@/stores/nativeHistory'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { SourceRef } from '@/types/nativeProjection'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'

const io = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
const uuid = (n: number) => `50000000-0000-4000-8000-${String(n).padStart(12, '0')}`
const context = { cli: 'codex' as const, profileId: 'profile', profileRevision: '1', projectId: 'project', projectPath: '/synthetic/project' }
const source: SourceRef = { scopeId: 'scope', instanceId: 'instance', cli: 'codex', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '1', projectIds: ['project'] } }
const row = (n: number): UnifiedSession => ({ id: `desk-${n}`, projectKey: 'project', projectPath: context.projectPath, cli: 'codex', runtime: 'native-cli', title: `Synthetic archive ${n}`, processState: 'stopped', attentionState: 'none', lastActivityAt: n, archived: true, opened: false, resumable: true, adapterSessionId: `adapter-${n}`, nativeSessionId: uuid(n), nativeRootKey: 'root', nativeHistorySessionKey: `key-${n}`, nativeOrigin: context })
let view: VueWrapper | undefined
let refresh: ReturnType<typeof vi.spyOn>
beforeEach(() => {
  setActivePinia(createPinia()); vi.resetAllMocks()
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke: io.invoke } })
  useNativeHistoryStore().entries.set(nativeHistoryContextKey(context), { key: nativeHistoryContextKey(context), context, source: structuredClone(source), sourceRootKey: 'root', loaded: true, loading: false, error: null, requestEpoch: '1', sessions: [1, 2, 3].map(n => ({ type: 'session', sessionKey: `key-${n}`, nativeSessionId: uuid(n), projectId: 'project', title: `Synthetic archive ${n}`, truncated: false, archived: true, cwd: context.projectPath, updatedAt: null })) })
  refresh = vi.spyOn(useUnifiedSessionsStore(), 'refresh').mockResolvedValue(undefined)
  io.invoke.mockImplementation(async (command, request) => {
    if (command === 'cli_preview_session_delete') return { operationId: request.operationId, previewId: uuid(100 + Number(request.sessionKey.split('-')[1])), nativeSessionId: uuid(Number(request.sessionKey.split('-')[1])), cli: 'codex', state: 'ready', rootArchived: true, scope: 'native-subtree', knownDescendantCount: null, reason: null }
    if (command === 'native_get_scope') return source
    if (command === 'native_list_resources') return { source: request.source, resourceKind: 'history', requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: [], hasMore: false }
    return { operationId: request.operationId, cli: 'codex', state: 'acknowledged', rootPresent: false, descendants: 'unverified', reason: null }
  })
})
afterEach(() => { view?.unmount(); view = undefined; delete (window as any).__CC_DESK_DOCUMENT__; document.body.innerHTML = '' })
function render(sessions = [row(1), row(2), row(3)]) {
  view = mount(NativeSessionBatchDeletePanel, { props: { sessions, active: true }, attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  return view
}
const element = (s: string) => document.querySelector<HTMLElement>(s)!
const calls = (command: string) => io.invoke.mock.calls.filter(([name]) => name === command)
async function click(s: string) { element(s).click(); await flushPromises() }
async function check(s: string) {
  const box = element(s) as HTMLInputElement
  box.checked = true; box.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises()
}
async function review() {
  render(); await click('[data-batch-start]'); await click('[data-batch-select-all]'); await click('[data-batch-review]')
}
async function confirm() {
  await click('[data-batch-next]')
  await check('[data-batch-ack-permanent]'); await check('[data-batch-ack-subtree]')
}
describe('native archived session batch deletion', () => {
  it('BatchDelete_OneConfirmationDeletesSelectedRootsOnceInOrder_001', async () => {
    await review()
    expect(calls('cli_preview_session_delete')).toHaveLength(3)
    expect(calls('cli_confirm_session_delete')).toHaveLength(0)
    expect(element('[data-batch-dialog]').textContent).toContain('3')
    await confirm()
    element('[data-batch-final]').click(); element('[data-batch-final]')?.click(); await flushPromises()
    expect(calls('cli_confirm_session_delete')).toHaveLength(3)
    expect(calls('cli_confirm_session_delete').map(([, r]) => r.previewId)).toEqual([uuid(101), uuid(102), uuid(103)])
    expect(element('[data-batch-result]').textContent).toContain('3 acknowledged')
    expect(calls('native_list_resources')).toHaveLength(1)
    expect(refresh).toHaveBeenCalledExactlyOnceWith(context.projectPath, { cli: 'codex' })
  })
  it('BatchDelete_CancelAndLatePreviewCannotMutateOrReopen_002', async () => {
    let finish!: (v: unknown) => void, pending: any
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => command === 'cli_preview_session_delete'
      ? new Promise(done => { finish = done; pending = request }) : previous(command, request))
    render(); await click('[data-batch-start]'); await click('[data-batch-select-all]')
    element('[data-batch-review]').click(); await flushPromises(); await click('[data-batch-close]')
    finish(await previous('cli_preview_session_delete', pending)); await flushPromises()
    expect(document.querySelector('[data-batch-dialog]')).toBeNull()
    expect(calls('cli_preview_session_delete')).toHaveLength(1)
    expect(calls('cli_confirm_session_delete')).toHaveLength(0)
  })
  it('BatchDelete_SourceChangeInvalidatesWholeUnsentConfirmation_003', async () => {
    await review(); await confirm()
    useNativeHistoryStore().get(context)!.source!.scopeId = 'replaced'
    await flushPromises()
    expect(document.querySelector('[data-batch-final]')).toBeNull()
    expect(calls('cli_confirm_session_delete')).toHaveLength(0)
  })
  it('BatchDelete_UnknownStopsRemainingAndOnlyInspectsOriginal_004', async () => {
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => command === 'cli_confirm_session_delete'
      ? Promise.reject(new Error('private transport payload')) : previous(command, request))
    await review(); await confirm(); await click('[data-batch-final]')
    expect(calls('cli_confirm_session_delete')).toHaveLength(1)
    expect(element('[data-batch-result]').textContent).toContain('2 not sent')
    expect(element('[data-batch-result]').textContent).not.toContain('private transport')
    await click('[data-batch-inspect]')
    expect(calls('cli_inspect_session_delete')).toHaveLength(1)
    expect(calls('cli_confirm_session_delete')).toHaveLength(1)
    expect(refresh).toHaveBeenCalledExactlyOnceWith(context.projectPath, { cli: 'codex' })
  })
  it('BatchDelete_CloseDuringExecutionStopsQueueAndPreservesReceipt_005', async () => {
    let finish!: (v: unknown) => void, pending: any
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => command === 'cli_confirm_session_delete'
      ? new Promise(done => { finish = done; pending = request }) : previous(command, request))
    await review(); await confirm(); element('[data-batch-final]').click(); await flushPromises()
    await click('[data-batch-close]')
    finish(await previous('cli_confirm_session_delete', pending)); await flushPromises()
    expect(calls('cli_confirm_session_delete')).toHaveLength(1)
    await click('[data-batch-existing]')
    expect(element('[data-batch-result]').textContent).toContain('1 acknowledged')
    expect(element('[data-batch-result]').textContent).toContain('2 not sent')
  })
  it('BatchDelete_SelectAllExcludesProtectedUnsupportedAndDuplicateRoots_006', async () => {
    render([row(1), { ...row(2), opened: true, processState: 'running' }, { ...row(3), cli: 'claude' }, { ...row(1), id: 'duplicate' }])
    await click('[data-batch-start]'); await click('[data-batch-select-all]'); await click('[data-batch-review]')
    expect(calls('cli_preview_session_delete')).toHaveLength(1)
    expect(calls('cli_preview_session_delete')[0]![1].sessionKey).toBe('key-1')
  })
  it('BatchDelete_NewRowsCannotJoinFrozenSelectionAndCancelClearsSelection_007', async () => {
    render([row(1), row(2)]); await click('[data-batch-start]'); await click('[data-batch-select-all]')
    await view!.setProps({ sessions: [row(1), row(2), row(3)] })
    await click('[data-batch-review]')
    expect(calls('cli_preview_session_delete')).toHaveLength(2)
    await click('[data-batch-close]'); await click('[data-batch-cancel-selection]'); await click('[data-batch-start]')
    expect((element('[data-batch-review]') as HTMLButtonElement).disabled).toBe(true)
  })
  it('BatchDelete_NoPartialDispatchWhenAnyPreviewBlocked_008', async () => {
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation(async (command, request) => {
      const result = await previous(command, request)
      return command === 'cli_preview_session_delete' && request.sessionKey === 'key-2'
        ? { ...result, state: 'blocked', previewId: null, reason: 'DELETE_SOURCE_BUSY' } : result
    })
    await review()
    expect(document.querySelector('[data-batch-next]')).toBeNull()
    expect(calls('cli_confirm_session_delete')).toHaveLength(0)
    expect(element('[data-batch-result]').textContent).toContain('not sent')
  })

  it('BatchDelete_SearchSelectsOnlyVisibleEligibleRows_009', async () => {
    render(); await click('[data-batch-start]')
    await view!.get('input[type=search]').setValue('archive 2')
    await click('[data-batch-select-all]'); await view!.get('input[type=search]').setValue('')
    await click('[data-batch-review]')
    expect(calls('cli_preview_session_delete')).toHaveLength(1)
    expect(calls('cli_preview_session_delete')[0]![1].sessionKey).toBe('key-2')
  })
  it('BatchDelete_PendingFirstRequestPreventsParallelDispatch_010', async () => {
    let finish!: (v: unknown) => void, pending: any
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => command === 'cli_confirm_session_delete' && request.previewId === uuid(101)
      ? new Promise(done => { finish = done; pending = request }) : previous(command, request))
    await review(); await confirm(); element('[data-batch-final]').click(); await flushPromises()
    expect(calls('cli_confirm_session_delete')).toHaveLength(1)
    finish(await previous('cli_confirm_session_delete', pending)); await flushPromises()
    expect(calls('cli_confirm_session_delete')).toHaveLength(3)
  })
  it.each(['blocked', 'partial'] as const)('BatchDelete_FailedOrExpiredTicketStopsRemaining_011 (%s)', async state => {
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation(async (command, request) => command === 'cli_confirm_session_delete'
      ? { operationId: request.operationId, cli: 'codex', state, rootPresent: true, descendants: 'unverified', reason: state === 'blocked' ? 'DELETE_PREVIEW_EXPIRED' : 'DELETE_ROOT_PRESENT' }
      : previous(command, request))
    await review(); await confirm(); await click('[data-batch-final]')
    expect(calls('cli_confirm_session_delete')).toHaveLength(1)
    expect(element('[data-batch-result]').textContent).toContain('2 not sent')
    expect(element('[data-batch-dialog]').textContent).toContain(state === 'blocked' ? 'DELETE_PREVIEW_EXPIRED' : 'DELETE_ROOT_PRESENT')
  })
  it('BatchDelete_CrossProjectOriginsRefreshSeparately_012', async () => {
    const otherContext = { ...context, projectId: 'other-project', projectPath: '/synthetic/other-project', profileId: 'other-profile' }
    const otherSource: SourceRef = { ...source, scopeId: 'other-scope', profileId: 'other-profile', sourceRootKey: 'other-root', target: { kind: 'profile', profileId: 'other-profile', expectedProfileRevision: '1', projectIds: ['other-project'] } }
    const history = useNativeHistoryStore()
    history.entries.set(nativeHistoryContextKey(otherContext), { ...history.get(context)!, key: nativeHistoryContextKey(otherContext), context: otherContext, source: otherSource, sourceRootKey: 'other-root', sessions: [{ ...history.get(context)!.sessions[1]!, projectId: 'other-project', cwd: otherContext.projectPath }] })
    const other = { ...row(2), projectKey: 'other-project', projectPath: otherContext.projectPath, nativeOrigin: otherContext, nativeRootKey: 'other-root' }
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => command === 'native_get_scope' && request.profileId === 'other-profile' ? Promise.resolve(otherSource) : previous(command, request))
    render([row(1), other]); await click('[data-batch-start]'); await click('[data-batch-select-all]'); await click('[data-batch-review]'); await confirm(); await click('[data-batch-final]')
    expect(calls('cli_confirm_session_delete')).toHaveLength(2)
    expect(calls('native_get_scope').map(([, request]) => request.profileId)).toEqual(['profile', 'other-profile'])
    expect(refresh).toHaveBeenCalledTimes(2)
    expect(refresh).toHaveBeenCalledWith(otherContext.projectPath, { cli: 'codex' })
  })
  it.each(['acknowledged', 'unknown'] as const)('BatchDelete_BothAcknowledgementsRequiredAndUnmountStopsQueue_013 (%s)', async state => {
    let finish!: (v: unknown) => void, pending: any
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => command === 'cli_confirm_session_delete'
      ? new Promise(done => { finish = done; pending = request }) : previous(command, request))
    await review(); await click('[data-batch-next]')
    await check('[data-batch-ack-permanent]')
    expect((element('[data-batch-final]') as HTMLButtonElement).disabled).toBe(true)
    await check('[data-batch-ack-subtree]'); element('[data-batch-final]').click(); await flushPromises()
    view!.unmount(); view = undefined
    finish(state === 'unknown' ? { operationId: pending.operationId, cli: 'codex', state: 'unknown', rootPresent: null, descendants: 'unverified', reason: 'DELETE_OUTCOME_UNKNOWN' } : await previous('cli_confirm_session_delete', pending)); await flushPromises()
    expect(calls('cli_confirm_session_delete')).toHaveLength(1)
    render(); await click('[data-batch-existing]')
    expect(element('[data-batch-result]').textContent).toContain('2 not sent')
    if (state === 'unknown') expect(element('[data-batch-dialog]').textContent).toContain('DELETE_OUTCOME_UNKNOWN')
  })

  it('BatchDelete_OldPartialReceiptSurvivesNewPreview_014', async () => {
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => ['cli_confirm_session_delete', 'cli_inspect_session_delete'].includes(command)
      ? Promise.resolve({ operationId: request.operationId, cli: 'codex', state: 'partial', rootPresent: true, descendants: 'unverified', reason: 'DELETE_ROOT_PRESENT' }) : previous(command, request))
    await review(); await confirm(); await click('[data-batch-final]'); await click('[data-batch-inspect]')
    await click('[data-batch-close]'); await click('[data-batch-review]')
    await click('[data-batch-close]')
    await view!.findAll('[data-batch-existing]')[0]!.trigger('click'); await flushPromises()
    expect(element('[data-batch-dialog]').textContent).toContain('Partial result')
    expect(element('[data-batch-dialog]').textContent).toContain('DELETE_ROOT_PRESENT')
    expect(element('[data-batch-dialog]').textContent).toContain('still present')
    expect(calls('cli_confirm_session_delete')).toHaveLength(1)
  })
  it('BatchDelete_TransientQueuedOwnerLossCannotRecoverOldConfirmation_015', async () => {
    let finish!: (v: unknown) => void, pending: any
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, request) => command === 'cli_confirm_session_delete'
      ? new Promise(done => { finish = done; pending = request }) : previous(command, request))
    await review(); await confirm(); element('[data-batch-final]').click(); await flushPromises()
    await view!.setProps({ sessions: [row(1), { ...row(2), opened: true, processState: 'running' }, row(3)] })
    await view!.setProps({ sessions: [row(1), row(2), row(3)] })
    finish(await previous('cli_confirm_session_delete', pending)); await flushPromises()
    expect(calls('cli_confirm_session_delete')).toHaveLength(1)
    expect(element('[data-batch-result]').textContent).toContain('2 not sent')
  })
})
