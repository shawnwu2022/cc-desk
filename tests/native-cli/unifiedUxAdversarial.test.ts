import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import ProjectResourcesDrawer from '@/components/workspace/ProjectResourcesDrawer.vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore, captureNativeAttempt } from '@/stores/nativeTabs'
import { useSessionStore } from '@/stores/session'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useProjectResourcesStore } from '@/stores/projectResources'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import type { NativeLaunchEntryInput } from '@/terminal/nativeLaunchEntry'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import type { ReadRequest } from '@/types/nativeProjection'
import en from '@/i18n/locales/en'
import { createPinia, setActivePinia } from 'pinia'
import { defineComponent, ref } from 'vue'
import { type VueWrapper } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { webcrypto } from 'node:crypto'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
import { normalizePath } from '@/utils/path'
import type { ProjectsState } from '@/types/app'
import type { CliProfile } from '@/types/profile'
import type { RegisteredProject } from '@/types/workspace'
import type { NativeHistorySession } from '@/stores/nativeHistory'
import type { SourceRef, ScopeTarget, ResourceItem, ProjectionResult } from '@/types/nativeProjection'
import type { UnifiedTerminalHostPort } from '@/terminal/unifiedTerminalHost'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value))
function profile(cli: 'claude' | 'codex', id: string = cli): CliProfile {
  return { id, revision: '1', cli, name: cli, launcher: { kind: 'native' }, programPath: { mode: 'inherit' },
    defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }
}
function project(path: string, id = 'project'): RegisteredProject {
  return { projectId: id, hostId: 'host', sourcePathKey: `path-${id}`, selectedPath: path, canonicalPath: path,
    alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }
}
function history(cli: 'claude' | 'codex', root: string, id: string, path: string, title = id): NativeHistorySession {
  return { type: 'session', sessionKey: JSON.stringify(['local', cli, root, id]), nativeSessionId: id, title,
    truncated: false, cwd: path, updatedAt: '2026-09-30T12:00:00Z' }
}
function reply(request: ReadRequest, items: ResourceItem[] = [], hasMore = false): ProjectionResult {
  return { source: request.source, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch,
    observedAt: '1', state: 'ready', reason: null, items, hasMore }
}

/** Only host I/O is replaced. Stores, adapters, runtime composition, wire decoding,
 * selection ownership and canonical writer queues are the production modules. */
function workspaceFixture() {
  setActivePinia(createPinia())
  vi.stubGlobal('crypto', webcrypto)
  localStorage.clear()
  const fixture = {
    state: { pinnedProjects: [], archivedSessions: {}, displayNames: {}, sessionRecords: {}, launchPreferences: {} } as ProjectsState,
    config: { theme: 'light', terminalTheme: 'cc-box-light', language: 'en', hiddenProjects: [] } as Record<string, unknown>,
    profiles: [profile('codex')], registered: [project('/repo')],
    legacyProjects: [] as { path: string; name: string }[],
    legacyHistory: new Map<string, { sessionId: string; name: string; projectPath: string; lastActiveAt: number }[]>(),
    histories: new Map<string, NativeHistorySession[]>(),
    resources: [] as ResourceItem[],
    roots: new Map<string, string>(),
    unavailable: new Set<string>(),
    wrappers: [] as VueWrapper[],
  }
  function source(target: ScopeTarget): SourceRef {
    const tab = target.kind === 'run' ? [...useNativeTabsStore().tabs.values()].find(row => row.runId === target.runId && row.generation === target.generation) : undefined
    const config = fixture.profiles.find(row => row.id === (target.kind === 'profile' ? target.profileId : tab?.profileId))
    if (!config || config.cli === 'shell') throw { code: 'SOURCE_UNAVAILABLE' }
    return { scopeId: `scope-${config.id}-${target.kind === 'profile' ? target.projectId : tab!.runId}`, instanceId: 'fixture-instance',
      cli: config.cli, sourceRootKey: fixture.roots.get(config.id) ?? `root-${config.id}`, identityEpoch: '1',
      profileId: config.id, profileRevision: config.revision, target,
      basis: target.kind === 'run' ? 'launch-environment' : 'configured-profile' }
  }
  const bridge = vi.fn(async (command: string, argument: any): Promise<unknown> => {
    if (command === 'native_get_scope') return source(argument)
    if (command === 'native_list_resources') {
      const request = argument as ReadRequest
      const target = request.source.target
      const rows = request.resourceKind === 'history'
        ? fixture.histories.get(`${request.source.profileId}:${target.kind === 'profile' ? target.projectId : ''}`) ?? [] : fixture.resources
      const offset = request.offset ?? 0, limit = request.limit ?? 200
      return reply(request, rows.slice(offset, offset + limit), rows.length > offset + limit)
    }
    throw new Error(`Unexpected document command: ${command}`)
  })
  const ipc = vi.fn(async (command: string, args: any = {}): Promise<unknown> => {
    switch (command) {
      case 'get_projects_state': return clone(fixture.state)
      case 'get_app_config': return clone(fixture.config)
      case 'update_app_config': Object.assign(fixture.config, clone(args.updates)); return
      case 'get_projects': return clone(fixture.legacyProjects)
      case 'get_sessions': return clone((fixture.legacyHistory.get(normalizePath(args.projectPath)) ?? []).slice(args.offset ?? 0, args.limit == null ? undefined : (args.offset ?? 0) + args.limit))
      case 'cli_list_profiles': return { revision: '1', profiles: clone(fixture.profiles) }
      case 'cli_list_projects': return { revision: '1', projects: clone(fixture.registered) }
      case 'cli_get_availability': {
        const config = fixture.profiles.find(row => row.id === args.request.profileId)!
        const unavailable = fixture.unavailable.has(config.cli)
        return { profileId: config.id, profileRevision: config.revision, cli: config.cli,
          state: unavailable ? 'unavailable' : 'available-unverified', hostStatus: unavailable ? 'unavailable' : 'available', certified: false,
          ...(unavailable ? { issue: { code: 'PROGRAM_UNAVAILABLE', retryable: true } } : {}) }
      }
      case 'upsert_session_ui_record': fixture.state.sessionRecords ??= {}; fixture.state.sessionRecords[args.recordKey] = clone(args.record); return clone(fixture.state)
      case 'set_display_name': fixture.state.displayNames ??= {}; fixture.state.displayNames[normalizePath(args.path)] = args.alias; return clone(fixture.state)
      case 'set_project_launch_preference': fixture.state.launchPreferences ??= {}; fixture.state.launchPreferences[normalizePath(args.projectPath)] = clone(args.preference); return clone(fixture.state)
      case 'archive_session': fixture.state.archivedSessions[normalizePath(args.projectPath)] = [...(fixture.state.archivedSessions[normalizePath(args.projectPath)] ?? []), args.sessionId]; return clone(fixture.state)
      case 'restore_session': fixture.state.archivedSessions[normalizePath(args.projectPath)] = (fixture.state.archivedSessions[normalizePath(args.projectPath)] ?? []).filter(id => id !== args.sessionId); return clone(fixture.state)
      case 'plugin:event|listen': return 1
      case 'plugin:event|unlisten': return
      default: throw new Error(`Unexpected host command: ${command}`)
    }
  })
  mockIPC(ipc)
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'fixture-instance', invoke: bridge } })
  const host = { startLegacy: vi.fn(), stopLegacy: vi.fn(), restartLegacy: vi.fn(), renameLegacy: vi.fn(),
    stopNative: vi.fn().mockResolvedValue(undefined), recoverNative: vi.fn().mockResolvedValue(undefined), focus: vi.fn() } satisfies UnifiedTerminalHostPort
  function mountRuntime() {
    let runtime!: ReturnType<typeof useUnifiedWorkspaceRuntime>
    const wrapper = mount(defineComponent({ setup() { runtime = useUnifiedWorkspaceRuntime(ref(host)); return () => null } }))
    fixture.wrappers.push(wrapper)
    return { runtime, wrapper }
  }
  function resetStores() { fixture.wrappers.splice(0).forEach(wrapper => wrapper.unmount()); setActivePinia(createPinia()) }
  function dispose() { resetStores(); clearMocks(); delete (window as any).__CC_DESK_DOCUMENT__; vi.unstubAllGlobals() }
  return Object.assign(fixture, { bridge, ipc, host, source, mountRuntime, resetStores, dispose })
}

const terminal = vi.hoisted(() => ({ start: vi.fn(), recover: vi.fn(), cancel: vi.fn(), stop: vi.fn(), resize: vi.fn(), bindings: [] as any[] }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {
  options: any; cols = 80; rows = 24; modes = { bracketedPasteMode: false }; element!: HTMLElement; textarea!: HTMLTextAreaElement
  constructor(options: any) { this.options = options }
  loadAddon() {} open(element: HTMLElement) { this.element = element; this.textarea = document.createElement('textarea'); element.append(this.textarea) }
  onData() { return { dispose() {} } } attachCustomKeyEventHandler() {} getSelection() { return '' } write() {} focus() {} dispose() {}
} }))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit() {} } }))
// Entry and terminal binding are already independently protocol-tested in the
// required Native suite. Here deferred receipts exercise the real component's
// run token plus store/adapter ownership, without starting a process.
vi.mock('@/terminal/nativeLaunchEntry', async original => ({ ...await original<object>(), createNativeLaunchEntry: () => ({ start: terminal.start, recover: terminal.recover, cancel: terminal.cancel, latest: vi.fn() }) }))
vi.mock('@/terminal/deskNativeTerminal', () => ({ createDeskNativeTerminalBinding: (options: any) => {
  const binding = { options, acceptOutput: vi.fn(() => true), dispose: vi.fn(), sendUserText: vi.fn(), reserveUserPaste: vi.fn() }
  terminal.bindings.push(binding); return binding
} }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), cliStop: terminal.stop, cliResize: terminal.resize }))

let f: ReturnType<typeof workspaceFixture>
function status(input: NativeLaunchEntryInput, phase: LaunchStatus['phase'] = 'running'): LaunchStatus {
  return { instanceId: 'fixture-instance', requestId: input.requestId, run: { runId: input.runId, generation: input.generation }, revision: '1', phase, failure: null }
}
beforeEach(() => {
  vi.clearAllMocks(); terminal.bindings.length = 0
  f = workspaceFixture()
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  terminal.start.mockImplementation(async input => status(input))
  terminal.recover.mockImplementation(async request => status(terminal.start.mock.calls.map(([input]) => input).find(input => input.requestId === request)))
  terminal.cancel.mockImplementation(async request => status(terminal.start.mock.calls.map(([input]) => input).find(input => input.requestId === request)))
  terminal.stop.mockResolvedValue(undefined); terminal.resize.mockResolvedValue(undefined)
})
afterEach(() => { f.dispose(); vi.restoreAllMocks() })
async function boot() { const value = f.mountRuntime(); await vi.waitFor(() => expect(value.runtime.ready.value).toBe(true)); return value }
function seedHistory() { f.histories.set('codex:project', [history('codex', 'root-codex', 'same', '/repo', 'History')]) }
async function openTerminal() {
  const catalog = useUnifiedSessionsStore()
  const row = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', title: 'Current attempt' })
  const wrapper = mount(NativeCliTerminal, { props: { tabId: row.adapterSessionId, active: true } })
  f.wrappers.push(wrapper); await flushPromises()
  return { row, wrapper, vm: wrapper.vm as unknown as { stop(attempt: ReturnType<typeof captureNativeAttempt>): Promise<void>; recover(attempt: ReturnType<typeof captureNativeAttempt>): Promise<void> } }
}

describe('Adversarial unified ownership and persistence', () => {
  it('Adversarial_CancelledCreate_001', async () => {
    await boot()
    const original = f.ipc.getMockImplementation()!, registration = deferred<unknown>()
    f.ipc.mockImplementation((command, args) => command === 'cli_register_project' ? registration.promise : original(command, args))
    const catalog = useUnifiedSessionsStore()
    const pending = catalog.createSession({ cli: 'codex', projectKey: '/new', projectPath: '/new' })
    const result = pending.catch(failure => failure)
    await vi.waitFor(() => expect(f.ipc.mock.calls.some(([command]) => command === 'cli_register_project')).toBe(true))
    const oldId = catalog.activeSessionId!
    await catalog.closeSession(oldId)
    const selected = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    registration.resolve({ revision: '2', projects: [project('/new', 'new')], projectId: 'new' })
    await result; await flushPromises()
    expect(catalog.activeSessionId).toBe(selected.id)
    expect([...useNativeTabsStore().tabs.values()].map(tab => tab.projectPath)).toEqual(['/repo'])
    expect(catalog.sessions.some(row => row.id === oldId)).toBe(false)
  })

  it.each(['recover-success', 'recover-failure', 'stop'] as const)('Adversarial_StaleTerminal_%s_002', async mode => {
    await boot(); const { row, vm } = await openTerminal()
    const tabs = useNativeTabsStore(), old = { ...tabs.tab(row.adapterSessionId)! }, pending = deferred<any>()
    if (mode === 'stop') terminal.stop.mockReturnValueOnce(pending.promise)
    else terminal.recover.mockReturnValueOnce(pending.promise)
    const completion = mode === 'stop' ? vm.stop(captureNativeAttempt(old)) : vm.recover(captureNativeAttempt(old))
    await flushPromises()
    tabs.tab(old.tabId)!.status = 'exited'
    const current = tabs.restart(old.tabId, { profileId: 'codex', profileRevision: '1' })
    await flushPromises()
    if (mode === 'recover-failure') pending.reject({ code: 'RUN_NOT_FOUND', message: 'PRIVATE raw transport /secret' })
    else pending.resolve(mode === 'stop' ? undefined : { ...status({ ...old, launchCwd: old.projectPath, extraArgs: [], cols: 80, rows: 24 }), phase: 'exited' })
    await completion; await flushPromises()
    expect(tabs.tab(old.tabId)).toMatchObject({ requestId: current.requestId, runId: current.runId, generation: 2, status: 'running', errorCode: null })
    expect(terminal.start).toHaveBeenCalledTimes(2)
    if (mode === 'stop') { expect(terminal.stop).toHaveBeenCalledExactlyOnceWith({ runId: old.runId, generation: 1 }); expect(terminal.recover).not.toHaveBeenCalled() }
  })

  it.each(['history', 'native-live', 'legacy-live'] as const)('Adversarial_QueuedRename_%s_003', async kind => {
    seedHistory()
    f.legacyProjects = [{ path: '/repo', name: 'Repo' }]
    f.legacyHistory.set('/repo', [{ sessionId: 'same', name: 'Legacy', projectPath: '/repo', lastActiveAt: 1 }])
    await boot()
    const catalog = useUnifiedSessionsStore()
    let row = catalog.sessions.find(row => row.runtime === (kind === 'legacy-live' ? 'legacy-claude' : 'native-cli'))!
    if (kind !== 'history') row = await catalog.resumeCatalogSession(row)
    const barrier = deferred<any>(), original = f.ipc.getMockImplementation()!
    f.ipc.mockImplementation((command, args) => command === 'pin_project' ? barrier.promise : original(command, args))
    const projects = useProjectsStateStore(), pinning = projects.pinProject('/other')
    await vi.waitFor(() => expect(f.ipc.mock.calls.some(([command]) => command === 'pin_project')).toBe(true))
    catalog.beginRename(row.id)
    const renaming = catalog.renameSession(row.id, 'Must not persist stale name')
    const outcome = renaming.then(() => null, failure => failure)
    await flushPromises()
    if (kind === 'history') useNativeHistoryStore().invalidate(row.nativeOrigin)
    else if (kind === 'native-live') useNativeTabsStore().restart(row.adapterSessionId, { profileId: 'codex', profileRevision: '1' })
    else useSessionStore().tabs.get(row.adapterSessionId)!.ptyGeneration = 2
    await catalog.refresh(); await flushPromises()
    barrier.resolve(clone(f.state)); await pinning
    expect(await outcome).toMatchObject({ message: 'STALE_SESSION_ATTEMPT' })
    expect(f.ipc.mock.calls.filter(([command]) => command === 'upsert_session_ui_record')).toHaveLength(0)
    expect(f.state.sessionRecords).toEqual({}); expect(projects.error).toBe(false)
    expect(f.host.renameLegacy).not.toHaveBeenCalled()
  })

  it('Adversarial_RenameConflict_004', async () => {
    seedHistory(); await boot()
    const catalog = useUnifiedSessionsStore(), row = catalog.sessions[0], original = f.ipc.getMockImplementation()!
    f.ipc.mockImplementation(async (command, args) => {
      if (command === 'upsert_session_ui_record') {
        await original(command, { ...args, record: { ...args.record, title: 'Other instance name' } })
        throw { code: 'REVISION_CONFLICT', message: 'private credential=DO_NOT_RENDER' }
      }
      return original(command, args)
    })
    await expect(catalog.renameSession(row.id, 'My name')).rejects.toMatchObject({ code: 'REVISION_CONFLICT' })
    await catalog.refresh()
    expect(catalog.sessions[0].title).toBe('Other instance name')
    expect(f.ipc.mock.calls.filter(([command]) => command === 'upsert_session_ui_record')).toHaveLength(1)
    expect(f.ipc.mock.calls.filter(([command]) => command === 'get_projects_state')).toHaveLength(2)
    expect(useProjectsStateStore().lastErrorCode).toBe('REVISION_CONFLICT')
    expect(terminal.start).not.toHaveBeenCalled()
  })

  it('Adversarial_DuplicateRoots_005', async () => {
    seedHistory(); f.profiles.push(profile('codex', 'other')); f.roots.set('other', 'other-root')
    f.histories.set('other:project', [history('codex', 'other-root', 'same', '/repo', 'Other history')])
    f.legacyProjects = [{ path: '/repo', name: 'Repo' }]; f.legacyHistory.set('/repo', [{ sessionId: 'same', name: 'Legacy', projectPath: '/repo', lastActiveAt: 1 }])
    await boot()
    const catalog = useUnifiedSessionsStore()
    expect(catalog.sessions).toHaveLength(3)
    const native = catalog.sessions.find(row => row.title === 'History')!, other = catalog.sessions.find(row => row.title === 'Other history')!
    await catalog.renameSession(native.id, 'Only this origin')
    expect(catalog.sessions.find(row => row.id === other.id)?.title).toBe('Other history')
    const opened = await catalog.resumeCatalogSession(native.id)
    await catalog.stopSession(opened.id)
    expect(f.host.stopNative).toHaveBeenCalledExactlyOnceWith(opened.adapterSessionId, captureNativeAttempt(useNativeTabsStore().tab(opened.adapterSessionId)!))
    expect(f.host.stopLegacy).not.toHaveBeenCalled()
    await expect(catalog.stopSession(other.id)).rejects.toThrow('NATIVE_ACTIVE_SESSION_REQUIRED')
    expect(f.host.stopNative).toHaveBeenCalledTimes(1)
    const old = await catalog.resumeCatalogSession(catalog.sessions.find(row => row.runtime === 'legacy-claude')!.id)
    await catalog.stopSession(old.id)
    expect(f.host.stopLegacy).toHaveBeenCalledExactlyOnceWith(old.adapterSessionId)
    expect(f.host.stopNative).toHaveBeenCalledTimes(1)
    expect(f.ipc.mock.calls.some(([command]) => /^pty_/.test(command))).toBe(false)
  })

  it.each(['claude', 'codex'] as const)('Adversarial_OneCliMissing_%s_006', async absent => {
    f.profiles = [profile('claude'), profile('codex')]; f.unavailable.add(absent)
    const other = absent === 'claude' ? 'codex' : 'claude'
    const { runtime } = await boot()
    expect(useNewSessionDraftStore().cliAvailability[absent]).toBe('unavailable')
    // Filesystem preflight is not a real successful CLI launch receipt.
    expect(useNewSessionDraftStore().cliAvailability[other]).toBe('unknown')
    const row = await useUnifiedSessionsStore().createSession({ cli: other, projectKey: '/repo', projectPath: '/repo' })
    expect(useNativeTabsStore().tab(row.adapterSessionId)?.cli).toBe(other)
    expect(runtime.cliProblems.value.map(value => value.cli)).toEqual([absent])
    expect(f.host.startLegacy).not.toHaveBeenCalled()
  })

  it.each(['success', 'failure'] as const)('Adversarial_StaleResources_%s_007', async result => {
    await boot(); const catalog = useUnifiedSessionsStore()
    const old = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    const pending = deferred<unknown>(), original = f.bridge.getMockImplementation()!
    let request!: ReadRequest
    f.bridge.mockImplementation((command, args) => command === 'native_list_resources' ? (request = args, pending.promise) : original(command, args))
    const resources = useProjectResourcesStore(); resources.setActive(true); await flushPromises()
    expect(request.source.target.kind).toBe('profile')
    f.bridge.mockImplementation(original)
    const current = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
    expect(resources.context?.sessionId).toBe(current.id)
    if (result === 'success') pending.resolve(reply(request, [{ type: 'document', name: 'CLAUDE.md', text: 'Old instructions', origin: 'project', truncated: false }]))
    else pending.reject({ code: 'SOURCE_CHANGED', message: 'PRIVATE old resource failure' })
    await flushPromises()
    expect(resources.items).toEqual([]); expect(resources.error).toBeNull(); expect(resources.loading).toBe(false)
    expect(catalog.activeSessionId).toBe(current.id); expect(current.id).not.toBe(old.id)
  })

  it('Adversarial_ResourceRedaction_008', async () => {
    await boot(); await useUnifiedSessionsStore().createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    f.resources = [{ type: 'setting', name: 'model', value: 'sk-proj-DO_NOT_RENDER', origin: '/private/DO_NOT_RENDER' },
      { type: 'setting', name: 'language', value: 'English', origin: 'project' },
      { type: 'setting', name: 'outputStyle', value: 'Authorization: Bearer DO_NOT_RENDER', origin: 'global' }]
    const resources = useProjectResourcesStore(); resources.kind = 'config'; resources.setActive(true)
    const wrapper = mount(ProjectResourcesDrawer, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
    f.wrappers.push(wrapper); await flushPromises()
    expect(wrapper.text()).toContain('English'); expect(wrapper.html()).not.toContain('DO_NOT_RENDER')
    expect(resources.items.filter(value => value.withheld)).toHaveLength(2)
    f.bridge.mockRejectedValue({ code: 'SOURCE_CHANGED', message: 'PRIVATE token=DO_NOT_RENDER C:\\secret' })
    await resources.refresh(); await flushPromises()
    expect(resources.error).toBe('SOURCE_CHANGED'); expect(wrapper.html()).not.toContain('DO_NOT_RENDER')
    expect(wrapper.html()).not.toContain('C:\\secret')
  })
})
