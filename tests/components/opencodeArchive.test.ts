import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import OpencodeArchivePanel from '@/components/sessions/OpencodeArchivePanel.vue'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import { useNativeHistoryStore, nativeHistoryContextKey } from '@/stores/nativeHistory'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import type { UnifiedSession } from '@/types/unifiedSession'
import type { SourceRef } from '@/types/nativeProjection'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
const io = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
const context = { cli: 'opencode' as const, profileId: 'profile', profileRevision: '1', projectId: 'project', projectPath: '/synthetic/project' }
const source: SourceRef = { scopeId: 'scope', instanceId: 'instance', cli: 'opencode', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', basis: 'configured-profile', target: { kind: 'profile', profileId: 'profile', expectedProfileRevision: '1', projectIds: ['project'] } }
const target = (): UnifiedSession => ({ id: 'desk', projectKey: 'project', projectPath: context.projectPath, cli: 'opencode', runtime: 'native-cli', title: 'Synthetic session', processState: 'stopped', attentionState: 'none', lastActivityAt: 1, archived: true, opened: false, resumable: true, adapterSessionId: 'adapter', nativeSessionId: 'ses_synthetic', nativeRootKey: 'root', nativeHistorySessionKey: 'key', nativeOrigin: context })
let view: VueWrapper | undefined
const entry = () => useNativeHistoryStore().get(context)!
beforeEach(() => {
  setActivePinia(createPinia()); vi.resetAllMocks()
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke: io.invoke } })
  const history = useNativeHistoryStore()
  history.entries.set(nativeHistoryContextKey(context), { key: nativeHistoryContextKey(context), context, source, sourceRootKey: 'root', loaded: true, loading: false, error: null, requestEpoch: '1', sessions: [{ type: 'session', sessionKey: 'key', nativeSessionId: 'ses_synthetic', projectId: 'project', title: 'Synthetic session', truncated: false, archived: false, cwd: context.projectPath, updatedAt: null }] })
  vi.spyOn(history, 'load').mockImplementation(async () => { entry().source = { ...source, scopeId: 'fresh' }; return entry() })
  vi.spyOn(useUnifiedSessionsStore(), 'refresh').mockResolvedValue(undefined)
  io.invoke.mockImplementation(async (command, request) => {
    if (command !== 'cli_set_opencode_archive') throw new Error('unexpected command')
    entry().sessions[0]!.archived = request.archived
    return { operationId: request.operationId, source: request.source, sessionKey: request.sessionKey, archived: request.archived, state: 'archived' }
  })
})
afterEach(() => { view?.unmount(); view = undefined; delete (window as any).__CC_DESK_DOCUMENT__; document.body.innerHTML = '' })
const element = (selector: string) => document.querySelector<HTMLElement>(selector)!
async function show(sessions = [target()], locale = 'en') {
  view = mount(OpencodeArchivePanel, { props: { sessions, active: true }, attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] } })
  await view.get('[data-opencode-archive-open]').trigger('click'); await flushPromises(); return view
}
async function choose() { const select = element('[data-opencode-archive-select]') as HTMLSelectElement; select.value = 'desk'; select.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises() }
async function acknowledge() { const box = element('[data-opencode-archive-ack]') as HTMLInputElement; box.checked = true; box.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises() }
async function submit() { await choose(); await acknowledge(); element('[data-opencode-archive-submit]').click(); await flushPromises() }
describe('independent OpenCode native archive panel', () => {
  it('OpenCodeArchiveUi_ExplicitScopeCancelAndClosedCandidates_001', async () => {
    const w = await show(); await choose()
    expect(element('[data-opencode-archive-dialog]').textContent).toContain('not permanent deletion')
    expect(element('[data-opencode-archive-dialog]').textContent).toContain('Child sessions and attachments remain')
    expect((element('[data-opencode-archive-submit]') as HTMLButtonElement).disabled).toBe(true)
    await acknowledge(); await w.setProps({ active: false }); await flushPromises()
    expect(io.invoke).not.toHaveBeenCalled()
  })
  it('OpenCodeArchiveUi_ArchiveThenExplicitRestoreRefreshesNativeHistory_002', async () => {
    await show(); await choose(); await acknowledge()
    const button = element('[data-opencode-archive-submit]'); button.click(); button.click(); await flushPromises()
    expect(io.invoke).toHaveBeenCalledTimes(1)
    expect(io.invoke.mock.calls[0]).toEqual(['cli_set_opencode_archive', expect.objectContaining({ source, sessionKey: 'key', archived: true, acknowledgeRecoverable: true })])
    expect(useNativeHistoryStore().load).toHaveBeenCalledWith(expect.objectContaining({ ...context, force: true, retainOnFailure: true }))
    expect(useUnifiedSessionsStore().refresh).toHaveBeenCalledWith(context.projectPath, { cli: 'opencode' })
    await choose(); expect(element('[data-opencode-archive-submit]').textContent).toContain('Restore from native archive')
    await acknowledge(); element('[data-opencode-archive-submit]').click(); await flushPromises()
    expect(io.invoke).toHaveBeenCalledTimes(2); expect(io.invoke.mock.calls[1]![1].archived).toBe(false)
    expect(io.invoke.mock.calls.every(([command]) => command === 'cli_set_opencode_archive')).toBe(true)
    expect(target().archived).toBe(true)
  })
  it('OpenCodeArchiveUi_UnknownPersistsAcrossReopenMissingIsNotSuccess_003', async () => {
    io.invoke.mockRejectedValue({ code: 'ARCHIVE_OUTCOME_UNKNOWN', privatePath: 'secret' })
    await show(); await submit(); expect(element('[data-opencode-archive-error]').textContent).toBe('ARCHIVE_OUTCOME_UNKNOWN')
    view!.unmount(); view = undefined; await show()
    expect(element('[data-opencode-archive-pending]').textContent).toContain('ses_synthetic')
    expect((element('[data-opencode-archive-select]') as HTMLSelectElement).options).toHaveLength(1)
    const session = entry().sessions[0]!; entry().sessions = []
    element('[data-opencode-archive-refresh]').click(); await flushPromises()
    expect(element('[data-opencode-archive-pending]')).not.toBeNull()
    entry().sessions = [{ ...session, archived: true }]
    element('[data-opencode-archive-refresh]').click(); await flushPromises()
    expect(element('[data-opencode-archive-pending]')).toBeNull()
    expect(io.invoke).toHaveBeenCalledTimes(1)
  })
  it('OpenCodeArchiveUi_IdentityAndStateChangeRevokeAcknowledgment_004', async () => {
    await show(); await choose(); await acknowledge()
    entry().source = { ...source, scopeId: 'replacement' }; await flushPromises()
    expect((element('[data-opencode-archive-submit]') as HTMLButtonElement).disabled).toBe(true)
    await acknowledge(); entry().sessions[0]!.archived = true; await flushPromises()
    expect((element('[data-opencode-archive-submit]') as HTMLButtonElement).disabled).toBe(true)
    entry().source = { ...source, profileRevision: '2' }; await flushPromises()
    expect(element('[data-opencode-archive-submit]')).toBeNull()
    expect(io.invoke).not.toHaveBeenCalled()
  })
  it('OpenCodeArchiveUi_ExcludesUnverifiedAndActiveRows_005', async () => {
    const w = await show()
    for (const change of [{ opened: true }, { archived: false }, { cli: 'claude' }, { runtime: 'legacy-claude' }, { nativeRootKey: 'different' }, { resumePending: true }, { preparationState: 'pending' }, { processState: 'unknown' }, { nativeHistorySessionKey: undefined }]) {
      await w.setProps({ sessions: [{ ...target(), ...change } as UnifiedSession] }); await flushPromises()
      expect((element('[data-opencode-archive-select]') as HTMLSelectElement).options).toHaveLength(1)
    }
    expect(io.invoke).not.toHaveBeenCalled()
  })
  it('OpenCodeArchiveUi_ReadFailureOrChangedRootCannotResolveUnknown_006', async () => {
    io.invoke.mockRejectedValue({ code: 'ARCHIVE_OUTCOME_UNKNOWN' }); await show(); await submit()
    entry().sessions[0]!.archived = true
    vi.mocked(useNativeHistoryStore().load).mockImplementation(async () => { entry().source = { ...source, sourceRootKey: 'different' }; return entry() })
    element('[data-opencode-archive-refresh]').click(); await flushPromises()
    expect(element('[data-opencode-archive-pending]')).not.toBeNull()
    vi.mocked(useNativeHistoryStore().load).mockRejectedValue({ code: 'SOURCE_BUSY' })
    element('[data-opencode-archive-refresh]').click(); await flushPromises()
    expect(element('[data-opencode-archive-pending]')).not.toBeNull()
    expect(io.invoke).toHaveBeenCalledTimes(1)
  })
  it('OpenCodeArchiveUi_CloseInflightDoesNotReplayOrLosePending_007', async () => {
    let reject!: (error: unknown) => void
    io.invoke.mockImplementation(() => new Promise((_, failure) => { reject = failure }))
    const w = await show(); await choose(); await acknowledge(); element('[data-opencode-archive-submit]').click(); await flushPromises()
    await w.setProps({ active: false }); reject({ code: 'BACKEND_INSTANCE_CHANGED' }); await flushPromises()
    view!.unmount(); view = undefined; await show([], 'zh')
    expect(element('[data-opencode-archive-pending]').textContent).toContain('ses_synthetic')
    expect(io.invoke).toHaveBeenCalledTimes(1)
  })
  it('OpenCodeArchiveUi_SameTickScopeChangeCannotReuseConsent_008', async () => {
    await show(); await choose(); await acknowledge()
    const button = element('[data-opencode-archive-submit]')
    entry().source = { ...source, scopeId: 'replaced-before-render' }
    button.click(); await flushPromises()
    expect(io.invoke).not.toHaveBeenCalled()
  })
  it('OpenCodeArchiveUi_DrawerKeepsPermanentDeleteSeparate_009', async () => {
    view = mount(ArchivedSessionsDrawer, { props: { sessions: [target()], open: true }, attachTo: document.body,
      global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    await flushPromises()
    expect((element('[data-session-delete]') as HTMLButtonElement).disabled).toBe(true)
    element('[data-opencode-archive-open]').click(); await flushPromises(); await choose()
    expect(element('[data-opencode-archive-submit]').textContent).toContain('Archive in OpenCode')
    expect(io.invoke).not.toHaveBeenCalled()
  })
  it('OpenCodeArchiveUi_PreflightFailureAllowsOnlyExplicitNewAttempt_010', async () => {
    const succeed = io.invoke.getMockImplementation()!
    io.invoke.mockRejectedValue({ code: 'SOURCE_BUSY' }); await show(); await submit()
    expect(element('[data-opencode-archive-pending]')).toBeNull()
    expect(io.invoke).toHaveBeenCalledTimes(1)
    await choose(); expect((element('[data-opencode-archive-submit]') as HTMLButtonElement).disabled).toBe(true)
    io.invoke.mockImplementation(succeed); await acknowledge(); element('[data-opencode-archive-submit]').click(); await flushPromises()
    expect(io.invoke).toHaveBeenCalledTimes(2)
    expect(io.invoke.mock.calls[1]![1].operationId).not.toBe(io.invoke.mock.calls[0]![1].operationId)
    expect(io.invoke.mock.calls[1]![1].archived).toBe(true)
  })
  it('OpenCodeArchiveUi_PostReceiptReadBusyKeepsPending_011', async () => {
    vi.mocked(useNativeHistoryStore().load).mockRejectedValue({ code: 'SOURCE_BUSY' })
    await show(); await submit()
    expect(element('[data-opencode-archive-pending]')).not.toBeNull()
    expect(io.invoke).toHaveBeenCalledTimes(1)
  })
  it('OpenCodeArchiveUi_NativeArchivedRowOpensSeparateRestoreBeforeDeskRestore_012', async () => {
    entry().sessions[0]!.archived = true
    let deskIds: string[] = []
    const adapter = createNativeCliAdapter({ tabs: { tabs: new Map(), activeTabId: null, setActive: vi.fn(), close: vi.fn(), rename: vi.fn() },
      history: { all: () => [entry()], load: useNativeHistoryStore().load }, runtime: {} as any,
      archive: { getArchivedSessions: () => deskIds, archiveSession: vi.fn(), restoreSession: vi.fn() } })
    const [row] = await adapter.listSessions(); deskIds = [row.id]
    view = mount(ArchivedSessionsDrawer, { props: { sessions: [row], open: true }, attachTo: document.body,
      global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    await flushPromises()
    expect(element('.session-primary-action')).toBeNull()
    element('[data-session-opencode-archive]').click(); await flushPromises()
    expect(element('[data-opencode-archive-submit]').textContent).toContain('Restore from native archive')
    expect((element('[data-opencode-archive-select]') as HTMLSelectElement).value).toBe(row.id)
    await acknowledge(); element('[data-opencode-archive-submit]').click(); await flushPromises()
    expect(io.invoke).toHaveBeenCalledTimes(1)
    expect(io.invoke.mock.calls[0]![1].archived).toBe(false)
    expect(view.emitted('restore-request')).toBeUndefined()
    await view.setProps({ sessions: await adapter.listSessions() }); await flushPromises()
    element('.session-primary-action button').click(); await flushPromises()
    expect(view.emitted('restore-request')).toEqual([[row.id]])
  })
})
