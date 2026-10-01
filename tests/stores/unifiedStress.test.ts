import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent, h, onMounted, onUnmounted } from 'vue'
import { createI18n } from 'vue-i18n'
import UnifiedTerminalHost from '@/components/workspace/UnifiedTerminalHost.vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useSessionStore } from '@/stores/session'
import { useShellStore } from '@/stores/shell'
import { useAppStore } from '@/stores/app'
import { normalizePath } from '@/utils/path'
import en from '@/i18n/locales/en'
import { createPinia, setActivePinia } from 'pinia'
import { ref } from 'vue'
import { type VueWrapper } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { webcrypto } from 'node:crypto'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
import type { ProjectsState } from '@/types/app'
import type { CliProfile } from '@/types/profile'
import type { RegisteredProject } from '@/types/workspace'
import type { NativeHistorySession } from '@/stores/nativeHistory'
import type { SourceRef, ScopeTarget, ReadRequest, ResourceItem, ProjectionResult } from '@/types/nativeProjection'
import type { UnifiedTerminalHostPort } from '@/terminal/unifiedTerminalHost'

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

vi.mock('@/utils/platform', () => ({ isWindows: true, isMac: false, platform: 'windows' }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
let f: ReturnType<typeof workspaceFixture>
beforeEach(() => { f = workspaceFixture() })
afterEach(() => { f.dispose(); vi.restoreAllMocks() })
function seed() {
  f.registered = []
  for (let p = 0; p < 50; p++) {
    const path = `D:\\Work\\${'very-long-directory\\'.repeat(15)}Project-${p}`
    f.legacyProjects.push({ path, name: '项目'.repeat(39) + String(p).padStart(2, '0') })
    f.registered.push(project(path.toLowerCase().replace(/\\/g, '/'), `p-${p}`))
    f.state.displayNames![normalizePath(path)] = f.legacyProjects[p].name
    f.legacyHistory.set(normalizePath(path), Array.from({ length: 50 }, (_, s) => ({
      sessionId: `s-${s}`, name: `${'旧'.repeat(193)}${String(p).padStart(3, '0')}-${String(s).padStart(3, '0')}`,
      projectPath: path, lastActiveAt: s + 1,
    })))
    f.histories.set(`codex:p-${p}`, Array.from({ length: 50 }, (_, s) => history('codex', 'root-codex', `s-${s}`, path,
      `${'N'.repeat(193)}${String(p).padStart(3, '0')}-${String(s).padStart(3, '0')}`)))
  }
}

it('Stress_50Projects100Sessions_001', async () => {
  seed()
  const started = performance.now()
  const { runtime } = f.mountRuntime()
  await vi.waitFor(() => expect(runtime.ready.value).toBe(true))
  const catalog = useUnifiedSessionsStore()
  expect(catalog.projectGroups).toHaveLength(50)
  expect(catalog.sessions).toHaveLength(5000)
  expect(new Set(catalog.sessions.map(row => row.id)).size).toBe(5000)
  for (const group of catalog.projectGroups) {
    expect(group.sessions).toHaveLength(100)
    expect(group.name).toHaveLength(80)
    expect(group.projectPath.length).toBeGreaterThan(260)
    expect(group.sessions.every(row => row.title.length === 200)).toBe(true)
    expect(group.sessions.filter(row => row.runtime === 'legacy-claude')).toHaveLength(50)
    expect(group.sessions.filter(row => row.runtime === 'native-cli')).toHaveLength(50)
  }
  await catalog.refresh(f.registered[12].selectedPath.toUpperCase())
  expect(catalog.sessions).toHaveLength(5000)
  const found = await catalog.searchSessions({ scope: 'current-project', projectPath: f.registered[12].selectedPath, query: 'N'.repeat(193) })
  expect(found.partial).toBe(false); expect(found.sessions).toHaveLength(50)
  expect(found.sessions.every(row => row.projectKey === normalizePath(f.registered[12].selectedPath))).toBe(true)
  expect(useNativeTabsStore().tabs.size).toBe(0); expect(useSessionStore().tabs.size).toBe(0)
  expect(f.host.startLegacy).not.toHaveBeenCalled()
  console.info(`Task24 stress catalog: 50 projects, 5000 sessions, ${Math.round(performance.now() - started)}ms`)
}, 30000)

it('Stress_30Owners120Changes_002', async () => {
  seed()
  const tabs = useNativeTabsStore(), legacy = useSessionStore()
  for (let i = 0; i < 20; i++) {
    const entry = f.histories.get(`codex:p-${i}`)![0]
    const tab = tabs.create({ cli: 'codex', projectId: `p-${i}`, projectPath: f.registered[i].selectedPath,
      profileId: 'codex', profileRevision: '1', sourceSessionKey: entry.sessionKey,
      action: { kind: 'resume-id', nativeSessionId: entry.nativeSessionId }, title: entry.title })
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'fixture-instance', requestId: tab.requestId, run: { runId: tab.runId, generation: 1 }, revision: '1', phase: 'running', failure: null })
  }
  for (let i = 20; i < 30; i++) {
    const id = legacy.createTab(f.legacyProjects[i].path, { sessionId: 's-0', name: f.legacyHistory.get(normalizePath(f.legacyProjects[i].path))![0].name })
    Object.assign(legacy.tabs.get(id)!, { status: 'running', ptyId: `pty-${i}`, ptyGeneration: 1 })
  }
  const initialNative = [...tabs.tabs.values()].map(tab => [tab.tabId, tab.requestId, tab.runId, tab.generation, tab.action])
  const initialLegacy = [...legacy.tabs.values()].map(tab => [tab.tabId, tab.ptyId, tab.ptyGeneration, tab.sessionId])
  const { runtime } = f.mountRuntime(); await vi.waitFor(() => expect(runtime.ready.value).toBe(true))
  expect(runtime.openSessions.value).toHaveLength(30)
  const mounted: string[] = [], unmounted: string[] = []
  const TerminalPort = defineComponent({ props: ['tabId', 'active', 'visible'], setup(props, { expose }) {
    const id = props.tabId ?? 'legacy-aggregator'
    onMounted(() => mounted.push(id)); onUnmounted(() => unmounted.push(id))
    expose({ focus() {}, fitVisible() {} })
    return () => h('div', { 'data-owner': id, 'data-visible': props.active ?? props.visible })
  } })
  const catalog = useUnifiedSessionsStore(), shell = useShellStore(), app = useAppStore()
  const wrapper = mount(defineComponent({ setup: () => () => h(UnifiedTerminalHost, { sessions: runtime.openSessions.value,
    activeSessionId: catalog.activeSessionId, visible: shell.section === 'workspace' }) }), {
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { TerminalView: TerminalPort, NativeCliTerminal: TerminalPort } },
  })
  f.wrappers.push(wrapper); await flushPromises()
  const surfaces = wrapper.findAll('[data-owner]').map(value => value.element)
  const started = performance.now()
  for (let n = 0; n < 120; n++) {
    const descriptor = runtime.openSessions.value[n % 30]
    await catalog.activateSession(descriptor.id)
    if (descriptor.runtime === 'native-cli') {
      const tab = tabs.tab(descriptor.adapterSessionId)!
      tabs.markUnknown(tab.tabId)
      tabs.applyLaunchStatus(tab.tabId, { instanceId: 'fixture-instance', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: String(n + 2), phase: 'running', failure: null })
    } else legacy.tabs.get(descriptor.adapterSessionId)!.pending = n % 2 === 0
    shell.drawerVisible = n % 2 === 0; shell.setSidebarWidth(240 + n % 120)
    app.theme = n % 2 ? 'dark' : 'light'; app.terminalTheme = n % 2 ? 'cc-box-light' : 'cc-box-dark'
    shell.navigate(n % 3 ? 'workspace' : 'settings')
    await flushPromises()
    expect(catalog.activeSessionId).toBe(descriptor.id)
  }
  expect(catalog.sessions).toHaveLength(5000)
  expect(runtime.openSessions.value).toHaveLength(30)
  expect(wrapper.findAll('[data-owner]').map(value => value.element)).toEqual(surfaces)
  expect(mounted).toHaveLength(21); expect(unmounted).toEqual([])
  expect([...tabs.tabs.values()].map(tab => [tab.tabId, tab.requestId, tab.runId, tab.generation, tab.action])).toEqual(initialNative)
  expect([...legacy.tabs.values()].map(tab => [tab.tabId, tab.ptyId, tab.ptyGeneration, tab.sessionId])).toEqual(initialLegacy)
  expect(f.host.stopNative).not.toHaveBeenCalled(); expect(f.host.startLegacy).not.toHaveBeenCalled()
  expect(f.ipc.mock.calls.some(([command]) => /^(pty_|cli_launch|cli_stop)/.test(command))).toBe(false)
  console.info(`Task24 stress continuity: 30 descriptors, 120 state/selection/layout changes, ${Math.round(performance.now() - started)}ms`)
}, 30000)
