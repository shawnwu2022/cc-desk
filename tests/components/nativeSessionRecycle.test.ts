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
async function show(sessions = [target()]) { const w = render(sessions); await w.get('[data-recycle-open]').trigger('click'); await flushPromises(); return w }
async function choose() { const select = element('[data-recycle-select]') as HTMLSelectElement; select.value = 'desk'; select.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises() }
async function acknowledge() { const checkbox = element('[data-recycle-ack]') as HTMLInputElement; checkbox.checked = true; checkbox.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises() }
const mutations = () => io.invoke.mock.calls.filter(([, request]) => request.action !== 'list')
describe('separate recoverable Claude removal UI', () => {
  it('NativeRecycleUi_ExplicitScopeAndCancelNeverMutate_001', async () => {
    const w = await show(); await choose()
    expect(element('[data-recycle-dialog]').textContent).toContain('not native archiving or permanent deletion')
    expect(element('[data-recycle-dialog]').textContent).toContain('Child tasks, attachments and native indexes remain')
    expect((element('[data-recycle-remove]') as HTMLButtonElement).disabled).toBe(true)
    await acknowledge(); await w.setProps({ active: false }); await flushPromises()
    expect(mutations()).toHaveLength(0)
  })
  it('NativeRecycleUi_OneMoveThenReopenedPersistentRecovery_002', async () => {
    await show(); await choose(); await acknowledge()
    const button = element('[data-recycle-remove]'); button.click(); button.click(); await flushPromises()
    expect(mutations()).toHaveLength(1)
    expect(mutations()[0]![1]).toMatchObject({ action: 'remove', acknowledgeRecoverable: true, source, sessionKey: 'key' })
    view!.unmount(); view = undefined; await show([])
    expect(element('[data-recycle-records]').textContent).toContain(nativeId)
    element('[data-recycle-restore]').click(); await flushPromises()
    expect(mutations()).toHaveLength(2)
    expect(mutations()[1]![1]).toMatchObject({ action: 'restore', operationId: saved[0]!.operationId, sessionKey: null })
    expect(io.invoke.mock.calls.every(([command]) => command === 'cli_recycle_session')).toBe(true)
  })
  it('NativeRecycleUi_ConflictDisablesRestoreAndUnknownOnlyRefreshes_003', async () => {
    saved = [{ operationId: '70000000-0000-4000-8000-000000000002', nativeSessionId: nativeId, projectPath: context.projectPath, state: 'stored', originalOccupied: true, scope: 'claude-root-transcript' }]
    await show([]); expect((element('[data-recycle-restore]') as HTMLButtonElement).disabled).toBe(true); expect(mutations()).toHaveLength(0)
    view!.unmount(); view = undefined; saved = []; await show(); await choose(); await acknowledge()
    const previous = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation(async (command, request) => { if (request.action === 'remove') throw { code: 'RECYCLE_OUTCOME_UNKNOWN' }; return previous(command, request) })
    element('[data-recycle-remove]').click(); await flushPromises()
    expect(element('[data-recycle-error]').textContent).toBe('RECYCLE_OUTCOME_UNKNOWN')
    view!.unmount(); view = undefined; await show()
    expect(element('[data-recycle-pending]').textContent).toContain(nativeId)
    expect((element('[data-recycle-select]') as HTMLSelectElement).options).toHaveLength(1)
    expect(mutations()).toHaveLength(1)
    saved = [{ operationId: mutations()[0]![1].operationId, nativeSessionId: nativeId, projectPath: context.projectPath, state: 'stored', originalOccupied: false, scope: 'claude-root-transcript' }]
    element('[data-recycle-refresh]').click(); await flushPromises()
    expect(element('[data-recycle-pending]')).toBeNull()
    expect(mutations()).toHaveLength(1)
  })
  it('NativeRecycleUi_SourceOrTargetChangeRevokesConfirmation_004', async () => {
    await show(); await choose(); await acknowledge()
    useNativeHistoryStore().get(context)!.source = { ...source, profileRevision: '2' }
    await flushPromises()
    expect(element('[data-recycle-remove]')).toBeNull()
    expect(mutations()).toHaveLength(0)
  })
})
