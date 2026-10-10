import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { randomUUID } from 'node:crypto'
import App from '@/App.vue'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import { useNativeTabsStore, captureNativeAttempt } from '@/stores/nativeTabs'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useShellStore } from '@/stores/shell'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import type { NativeLaunchEntryInput } from '@/terminal/nativeLaunchEntry'
import en from '@/i18n/locales/en'

// Keep the real App, tree button, runtime, host, Native component, adapters and
// facade. Only process/IPC, renderer and window boundaries are synthetic.
const io = vi.hoisted(() => ({ start: vi.fn(), cancel: vi.fn(), recover: vi.fn(), stop: vi.fn(), resize: vi.fn(), historyRead: vi.fn(), terms: [] as any[], stopped: new Set<string>(), records: {} as Record<string, any> }))
const profile = { id: 'cx', revision: '7', cli: 'codex', name: 'Work', launcher: { kind: 'native' }, programPath: { mode: 'inherit' },
  defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  cliStop: io.stop, cliResize: io.resize,
  getProjectsState: async () => ({ pinnedProjects: [], archivedSessions: {}, sessionRecords: { ...io.records } }), getProjects: async () => [], getSessions: async () => [],
  upsertSessionUiRecord: async (key: string, record: any) => { io.records[key] = { ...record }; return { pinnedProjects: [], archivedSessions: {}, sessionRecords: { ...io.records } } },
  getAppConfig: async () => ({ language: 'en', theme: 'light', terminalTheme: 'cc-box-light' }), updateAppConfig: async () => {}, onHookEvent: async () => () => {},
  createNativeProjectionClient: () => ({
    scope: async (target: any) => ({ scopeId: 'scope-cx', instanceId: 'fixture-instance', cli: 'codex', sourceRootKey: 'root-cx', identityEpoch: '1', profileId: 'cx', profileRevision: '7', target, basis: target.kind === 'run' ? 'launch-environment' : 'configured-profile' }),
    read: io.historyRead,
  }),
}))
vi.mock('@/api/cli', () => ({ cliListProfiles: async () => ({ revision: '7', profiles: [profile] }) }))
vi.mock('@/api/cliAvailability', () => ({ cliGetAvailability: async (profileId: string, profileRevision: string) => ({ profileId, profileRevision, cli: 'codex', state: 'available-unverified', hostStatus: 'available', certified: false }) }))
vi.mock('@/api/programDiscovery', () => ({ cliDiscoverPrograms: async () => ({ candidates: [] }) }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: async () => ({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }) }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class { onmessage: any } }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMaximized: async () => false,
  isFocused: async () => true, onFocusChanged: async () => () => {}, requestUserAttention: async () => {} }) }))
vi.mock('@/terminal/nativeLaunchEntry', async original => ({ ...await original<object>(), createNativeLaunchEntry: () => ({ start: io.start, cancel: io.cancel, recover: io.recover, latest: vi.fn() }) }))
vi.mock('@/terminal/deskNativeTerminal', () => ({ createDeskNativeTerminalBinding: () => ({ dispose: vi.fn(), acceptOutput: vi.fn(), sendUserText: vi.fn(), reserveUserPaste: vi.fn() }) }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit() {} } }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {
  options: any; cols = 80; rows = 24; element!: HTMLElement; textarea!: HTMLTextAreaElement
  modes = { applicationCursorKeysMode: false, applicationKeypadMode: false, bracketedPasteMode: false, insertMode: false, mouseTrackingMode: 'none', originMode: false, reverseWraparoundMode: false, sendFocusMode: false, wraparoundMode: true }
  dispose = vi.fn(); focus() {} loadAddon() {} attachCustomKeyEventHandler() {} getSelection() { return '' }
  constructor(options: any) { this.options = options; io.terms.push(this) }
  open(element: HTMLElement) { this.element = element; this.textarea = document.createElement('textarea'); element.append(this.textarea) }
  onData() { return { dispose() {} } } onWriteParsed() { return { dispose() {} } }
  write(_data: string | Uint8Array, callback?: () => void) { callback?.() } reset() {}
} }))

const wrappers: VueWrapper[] = []
function receipt(input: NativeLaunchEntryInput, phase: LaunchStatus['phase'], revision = '2'): LaunchStatus {
  return { instanceId: 'fixture-instance', requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, phase, revision, failure: null }
}
function launched(requestId: string): NativeLaunchEntryInput {
  const input = io.start.mock.calls.map(([value]) => value).find(value => value.requestId === requestId)
  if (!input) throw new Error('TEST_LAUNCH_NOT_FOUND')
  return input
}
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(yes => { resolve = yes })
  return { promise, resolve }
}
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); io.terms.length = 0; io.stopped.clear(); io.records = {}; localStorage.clear()
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID })
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { callback(0); return 1 })
  io.start.mockImplementation(async input => receipt(input, 'running', '1'))
  io.cancel.mockImplementation(async id => receipt(launched(id), 'running', '1'))
  io.recover.mockImplementation(async id => receipt(launched(id), io.stopped.has(launched(id).runId) ? 'exited' : 'running'))
  io.stop.mockImplementation(async ({ runId }) => { io.stopped.add(runId) }); io.resize.mockResolvedValue(undefined)
  io.historyRead.mockImplementation(async request => ({ source: request.source, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: [], hasMore: false }))
})
afterEach(() => { wrappers.splice(0).forEach(wrapper => wrapper.unmount()); vi.useRealTimers(); vi.unstubAllGlobals(); document.body.innerHTML = '' })
async function render() {
  const wrapper = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { TerminalView: true, SettingsView: true } } })
  wrappers.push(wrapper); await flushPromises(); return wrapper
}
async function open(wrapper: VueWrapper, action: import('@/types/cli').LaunchAction = { kind: 'new' }) {
  const tabs = useNativeTabsStore(), catalog = useUnifiedSessionsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action, title: 'Native close target' })
  await catalog.refresh()
  await vi.waitFor(() => expect(catalog.sessions.some(row => row.id === `native-tab:${tab.tabId}`)).toBe(true))
  await catalog.activateSession(`native-tab:${tab.tabId}`); await flushPromises()
  expect(tabs.tab(tab.tabId)?.status).toBe('running')
  const expand = wrapper.get('[data-project-key="/repo"] .expand-arrow')
  if (expand.attributes('aria-expanded') === 'false') await expand.trigger('click')
  await flushPromises()
  io.recover.mockClear()
  return tab
}
function closeButton(wrapper: VueWrapper, tabId: string) { return wrapper.get(`[data-session-row="native-tab:${tabId}"] .session-primary-action button`) }

// A closed history row is the restore action; the full close/admission chain
// still verifies its exact source and creates only one new resume attempt.
it('Native_AppCloseThenRowResume_009', async () => {
  const item = { type: 'session', sessionKey: JSON.stringify(['local', 'codex', 'root-cx', 'closed']), nativeSessionId: 'closed', title: 'Closing history', cwd: '/repo', updatedAt: '2026-10-10T12:19:00Z', truncated: false }
  const response = (request: any) => ({ source: request.source, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: request.resourceKind === 'history' ? [item] : [], hasMore: false })
  io.historyRead.mockImplementation(async request => response(request))
  const wrapper = await render(), tab = await open(wrapper, { kind: 'resume-id', nativeSessionId: 'closed' }), catalog = useUnifiedSessionsStore()
  await closeButton(wrapper, tab.tabId).trigger('click')
  await vi.waitFor(() => expect(catalog.completedClose).not.toBeNull())
  const history = catalog.sessions.find(row => row.nativeSessionId === 'closed' && !row.opened)!
  expect(history).toBeDefined()
  const row = wrapper.findAll('[data-session-row]').find(value => value.attributes('data-session-row') === history.id)!
  const pending = deferred<any>()
  io.historyRead.mockClear(); io.historyRead.mockImplementationOnce(() => pending.promise)
  await row.trigger('click'); await flushPromises()
  expect(io.historyRead).toHaveBeenCalledOnce()
  expect(io.historyRead.mock.calls[0][0]).toMatchObject({ resourceKind: 'history', source: { profileId: 'cx', profileRevision: '7', target: { projectId: 'project' } } })
  expect(io.start).toHaveBeenCalledOnce()
  expect(catalog.isResumePending(history)).toBe(true)
  await row.trigger('click')
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true }))
  await row.trigger('dblclick')
  pending.resolve(response(io.historyRead.mock.calls[0][0]))
  await vi.waitFor(() => expect(io.start).toHaveBeenCalledTimes(2))
  expect(io.start.mock.calls[1][0]).toMatchObject({ action: { kind: 'resume-id', nativeSessionId: 'closed' } })
  expect(useNativeTabsStore().tabs.size).toBe(1)
  expect(io.stop).toHaveBeenCalledOnce()
  expect(wrapper.find('button[aria-label="Resume session"]').exists()).toBe(false)
  const restored = [...useNativeTabsStore().tabs.values()][0]
  await wrapper.get(`[data-session-row="native-tab:${restored.tabId}"]`).trigger('click'); await flushPromises()
  expect(io.start).toHaveBeenCalledTimes(2)
  expect(catalog.actionFeedback).toBeNull()
})

// A current read failure cannot be replaced with stale cached presence. Leaving
// the workspace during the read likewise cancels admission without a late start.
it.each(['budget', 'navigation'] as const)('Native_AppRowResumeNoStaleAdmission_%s_010', async failure => {
  const item = { type: 'session', sessionKey: JSON.stringify(['local', 'codex', 'root-cx', 'saved']), nativeSessionId: 'saved', title: 'Saved history', cwd: '/repo', updatedAt: '2026-10-10T12:19:00Z', truncated: false }
  const response = (request: any) => ({ source: request.source, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: request.resourceKind === 'history' ? [item] : [], hasMore: false })
  io.historyRead.mockImplementation(async request => response(request))
  const wrapper = await render(), catalog = useUnifiedSessionsStore()
  await wrapper.get('[data-project-key="/repo"] .expand-arrow').trigger('click')
  const history = catalog.sessions.find(row => row.nativeSessionId === 'saved')!
  const row = wrapper.findAll('[data-session-row]').find(value => value.attributes('data-session-row') === history.id)!
  const pending = deferred<any>()
  io.historyRead.mockClear(); io.historyRead.mockImplementationOnce(() => pending.promise)
  await row.trigger('click'); await flushPromises()
  expect(io.historyRead).toHaveBeenCalledOnce()
  expect(catalog.isResumePending(history)).toBe(true)
  if (failure === 'navigation') useShellStore().navigate('settings')
  const request = io.historyRead.mock.calls[0][0]
  pending.resolve(failure === 'budget' ? { ...response(request), state: 'unavailable', reason: 'SOURCE_BUDGET_EXCEEDED', items: [] } : response(request))
  await flushPromises()
  await vi.waitFor(() => expect(catalog.isResumePending(history)).toBe(false))
  expect(io.start).not.toHaveBeenCalled(); expect(io.stop).not.toHaveBeenCalled()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  if (failure === 'budget') expect(catalog.actionFeedback).not.toBeNull()
  else expect(catalog.actionFeedback).toBeNull()
})

it.each(['running', 'unknown'] as const)('Native_AppPrimaryCloseOnce_%s_001', async state => {
  const wrapper = await render(), tab = await open(wrapper), tabs = useNativeTabsStore(), catalog = useUnifiedSessionsStore()
  if (state === 'unknown') { tabs.markUnknown(tab.tabId); await flushPromises() }
  const sequence = useShellStore().requestSequence
  expect(closeButton(wrapper, tab.tabId).attributes('aria-label')).toBe(en.sessionActionClose)
  await closeButton(wrapper, tab.tabId).trigger('click')
  expect(catalog.sessionConfirmation).toBeNull()
  await vi.waitFor(() => expect(tabs.tab(tab.tabId)).toBeUndefined())
  expect(useShellStore().requestSequence).toBe(sequence + 1)
  expect(io.cancel).toHaveBeenCalledExactlyOnceWith(tab.requestId)
  expect(io.stop).toHaveBeenCalledExactlyOnceWith({ runId: tab.runId, generation: tab.generation })
  expect(io.recover).toHaveBeenCalledExactlyOnceWith(tab.requestId)
  expect(io.start).toHaveBeenCalledTimes(1)
  expect(catalog.activeSessionId).toBeNull()
  expect(wrapper.findComponent(NativeCliTerminal).exists()).toBe(false)
  expect(io.terms[0].dispose).toHaveBeenCalledOnce()
  expect(wrapper.find('[data-session-confirm]').exists()).toBe(false)
})

// 停止已生效后的状态读取瞬断，首次关闭继续检查精确回执，不要求第二次点击。
it('Native_AppCloseReadRetry_007', async () => {
  const wrapper = await render(), tab = await open(wrapper)
  io.recover.mockRejectedValueOnce(new Error('LAUNCH_STATE_UNKNOWN'))
  await closeButton(wrapper, tab.tabId).trigger('click')
  await vi.waitFor(() => expect(useNativeTabsStore().tab(tab.tabId)).toBeUndefined())
  expect(io.stop).toHaveBeenCalledExactlyOnceWith({ runId: tab.runId, generation: tab.generation })
  expect(io.cancel).toHaveBeenCalledExactlyOnceWith(tab.requestId)
  expect(io.recover).toHaveBeenCalledTimes(2)
  expect(io.start).toHaveBeenCalledOnce()
  expect(wrapper.findComponent(NativeCliTerminal).exists()).toBe(false)
  expect(useUnifiedSessionsStore().actionFeedback).toBeNull()
})

// 整合真实响应式历史后，关闭拥有的迟到来源读取必须进入同一次排序快照。
it('Native_AppCloseLateHistory_008', async () => {
  const old = { type: 'session', sessionKey: JSON.stringify(['local', 'codex', 'root-cx', 'old']), nativeSessionId: 'old', title: 'Old history', cwd: '/repo', updatedAt: '2026-09-17T00:00:00Z', truncated: false }
  const closingHistory = { ...old, sessionKey: JSON.stringify(['local', 'codex', 'root-cx', 'closed']), nativeSessionId: 'closed', title: 'Closing history', updatedAt: '2026-09-10T00:00:00Z' }
  const result = (request: any, items: any[]) => ({ source: request.source, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items, hasMore: false })
  io.historyRead.mockImplementation(async request => result(request, request.resourceKind === 'history' ? [old, closingHistory] : []))
  const wrapper = await render(), tab = await open(wrapper, { kind: 'resume-id', nativeSessionId: 'closed' }), late = deferred<any>()
  io.historyRead.mockClear()
  io.historyRead.mockImplementationOnce(() => late.promise)
  await closeButton(wrapper, tab.tabId).trigger('click')
  await vi.waitFor(() => expect(useNativeTabsStore().tab(tab.tabId)).toBeUndefined())
  expect(wrapper.findComponent(NativeCliTerminal).exists()).toBe(false)
  expect(io.historyRead).toHaveBeenCalledOnce()
  const request = io.historyRead.mock.calls[io.historyRead.mock.calls.length - 1][0]
  expect(request).toMatchObject({ resourceKind: 'history', source: { cli: 'codex', profileId: 'cx', profileRevision: '7',
    target: { kind: 'profile', profileId: 'cx', expectedProfileRevision: '7', projectId: 'project' } } })
  expect(useUnifiedSessionsStore().completedClose).toBeNull()
  const fresh = { ...closingHistory, updatedAt: '2026-10-10T10:29:00Z' }
  io.historyRead.mockImplementation(async read => result(read, read.resourceKind === 'history' ? [old, fresh] : []))
  late.resolve(result(request, [old, fresh]))
  await vi.waitFor(() => expect(useUnifiedSessionsStore().completedClose).not.toBeNull())
  expect(useUnifiedSessionsStore().sessions.find(row => row.nativeSessionId === 'closed'))
    .toMatchObject({ opened: false, lastActivityAt: Date.parse(fresh.updatedAt) })
  const order = () => wrapper.findAll('[data-session-row]').map(row => row.get('.session-name').text())
  expect(order()).toEqual(['Closing history', 'Old history'])
  await wrapper.get('button[aria-label="Refresh sessions"]').trigger('click')
  await flushPromises()
  expect(order()).toEqual(['Closing history', 'Old history'])
  expect(io.start).toHaveBeenCalledOnce(); expect(io.stop).toHaveBeenCalledOnce()
  expect(useUnifiedSessionsStore().actionFeedback).toBeNull()
})

it('Native_AppCloseKeepsUnconfirmedAttempt_002', async () => {
  const wrapper = await render(), tab = await open(wrapper), tabs = useNativeTabsStore()
  io.recover.mockImplementation(async id => receipt(launched(id), 'running'))
  vi.useFakeTimers()
  await closeButton(wrapper, tab.tabId).trigger('click'); await flushPromises()
  await vi.advanceTimersByTimeAsync(5100); await flushPromises()
  expect(tabs.tab(tab.tabId)).toMatchObject({ ...captureNativeAttempt(tab), status: 'unknown', errorCode: 'NATIVE_STOP_UNCONFIRMED' })
  expect(wrapper.findComponent(NativeCliTerminal).exists()).toBe(true)
  expect(useUnifiedSessionsStore().activeSessionId).toBe(`native-tab:${tab.tabId}`)
  expect(io.stop).toHaveBeenCalledOnce(); expect(io.start).toHaveBeenCalledOnce()
})

it('Native_AppCloseRejectsReplacementAttempt_003', async () => {
  const wrapper = await render(), tab = await open(wrapper), tabs = useNativeTabsStore(), stop = deferred<void>()
  io.stop.mockReturnValueOnce(stop.promise)
  await closeButton(wrapper, tab.tabId).trigger('click'); await flushPromises()
  expect(io.stop).toHaveBeenCalledOnce()
  tabs.tab(tab.tabId)!.status = 'exited'
  const replacement = tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); await flushPromises()
  stop.resolve(); await flushPromises()
  expect(tabs.tab(tab.tabId)).toMatchObject({ ...captureNativeAttempt(replacement), status: 'running' })
  expect(useUnifiedSessionsStore().activeSessionId).toBe(`native-tab:${tab.tabId}`)
  expect(wrapper.findComponent(NativeCliTerminal).exists()).toBe(true)
  expect(io.recover).not.toHaveBeenCalled(); expect(io.start).toHaveBeenCalledTimes(2)
})

it('Native_AppCloseIgnoresLateExitedReceipt_004', async () => {
  const wrapper = await render(), tab = await open(wrapper), late = deferred<LaunchStatus>()
  const component = wrapper.getComponent(NativeCliTerminal)
  io.recover.mockReturnValueOnce(late.promise)
  const recovering = (component.vm as any).recover(captureNativeAttempt(tab)); await flushPromises()
  await closeButton(wrapper, tab.tabId).trigger('click')
  await vi.waitFor(() => expect(useNativeTabsStore().tab(tab.tabId)).toBeUndefined())
  late.resolve(receipt(launched(tab.requestId), 'exited', '3')); await recovering; await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(useUnifiedSessionsStore().sessions.some(row => row.id === `native-tab:${tab.tabId}`)).toBe(false)
  expect(wrapper.findComponent(NativeCliTerminal).exists()).toBe(false)
  expect(io.start).toHaveBeenCalledOnce(); expect(io.stop).toHaveBeenCalledOnce()
})

it('Native_AppCloseCancelsStartingAndIgnoresLateStart_005', async () => {
  const wrapper = await render(), start = deferred<LaunchStatus>()
  io.start.mockReturnValueOnce(start.promise)
  io.cancel.mockImplementation(async id => receipt(launched(id), 'cancelled'))
  const tabs = useNativeTabsStore(), catalog = useUnifiedSessionsStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  await catalog.refresh()
  await vi.waitFor(() => expect(catalog.sessions.some(row => row.id === `native-tab:${tab.tabId}`)).toBe(true))
  await catalog.activateSession(`native-tab:${tab.tabId}`); await flushPromises()
  expect(tabs.tab(tab.tabId)?.status).toBe('starting')
  await wrapper.get('[data-project-key="/repo"] .expand-arrow').trigger('click')
  await closeButton(wrapper, tab.tabId).trigger('click')
  await vi.waitFor(() => expect(tabs.tab(tab.tabId)).toBeUndefined())
  start.resolve(receipt(launched(tab.requestId), 'running', '1')); await flushPromises()
  expect(tabs.tabs.size).toBe(0)
  expect(catalog.sessions.some(row => row.id === `native-tab:${tab.tabId}`)).toBe(false)
  expect(wrapper.findComponent(NativeCliTerminal).exists()).toBe(false)
  expect(io.cancel).toHaveBeenCalledExactlyOnceWith(tab.requestId)
  expect(io.stop).not.toHaveBeenCalled(); expect(io.start).toHaveBeenCalledOnce()
})

it('Native_AppDuplicateCloseKeepsRemainingTerminal_006', async () => {
  const wrapper = await render(), remaining = await open(wrapper), closing = await open(wrapper), stop = deferred<void>()
  const remainingTerminal = wrapper.get(`[data-native-tab="${remaining.tabId}"]`).element
  io.stop.mockImplementationOnce(async ({ runId }) => { await stop.promise; io.stopped.add(runId) })
  await closeButton(wrapper, closing.tabId).trigger('click'); await flushPromises()
  expect(io.stop).toHaveBeenCalledOnce()
  await closeButton(wrapper, closing.tabId).trigger('click'); await flushPromises()
  stop.resolve()
  await vi.waitFor(() => expect(useUnifiedSessionsStore().activeSessionId).toBe(`native-tab:${remaining.tabId}`))
  await flushPromises()
  expect(useNativeTabsStore().tab(closing.tabId)).toBeUndefined()
  expect(useNativeTabsStore().tab(remaining.tabId)?.status).toBe('running')
  expect(wrapper.get(`[data-native-tab="${remaining.tabId}"]`).element).toBe(remainingTerminal)
  expect(io.stop).toHaveBeenCalledExactlyOnceWith({ runId: closing.runId, generation: closing.generation })
  expect(io.cancel).toHaveBeenCalledExactlyOnceWith(closing.requestId)
  expect(io.recover.mock.calls.filter(([id]) => id === closing.requestId)).toHaveLength(1)
  expect(io.start).toHaveBeenCalledTimes(2)
  expect(io.terms[0].dispose).not.toHaveBeenCalled(); expect(io.terms[1].dispose).toHaveBeenCalledOnce()
  expect(useUnifiedSessionsStore().actionFeedback).toBeNull()
})
