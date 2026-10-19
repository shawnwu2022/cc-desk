import { useNotificationsStore } from '@/stores/notifications'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useSessionStore } from '@/stores/session'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useAppStore } from '@/stores/app'
import { useShellStore } from '@/stores/shell'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { normalizePath } from '@/utils/path'
import type { SessionUiRecord } from '@/types/app'
import { createPinia, setActivePinia } from 'pinia'
import { defineComponent, ref } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { webcrypto } from 'node:crypto'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
import { legacyHistoryId } from '@/session/adapters/legacyClaudeAdapter'
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
    truncated: false, archived: false, cwd: path, updatedAt: '2026-09-30T12:00:00Z' }
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
    renameFailure: null as string | null,
    legacyRoot: undefined as string | undefined,
    wrappers: [] as VueWrapper[],
  }
  function source(target: ScopeTarget): SourceRef {
    const tab = target.kind === 'run' ? [...useNativeTabsStore().tabs.values()].find(row => row.runId === target.runId && row.generation === target.generation) : undefined
    const config = fixture.profiles.find(row => row.id === (target.kind === 'profile' ? target.profileId : tab?.profileId))
    if (!config || config.cli === 'shell') throw { code: 'SOURCE_UNAVAILABLE' }
    return { scopeId: `scope-${config.id}-${target.kind === 'profile' ? target.projectIds.join('|') : tab!.runId}`, instanceId: 'fixture-instance',
      cli: config.cli, sourceRootKey: fixture.roots.get(config.id) ?? `root-${config.id}`, identityEpoch: '1',
      profileId: config.id, profileRevision: config.revision, target,
      basis: target.kind === 'run' ? 'launch-environment' : 'configured-profile' }
  }
  const bridge = vi.fn(async (command: string, argument: any): Promise<unknown> => {
    if (command === 'native_get_scope') return source(argument)
    if (command === 'cli_rename_session') {
      if (fixture.renameFailure) throw new Error(fixture.renameFailure)
      for (const [key, rows] of fixture.histories) if (key.startsWith(argument.source.profileId + ':')) {
        const item = rows.find(row => row.sessionKey === argument.sessionKey)
        if (item) item.title = argument.title
      }
      return { ...argument, state: 'renamed' }
    }
    if (command === 'native_list_resources') {
      const request = argument as ReadRequest
      const target = request.source.target
      const scoped = target.kind === 'profile' ? target.projectIds : []
      const rows = request.resourceKind === 'history'
        ? scoped.flatMap(id => (fixture.histories.get(`${request.source.profileId}:${id}`) ?? []).map(row => ({ ...row, projectId: id }))) : fixture.resources
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
      case 'legacy_claude_root_key': return fixture.legacyRoot ? { rootKey: fixture.legacyRoot } : null
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
    stopNative: vi.fn().mockResolvedValue(undefined), recoverNative: vi.fn().mockResolvedValue(undefined), renameNative: vi.fn().mockResolvedValue(undefined), focus: vi.fn() } satisfies UnifiedTerminalHostPort
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
let f: ReturnType<typeof workspaceFixture>
beforeEach(() => { f = workspaceFixture() })
afterEach(() => { f.dispose(); vi.restoreAllMocks() })
async function boot() { const value = f.mountRuntime(); await flushPromises(); await vi.waitFor(() => expect(value.runtime.ready.value).toBe(true)); return value }
function seedBoth() {
  f.legacyProjects = [{ path: 'D:\\Work\\Project\\', name: 'Project' }]
  f.legacyHistory.set('d:/work/project', [{ sessionId: 'same', name: 'Old Claude', projectPath: 'D:\\Work\\Project', lastActiveAt: 10 }])
  f.registered = [project('d:/work/project')]
  f.histories.set('codex:project', [history('codex', 'root-codex', 'same', 'd:/work/project', 'Native history')])
}
const mutations = () => f.ipc.mock.calls.filter(([command]) => /^(pty_|delete_sessions|cli_patch_|cli_register_|upsert_session_|archive_session|restore_session)/.test(command))

describe('Unified migration through real workspace composition', () => {
  it.each(['legacy', 'native', 'both'] as const)('Migration_ReadOnlyOldData_%s_001', async kind => {
    seedBoth()
    if (kind === 'legacy') { f.profiles = []; f.registered = [] }
    if (kind === 'native') f.legacyProjects = []
    const legacy = useSessionStore()
    const oldId = kind !== 'native' ? legacy.createTab('D:/WORK/Project', { sessionId: 'same', name: 'Already running' }) : null
    if (oldId) { legacy.tabs.get(oldId)!.status = 'running'; legacy.tabs.get(oldId)!.ptyId = 'owned-old-pty' }
    const before = oldId ? { ...legacy.tabs.get(oldId)! } : null
    await boot()
    const catalog = useUnifiedSessionsStore()
    expect(catalog.sessions).toHaveLength(kind === 'both' ? 2 : 1)
    expect(catalog.projectGroups).toHaveLength(1)
    expect(new Set(catalog.sessions.map(row => row.id)).size).toBe(catalog.sessions.length)
    expect(catalog.sessions.every(row => row.nativeSessionId === 'same')).toBe(true)
    if (oldId) expect(legacy.tabs.get(oldId)).toEqual(before)
    expect(mutations()).toEqual([]); expect(f.host.startLegacy).not.toHaveBeenCalled(); expect(f.host.stopNative).not.toHaveBeenCalled()
  })

  it.each(['not-a-theme', 'cc-box-dark'])('Migration_ThemeFallback_%s_002', async terminalTheme => {
    f.config = { theme: 'dark', terminalTheme, language: 'zh', fontSize: 17, terminalFontFamily: 'Cascadia Code', shortcutBindings: { newSession: 'Alt+N' } }
    const app = useAppStore(); await app.loadAppConfig()
    expect(app.terminalTheme).toBe('cc-box-dark')
    expect(app.theme).toBe('dark'); expect(app.fontSize).toBe(17); expect(app.language).toBe('zh')
    expect(mutations()).toEqual([])
  })

  // The host boundary can contain partially damaged optional metadata. One bad
  // entry must not become a selected launch preference or replace a good name.
  it('Migration_BadOptionalEntries_003', async () => {
    seedBoth()
    f.state.sessionRecords = { good: { runtime: 'native-cli', cli: 'codex', projectPath: '/repo', adapterSessionId: 'a', title: 'Valid', lastActivityAt: 1 },
      null: null, scalar: 'bad', badRuntime: { runtime: 'shell', cli: 'codex', projectPath: '/repo', adapterSessionId: 'a', title: 'Bad', lastActivityAt: 1 },
      tooLong: { runtime: 'native-cli', cli: 'codex', projectPath: '/repo', adapterSessionId: 'a', title: 'x'.repeat(201), lastActivityAt: 1 },
      poisoned: { runtime: 'native-cli', cli: 'codex', projectPath: '/repo', adapterSessionId: 'a', title: 'Bad\0', lastActivityAt: 1 } } as unknown as Record<string, SessionUiRecord>
    f.state.launchPreferences = { 'd:/work/project': { lastCli: 'codex', codexLaunchConfigId: 42 }, good: { lastCli: 'claude', claudeLaunchConfigId: 'claude' } } as any
    f.state.displayNames = { 'd:/work/project': '保存的项目名', bad: null } as any
    await boot()
    const state = useProjectsStateStore()
    expect([...state.sessionRecords.keys()]).toEqual(['good'])
    expect([...state.launchPreferences.keys()]).toEqual(['good'])
    expect(useUnifiedSessionsStore().projectGroups[0].name).toBe('保存的项目名')
    expect(useNewSessionDraftStore().preferred({ projectPath: 'D:/Work/Project' }, 'codex')?.id).toBe('codex')
    expect(mutations()).toEqual([])
  })

  it.each([null, [], 'damaged', 17])('Migration_BadOptionalContainer_%j_004', async bad => {
    f.state.sessionRecords = bad as any; f.state.launchPreferences = bad as any; f.state.displayNames = bad as any
    await useProjectsStateStore().load()
    expect(useProjectsStateStore().sessionRecords.size).toBe(0)
    expect(useProjectsStateStore().launchPreferences.size).toBe(0)
    expect(useProjectsStateStore().displayNames.size).toBe(0)
  })

  it('Migration_ProjectAliasRestart_005', async () => {
    seedBoth(); await boot()
    await useProjectsStateStore().setProjectDisplayName('D:\\WORK\\PROJECT', '工作项目')
    f.resetStores(); await boot()
    expect(useUnifiedSessionsStore().projectGroups).toHaveLength(1)
    expect(useUnifiedSessionsStore().projectGroups[0].name).toBe('工作项目')
    expect(useUnifiedSessionsStore().projectGroups[0].projectKey).toBe(normalizePath('D:/Work/Project'))
  })

  it('Migration_NativeHistoryName_006', async () => {
    seedBoth(); await boot()
    const catalog = useUnifiedSessionsStore(), row = catalog.sessions.find(row => row.runtime === 'native-cli')!
    catalog.activeSessionId = row.id
    catalog.beginRename(row.id)
    useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: row.id, title: 'Saved history name' })
    await flushPromises()
    expect(catalog.sessions.find(value => value.id === row.id)?.title).toBe('Saved history name')
    expect(f.state.sessionRecords?.[row.id]?.title).not.toBe('Saved history name')
    f.resetStores(); await boot()
    expect(useUnifiedSessionsStore().sessions.find(value => value.id === row.id)?.title).toBe('Saved history name')
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(f.histories.get('codex:project')![0].title).toBe('Saved history name')
  })

  it('Migration_NativeLiveNameRestart_007', async () => {
    seedBoth(); await boot()
    const catalog = useUnifiedSessionsStore(), row = catalog.sessions.find(row => row.runtime === 'native-cli')!
    const active = await catalog.resumeCatalogSession(row)
    const tabs = useNativeTabsStore(), frozen = { ...tabs.tab(active.adapterSessionId)! }
    await expect(catalog.renameSession(active.id, 'Saved source name')).resolves.toBe('native-title-saved')
    await catalog.refresh()
    expect(catalog.sessions.find(value => value.id === active.id)?.title).toBe('Saved source name')
    expect(tabs.tab(active.adapterSessionId)).toMatchObject({ requestId: frozen.requestId, runId: frozen.runId, generation: frozen.generation, action: frozen.action })
    tabs.close(active.adapterSessionId); await catalog.refresh()
    expect(catalog.sessions.find(value => value.id === row.id)?.title).toBe('Saved source name')
    f.resetStores(); await boot()
    expect(useUnifiedSessionsStore().sessions.find(value => value.id === row.id)?.title).toBe('Saved source name')
    expect(f.histories.get('codex:project')![0].title).toBe('Saved source name')
    expect(f.host.stopNative).not.toHaveBeenCalled()
  })

  it.each(['codex','claude'] as const)('Migration_RenameSharedPhysicalSourceHasOneLifecycleRow_015 %s', async cli => {
    seedBoth()
    f.profiles=[profile(cli),profile(cli, 'other-'+cli)]
    f.histories.clear()
    f.histories.set(cli+':project', [history(cli, 'root-'+cli, 'same', 'D:/Work/Project', 'Native history')])
    f.roots.set('other-'+cli, 'root-'+cli)
    f.histories.set('other-'+cli+':project', [history(cli, 'root-'+cli, 'same', 'D:/Work/Project', 'Native history')])
    await boot()
    const catalog=useUnifiedSessionsStore(),original=catalog.sessions.find(row=>row.runtime==='native-cli'&&row.launchConfigId===cli)!
    const active=await catalog.resumeCatalogSession(original)
    await catalog.renameSession(active.id,'Persistent native name')
    await Promise.all([catalog.refresh(),catalog.refresh('D:/Work/Project',{background:true})])
    expect(catalog.sessions.filter(row=>row.runtime==='native-cli'&&row.cli===cli&&row.nativeSessionId==='same')).toHaveLength(1)
    useNativeTabsStore().close(active.adapterSessionId);await catalog.refresh()
    const closed=catalog.sessions.filter(row=>row.runtime==='native-cli'&&row.cli===cli&&row.nativeSessionId==='same')
    expect(closed).toHaveLength(1);expect(closed[0].title).toBe('Persistent native name')
    const restored=await catalog.resumeCatalogSession(closed[0]);await catalog.refresh()
    expect(catalog.sessions.filter(row=>row.runtime==='native-cli'&&row.cli===cli&&row.nativeSessionId==='same')).toHaveLength(1)
    useNativeTabsStore().close(restored.adapterSessionId)
    f.resetStores();await boot()
    const rebooted=useUnifiedSessionsStore().sessions.filter(row=>row.runtime==='native-cli'&&row.cli===cli&&row.nativeSessionId==='same')
    expect(rebooted).toHaveLength(1);expect(rebooted[0].title).toBe('Persistent native name')
    expect(f.histories.get(cli+':project')![0].nativeSessionId).toBe('same')
  })

  it.each(['codex','claude'] as const)('Migration_RenameFailureKeepsNativeHistoryAndAlias_016 %s', async cli => {
    f.profiles=[profile(cli)]
    f.histories.set(cli+':project',[history(cli,'root-'+cli,'same','/repo','Native original')])
    await boot()
    const catalog=useUnifiedSessionsStore(),original=catalog.sessions.find(row=>row.runtime==='native-cli')!
    await useProjectsStateStore().upsertSessionRecord(original.id,{runtime:original.runtime,cli,projectPath:original.projectPath,adapterSessionId:original.adapterSessionId,nativeSessionId:'same',title:'Existing alias',lastActivityAt:1})
    const active=await catalog.resumeCatalogSession(original)
    f.renameFailure='RENAME_OUTCOME_UNKNOWN'
    await expect(catalog.renameSession(active.id,'Unconfirmed new name')).rejects.toThrow()
    await Promise.all([catalog.refresh(),catalog.refresh('/repo',{background:true})])
    expect(catalog.sessions.filter(row=>row.runtime==='native-cli')).toHaveLength(1)
    expect(f.histories.get(cli+':project')![0]).toMatchObject({nativeSessionId:'same',title:'Native original'})
    useNativeTabsStore().close(active.adapterSessionId);await catalog.refresh()
    expect(catalog.sessions[0].title).toBe('Existing alias')
    const restored=await catalog.resumeCatalogSession(catalog.sessions[0]);useNativeTabsStore().close(restored.adapterSessionId)
    f.resetStores();await boot()
    expect(useUnifiedSessionsStore().sessions).toHaveLength(1)
    expect(useUnifiedSessionsStore().sessions[0].title).toBe('Existing alias')
    expect(f.bridge.mock.calls.filter(([command])=>command==='cli_rename_session')).toHaveLength(1)
  })

  it('Migration_NativeRenameClearsProvenLegacyAliasAcrossReboot_017', async () => {
    seedBoth()
    f.profiles=[profile('claude')];f.histories.clear();f.legacyRoot='root-claude'
    f.histories.set('claude:project',[history('claude','root-claude','same','D:/Work/Project','Original native title')])
    const legacyKey=legacyHistoryId('D:/Work/Project','same')
    f.state.sessionRecords![legacyKey]={runtime:'legacy-claude',cli:'claude',projectPath:'D:/Work/Project',adapterSessionId:'same',nativeSessionId:'same',title:'Old saved alias',lastActivityAt:1}
    await boot()
    const catalog=useUnifiedSessionsStore()
    expect(catalog.sessions).toHaveLength(1);expect(catalog.sessions[0].title).toBe('Old saved alias')
    const active=await catalog.resumeCatalogSession(catalog.sessions[0])
    await catalog.renameSession(active.id,'Official persistent title')
    await Promise.all([catalog.refresh(),catalog.refresh('D:/Work/Project',{background:true})])
    expect(catalog.sessions).toHaveLength(1);expect(catalog.sessions[0].title).toBe('Official persistent title')
    useNativeTabsStore().close(active.adapterSessionId);await catalog.refresh()
    expect(catalog.sessions).toHaveLength(1);expect(catalog.sessions[0].title).toBe('Official persistent title')
    expect(f.state.sessionRecords![legacyKey].title).toBe('')
    f.resetStores();await boot()
    expect(useUnifiedSessionsStore().sessions).toHaveLength(1)
    expect(useUnifiedSessionsStore().sessions[0].title).toBe('Official persistent title')
  })

  it('Migration_ExactMetadataOrigin_008', async () => {
    seedBoth(); f.profiles.push(profile('codex', 'other')); f.roots.set('other', 'other-root')
    f.histories.set('other:project', [history('codex', 'other-root', 'same', 'd:/work/project', 'Other source')])
    await boot()
    const row = useUnifiedSessionsStore().sessions.find(row => row.title === 'Native history')!
    const record: SessionUiRecord = { runtime: row.runtime, cli: row.cli, projectPath: row.projectPath, adapterSessionId: row.adapterSessionId,
      nativeSessionId: row.nativeSessionId, title: 'Exact name', lastActivityAt: row.lastActivityAt }
    f.state.sessionRecords = { [row.id]: record, same: { ...record, title: 'Raw ID must not match' } }
    f.resetStores(); await boot()
    expect(useUnifiedSessionsStore().sessions.map(row => row.title).sort()).toEqual(['Exact name', 'Old Claude', 'Other source'])
    f.profiles[0].revision = '2'; f.resetStores(); await boot()
    expect(useUnifiedSessionsStore().sessions.map(row => row.title).sort()).toEqual(['Native history', 'Old Claude', 'Other source'])
  })

  it.each(['history', 'live'] as const)('Migration_LegacyName_%s_009', async kind => {
    f.legacyRoot = 'legacy-root'
    seedBoth(); await boot()
    const catalog = useUnifiedSessionsStore(), row = catalog.sessions.find(row => row.runtime === 'legacy-claude')!
    const target = kind === 'live' ? await catalog.resumeCatalogSession(row) : row
    if (kind === 'live') useSessionStore().tabs.get(target.adapterSessionId)!.status = 'running'
    catalog.activeSessionId = target.id
    catalog.beginRename(target.id)
    useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: target.id, title: 'Desk-only name' })
    await flushPromises()
    expect(catalog.sessions.find(value => value.id === target.id)?.title).toBe('Old Claude')
    expect(f.host.renameLegacy).not.toHaveBeenCalled()
    expect(f.ipc.mock.calls.some(([command]) => command === 'pty_input')).toBe(false)
    expect(catalog.actionFeedback?.detailCode).toBe('NATIVE_RENAME_UNSUPPORTED')
    expect(useNotificationsStore().toasts.some(toast => toast.messageKey === 'feedbackRenamed')).toBe(false)
    f.resetStores(); await boot()
    expect(useUnifiedSessionsStore().sessions.find(value => value.id === row.id)?.title).toBe('Old Claude')
    expect(f.legacyHistory.get('d:/work/project')![0].name).toBe('Old Claude')
  })

  it('Migration_UnassociatedName_010', async () => {
    await boot()
    const catalog = useUnifiedSessionsStore(), tabs = useNativeTabsStore()
    const row = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', action: { kind: 'raw', argv: ['', 'two words'] } })
    await expect(catalog.renameSession(row.id, 'Local tab name')).rejects.toThrow('SESSION_IDENTITY_UNKNOWN')
    expect(f.state.sessionRecords?.[row.id]?.title).not.toBe('Local tab name')
    const before = tabs.tab(row.adapterSessionId)!
    tabs.markError(before.tabId, 'LAUNCH_FAILED')
    tabs.restart(before.tabId, { profileId: 'codex', profileRevision: '1' }); await catalog.refresh()
    expect(catalog.sessions.find(value => value.id === row.id)?.title).not.toBe('Local tab name')
    f.histories.set('codex:project', [history('codex', 'root-codex', row.adapterSessionId, '/repo', 'Discovered native title')])
    f.resetStores(); await boot()
    expect(useUnifiedSessionsStore().sessions.map(value => value.title)).toEqual(['Discovered native title'])
    expect(useNativeTabsStore().tabs.size).toBe(0)
  })

  it.each([
    { runtime: 'legacy-claude' }, { cli: 'claude' }, { projectPath: '/other' },
    { adapterSessionId: 'other-key' }, { nativeSessionId: 'other-native-id' },
  ] as Partial<SessionUiRecord>[])('Migration_RecordIdentity_%j_011', async mismatch => {
    seedBoth(); await boot()
    const row = useUnifiedSessionsStore().sessions.find(value => value.runtime === 'native-cli')!
    f.state.sessionRecords = { [row.id]: { runtime: row.runtime, cli: row.cli, projectPath: row.projectPath, adapterSessionId: row.adapterSessionId,
      nativeSessionId: row.nativeSessionId, title: 'Wrong identity name', lastActivityAt: row.lastActivityAt, ...mismatch } }
    f.resetStores(); await boot()
    expect(useUnifiedSessionsStore().sessions.find(value => value.id === row.id)?.title).toBe('Native history')
  })
})
