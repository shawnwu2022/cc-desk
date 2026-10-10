import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, ref, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import App from '@/App.vue'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import { createI18n } from 'vue-i18n'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import LaunchConfigurationEditor from '@/components/settings/LaunchConfigurationEditor.vue'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useSessionStore } from '@/stores/session'
import type { LaunchAction } from '@/types/cli'
import type { ProjectsState } from '@/types/app'
import { useAppStore } from '@/stores/app'
import { useProjectManagementStore } from '@/stores/projectManagement'
import { useShellStore } from '@/stores/shell'

const io = vi.hoisted(() => ({ projects: vi.fn(), sessions: vi.fn(), profiles: vi.fn(), registered: vi.fn(), register: vi.fn(), patchProfile: vi.fn(), getState: vi.fn(), upsertRecord: vi.fn(), setPreference: vi.fn(), scope: vi.fn(), read: vi.fn(), writeText: vi.fn(), archive: vi.fn(), restore: vi.fn(), open: vi.fn(), remove: vi.fn(), runChecks: vi.fn(), ptySpawn: vi.fn(), ptyInput: vi.fn(), ptyKill: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), runChecks: io.runChecks, ptySpawn: io.ptySpawn, ptyInput: io.ptyInput, ptyKill: io.ptyKill, getProjectsState: io.getState, upsertSessionUiRecord: io.upsertRecord, setProjectLaunchPreference: io.setPreference, updateAppConfig: vi.fn().mockResolvedValue(undefined), getAppConfig: vi.fn().mockResolvedValue({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' }), archiveSession: io.archive, restoreSession: io.restore, getProjects: io.projects, getSessions: io.sessions, openInFileManager: io.open, createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }), onHookEvent: async () => () => {} }))
vi.mock('@/api/cli', () => ({ cliListProfiles: io.profiles, cliPatchProfile: io.patchProfile }))
vi.mock('@/api/cliAvailability', () => ({ cliGetAvailability: async (profileId: string, profileRevision: string) => {
  const profile = useCliProfilesStore().profile(profileId)!
  const selected = profile.programPath.mode === 'set'
  return { profileId, profileRevision, cli: profile.cli, state: selected ? 'available-unverified' : 'configuration-required', hostStatus: 'available', certified: false,
    ...(selected ? {} : { issue: { code: 'PROGRAM_TRUST_REQUIRED', retryable: false } }) }
} }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: io.registered, registerProject: io.register, removeProject: io.remove }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: io.writeText }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ onResized: async () => () => {}, isMaximized: async () => false, isFocused: async () => true, onFocusChanged: async () => () => {}, requestUserAttention: async () => {} }) }))
let persisted: ProjectsState
const wrappers: VueWrapper[] = []
beforeEach(() => {
  vi.stubGlobal('crypto', { getRandomValues: window.crypto.getRandomValues, randomUUID: () => 'legacy-tab-id' })
  localStorage.clear(); setActivePinia(createPinia()); vi.clearAllMocks(); useProjectsStateStore().loaded = true
  persisted = { pinnedProjects: [], archivedSessions: {}, launchPreferences: {} }
  io.getState.mockImplementation(async () => structuredClone(persisted))
  io.upsertRecord.mockImplementation(async (key, record) => { persisted.sessionRecords ??= {}; persisted.sessionRecords[key] = structuredClone(record); return structuredClone(persisted) })
  io.setPreference.mockImplementation(async (path, preference) => { persisted.launchPreferences![path] = structuredClone(preference); return structuredClone(persisted) })
  io.projects.mockResolvedValue([{ path: '/legacy', name: 'Legacy' }]); io.sessions.mockResolvedValue([])
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cx', revision: '7', cli: 'codex', name: 'CX', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/codex' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  io.register.mockImplementation(async path => ({ revision: '2', projectId: 'registered-new', projects: [{ projectId: 'registered-new', hostId: 'host', sourcePathKey: 'source-new', selectedPath: path, canonicalPath: path, alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }))
  io.scope.mockResolvedValue({ cli: 'codex' }); io.read.mockReset().mockResolvedValue({ state: 'ready', items: [{ type: 'session', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }] })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); vi.unstubAllGlobals() })
function render() {
  const port = { startLegacy: vi.fn(), stopLegacy: vi.fn(), restartLegacy: vi.fn(), renameLegacy: vi.fn(), stopNative: vi.fn().mockResolvedValue(undefined), recoverNative: vi.fn().mockResolvedValue(undefined), focus: vi.fn() }
  let runtime!: ReturnType<typeof useUnifiedWorkspaceRuntime>
  const w = mount(defineComponent({ setup() { runtime = useUnifiedWorkspaceRuntime(ref(port)); return () => null } })); wrappers.push(w)
  return { runtime, port }
}
describe('Workspace source warning diagnostics', () => {
  // 关闭只收起提示，加载中的空错误及相同重试结果不能重置关闭状态。
  it('Warnings_DismissRetry_001', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_TOO_LARGE', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    expect(runtime.sourceNoticeDismissed?.value).toBe(false)
    runtime.dismissSourceNotice()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    expect(runtime.error.value).toBe('workspaceRuntimePartial')
    expect(runtime.sourceWarnings.value[0].code).toBe('SOURCE_TOO_LARGE')
    let finish!: (value: unknown) => void
    io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    const refresh = runtime.refresh(); await flushPromises()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    finish({ state: 'unavailable', reason: 'SOURCE_TOO_LARGE', items: [], hasMore: false }); await refresh
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
  })

  // 新代码必须重新提示，成功检查之后相同失败复发也必须重新提示。
  it('Warnings_NewAndRecur_002', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_TOO_LARGE', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_UNSUPPORTED', items: [], hasMore: false })
    await runtime.refresh(); expect(runtime.sourceNoticeDismissed.value).toBe(false)
    runtime.dismissSourceNotice()
    io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
    await runtime.refresh(); expect(runtime.sourceWarnings.value).toEqual([])
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_UNSUPPORTED', items: [], hasMore: false })
    await runtime.refresh(); expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  // canonical配置名称不参与身份；同码新增配置不能继承另一个配置的关闭记录。
  it('Warnings_ProfileIdentity_003', async () => {
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation', profileId: 'forged', name: 'raw-secret' })
    const { runtime } = render(); await flushPromises()
    expect(runtime.sourceWarningConfigurations.value.map(row => row.name)).toEqual(['CX'])
    expect(JSON.stringify(runtime.sourceWarningConfigurations.value)).not.toContain('raw-secret')
    runtime.dismissSourceNotice()
    const original = structuredClone(io.profiles.mock.results[0].value instanceof Promise ? await io.profiles.mock.results[0].value : {})
    original.profiles[0].name = 'Renamed CX'
    io.profiles.mockResolvedValue(original); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    io.profiles.mockResolvedValue({ ...original, profiles: [...original.profiles, { ...original.profiles[0], id: 'cx-second', name: 'Second CX' }] })
    await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
    expect(runtime.sourceWarningConfigurations.value.map(row => row.name)).toEqual(['Renamed CX', 'Second CX'])
    expect(runtime.sourceWarnings.value).toEqual([{ source: 'codex-history', code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation' }])
  })

  // 两个来源中一个恢复不重新提示未变化的另一个，前者再次失败则重新提示。
  it('Warnings_SubsetRecur_006', async () => {
    io.projects.mockRejectedValue({ code: 'SOURCE_BUSY' })
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation' })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    io.projects.mockResolvedValue([{ path: '/legacy', name: 'Legacy' }]); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    expect(runtime.sourceWarnings.value).toHaveLength(1)
    io.projects.mockRejectedValue({ code: 'SOURCE_BUSY' }); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  // 同一个后台catalog失败保持收起；后台出现新的固定代码则重新提示。
  it('Warnings_CatalogRepeat_007', async () => {
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation' })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    vi.spyOn(useUnifiedSessionsStore(), 'refresh').mockRejectedValue({ code: 'SOURCE_BUSY' })
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    await flushPromises(); expect(runtime.sourceNoticeDismissed.value).toBe(false)
    runtime.dismissSourceNotice()
    tab.title = 'Updated'; await flushPromises()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    expect(runtime.sourceWarnings.value).toHaveLength(2)
  })

  // 同源同码在两个配置间交换仍是新的受影响配置，不能把警告键和配置键拆成两个集合。
  it('Warnings_ProfileSwap_008', async () => {
    const original = await io.profiles()
    io.profiles.mockResolvedValue({ ...original, profiles: [...original.profiles, { ...original.profiles[0], id: 'cx-second', name: 'Second CX' }] })
    let swap = false
    io.scope.mockImplementation(async (target: { profileId: string }) => { throw { code: (target.profileId === 'cx') !== swap ? 'SCOPE_UNKNOWN' : 'SCOPE_STALE', stage: 'scope-profile-validation' } })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    swap = true; await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
  })

  // 十二条显示上限之外的新代码和新配置同样重新提示，显示集合保持不变。
  it('Warnings_OmittedIdentity_009', async () => {
    const codes = ['SCOPE_UNKNOWN', 'SCOPE_STALE', 'SCOPE_REVOKED', 'SCOPE_CAPACITY', 'SCOPE_EPOCH_EXHAUSTED', 'SCOPE_UNAVAILABLE', 'SOURCE_UNSUPPORTED', 'SOURCE_INVALID', 'SOURCE_INVALID_TEXT', 'SOURCE_PATH_REJECTED', 'SOURCE_CHANGED', 'SOURCE_NOT_REGULAR', 'SOURCE_TOO_LARGE', 'SOURCE_TOO_MANY_ENTRIES', 'SOURCE_UNSUPPORTED']
    const original = await io.profiles()
    const profiles = codes.map((_code, index) => ({ ...original.profiles[0], id: `cx-${index}`, name: `Configuration ${index}` }))
    io.profiles.mockResolvedValue({ ...original, profiles: profiles.slice(0, 13) })
    io.scope.mockImplementation(async (target: { profileId: string }) => { throw { code: codes[Number(target.profileId.slice(3))] ?? 'SOURCE_UNSUPPORTED', stage: 'scope-profile-validation' } })
    const { runtime } = render(); await flushPromises(); runtime.dismissSourceNotice()
    const visible = structuredClone(runtime.sourceWarnings.value.map(row => ({ ...row })))
    expect(visible).toHaveLength(12)
    io.profiles.mockResolvedValue({ ...original, profiles: profiles.slice(0, 14) }); await runtime.refresh()
    expect(runtime.sourceWarnings.value).toEqual(visible)
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
    runtime.dismissSourceNotice()
    io.profiles.mockResolvedValue({ ...original, profiles }); await runtime.refresh()
    expect(runtime.sourceWarnings.value).toEqual(visible)
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
    runtime.dismissSourceNotice()
    io.profiles.mockResolvedValue({ ...original, profiles: [...profiles].reverse() }); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(true)
    expect(runtime.sourceWarnings.value).toEqual(visible)
    io.profiles.mockResolvedValue({ ...original, profiles: [...profiles.slice(0, 14), { ...profiles[14], id: 'cx-16' }] }); await runtime.refresh()
    expect(runtime.sourceNoticeDismissed.value).toBe(false)
    expect(runtime.sourceWarnings.value).toEqual(visible)
  })

  // 真实App收起后保留诊断和重试；切换语言不重新弹出，配置入口绑定canonical配置。
  it('Warnings_AppCollapseLocale_004', async () => {
    io.scope.mockRejectedValue({ code: 'SCOPE_UNKNOWN', stage: 'scope-profile-validation', field: '/private/path', name: 'raw-secret' })
    const i18n = createI18n({ legacy: false, locale: 'en', messages: { en, zh } })
    const w = mount(App, { attachTo: document.body, global: { plugins: [i18n], stubs: { NativeCliTerminal: true, SettingsView: true, LaunchConfigurationEditor: true } } })
    wrappers.push(w); await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(true)
    expect(w.find('[data-workspace-source-details]').text()).toContain('CX')
    expect(w.find('[data-workspace-source-details]').text()).toContain(en.sourceWarningScopeUnknown)
    ;(w.find('[data-dismiss-source-notice]').element as HTMLElement).focus()
    await w.find('[data-dismiss-source-notice]').trigger('click'); await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(false)
    expect(w.find('[data-workspace-source-compact]').exists()).toBe(true)
    const details = w.find('[data-workspace-source-details]')
    expect(document.activeElement).toBe(details.find('summary').element)
    ;(details.find('summary').element as HTMLElement).click()
    expect((details.element as HTMLDetailsElement).open).toBe(true)
    ;(details.find('summary').element as HTMLElement).click()
    expect((details.element as HTMLDetailsElement).open).toBe(false)
    ;(details.find('summary').element as HTMLElement).click()
    expect((details.element as HTMLDetailsElement).open).toBe(true)
    expect(details.text()).toContain('SCOPE_UNKNOWN')
    expect(details.text()).not.toMatch(/raw-secret|private\/path/)
    i18n.global.locale.value = 'zh'; await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(false)
    expect(w.find('[data-workspace-source-details]').text()).toContain(zh.sourceWarningScopeUnknown)
    await w.find('[data-source-warning-retry]').trigger('click'); await flushPromises()
    expect(w.find('[data-workspace-source-notice]').exists()).toBe(false)
    await w.find('[data-source-warning-configuration]').trigger('click'); await flushPromises()
    expect(w.findComponent(LaunchConfigurationEditor).props('request')).toEqual({ kind: 'edit', profileId: 'cx' })
  })

  it('keeps incomplete empty metadata visible without turning positive history into a failed source', async () => {
    io.read.mockResolvedValue({ state: 'ready', reason: null, items: [], hasMore: false, historyMetadataIncomplete: true })
    const { runtime } = render(); await flushPromises()
    expect(runtime.error.value).toBeNull()
    expect(runtime.historyMetadataPartial.value).toBe(true)
    io.read.mockResolvedValue({ state: 'ready', reason: null, items: [], hasMore: false })
    await runtime.refresh()
    expect(runtime.historyMetadataPartial.value).toBe(false)
  })
  it('retains authenticated safe error codes while hiding private exception fields and preserving open sessions', async () => {
    io.projects.mockRejectedValue(new Error('private C:\\users\\secret token=hidden'))
    io.scope.mockRejectedValue({ code: 'FORBIDDEN', field: 'private-path', message: 'token=hidden', retryable: false })
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' }, title: 'Private session title' })
    const { runtime } = render(); await flushPromises()
    expect(runtime.error.value).toBe('workspaceRuntimePartial')
    expect(runtime.sourceWarnings?.value).toEqual([
      { source: 'project-discovery', code: 'SOURCE_UNAVAILABLE' },
      { source: 'codex-history', code: 'FORBIDDEN', stage: 'scope-invoke' },
    ])
    expect(runtime.openSessions.value.some(row => row.adapterSessionId === tab.tabId)).toBe(true)
    expect(JSON.stringify(runtime.sourceWarnings?.value)).not.toMatch(/private|token|secret|title/i)
  })

  it('retains an unavailable history reason and clears its warning only after a successful refresh', async () => {
    io.read.mockResolvedValue({ state: 'unavailable', reason: 'SOURCE_TOO_LARGE', items: [], hasMore: false })
    const { runtime } = render(); await flushPromises()
    expect(runtime.sourceWarnings?.value).toEqual([{ source: 'codex-history', code: 'SOURCE_TOO_LARGE', stage: 'read-source-enumeration' }])
    expect(runtime.error.value).toBe('workspaceRuntimePartial')
    io.read.mockResolvedValue({ state: 'ready', reason: null, items: [], hasMore: false })
    await runtime.refresh()
    expect(runtime.sourceWarnings?.value).toEqual([])
    expect(runtime.error.value).toBeNull()
  })

  it('does not let an old refresh publish source failures after a newer successful refresh', async () => {
    const { runtime } = render(); await flushPromises()
    let rejectOld!: (reason: unknown) => void
    io.scope.mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectOld = reject }))
    const old = runtime.refresh(); await flushPromises()
    await runtime.refresh()
    rejectOld({ code: 'FORBIDDEN', retryable: false }); await old
    expect(runtime.sourceWarnings?.value).toEqual([])
    expect(runtime.error.value).toBeNull()
  })

  it('shows the failed source and safe code in collapsed details in the actual App warning', async () => {
    io.scope.mockRejectedValue({ code: 'SOURCE_READ_FORBIDDEN', stage: 'scope-source-root', field: '/private/path', message: 'raw-secret' })
    const w = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } })
    wrappers.push(w); await flushPromises()
    const details = w.find('[data-workspace-source-details]')
    expect(details.exists()).toBe(true)
    expect(details.attributes('open')).toBeUndefined()
    expect(details.text()).toContain('Codex CLI history')
    expect(details.text()).toContain('SOURCE_READ_FORBIDDEN')
    expect(details.text()).toContain('scope-source-root')
    expect(details.text()).not.toMatch(/private|raw-secret/)
    expect(w.text()).toContain(en.workspaceRuntimePartial)
  })

  it('attributes later catalog failures without allowing an older catalog read to overwrite a refreshed result', async () => {
    const { runtime } = render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const original = catalog.refresh.bind(catalog)
    const refresh = vi.spyOn(catalog, 'refresh').mockRejectedValueOnce({ code: 'SOURCE_BUSY' })
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    await flushPromises()
    expect(runtime.sourceWarnings?.value).toContainEqual({ source: 'catalog', code: 'SOURCE_BUSY' })
    let rejectOld!: (reason: unknown) => void
    refresh.mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectOld = reject })).mockImplementation(original)
    useNativeTabsStore().tab(tab.tabId)!.title = 'Changed'
    await flushPromises()
    await runtime.refresh()
    rejectOld({ code: 'FORBIDDEN' }); await flushPromises()
    expect(runtime.sourceWarnings?.value).toEqual([])
    expect(runtime.error.value).toBeNull()
  })
})

describe('Unified production runtime', () => {
  // 配置与项目共用后端 workspace revision；注册项目后创建另一CLI配置须先读取当前CAS。
  it.each([['claude', 'codex'], ['codex', 'claude']] as const)('Runtime_SequentialCliDefaults_045: %s then %s', async (firstCli, secondCli) => {
    let revision = 0
    const profiles: any[] = [], projects: any[] = []
    io.profiles.mockImplementation(async () => ({ revision: String(revision), profiles: structuredClone(profiles) }))
    io.registered.mockImplementation(async () => ({ revision: String(revision), projects: structuredClone(projects) }))
    io.patchProfile.mockImplementation(async (expectedRevision, patch) => {
      if (expectedRevision !== String(revision)) throw { code: 'REVISION_CONFLICT', retryable: false }
      ++revision
      if (patch.op === 'create') profiles.push({ ...structuredClone(patch.profile), revision: String(revision) })
      else Object.assign(profiles.find(profile => profile.id === patch.id), structuredClone(patch.changes), { revision: String(revision) })
      return { revision: String(revision), profiles: structuredClone(profiles) }
    })
    io.register.mockImplementation(async selectedPath => {
      ++revision
      projects.push({ projectId: 'registered', hostId: 'host', sourcePathKey: 'source', selectedPath, canonicalPath: selectedPath, alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } })
      return { revision: String(revision), projectId: 'registered', projects: structuredClone(projects) }
    })
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    await expect(catalog.createSession({ cli: firstCli, projectKey: '/new', projectPath: '/new' })).rejects.toThrow('LAUNCH_CONFIGURATION_REQUIRED')
    const firstId = catalog.activeSessionId!
    await useCliProfilesStore().patch('1', { op: 'update', id: `desk-safe-${firstCli}`, changes: { programPath: { mode: 'set', value: `/tools/${firstCli}` } } })
    const first = await catalog.restartSession(firstId)
    expect(useNativeTabsStore().tab(first.adapterSessionId)?.cli).toBe(firstCli)
    expect(revision).toBe(3)
    await expect(catalog.createSession({ cli: secondCli, projectKey: '/new', projectPath: '/new' })).rejects.toThrow('LAUNCH_CONFIGURATION_REQUIRED')
    const secondId = catalog.activeSessionId!
    await useCliProfilesStore().patch('4', { op: 'update', id: `desk-safe-${secondCli}`, changes: { programPath: { mode: 'set', value: `/tools/${secondCli}` } } })
    const second = await catalog.restartSession(secondId)
    expect(useNativeTabsStore().tab(second.adapterSessionId)).toMatchObject({ cli: secondCli, profileId: `desk-safe-${secondCli}`, profileRevision: '5', projectId: 'registered' })
    expect(io.patchProfile.mock.calls.filter(([, patch]) => patch.op === 'create').map(([expectedRevision]) => expectedRevision)).toEqual(['0', '3'])
    expect(io.register).toHaveBeenCalledOnce()
    expect(useNativeTabsStore().tabs.size).toBe(2)
  })
  // 一个来源失败不能隐藏另一个来源的已注册项目和历史；初始化不启动CLI。
  it('Runtime_BootsPartialSources_001', async () => {
    io.projects.mockRejectedValue(new Error('private /path secret'))
    const { runtime, port } = render(); await flushPromises()
    const unified = useUnifiedSessionsStore()
    expect(unified.initialized).toBe(true)
    expect(unified.sessions.map(s => s.title)).toContain('History')
    expect(port.startLegacy).not.toHaveBeenCalled(); expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(runtime.error.value).not.toContain('/path')
    expect(runtime.cliAvailability.value.codex).toBe('unknown')
    expect(io.runChecks).not.toHaveBeenCalled(); expect(io.ptySpawn).not.toHaveBeenCalled()
    expect(io.ptyInput).not.toHaveBeenCalled(); expect(io.ptyKill).not.toHaveBeenCalled()
  })
  // 恢复使用历史来源的完整身份，当前选项变化不能覆盖其修订号和项目。
  it('Runtime_PreservesResumeOrigin_002', async () => {
    render(); await flushPromises()
    const unified = useUnifiedSessionsStore(); const history = unified.sessions.find(s => s.title === 'History')!
    const opened = await unified.resumeSession({ runtime: 'native-cli', cli: history.cli, projectKey: history.projectKey, projectPath: history.projectPath, adapterSessionId: history.adapterSessionId, nativeSessionId: history.nativeSessionId })
    expect(useNativeTabsStore().tab(opened.adapterSessionId)).toMatchObject({ projectId: 'project', profileId: 'cx', profileRevision: '7', sourceSessionKey: 'root-key', action: { kind: 'resume-id', nativeSessionId: 'history-id' } })
    useNativeTabsStore().close(opened.adapterSessionId)
    useCliProfilesStore().profiles[0].revision = '8'
    await expect(unified.resumeSession({ runtime: 'native-cli', cli: history.cli, projectKey: history.projectKey, projectPath: history.projectPath, adapterSessionId: history.adapterSessionId, nativeSessionId: history.nativeSessionId })).rejects.toThrow('PROFILE_SELECTION_CHANGED')
    expect(useNativeTabsStore().tabs.size).toBe(0)
  })
  // 显式创建通过鉴权项目注册，原始argv保留字符串边界；初始化仍然只读。
  it('Runtime_RequiresRegisteredProject_003', async () => {
    render(); await flushPromises(); const unified = useUnifiedSessionsStore()
    expect(io.register).not.toHaveBeenCalled()
    const registered = await unified.createSession({ projectKey: '/unknown', projectPath: '/unknown', cli: 'codex', action: { kind: 'new' } })
    expect(useNativeTabsStore().tab(registered.adapterSessionId)?.projectId).toBe('registered-new')
    expect(io.register).toHaveBeenCalledWith('/unknown')
    const opened = await unified.createSession({ projectKey: '/repo', projectPath: '/repo', cli: 'codex', action: { kind: 'raw', argv: ['two words', '', '--literal= x'] } })
    expect(useNativeTabsStore().tab(opened.adapterSessionId)!.action).toEqual({ kind: 'raw', argv: ['two words', '', '--literal= x'] })
  })
  // 非 UI 来源的激活请求也不能把历史行恢复，防止陈旧请求越过按钮边界。
  it('Runtime_HistoryActivateDoesNotLaunch_057', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(), shell = useShellStore()
    const history = catalog.sessions.find(session => !session.opened)!
    shell.requestWorkspaceAction({ kind: 'activate', sessionId: history.id }); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(useSessionStore().tabs.size).toBe(0)
    expect(catalog.activeSessionId).toBeNull()
  })
  // 每个请求序号仅执行一次，较早完成不能清掉较新待确认请求。
  it('Runtime_OwnsRequestSequence_004', async () => {
    render(); await flushPromises(); const shell = useShellStore(); const unified = useUnifiedSessionsStore()
    const tab = useNativeTabsStore().create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); await unified.refresh()
    let finish!: () => void; const activate = vi.spyOn(unified, 'activateSession').mockImplementation(() => new Promise<void>(r => { finish = r }))
    shell.requestWorkspaceAction({ kind: 'activate', sessionId: `native-tab:${tab.tabId}` }); await flushPromises()
    shell.requestWorkspaceAction({ kind: 'confirmation', request: { kind: 'stop-and-archive', sessionId: `native-tab:${tab.tabId}`, projectKey: '/repo', projectPath: '/repo' } }); await flushPromises()
    finish(); await flushPromises()
    expect(activate).toHaveBeenCalledTimes(1); expect(shell.pendingRequest).toBeNull(); expect(unified.sessionConfirmation?.kind).toBe('stop-and-archive')
    shell.section = 'settings'; await flushPromises(); expect(activate).toHaveBeenCalledTimes(1)
  })
  // 运行中归档仍须确认；关闭是用户直接操作，不再弹第二层确认。
  it('Runtime_OpensOwnedConfirmations_005', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    const unified = useUnifiedSessionsStore(); await unified.refresh(); const shell = useShellStore()
    for (const action of ['archive'] as const) {
      shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action }); await flushPromises()
      expect(shell.pendingRequest).toBeNull(); expect(unified.sessionConfirmation?.kind).toBe('stop-and-archive'); expect(port.stopNative).not.toHaveBeenCalled(); expect(tabs.tab(tab.tabId)).toBeDefined()
    }
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/repo', projectPath: '/repo' } }); await flushPromises()
    expect(shell.pendingRequest).toBeNull(); expect(useNewSessionDraftStore().chooserVisible).toBe(true); expect(useNewSessionDraftStore().visible).toBe(false); expect(tabs.tabs.size).toBe(1)
  })
  // 后台store变化发布统一目录，不要求用户刷新；历史记录不创建终端。
  it('Runtime_ProjectsLiveStores_006', async () => {
    const { runtime } = render(); await flushPromises()
    const id = useSessionStore().createTab('/legacy', { name: 'Legacy open' }); await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(s => s.adapterSessionId === id)?.title).toBe('Legacy open')
    expect(runtime.openSessions.value.map(s => s.id)).toEqual([`legacy-tab:${id}`])
    useSessionStore().tabs.get(id)!.status = 'running'; useSessionStore().tabs.get(id)!.pending = true; await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(s => s.adapterSessionId === id)?.attentionState).toBe('needs-user')
    expect(useNativeHistoryStore().all()).toHaveLength(1); expect(useWorkspaceStore().projects).toHaveLength(1)
  })
  // 即使目录仍显示停止，直接关闭也必须停止真实store中的当前尝试。
  it('Runtime_CloseChecksLiveState_007', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
    const unified = useUnifiedSessionsStore(); await unified.refresh(); await flushPromises()
    tabs.tab(tab.tabId)!.status = 'running'
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action: 'close' })
    await flushPromises()
    expect(port.stopNative).toHaveBeenCalledOnce()
    expect(port.stopNative.mock.calls[0][0]).toBe(tab.tabId)
    expect(useShellStore().pendingRequest).toBeNull(); expect(unified.sessionConfirmation).toBeNull()
    expect(tabs.tab(tab.tabId)).toBeUndefined()
  })
  // 重启等待停止时出现新代次，旧完成不能覆盖新的代次或启动配置。
  it('Runtime_StaleRestartIsRejected_008', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    const unified = useUnifiedSessionsStore(); await unified.refresh()
    let finish!: () => void; port.stopNative.mockReturnValue(new Promise<void>(r => { finish = r }))
    const restarting = unified.restartSession(`native-tab:${tab.tabId}`); const rejected = expect(restarting).rejects.toThrow('STALE_NATIVE_ATTEMPT'); await flushPromises()
    tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' }); finish(); await rejected
    expect(tabs.tab(tab.tabId)?.generation).toBe(2)
  })
  // 完整App使用真实初始化/适配器/宿主，历史不自动启动，创建后导航不重建。
  it('Runtime_NormalAppWiresRealHost_009', async () => {
    const mounted: string[] = []; const unmounted: string[] = []
    const child = defineComponent({ props: ['tabId', 'active'], setup(props, { expose }) {
      mounted.push(props.tabId)
      expose({ focus() {}, fitVisible() {}, async recover() {}, async stop() {} })
      return () => h('div', { 'data-runtime-tab': props.tabId, 'data-visible': String(props.active) })
    }, unmounted() { unmounted.push(String(this.tabId)) } })
    const w = mount(App, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: child, SettingsView: true } } }); wrappers.push(w); await flushPromises()
    const unified = useUnifiedSessionsStore(); expect(unified.sessions.map(s => s.title)).toEqual(['History']); expect(mounted).toEqual([])
    const opened = await unified.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', action: { kind: 'new' } }); await flushPromises()
    expect(mounted).toEqual([opened.adapterSessionId]); const element = w.get('[data-runtime-tab]').element
    useShellStore().navigate('projects'); await flushPromises(); expect(w.get('[data-runtime-tab]').attributes('data-visible')).toBe('false')
    useShellStore().navigate('workspace'); await flushPromises(); expect(w.get('[data-runtime-tab]').element).toBe(element); expect(unmounted).toEqual([])
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: opened.id, action: 'copy-session-id' }); await flushPromises()
    expect(io.writeText).not.toHaveBeenCalled()
    useUnifiedSessionsStore().beginRename(opened.id)
    useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: opened.id, title: 'Unified name' }); await flushPromises()
    expect(tabsTitle()).toBe('Unified name'); expect(useShellStore().pendingRequest).toBeNull()
    function tabsTitle() { return useNativeTabsStore().tab(opened.adapterSessionId)?.title }
  })

  // 恢复条目的启动配置被替换时，不能丢失其原始来源并启动另一配置。
  it('Runtime_ResumeRejectsWrongOrigin_010', async () => {
    render(); await flushPromises()
    const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'other' })
    const unified = useUnifiedSessionsStore(); const history = unified.sessions.find(s => s.title === 'History')!
    await expect(unified.resumeSession({ runtime: 'native-cli', cli: 'codex', projectKey: '/repo', projectPath: '/repo', adapterSessionId: history.adapterSessionId, nativeSessionId: history.nativeSessionId, launchConfigId: 'other' })).rejects.toThrow('PROFILE_SELECTION_CHANGED')
    expect(useNativeTabsStore().tabs.size).toBe(0)
  })
  // 重启等待停止期间项目被移除，完成后不能使用前端缓存路径启动新代次。
  it('Runtime_RestartRechecksProject_011', async () => {
    const { port } = render(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } }); tabs.tab(tab.tabId)!.status = 'running'
    const unified = useUnifiedSessionsStore(); await unified.refresh()
    let finish!: () => void; port.stopNative.mockReturnValue(new Promise<void>(r => { finish = r }))
    const restarting = unified.restartSession(`native-tab:${tab.tabId}`); const rejected = expect(restarting).rejects.toThrow('PROJECT_NOT_FOUND'); await flushPromises()
    useWorkspaceStore().projects = []; tabs.tab(tab.tabId)!.status = 'exited'; finish(); await rejected
    expect(tabs.tab(tab.tabId)?.generation).toBe(1)
  })

  // 真实Legacy store归档后仍保留统一目录记录，普通列表隐藏而抽屉可恢复。
  it('Runtime_LegacyArchiveRemainsVisible_012', async () => {
    io.sessions.mockResolvedValue([{ sessionId: 'old-session', name: 'Old Legacy', projectPath: '/legacy', lastActiveAt: 10 }])
    io.archive.mockResolvedValue({ pinnedProjects: [], archivedSessions: { '/legacy': ['old-session'] } })
    io.restore.mockResolvedValue({ pinnedProjects: [], archivedSessions: {} })
    const { runtime, port } = render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(); const original = catalog.sessions.find(session => session.nativeSessionId === 'old-session')!
    await catalog.archiveSession(original.id); await runtime.refresh(); await flushPromises()
    expect(useSessionStore().getHistoryFor('/legacy')).toEqual([])
    expect(catalog.projectGroups.flatMap(group => group.sessions).some(session => session.id === original.id)).toBe(false)
    expect(catalog.sessions.find(session => session.id === original.id)).toMatchObject({ archived: true, title: 'Old Legacy' })
    const drawer = mount(ArchivedSessionsDrawer, { props: { open: true, sessions: catalog.sessions }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { teleport: true } } }); wrappers.push(drawer)
    expect(drawer.text()).toContain('Old Legacy')
    await catalog.restoreArchivedSession(original.id); await flushPromises()
    expect(catalog.projectGroups.flatMap(group => group.sessions).find(session => session.id === original.id)).toMatchObject({ archived: false })
    expect(useSessionStore().getHistoryFor('/legacy').map(session => session.sessionId)).toEqual(['old-session'])
    expect(port.startLegacy).not.toHaveBeenCalled()
  })

  it('Runtime_RealAppTwoClickAndFrozenSuccess_013', async () => {
    const child = defineComponent({ props: ['tabId', 'active'], setup(props, { expose }) {
      expose({ focus() {}, fitVisible() {}, async recover() {}, async stop() {} })
      return () => h('div', { 'data-runtime-tab': props.tabId })
    } })
    const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: child, SettingsView: true } } }); wrappers.push(w); await flushPromises()
    const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'custom', name: 'Custom' })
    const draft = useNewSessionDraftStore(); draft.setDefault('codex', 'cx'); profiles.select('codex', 'custom')
    await w.get('.project-node[aria-label="repo"] [data-project-quick-action]').trigger('click'); await flushPromises()
    ;(document.querySelector('[data-item-id="codex"]') as HTMLButtonElement).click(); await flushPromises()
    const tabs = useNativeTabsStore(); const tab = [...tabs.tabs.values()][0]
    expect(tab).toMatchObject({ cli: 'codex', profileId: 'cx', profileRevision: '7', projectId: 'project', action: { kind: 'new' } })
    expect(w.find('[data-runtime-tab]').exists()).toBe(true)
    expect(useUnifiedSessionsStore().sessions.find(row => row.id === `native-tab:${tab.tabId}`)?.processState).toBe('starting')
    expect(useUnifiedSessionsStore().activeSessionId).toBe(`native-tab:${tab.tabId}`)
    draft.setDefault('codex', 'custom')
    expect(draft.preferred({ projectPath: '/repo' }, 'codex')?.id).toBe('custom')
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    await flushPromises()
    expect(draft.preferred({ projectPath: '/repo' }, 'codex')?.id).toBe('cx')
    expect(persisted.launchPreferences?.['/repo']?.codexLaunchConfigId).toBe('cx')
    expect(io.setPreference).toHaveBeenCalledOnce()
    expect(useShellStore().pendingRequest).toBeNull()
  })
  it('Runtime_PreReadyCreateHasImmediatePlaceholder_014', async () => {
    let release!: (value: unknown[]) => void
    io.projects.mockReturnValue(new Promise(resolve => { release = resolve }))
    render(); await flushPromises()
    useShellStore().requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/repo', projectPath: '/repo', intent: 'codex' } })
    await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(1)
    release([]); await flushPromises()
  })
  it('Runtime_RetryPreparationDoesNotReplayUnknownLaunch_015', async () => {
    render(); await flushPromises()
    io.register.mockRejectedValueOnce(new Error('secret registration failure'))
    const shell = useShellStore(); const catalog = useUnifiedSessionsStore()
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/new', projectPath: '/new', intent: 'codex' } }); await flushPromises()
    const failed = catalog.sessions.find(row => row.projectPath === '/new')!
    expect(failed).toMatchObject({ processState: 'failed', safeErrorCode: 'NEW_SESSION_PREPARATION_FAILED' }); expect(useNativeTabsStore().tabs.size).toBe(0)
    shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: failed.id, action: 'retry' }); await flushPromises()
    expect(io.register).toHaveBeenCalledTimes(2); const tab = [...useNativeTabsStore().tabs.values()][0]
    expect(catalog.sessions.some(row => row.id === failed.id)).toBe(false)
    useNativeTabsStore().markUnknown(tab.tabId); await flushPromises()
    shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: `native-tab:${tab.tabId}`, action: 'retry' }); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(1); expect(useNativeTabsStore().tab(tab.tabId)?.generation).toBe(1)
    expect(io.register).toHaveBeenCalledTimes(2)
  })
  it('Runtime_ReplacedAttemptCannotRecordSuccess_016', async () => {
    render(); await flushPromises()
    const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'other' })
    const catalog = useUnifiedSessionsStore(); const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'other' })
    const tabs = useNativeTabsStore(); const old = { ...tabs.tab(opened.adapterSessionId)! }
    tabs.markError(old.tabId, 'LAUNCH_FAILED'); tabs.restart(old.tabId, { profileId: 'other', profileRevision: '7' })
    expect(tabs.applyLaunchStatus(old.tabId, { instanceId: 'i', requestId: old.requestId, run: { runId: old.runId, generation: old.generation }, revision: '1', phase: 'running', failure: null })).toBe(false)
    expect(useNewSessionDraftStore().preferred({ projectPath: '/repo' }, 'codex')?.id).toBe('cx')
  })

  it('Runtime_RetriedLaunchRecordsOnlyNewSuccess_017', async () => {
    render(); await flushPromises()
    const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'other' })
    const catalog = useUnifiedSessionsStore(); const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', launchConfigId: 'other' })
    const tabs = useNativeTabsStore(); tabs.markError(opened.adapterSessionId, 'LAUNCH_FAILED'); await catalog.restartSession(opened.id)
    const tab = tabs.tab(opened.adapterSessionId)!
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    await flushPromises()
    expect(useNewSessionDraftStore().preferred({ projectPath: '/repo' }, 'codex')?.id).toBe('other')
  })
  it('Runtime_ProfileChangeDuringRegistrationFailsSafely_018', async () => {
    render(); await flushPromises(); let finish!: (value: unknown) => void
    io.register.mockReturnValue(new Promise(resolve => { finish = resolve }))
    const creating = useUnifiedSessionsStore().createSession({ cli: 'codex', projectKey: '/new', projectPath: '/new' })
    const failed = expect(creating).rejects.toThrow('NEW_SESSION_PREPARATION_FAILED'); await flushPromises()
    useCliProfilesStore().profiles[0].revision = '8'
    finish({ revision: '2', projectId: 'new', projects: [{ projectId: 'new', hostId: 'h', sourcePathKey: 's', selectedPath: '/new', canonicalPath: '/new', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }); await failed
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(useUnifiedSessionsStore().sessions.find(s => s.projectPath === '/new')?.processState).toBe('failed')
  })

  it('Runtime_RestoreRequestOpensDialog_019', async () => {
    render(); await flushPromises(); const shell = useShellStore()
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectPath: '/repo', projectKey: '/repo', intent: 'restore' } }); await flushPromises()
    expect(shell.pendingRequest).toBeNull()
    expect(useUnifiedSessionsStore().resumeDialog).toMatchObject({ project: { projectPath: '/repo', projectKey: '/repo' }, mode: 'history' })
    expect(useNativeTabsStore().tabs.size).toBe(0)
  })

  it('Runtime_UnstartedAdmissionCanBeCancelled_020', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(); const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    expect(useNativeTabsStore().tab(opened.adapterSessionId)?.status).toBe('stopped')
    useShellStore().requestWorkspaceAction({ kind: 'primary-action', sessionId: opened.id, action: 'cancel-start' }); await flushPromises()
    expect(useNativeTabsStore().tab(opened.adapterSessionId)).toBeUndefined()
    expect(catalog.sessions.some(s => s.id === opened.id)).toBe(false)
  })

  it('Runtime_HeaderUsesQuickChooserBeforeAdvanced_021', async () => {
    const terminal = defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-runtime-tab': props.tabId }) } })
    const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
    await w.get('button[data-new-session]').trigger('click'); await flushPromises()
    expect(document.querySelector('[role=dialog]')).toBeNull()
    const codex = document.querySelector('[data-item-id=codex]') as HTMLButtonElement
    expect(codex).not.toBeNull(); codex.click(); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(1); expect(w.find('[data-runtime-tab]').exists()).toBe(true)
    await w.get('button[data-new-session]').trigger('click'); await flushPromises()
    ;(document.querySelector('[data-item-id=options]') as HTMLButtonElement).click(); await flushPromises()
    expect(document.querySelector('[role=dialog]')).not.toBeNull(); expect(useNativeTabsStore().tabs.size).toBe(1)
  })
  it('Runtime_PreferenceSaveFailureNeverRetriesLaunch_022', async () => {
    const { runtime } = render(); await flushPromises()
    io.setPreference.mockRejectedValue(new Error('private storage path'))
    const catalog = useUnifiedSessionsStore(); const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    const tabs = useNativeTabsStore(); const tab = { ...tabs.tab(opened.adapterSessionId)! }
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    await flushPromises()
    expect(tabs.tab(tab.tabId)).toMatchObject({ status: 'running', generation: 1, requestId: tab.requestId, runId: tab.runId })
    expect(runtime.error.value).toBe('newSessionPreferenceSaveFailed')
    expect(io.setPreference).toHaveBeenCalledOnce(); expect(io.getState).toHaveBeenCalledOnce()
    await runtime.refresh(); await flushPromises()
    expect(io.setPreference).toHaveBeenCalledOnce(); expect(tabs.tabs.size).toBe(1)
    expect(catalog.sessions.find(row => row.id === opened.id)?.processState).toBe('running')
  })

  it('Runtime_PreReadyCancelPreventsLateAdmission_023', async () => {
    let releaseBootstrap!: (value: unknown[]) => void
    io.projects.mockReturnValue(new Promise(resolve => { releaseBootstrap = resolve }))
    const { runtime } = render(); await flushPromises()
    let releaseRegistration!: (value: unknown) => void
    io.register.mockReturnValue(new Promise(resolve => { releaseRegistration = resolve }))
    const shell = useShellStore(); const catalog = useUnifiedSessionsStore()
    shell.requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/new', projectPath: '/new', intent: 'codex' } }); await flushPromises()
    expect(runtime.ready.value).toBe(false)
    const placeholder = catalog.sessions.find(row => row.projectPath === '/new')!
    shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: placeholder.id, action: 'cancel-start' }); await flushPromises()
    const afterCancel = catalog.sessions.find(row => row.id === placeholder.id)?.processState
    releaseRegistration({ revision: '2', projectId: 'new', projects: [{ projectId: 'new', hostId: 'h', sourcePathKey: 's', selectedPath: '/new', canonicalPath: '/new', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(afterCancel).toBe('failed'); expect(shell.pendingRequest).toBeNull()
    releaseBootstrap([]); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(0)
    expect(catalog.sessions.find(row => row.id === placeholder.id)?.safeErrorCode).toBe('NEW_SESSION_CANCELLED')
  })
  it('Runtime_ReselectedPlaceholderTransfersSelection_024', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore(); const draft = useNewSessionDraftStore()
    let release!: () => void; const prepare = draft.prepareInput
    vi.spyOn(draft, 'prepareInput').mockImplementation(async input => { await new Promise<void>(resolve => { release = resolve }); return prepare(input) })
    const creating = catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
    const placeholder = catalog.activeSessionId!
    useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: placeholder }); await flushPromises()
    release(); const created = await creating; await flushPromises()
    expect(catalog.activeSessionId).toBe(created.id)
    expect(catalog.sessions.some(row => row.id === placeholder)).toBe(false)
  })
  it('Runtime_PlaceholderCannotStealNewerSessionSelection_025', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const existing = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    const draft = useNewSessionDraftStore(); let release!: () => void; const prepare = draft.prepareInput
    vi.spyOn(draft, 'prepareInput').mockImplementation(async input => { await new Promise<void>(resolve => { release = resolve }); return prepare(input) })
    const creating = catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
    await catalog.activateSession(catalog.activeSessionId!)
    await catalog.activateSession(existing.id)
    release(); await creating; await flushPromises()
    expect(catalog.activeSessionId).toBe(existing.id)
  })

  it('Runtime_UnrelatedClosePreservesPlaceholderSelection_026', async () => {
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const existing = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' })
    useNativeTabsStore().markError(existing.adapterSessionId, 'LAUNCH_FAILED'); await flushPromises()
    const draft = useNewSessionDraftStore(); const prepare = draft.prepareInput; let release!: () => void
    vi.spyOn(draft, 'prepareInput').mockImplementation(async input => { await new Promise<void>(resolve => { release = resolve }); return prepare(input) })
    const creating = catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
    const placeholder = catalog.activeSessionId!
    useShellStore().requestWorkspaceAction({ kind: 'menu-action', sessionId: existing.id, action: 'close' }); await flushPromises()
    expect(catalog.activeSessionId).toBe(placeholder)
    expect(catalog.sessions.some(row => row.id === existing.id)).toBe(false)
    release(); const created = await creating; await flushPromises()
    expect(catalog.activeSessionId).toBe(created.id)
  })

})

// 旧归档键映射多个来源时显示明确、安全的歧义提示，保留所有记录。
it('Runtime_ReportsArchiveAmbiguity_027', async () => {
  const { runtime } = render(); await flushPromises()
  const profiles = useCliProfilesStore(); profiles.profiles.push({ ...profiles.profiles[0], id: 'other' })
  const history = useNativeHistoryStore(); await history.load({ cli: 'codex', profileId: 'other', profileRevision: '7', projectId: 'project', projectPath: '/repo' })
  const { makeSessionCatalogKey } = await import('@/utils/sessionPresentation')
  const key = 'native-history:' + makeSessionCatalogKey({ runtime: 'native-cli', cli: 'codex', projectPath: '/repo', adapterSessionId: 'root-key', nativeSessionId: 'history-id' })
  useProjectsStateStore().archivedSessions.set('/repo', [key]); await useUnifiedSessionsStore().refresh(); await flushPromises()
  const row = useUnifiedSessionsStore().sessions.find(row => row.archived)!
  useShellStore().requestWorkspaceAction({ kind: 'restore-archive', sessionId: row.id }); await flushPromises()
  expect(runtime.error.value).toBe('resumeAmbiguous'); expect(io.restore).not.toHaveBeenCalled()
})

// 移除等待原生取消注册时，Legacy恢复不能创建或启动新的终端所有者。
it('Runtime_RemoveBlocksLegacyRestore_028', async () => {
  io.sessions.mockResolvedValue([{ sessionId: 'legacy-history', name: 'Legacy history', projectPath: '/legacy', lastActiveAt: 10 }])
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'p', hostId: 'h', sourcePathKey: 's', selectedPath: '/legacy', canonicalPath: '/legacy', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  const { port } = render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(); const row = catalog.sessions.find(session => session.nativeSessionId === 'legacy-history')!
  const management = useProjectManagementStore(); management.beginRemove({ projectKey: '/legacy', projectPath: '/legacy' })
  let release!: (value: unknown) => void
  io.remove.mockImplementation(() => new Promise(resolve => { release = resolve }))
  const removing = management.remove(); await flushPromises()
  expect(useAppStore().isProjectRemoving('/legacy')).toBe(true); expect(io.remove).toHaveBeenCalledOnce()
  const restoring = catalog.resumeSession({ runtime: 'legacy-claude', cli: 'claude', projectKey: '/legacy', projectPath: '/legacy', adapterSessionId: row.adapterSessionId, nativeSessionId: row.nativeSessionId }).catch(() => undefined)
  await flushPromises(); release({ revision: '2', projects: [] }); await removing; await restoring
  expect(port.startLegacy).not.toHaveBeenCalled(); expect(useSessionStore().tabs.size).toBe(0)
})
// 较早恢复的历史读取在移除期间或完成后返回，也不能取得新的终端所有权。
it.each(['during', 'after'])('Runtime_RemoveRevokesDeferredLegacy_%s', async when => {
  io.sessions.mockResolvedValue([{ sessionId: 'legacy-history', name: 'Legacy history', projectPath: '/legacy', lastActiveAt: 10 }])
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'p', hostId: 'h', sourcePathKey: 's', selectedPath: '/legacy', canonicalPath: '/legacy', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  const { port } = render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(); const row = catalog.sessions.find(session => session.nativeSessionId === 'legacy-history')!
  let finishHistory!: (value: unknown) => void
  io.sessions.mockImplementation(() => new Promise(resolve => { finishHistory = resolve }))
  const restoring = catalog.resumeSession({ runtime: 'legacy-claude', cli: 'claude', projectKey: '/legacy', projectPath: '/legacy', adapterSessionId: row.adapterSessionId, nativeSessionId: row.nativeSessionId }).catch(() => undefined)
  await flushPromises()
  const management = useProjectManagementStore(); management.beginRemove({ projectKey: '/legacy', projectPath: '/legacy' })
  let release!: (value: unknown) => void
  io.remove.mockImplementation(() => new Promise(resolve => { release = resolve }))
  const removing = management.remove(); await flushPromises()
  if (when === 'after') { release({ revision: '2', projects: [] }); await removing }
  finishHistory([{ sessionId: 'legacy-history', name: 'Legacy history', projectPath: '/legacy', lastActiveAt: 10 }]); await restoring
  if (when === 'during') { release({ revision: '2', projects: [] }); await removing }
  expect(port.startLegacy).not.toHaveBeenCalled(); expect(useSessionStore().tabs.size).toBe(0)
})

// 旧工作台身份测试迁移到真实统一runtime；新建/恢复/原始参数均固定所选CLI、配置修订和注册项目。
describe.each(['claude', 'codex'] as const)('Unified action identity: %s', cli => {
  it.each<LaunchAction>([
    { kind: 'new' }, { kind: 'raw', argv: ['', '中文', 'two words', '--future'] },
    { kind: 'resume-picker', scope: 'current-project' }, { kind: 'resume-id', nativeSessionId: 'session-123' },
  ])('Runtime_FreezesActionIdentity_029: $kind', async action => {
    io.profiles.mockResolvedValue({ revision: '9', profiles: ['claude', 'codex'].map(kind => ({
      id: kind, revision: kind === 'claude' ? '3' : '8', cli: kind, name: kind,
      launcher: { kind: 'native' }, programPath: { mode: 'set', value: `/tools/${kind}` }, defaultArgs: { mode: 'inherit' },
      skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {},
    })) })
    render(); await flushPromises()
    const unified = useUnifiedSessionsStore()
    const input = { cli, projectKey: '/repo', projectPath: '/repo', registeredProjectId: 'project',
      launchConfigId: cli, launchConfigRevision: cli === 'claude' ? '3' : '8', action }
    const opened = action.kind === 'resume-id' || action.kind === 'resume-picker'
      ? await unified.launchResume(input) : await unified.createSession(input)
    expect(useNativeTabsStore().tab(opened.adapterSessionId)).toMatchObject({
      cli, projectId: 'project', projectPath: '/repo', profileId: cli,
      profileRevision: input.launchConfigRevision, action,
    })
    expect(io.patchProfile).not.toHaveBeenCalled(); expect(io.register).not.toHaveBeenCalled()
    expect(io.runChecks).not.toHaveBeenCalled(); expect(io.ptySpawn).not.toHaveBeenCalled()
    expect(io.ptyInput).not.toHaveBeenCalled(); expect(io.ptyKill).not.toHaveBeenCalled()
  })
})

// 正常App根据真实适配器的打开/历史投影显示菜单，激活后的第二次双击才编辑。
it('Runtime_RowDoubleClickSelection_028', async () => {
  const terminal = defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-runtime-tab': props.tabId }) } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(), tabs = useNativeTabsStore()
  const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
  const tab = tabs.tab(opened.adapterSessionId)!
  tab.status = 'exited'; tab.launchRevision = '1'; await catalog.refresh(); await flushPromises()
  catalog.selectProjectContext('/legacy'); await flushPromises()
  await w.get('.search-input').setValue('/repo'); await flushPromises()
  const row = w.get(`[data-session-row="${opened.id}"]`)
  await row.trigger('contextmenu'); await flushPromises()
  expect(document.querySelector('[data-item-id="rename"]')).toBeNull()
  expect(document.querySelector('[data-item-id="restart"]')).not.toBeNull()
  expect(document.querySelector('[data-item-id="close"]')).toBeNull()
  expect(row.get('.session-primary-action button').attributes('aria-label')).toBe('Close')
  document.querySelector('[role="menu"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); await flushPromises()
  await row.trigger('keydown', { key: 'F2', code: 'F2' }); await flushPromises()
  expect(catalog.sessions.find(session => session.id === opened.id)?.renameState).toBe('idle')
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await flushPromises(); row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await flushPromises()
  expect(catalog.activeSessionId).toBe(opened.id)
  expect(row.find('input').exists()).toBe(false)
  expect(w.emitted('workspace-request')?.filter(([request]) => (request as { kind: string }).kind === 'activate')).toHaveLength(1)
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await flushPromises(); row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await flushPromises()
  expect(row.find('input').exists()).toBe(true)
  await row.get('input').setValue('Renamed by double click')
  await row.get('input').trigger('keydown', { key: 'Enter' }); await flushPromises()
  expect(tabs.tab(opened.adapterSessionId)?.title).toBe('Renamed by double click')
  expect(tabs.tab(opened.adapterSessionId)?.status).toBe('exited')
  expect(tabs.tab(opened.adapterSessionId)?.generation).toBe(1)
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'F2', code: 'F2', bubbles: true })); await flushPromises()
  expect(row.find('input').exists()).toBe(true)
  expect(tabs.tabs.size).toBe(1)
  expect(io.ptySpawn).not.toHaveBeenCalled()
})

// 历史行点击和双击保持关闭，仅独立恢复按钮启动精确来源。
it.each(['legacy-claude', 'native-cli'] as const)('Runtime_HistoryMenuAndDoubleClick_029: %s', async runtime => {
  io.sessions.mockResolvedValue([{ sessionId: 'legacy-history', name: 'Legacy history', projectPath: '/legacy', lastActiveAt: 10 }])
  const startLegacy = vi.fn().mockResolvedValue({ ok: true })
  const nativeTerminal = defineComponent({ setup(_props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div') } })
  const legacyTerminal = defineComponent({ setup(_props, { expose }) { expose({ startTab: startLegacy, focus() {}, fitVisible() {} }); return () => h('div') } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: nativeTerminal, XTermTerminal: legacyTerminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const history = catalog.sessions.find(session => session.runtime === runtime)!
  await w.get('.search-input').setValue(history.projectPath); await flushPromises()
  const row = w.findAll('[data-session-row]').find(item => item.attributes('data-session-row') === history.id)!
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true })); await flushPromises(); row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await flushPromises()
  expect(catalog.resumeDialog).toBeNull()
  expect(document.querySelector('[data-resume-target]')).toBeNull()
  expect(document.querySelector('.rename-input')).toBeNull()
  expect(w.emitted('workspace-request')?.filter(([request]) => (request as { kind: string }).kind === 'activate') ?? []).toHaveLength(0)
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(useSessionStore().tabs.size).toBe(0)
  await row.get('[data-session-launch] button').trigger('click'); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(runtime === 'native-cli' ? 1 : 0)
  expect(useSessionStore().tabs.size).toBe(runtime === 'legacy-claude' ? 1 : 0)
  if (runtime === 'legacy-claude') expect(startLegacy).toHaveBeenCalledOnce()
  else expect(useNativeTabsStore().tabs.values().next().value?.action).toEqual({ kind: 'resume-id', nativeSessionId: 'history-id' })
})

// 快速恢复的异步来源检查不得越过导航、替代请求或配置变更，且不能重放副作用。
it.each(['navigation', 'project', 'replacement', 'profile'] as const)('Runtime_DirectResumeAdmission_046: %s', async change => {
  render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const history = catalog.sessions.find(session => session.title === 'History')!
  let finish!: (value: unknown) => void
  io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: history.id, action: 'resume' }); await flushPromises()
  expect(catalog.resumeDialog).toBeNull()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  if (change === 'navigation') shell.navigate('settings')
  else if (change === 'project') { catalog.selectProjectContext('/other'); shell.navigate('workspace') }
  else if (change === 'replacement') shell.requestWorkspaceAction({ kind: 'restore-session', project: history, mode: 'history' })
  else useCliProfilesStore().profiles[0].revision = '8'
  finish({ state: 'ready', items: [{ type: 'session', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }] }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  if (change === 'replacement') expect(catalog.resumeDialog?.mode).toBe('history')
})

// 实际 App 的 projectManagement 投影发布待定按钮禁用态，不仅测试独立目录组件。
it('Runtime_AppResumePendingButton_058', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, XTermTerminal: true, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const history = catalog.sessions.find(session => session.title === 'History')!
  await w.get('.search-input').setValue('/repo'); await flushPromises()
  let finish!: (value: unknown) => void
  io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const row = w.findAll('[data-session-row]').find(item => item.attributes('data-session-row') === history.id)!
  const button = row.get('[data-session-launch] button')
  await button.trigger('click'); await flushPromises()
  expect(button.attributes('disabled')).toBeDefined()
  await button.trigger('click'); await flushPromises()
  expect(w.emitted('workspace-request')?.filter(([request]) => (request as { kind: string }).kind === 'primary-action')).toHaveLength(1)
  finish({ state: 'unavailable', reason: 'SOURCE_UNKNOWN', items: [], hasMore: false }); await flushPromises()
  expect(row.get('[data-session-launch] button').attributes('disabled')).toBeUndefined()
})

// 同一历史的两次显式请求共享核实；较早请求失效不能取消仍有效的新请求或重复启动。
it('Runtime_DirectResumeCoalesces_047', async () => {
  render(); await flushPromises()
  const catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const history = catalog.sessions.find(session => session.title === 'History')!
  let finish!: (value: unknown) => void
  io.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: history.id, action: 'resume' }); await flushPromises()
  shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: history.id, action: 'resume' }); await flushPromises()
  finish({ state: 'ready', items: [{ type: 'session', sessionKey: 'root-key', nativeSessionId: 'history-id', title: 'History', cwd: '/repo' }] }); await flushPromises()
  expect(catalog.resumeDialog).toBeNull()
  expect(useNativeTabsStore().tabs.size).toBe(1)
  expect(catalog.activeSessionId).toMatch(/^native-tab:/)
})

// 等待停止期间被更新的尝试不能被原关闭操作删除。
it('Runtime_DirectCloseRechecksAttempt_048', async () => {
  const { port } = render(); await flushPromises()
  const tabs = useNativeTabsStore(), catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const tab = tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' } })
  tabs.tab(tab.tabId)!.status = 'running'; await catalog.refresh()
  let finish!: () => void
  port.stopNative.mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve }))
  shell.requestWorkspaceAction({ kind: 'primary-action', sessionId: `native-tab:${tab.tabId}`, action: 'close' }); await flushPromises()
  expect(port.stopNative).toHaveBeenCalledOnce()
  tabs.tab(tab.tabId)!.status = 'exited'; tabs.restart(tab.tabId, { profileId: 'cx', profileRevision: '7' })
  finish(); await flushPromises()
  expect(tabs.tab(tab.tabId)?.generation).toBe(2)
  expect(catalog.sessionConfirmation).toBeNull()
})

// 已打开的结束Legacy恢复在异步来源核实后才选择；失败提示与显式Retry仍属于该请求。
it('Runtime_EndedLegacyResumeFailureKeepsRetry_049', async () => {
  const { port, runtime } = render(); await flushPromises()
  const legacy = useSessionStore(), catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const tabId = legacy.createTab('/legacy', { sessionId: 'owned-history', name: 'Ended owner' })
  await catalog.refresh()
  port.restartLegacy.mockRejectedValueOnce({ code: 'RESOURCE_UNAVAILABLE', message: '/private/secret TOKEN' })
  shell.requestWorkspaceAction({ kind: 'menu-action', sessionId: `legacy-tab:${tabId}`, action: 'resume' }); await flushPromises()
  expect(port.restartLegacy).toHaveBeenCalledOnce()
  expect(catalog.actionFeedback).toMatchObject({ detailCode: 'RESOURCE_UNAVAILABLE', retryable: true })
  expect(JSON.stringify(catalog.actionFeedback)).not.toContain('/private/secret')
  expect(catalog.resumeDialog).toBeNull()
  runtime.retryAction(); await flushPromises()
  expect(port.restartLegacy).toHaveBeenCalledTimes(2)
  expect(port.restartLegacy.mock.calls.every(([id]) => id === tabId)).toBe(true)
  expect(legacy.tabs.size).toBe(1)
  expect(catalog.activeSessionId).toBe(`legacy-tab:${tabId}`)
})

// 已结束的 Legacy 终端在异步核实之后才选择；重启错误仍属于本次请求，并支持明确重试。
it('Runtime_EndedLegacyResumeRetainsFailureAndRetry_049', async () => {
  const { runtime, port } = render(); await flushPromises()
  const legacy = useSessionStore(), catalog = useUnifiedSessionsStore(), shell = useShellStore()
  const id = legacy.createTab('/legacy', { sessionId: 'legacy-ended', name: 'Ended Legacy' })
  await catalog.refresh(); await flushPromises()
  port.restartLegacy.mockRejectedValueOnce({ code: 'RESOURCE_UNAVAILABLE', message: '/private/secret TOKEN' })
  shell.requestWorkspaceAction({ kind: 'menu-action', action: 'resume', sessionId: `legacy-tab:${id}` }); await flushPromises()
  expect(port.restartLegacy).toHaveBeenCalledOnce()
  expect(catalog.actionFeedback).toMatchObject({ messageKey: 'errorResourceUnavailable', retryable: true })
  expect(JSON.stringify(catalog.actionFeedback)).not.toContain('/private/secret')
  expect(catalog.resumeDialog).toBeNull()
  port.restartLegacy.mockResolvedValueOnce(undefined)
  runtime.retryAction(); await flushPromises()
  expect(port.restartLegacy).toHaveBeenCalledTimes(2)
  expect(port.restartLegacy.mock.calls.map(call => call[0])).toEqual([id, id])
  expect(legacy.tabs.size).toBe(1)
  expect(catalog.activeSessionId).toBe(`legacy-tab:${id}`)
})

// 默认配置缺少程序路径时保留失败占位的明确取消入口，不把历史或未知尝试当作可丢弃新建。
it('Runtime_DiscardFailedPreparation_030', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useCliProfilesStore().profiles[0].programPath = { mode: 'inherit' }
  await w.get('button[data-new-session]').trigger('click'); await flushPromises()
  document.querySelector<HTMLButtonElement>('[data-item-id="codex"]')!.click(); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const failed = catalog.sessions.find(row => row.safeErrorCode === 'LAUNCH_CONFIGURATION_REQUIRED')!
  expect(failed).toBeDefined()
  await w.get('.search-input').setValue(failed.projectPath); await flushPromises()
  const row = w.findAll('[data-session-row]').find(item => item.attributes('data-session-row') === failed.id)!
  await row.trigger('contextmenu'); await flushPromises()
  expect(document.querySelector('[data-item-id="close"]')).toBeNull()
  expect(document.querySelector('[data-item-id="restart"]')).toBeNull()
  expect(document.querySelector('[data-item-id="rename"]')).toBeNull()
  expect(document.querySelector('[data-item-id="archive"]')).toBeNull()
  const discard = document.querySelector<HTMLButtonElement>('[data-item-id="discard-creation"]')
  expect(discard?.textContent).toBe('Cancel creation')
  discard!.click(); await flushPromises()
  expect(catalog.sessions.some(session => session.id === failed.id)).toBe(false)
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(useSessionStore().tabs.size).toBe(0)
  expect(io.ptySpawn).not.toHaveBeenCalled()
})

// owning store先重启而catalog尚未刷新时，双击不能把旧尝试的点击授权给新generation。
it('Runtime_DoubleClickOwnsAttempt_031', async () => {
  const terminal = defineComponent({ props: ['tabId'], setup(_props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div') } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(), tabs = useNativeTabsStore()
  const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
  await w.get('.search-input').setValue('/repo'); await flushPromises()
  const row = w.get(`[data-session-row="${opened.id}"]`)
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  const previous = catalog.sessions.find(session => session.id === opened.id)
  tabs.restart(opened.adapterSessionId, { profileId: 'cx', profileRevision: '7' })
  expect(catalog.sessions.find(session => session.id === opened.id)).toBe(previous)
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true }))
  row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true }))
  await flushPromises()
  expect(w.emitted('workspace-request')?.filter(([request]) => (request as { action?: string }).action === 'rename') ?? []).toHaveLength(0)
  expect(row.find('input').exists()).toBe(false)
  expect(tabs.tab(opened.adapterSessionId)?.generation).toBe(2)
})

// 同一真实尝试的只读目录刷新不能打断当前行双击重命名。
it('Runtime_DoubleClickSurvivesRefresh_032', async () => {
  const terminal = defineComponent({ props: ['tabId'], setup(_props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div') } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const opened = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo' }); await flushPromises()
  await w.get('.search-input').setValue('/repo'); await flushPromises()
  const row = w.get(`[data-session-row="${opened.id}"]`)
  row.element.dispatchEvent(new MouseEvent('click', { detail: 1, bubbles: true })); await flushPromises()
  await catalog.refresh(); await flushPromises()
  row.element.dispatchEvent(new MouseEvent('click', { detail: 2, bubbles: true }))
  row.element.dispatchEvent(new MouseEvent('dblclick', { detail: 2, bubbles: true })); await flushPromises()
  expect(row.find('input').exists()).toBe(true)
})

// 实际App切换到另一会话立即移走旧编辑器，迟到的旧提交不写metadata也不抢回选择。
it('Runtime_SwitchRevokesRowEditor_033', async () => {
  const terminal = defineComponent({ props: ['tabId'], setup(_props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div') } })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: terminal, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const first = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', title: 'First' })
  const second = await catalog.createSession({ cli: 'codex', projectKey: '/repo', projectPath: '/repo', title: 'Second' }); await flushPromises()
  expect(io.upsertRecord).toHaveBeenCalledTimes(2)
  expect(io.upsertRecord).toHaveBeenNthCalledWith(1, first.id, expect.objectContaining({ title: 'First', lastOpenedAt: expect.any(Number) }), true)
  expect(io.upsertRecord).toHaveBeenNthCalledWith(2, second.id, expect.objectContaining({ title: 'Second', lastOpenedAt: expect.any(Number) }), true)
  io.upsertRecord.mockClear()
  await catalog.activateSession(first.id)
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'F2', code: 'F2', bubbles: true })); await flushPromises()
  const firstRow = w.get(`[data-session-row="${first.id}"]`)
  await firstRow.get('input').setValue('Old draft')
  await w.get(`[data-session-row="${second.id}"]`).trigger('click'); await flushPromises()
  expect(catalog.activeSessionId).toBe(second.id)
  expect(catalog.sessions.find(row => row.id === first.id)?.renameState).toBe('idle')
  expect(w.find(`[data-session-row="${first.id}"] input`).exists()).toBe(false)
  useShellStore().requestWorkspaceAction({ kind: 'rename', sessionId: first.id, title: 'Stale submit' }); await flushPromises()
  expect(io.upsertRecord).not.toHaveBeenCalled()
  expect(catalog.activeSessionId).toBe(second.id)
  expect(catalog.sessions.find(row => row.id === first.id)?.title).toBe('First')
})
