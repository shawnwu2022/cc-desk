// 反馈环：运行中会话标题变化（首问 prompt 标题 → AI 标题）必须同步到会话列表，
// 且同步只刷新受影响项目，不触发全量 catalog 刷新（用户无感目标）。
// 生产链路：observer notice → tab watcher → queueHistory → loadGrouped(force)
// → history deep watch → refreshCatalog → catalog.refresh → projectActive 认领历史标题。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { defineComponent, ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { useUnifiedWorkspaceRuntime } from '@/composables/useUnifiedWorkspaceRuntime'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore, captureNativeAttempt } from '@/stores/nativeTabs'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useProjectsStateStore } from '@/stores/projectsState'
import type { ProjectsState } from '@/types/app'

const io = vi.hoisted(() => ({ projects: vi.fn(), sessions: vi.fn(), profiles: vi.fn(), registered: vi.fn(), register: vi.fn(), patchProfile: vi.fn(), getState: vi.fn(), restore: vi.fn(), upsertRecord: vi.fn(), setPreference: vi.fn(), scope: vi.fn(), read: vi.fn(), writeText: vi.fn(), legacyRoot: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), ptySpawn: vi.fn(), ptyInput: vi.fn(), ptyKill: vi.fn(), getProjectsState: io.getState, restoreSession: io.restore, upsertSessionUiRecord: io.upsertRecord, setProjectLaunchPreference: io.setPreference, updateAppConfig: vi.fn().mockResolvedValue(undefined), getAppConfig: vi.fn().mockResolvedValue({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' }), getProjects: io.projects, getSessions: io.sessions, createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }), legacyClaudeRootKey: io.legacyRoot, onHookEvent: async () => () => {} }))
vi.mock('@/api/cli', () => ({ cliListProfiles: io.profiles, cliPatchProfile: io.patchProfile }))
vi.mock('@/api/cliAvailability', () => ({ cliGetAvailability: async (profileId: string, profileRevision: string) => {
  const profile = useCliProfilesStore().profile(profileId)!
  const selected = profile.programPath.mode === 'set'
  return { profileId, profileRevision, cli: profile.cli, state: selected ? 'available-unverified' : 'configuration-required', hostStatus: 'available', certified: false,
    ...(selected ? {} : { issue: { code: 'PROGRAM_TRUST_REQUIRED', retryable: false } }) }
} }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: io.registered, registerProject: io.register, removeProject: vi.fn() }))
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
  io.projects.mockResolvedValue([]); io.sessions.mockResolvedValue([])
  io.profiles.mockResolvedValue({ revision: '7', profiles: [{ id: 'cl', revision: '7', cli: 'claude', name: 'CL', launcher: { kind: 'native' }, programPath: { mode: 'set', value: '/tools/claude' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
  io.registered.mockResolvedValue({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] })
  io.legacyRoot.mockResolvedValue({ rootKey: '', tabsUseDefaultRoot: false })
  io.scope.mockResolvedValue({ cli: 'claude', sourceRootKey: 'root' })
  io.read.mockReset().mockResolvedValue({ state: 'ready', items: [sessionItem('First prompt title')], hasMore: false })
  vi.useFakeTimers()
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); vi.useRealTimers(); vi.unstubAllGlobals() })

function sessionItem(title: string, titleSource = 'prompt') {
  return { type: 'session', projectId: 'project', sessionKey: 'root-key', nativeSessionId: 'session-1', title, titleSource, cwd: '/repo' }
}

function render() {
  const port = { startLegacy: vi.fn(), stopLegacy: vi.fn(), restartLegacy: vi.fn(), renameLegacy: vi.fn(), stopNative: vi.fn().mockResolvedValue(undefined), recoverNative: vi.fn().mockResolvedValue(undefined), renameNative: vi.fn().mockResolvedValue(undefined), focus: vi.fn() }
  let runtime!: ReturnType<typeof useUnifiedWorkspaceRuntime>
  const mounted = mount(defineComponent({ setup() { runtime = useUnifiedWorkspaceRuntime(ref(port)); return () => null } }))
  wrappers.push(mounted)
  return { runtime, port }
}

describe('running session title sync to session list', () => {
  // 症状①：首问后 CLI 标题变化（prompt → AI 标题）必须出现在会话列表。
  it('TitleSync_HistoryUpdateReachesRunningRow_001', async () => {
    render(); await flushPromises()
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
    const tab = tabs.tab(created.tabId)!
    const attempt = captureNativeAttempt(tab)
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    tabs.applyObservation(tab.tabId, attempt, { observation: 'active', activity: 'idle' })
    tabs.applyObservationIdentity(tab.tabId, attempt, 'session-1')
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'prompt-submitted', eventId: 'evt-1', receivedAt: 1, runId: tab.runId, generation: tab.generation })
    await flushPromises()

    // 首问标题已观察到，历史重读返回 AI 标题。
    io.read.mockResolvedValue({ state: 'ready', items: [sessionItem('AI title', 'ai')], hasMore: false })
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'reply-ended', eventId: 'evt-2', receivedAt: 2, runId: tab.runId, generation: tab.generation })
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()

    const catalog = useUnifiedSessionsStore()
    const row = catalog.sessions.find(value => value.opened && value.nativeSessionId === 'session-1')
    expect(row?.title).toBe('AI title')
  })

  // 症状②：标题同步引发的刷新必须是项目级，不得全量刷新所有项目。
  it('TitleSync_RefreshScopeIsProjectOnly_002', async () => {
    render(); await flushPromises()
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
    const tab = tabs.tab(created.tabId)!
    const attempt = captureNativeAttempt(tab)
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    tabs.applyObservation(tab.tabId, attempt, { observation: 'active', activity: 'idle' })
    tabs.applyObservationIdentity(tab.tabId, attempt, 'session-1')
    await flushPromises()

    const catalog = useUnifiedSessionsStore()
    const refreshSpy = vi.spyOn(catalog, 'refresh')
    refreshSpy.mockClear()
    io.read.mockResolvedValue({ state: 'ready', items: [sessionItem('AI title', 'ai')], hasMore: false })
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'reply-ended', eventId: 'evt-2', receivedAt: 2, runId: tab.runId, generation: tab.generation })
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()

    const scopes = refreshSpy.mock.calls.map(([projectKey]) => projectKey)
    expect(scopes).not.toContain(undefined)
  })

  // 症状③：AI 标题常晚于 reply-ended 写盘。首次对话结束后标题必须最终同步，
  // 而不是停留到下一次交互（中断继续）才跳变。
  it('TitleSync_LateAiTitleArrivesWithoutNewCue_003', async () => {
    render(); await flushPromises()
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
    const tab = tabs.tab(created.tabId)!
    const attempt = captureNativeAttempt(tab)
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    tabs.applyObservation(tab.tabId, attempt, { observation: 'active', activity: 'idle' })
    tabs.applyObservationIdentity(tab.tabId, attempt, 'session-1')
    // 回复结束时 ai-title 尚未写盘：此刻读取只能拿到 prompt 标题。
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'prompt-submitted', eventId: 'evt-1', receivedAt: 1, runId: tab.runId, generation: tab.generation })
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'reply-ended', eventId: 'evt-2', receivedAt: 2, runId: tab.runId, generation: tab.generation })
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()
    const catalog = useUnifiedSessionsStore()
    expect(catalog.sessions.find(value => value.opened && value.nativeSessionId === 'session-1')?.title).toBe('First prompt title')

    // 之后 ai-title 写盘，且不再有新的 observer cue。
    io.read.mockResolvedValue({ state: 'ready', items: [sessionItem('AI title', 'ai')], hasMore: false })
    await vi.advanceTimersByTimeAsync(60_000)
    await flushPromises()
    expect(catalog.sessions.find(value => value.opened && value.nativeSessionId === 'session-1')?.title).toBe('AI title')
  })

  // 恢复提交成功但回执失败时，失败协调的重读删除空归档键并抛错；删除键的
  // 项目必须仍在刷新范围（上一批并集）内，否则已恢复会话留在归档抽屉。
  it('TitleSync_RestoreAckLoss_004', async () => {
    persisted.archivedSessions = { '/legacy': ['old-session'] }
    const projects = useProjectsStateStore()
    await projects.reload()
    io.projects.mockResolvedValue([{ path: '/legacy', name: 'Legacy' }])
    io.sessions.mockImplementation(async (path: string) => path === '/legacy'
      ? [{ sessionId: 'old-session', name: 'Old Legacy', lastActiveAt: 1 }] : [])
    render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    const original = catalog.sessions.find(row => row.nativeSessionId === 'old-session')!
    expect(original.archived).toBe(true)
    io.restore.mockImplementationOnce(async () => {
      persisted.archivedSessions = {}
      throw new Error('ACK_LOST')
    })
    await expect(catalog.restoreArchivedSession(original.id)).rejects.toThrow('ACK_LOST')
    await flushPromises()
    expect(projects.archivedSessions.has('/legacy')).toBe(false)
    expect(catalog.sessions.find(row => row.id === original.id)?.archived).toBe(false)
    expect(catalog.projectGroups.flatMap(group => group.sessions).some(row => row.id === original.id)).toBe(true)
    expect(io.restore).toHaveBeenCalledTimes(1)
  })
})
