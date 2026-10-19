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
import { useSessionStore } from '@/stores/session'
import type { HookEventPayload } from '@/types/hook'
import type { ProjectsState } from '@/types/app'

const io = vi.hoisted(() => ({ projects: vi.fn(), sessions: vi.fn(), profiles: vi.fn(), registered: vi.fn(), register: vi.fn(), patchProfile: vi.fn(), getState: vi.fn(), restore: vi.fn(), upsertRecord: vi.fn(), setPreference: vi.fn(), scope: vi.fn(), read: vi.fn(), writeText: vi.fn(), legacyRoot: vi.fn(), hook: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), ptySpawn: vi.fn(), ptyInput: vi.fn(), ptyKill: vi.fn(), getProjectsState: io.getState, restoreSession: io.restore, upsertSessionUiRecord: io.upsertRecord, setProjectLaunchPreference: io.setPreference, updateAppConfig: vi.fn().mockResolvedValue(undefined), getAppConfig: vi.fn().mockResolvedValue({ theme: 'light', terminalTheme: 'cc-box-light', language: 'en' }), getProjects: io.projects, getSessions: io.sessions, createNativeProjectionClient: () => ({ scope: io.scope, read: io.read }), legacyClaudeRootKey: io.legacyRoot, onHookEvent: io.hook }))
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
  io.hook.mockResolvedValue(() => {})
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
  // 恢复来源已冻结；关闭或不可用的 hook observer 不应阻止当前运行的 OSC 失效信号。
  it.each(['off', 'unavailable'] as const)('TitleSync_NativeOscWithoutObserver_016 %s', async observation => {
    render(); await flushPromises()
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7',
      sourceSessionKey: 'root-key', action: { kind: 'resume-id', nativeSessionId: 'session-1' } })
    const tab = tabs.tab(created.tabId)!, attempt = captureNativeAttempt(tab)
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    tabs.applyObservation(tab.tabId, attempt, { observation, activity: 'unknown' })
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    io.read.mockClear().mockResolvedValue({ state: 'ready', items: [sessionItem('CLI renamed without hook', 'custom')], hasMore: false })
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'reply-ended', eventId: 'inactive-hook', receivedAt: 1, runId: tab.runId, generation: tab.generation })
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    expect(io.read).not.toHaveBeenCalled()
    tab.titleChangeRevision = (tab.titleChangeRevision ?? 0) + 1
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    expect(useUnifiedSessionsStore().sessions.find(row => row.opened)?.title).toBe('CLI renamed without hook')
    expect(io.read).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(120_000)
    expect(io.read).toHaveBeenCalledTimes(1)
  })

  // observer 关闭仍接受精确 PTY 的 OSC；伪造的 hook 不得变成读取或迟到补读。
  it('TitleSync_LegacyOscWithoutObserver_017', async () => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    io.sessions.mockResolvedValue([{ sessionId: 'session-1', name: 'First prompt title', projectPath: '/repo', lastActiveAt: 1 }])
    let receive!: (event: HookEventPayload) => void
    io.hook.mockImplementation(async handler => { receive = handler; return () => {} })
    const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'session-1', name: 'First prompt title' })
    const tab = Object.assign(legacy.tabs.get(id)!, { status: 'running' as const, ptyId: 'pty', usesDefaultClaudeRoot: true, observerEnabled: false })
    render(); await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    io.sessions.mockClear()
    receive({ ptyId: 'pty', runId: 'pty', generation: 1, sessionId: 'session-1', eventId: 'disabled-hook', observerSource: 'claude-hook',
      eventName: 'Stop', state: 'idle', timestamp: 1, detail: { type: 'stop', data: {} } } as HookEventPayload)
    await vi.advanceTimersByTimeAsync(300)
    expect(io.sessions).not.toHaveBeenCalled()
    io.sessions.mockResolvedValue([{ sessionId: 'session-1', name: 'CLI renamed without hook', projectPath: '/repo', lastActiveAt: 2 }])
    tab.titleChangeRevision = (tab.titleChangeRevision ?? 0) + 1
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    expect(useUnifiedSessionsStore().sessions.find(row => row.opened)?.title).toBe('CLI renamed without hook')
    expect(io.sessions).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(120_000)
    expect(io.sessions).toHaveBeenCalledTimes(1)
  })

  // 放宽 observer 条件不等于放宽来源证明；新 Native 身份未知、Legacy 根未知都不扫描。
  it.each(['native', 'legacy'] as const)('TitleSync_OscRequiresSourceProof_019 %s', async runtimeKind => {
    let tab: { titleChangeRevision?: number }
    if (runtimeKind === 'native') {
      render(); await flushPromises()
      const tabs = useNativeTabsStore()
      const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
      tab = tabs.tab(created.tabId)!
      tabs.applyLaunchStatus(created.tabId, { instanceId: 'i', requestId: created.requestId, run: { runId: created.runId, generation: created.generation }, revision: '1', phase: 'running', failure: null })
    } else {
      io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
      const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'session-1', name: 'Unproven' })
      tab = Object.assign(legacy.tabs.get(id)!, { status: 'running' as const, ptyId: 'pty', usesDefaultClaudeRoot: false, observerEnabled: false })
      render()
    }
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    io.read.mockClear(); io.sessions.mockClear()
    tab.titleChangeRevision = (tab.titleChangeRevision ?? 0) + 1
    await flushPromises(); await vi.advanceTimersByTimeAsync(120_000)
    expect(io.read).not.toHaveBeenCalled()
    expect(io.sessions).not.toHaveBeenCalled()
  })

  // 关闭 observer 撤销旧 Stop 的定时器，但同一可信 PTY 后续 OSC 仍可同步标题。
  it('TitleSync_LegacyDisableObserverCancelsHookRetries_020', async () => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    io.sessions.mockResolvedValue([{ sessionId: 'session-1', name: 'Trusted', projectPath: '/repo', lastActiveAt: 1 }])
    let receive!: (event: HookEventPayload) => void
    io.hook.mockImplementation(async handler => { receive = handler; return () => {} })
    const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'session-1', name: 'Trusted' })
    const tab = Object.assign(legacy.tabs.get(id)!, { status: 'running' as const, ptyId: 'pty', usesDefaultClaudeRoot: true, observerEnabled: true })
    render(); await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    receive({ ptyId: 'pty', runId: 'pty', generation: 1, sessionId: 'session-1', eventId: 'old-stop', observerSource: 'claude-hook',
      eventName: 'Stop', state: 'idle', timestamp: 1, detail: { type: 'stop', data: {} } } as HookEventPayload)
    await vi.advanceTimersByTimeAsync(300)
    tab.observerEnabled = false
    await flushPromises(); io.sessions.mockClear()
    await vi.advanceTimersByTimeAsync(120_000)
    expect(io.sessions).not.toHaveBeenCalled()
    io.sessions.mockResolvedValue([{ sessionId: 'session-1', name: 'After disabling', projectPath: '/repo', lastActiveAt: 2 }])
    tab.titleChangeRevision = (tab.titleChangeRevision ?? 0) + 1
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    expect(useUnifiedSessionsStore().sessions.find(row => row.opened)?.title).toBe('After disabling')
    expect(io.sessions).toHaveBeenCalledTimes(1)
  })

  // 自动分页读取中不能发布前二十行；后页失败保留完整缓存，成功一次发布完整新行。
  it.each(['failure', 'success'] as const)('TitleSync_LegacyAtomicPages_018 %s', async outcome => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    const rows = Array.from({ length: 25 }, (_, i) => ({ sessionId: `session-${i}`, name: `Trusted ${i}`, projectPath: '/repo', lastActiveAt: i }))
    io.sessions.mockImplementation(async (_path, limit, offset) => rows.slice(offset, offset + limit))
    const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'session-0', name: 'Trusted 0' })
    const tab = Object.assign(legacy.tabs.get(id)!, { status: 'running' as const, ptyId: 'pty', usesDefaultClaudeRoot: true })
    render(); await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    const prior = legacy.getCatalogHistoryFor('/repo')
    expect(prior).toHaveLength(25)
    let finish!: (rows: unknown[]) => void, fail!: (error: Error) => void
    const updated = rows.map(row => ({ ...row, name: `Updated ${row.sessionId}` }))
    io.sessions.mockImplementation((_path, limit, offset) => offset === 0 ? Promise.resolve(updated.slice(0, limit))
      : new Promise((resolve, reject) => { finish = resolve; fail = reject }))
    tab.titleChangeRevision = (tab.titleChangeRevision ?? 0) + 1
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    const duringRead = legacy.getCatalogHistoryFor('/repo')
    if (outcome === 'failure') fail(new Error('later page failed'))
    else finish(updated.slice(20))
    await flushPromises()
    expect(duringRead, '分页中的首二十行不能替换已发布快照').toBe(prior)
    const catalog = useUnifiedSessionsStore()
    await catalog.refresh('/repo')
    expect(catalog.sessions).toHaveLength(25)
    expect(legacy.isLoadingMore).toBe(false)
    if (outcome === 'failure') {
      expect(legacy.getCatalogHistoryFor('/repo')).toBe(prior)
      expect(legacy.historyLoadState.get('/repo')?.error).toContain('later page failed')
      expect(catalog.sessions.find(row => row.opened)?.title).toBe('Trusted 0')
    } else {
      expect(legacy.getCatalogHistoryFor('/repo').every(row => row.name.startsWith('Updated '))).toBe(true)
      expect(catalog.sessions.find(row => row.opened)?.title).toBe('Updated session-0')
    }
  })

  // CLI /rename 没有 UserPromptSubmit/Stop；OSC 只使精确来源过期，显示名仍从历史读取。
  it.each(['native', 'legacy'] as const)('TitleSync_IdleCliRenameSignal_015 %s', async runtimeKind => {
    let tab: { titleChangeRevision?: number }
    if (runtimeKind === 'legacy') {
      io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
      io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
      io.sessions.mockResolvedValue([{ sessionId: 'session-1', name: 'First prompt title', lastActiveAt: 1 }])
      const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'session-1', name: 'First prompt title' })
      tab = Object.assign(legacy.tabs.get(id)!, { status: 'running' as const, ptyId: 'pty', usesDefaultClaudeRoot: true, titleChangeRevision: 0 })
      render()
    } else {
      render(); await flushPromises()
      const tabs = useNativeTabsStore()
      const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
      const live = tabs.tab(created.tabId)!, attempt = captureNativeAttempt(live)
      tabs.applyLaunchStatus(live.tabId, { instanceId: 'i', requestId: live.requestId, run: { runId: live.runId, generation: live.generation }, revision: '1', phase: 'running', failure: null })
      tabs.applyObservation(live.tabId, attempt, { observation: 'active', activity: 'idle' })
      tabs.applyObservationIdentity(live.tabId, attempt, 'session-1')
      tab = Object.assign(live, { titleChangeRevision: 0 })
    }
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    const catalog = useUnifiedSessionsStore(), refresh = vi.spyOn(catalog, 'refresh')
    for (const name of ['CLI rename A', 'CLI rename B']) {
      io.read.mockResolvedValue({ state: 'ready', items: [sessionItem(name, 'custom')], hasMore: false })
      io.sessions.mockResolvedValue([{ sessionId: 'session-1', name, lastActiveAt: 1 }])
      tab.titleChangeRevision = (tab.titleChangeRevision ?? 0) + 1
      await flushPromises(); await vi.advanceTimersByTimeAsync(300)
      expect(catalog.sessions.find(row => row.opened)?.title).toBe(name)
    }
    expect(refresh.mock.calls.every(([project]) => project === '/repo')).toBe(true)
    const reads = io.read.mock.calls.length + io.sessions.mock.calls.length
    await vi.advanceTimersByTimeAsync(120_000)
    expect(io.read.mock.calls.length + io.sessions.mock.calls.length).toBe(reads)
  })

  // 已有 AI 标题不能阻止后续回合迟写的新标题，避免 Native 打开行永久停留在旧摘要。
  it('TitleSync_NativeRevisedAi_011', async () => {
    io.read.mockResolvedValue({ state: 'ready', items: [sessionItem('Old AI title', 'ai')], hasMore: false })
    render(); await flushPromises()
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
    const tab = tabs.tab(created.tabId)!, attempt = captureNativeAttempt(tab)
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    tabs.applyObservation(tab.tabId, attempt, { observation: 'active', activity: 'idle' })
    tabs.applyObservationIdentity(tab.tabId, attempt, 'session-1')
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'reply-ended', eventId: 'reply-2', receivedAt: 2, runId: tab.runId, generation: tab.generation })
    await vi.advanceTimersByTimeAsync(300)
    io.read.mockResolvedValue({ state: 'ready', items: [sessionItem('Revised AI title', 'ai')], hasMore: false })
    await vi.advanceTimersByTimeAsync(60_000)
    expect(useUnifiedSessionsStore().sessions.find(row => row.opened)?.title).toBe('Revised AI title')
  })

  // 旧运行行在刷新后跟随精确来源标题，Desk 显式别名继续覆盖，未知根不得借名。
  it.each(['cli', 'alias', 'unknown-root'] as const)('TitleSync_LegacyRefresh_007 %s', async mode => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    io.sessions.mockResolvedValue([{ sessionId: 'legacy-session', name: 'AI title', projectPath: '/repo', lastActiveAt: 1 }])
    const legacy = useSessionStore()
    const id = legacy.createTab('/repo', { sessionId: 'legacy-session', name: 'First prompt' })
    Object.assign(legacy.tabs.get(id)!, { status: 'running', ptyId: 'pty', usesDefaultClaudeRoot: mode !== 'unknown-root' })
    if (mode === 'alias') useProjectsStateStore().sessionRecords.set('legacy-history:/repo:legacy-session', {
      runtime: 'legacy-claude', cli: 'claude', projectPath: '/repo', adapterSessionId: 'legacy-session', nativeSessionId: 'legacy-session', title: 'Desk alias', lastActivityAt: 1,
    })
    const { runtime } = render(); await flushPromises()
    const catalog = useUnifiedSessionsStore()
    expect(catalog.sessions.find(row => row.opened)?.title).toBe(mode === 'alias' ? 'Desk alias' : mode === 'cli' ? 'AI title' : 'First prompt')
    io.sessions.mockResolvedValue([{ sessionId: 'legacy-session', name: 'CLI renamed', projectPath: '/repo', lastActiveAt: 2 }])
    await runtime.refresh(); await flushPromises()
    expect(catalog.sessions.find(row => row.opened)?.title).toBe(mode === 'alias' ? 'Desk alias' : mode === 'cli' ? 'CLI renamed' : 'First prompt')
  })

  // 真实 hook 分发触发旧会话补读；AI 标题迟写盘也能更新，读取仅限所在项目且有界。
  it('TitleSync_LegacyLateTitle_008', async () => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    io.sessions.mockResolvedValue([{ sessionId: 'legacy-session', name: 'First prompt', projectPath: '/repo', lastActiveAt: 1 }])
    let receive!: (event: HookEventPayload) => void
    io.hook.mockImplementation(async handler => { receive = handler; return () => {} })
    const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'legacy-session', name: 'First prompt' })
    Object.assign(legacy.tabs.get(id)!, { status: 'running', ptyId: 'pty', usesDefaultClaudeRoot: true })
    render(); await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    const catalog = useUnifiedSessionsStore(), refresh = vi.spyOn(catalog, 'refresh')
    io.sessions.mockClear()
    receive({ ptyId: 'pty', runId: 'pty', generation: 1, sessionId: 'legacy-session', eventId: 'reply-1', observerSource: 'claude-hook',
      eventName: 'Stop', state: 'idle', timestamp: 1, detail: { type: 'stop', data: {} } } as HookEventPayload)
    await vi.advanceTimersByTimeAsync(300)
    expect(catalog.sessions.find(row => row.opened)?.title).toBe('First prompt')
    io.sessions.mockResolvedValue([{ sessionId: 'legacy-session', name: 'Late AI title', projectPath: '/repo', lastActiveAt: 2 }])
    await vi.advanceTimersByTimeAsync(60_000); await flushPromises()
    expect(catalog.sessions.find(row => row.opened)?.title).toBe('Late AI title')
    expect(io.sessions.mock.calls.every(([project]) => project === '/repo')).toBe(true)
    expect(io.sessions.mock.calls.length).toBeLessThanOrEqual(4)
    expect(refresh.mock.calls.every(([project]) => project === '/repo')).toBe(true)
    const calls = io.sessions.mock.calls.length
    await vi.advanceTimersByTimeAsync(120_000)
    expect(io.sessions).toHaveBeenCalledTimes(calls)
  })

  // 同项目多个运行会话的事件突发合并为一次读取，标题按各自 sessionId 发布。
  it('TitleSync_LegacyCoalesced_012', async () => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    io.sessions.mockResolvedValue(['first', 'second'].map(sessionId => ({ sessionId, name: sessionId, lastActiveAt: 1 })))
    let receive!: (event: HookEventPayload) => void
    io.hook.mockImplementation(async handler => { receive = handler; return () => {} })
    const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'first', name: 'first' })
    Object.assign(legacy.tabs.get(id)!, { status: 'running', ptyId: 'pty-first', usesDefaultClaudeRoot: true })
    legacy.tabs.set('second-tab', { ...legacy.tabs.get(id)!, tabId: 'second-tab', ptyId: 'pty-second', sessionId: 'second' })
    render(); await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    io.sessions.mockClear().mockResolvedValue(['first', 'second'].map(sessionId => ({ sessionId, name: `${sessionId} AI`, lastActiveAt: 2 })))
    for (const sessionId of ['first', 'second']) receive({ ptyId: `pty-${sessionId}`, runId: `pty-${sessionId}`, generation: 1,
      sessionId, eventId: `prompt-${sessionId}`, observerSource: 'claude-hook', eventName: 'UserPromptSubmit', state: 'thinking',
      timestamp: 1, detail: { type: 'userPromptSubmit', data: { prompt: 'next' } } } as HookEventPayload)
    await vi.advanceTimersByTimeAsync(300)
    expect(io.sessions).toHaveBeenCalledExactlyOnceWith('/repo', 21, 0)
    const rows = useUnifiedSessionsStore().sessions.filter(row => row.opened)
    expect(rows.map(row => row.title).sort()).toEqual(['first AI', 'second AI'])
  })

  // 读取已发出后卸载也不得把旧结果写回运行行或继续补读。
  it('TitleSync_LegacyInflightUnmount_013', async () => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    io.sessions.mockResolvedValue([{ sessionId: 'session', name: 'Trusted title', lastActiveAt: 1 }])
    let receive!: (event: HookEventPayload) => void
    io.hook.mockImplementation(async handler => { receive = handler; return () => {} })
    const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'session', name: 'Trusted title' })
    const tab = legacy.tabs.get(id)!
    Object.assign(tab, { status: 'running', ptyId: 'pty', usesDefaultClaudeRoot: true })
    render(); await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    let resolve!: (rows: unknown[]) => void
    io.sessions.mockClear().mockImplementationOnce(() => new Promise(done => { resolve = done }))
    receive({ ptyId: 'pty', runId: 'pty', generation: 1, sessionId: 'session', eventId: 'reply', observerSource: 'claude-hook',
      eventName: 'Stop', state: 'idle', timestamp: 1, detail: { type: 'stop', data: {} } } as HookEventPayload)
    await vi.advanceTimersByTimeAsync(300)
    expect(io.sessions).toHaveBeenCalledTimes(1)
    wrappers.pop()!.unmount()
    resolve([{ sessionId: 'session', name: 'Late result', lastActiveAt: 2 }])
    await flushPromises(); await vi.advanceTimersByTimeAsync(120_000)
    expect(tab.name).toBe('Trusted title')
    expect(io.sessions).toHaveBeenCalledTimes(1)
  })

  // 精确 PTY、根或交互失效后，旧回复的延迟补读不能继续扫描或改变新尝试的标题。
  it.each(['restart', 'close', 'unmount', 'root', 'prompt'] as const)('TitleSync_LegacyCancel_009 %s', async reason => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    io.sessions.mockResolvedValue([{ sessionId: 'legacy-session', name: 'First prompt', projectPath: '/repo', lastActiveAt: 1 }])
    let receive!: (event: HookEventPayload) => void
    io.hook.mockImplementation(async handler => { receive = handler; return () => {} })
    const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'legacy-session', name: 'First prompt' })
    const tab = legacy.tabs.get(id)!
    Object.assign(tab, { status: 'running', ptyId: 'pty', usesDefaultClaudeRoot: true })
    render(); await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    const event = { ptyId: 'pty', runId: 'pty', generation: 1, sessionId: 'legacy-session', eventId: 'reply-1', observerSource: 'claude-hook',
      eventName: 'Stop', state: 'idle', timestamp: 1, detail: { type: 'stop', data: {} } } as HookEventPayload
    receive(event); await vi.advanceTimersByTimeAsync(300)
    if (reason === 'restart') { tab.ptyId = 'new-pty'; tab.ptyGeneration = 2 }
    if (reason === 'close') legacy.removeTab(id)
    if (reason === 'unmount') wrappers.pop()!.unmount()
    if (reason === 'root') io.legacyRoot.mockResolvedValue({ rootKey: 'replacement', tabsUseDefaultRoot: true })
    if (reason === 'prompt') receive({ ...event, eventId: 'prompt-2', detail: { type: 'userPromptSubmit', data: { prompt: 'next' } } })
    await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    const calls = io.sessions.mock.calls.length
    io.sessions.mockResolvedValue([{ sessionId: 'legacy-session', name: 'Stale title', projectPath: '/repo', lastActiveAt: 2 }])
    await vi.advanceTimersByTimeAsync(120_000)
    expect(io.sessions).toHaveBeenCalledTimes(calls)
    expect(tab.name).toBe('First prompt')
  })

  // 读取失败保留最近标题；无 PTY 来源证明的事件不能触发磁盘扫描。
  it('TitleSync_LegacyReadFailure_010', async () => {
    io.profiles.mockResolvedValue({ revision: '1', profiles: [] })
    io.legacyRoot.mockResolvedValue({ rootKey: 'root', tabsUseDefaultRoot: true })
    io.sessions.mockResolvedValue([{ sessionId: 'legacy-session', name: 'AI title', projectPath: '/repo', lastActiveAt: 1 }])
    let receive!: (event: HookEventPayload) => void
    io.hook.mockImplementation(async handler => { receive = handler; return () => {} })
    const legacy = useSessionStore(), id = legacy.createTab('/repo', { sessionId: 'legacy-session', name: 'First prompt' })
    Object.assign(legacy.tabs.get(id)!, { status: 'running', ptyId: 'pty', usesDefaultClaudeRoot: true })
    render(); await flushPromises(); await vi.advanceTimersByTimeAsync(300)
    const event = { ptyId: 'pty', runId: 'pty', generation: 1, sessionId: 'legacy-session', eventId: 'prompt-1', observerSource: 'claude-hook',
      eventName: 'UserPromptSubmit', state: 'thinking', timestamp: 1, detail: { type: 'userPromptSubmit', data: { prompt: 'next' } } } as HookEventPayload
    io.sessions.mockClear()
    receive({ ...event, observerSource: undefined, runId: undefined, generation: undefined })
    await vi.advanceTimersByTimeAsync(300)
    expect(io.sessions).not.toHaveBeenCalled()
    io.sessions.mockRejectedValueOnce(new Error('read failed'))
    receive(event); await vi.advanceTimersByTimeAsync(300)
    const catalog = useUnifiedSessionsStore()
    await catalog.refresh('/repo')
    expect(catalog.sessions.find(row => row.opened)?.title).toBe('AI title')
    expect(io.sessions).toHaveBeenCalledTimes(1)
    expect(legacy.historyLoadState.get('/repo')?.error).toContain('read failed')
  })

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

  // 第一轮补读之后才写入 AI 标题，仍应有界重读，并在观察到标题后停止 IO。
  it('TitleSync_AiTitleBeyondFirstRetry_005', async () => {
    render(); await flushPromises()
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
    const tab = tabs.tab(created.tabId)!
    const attempt = captureNativeAttempt(tab)
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    tabs.applyObservation(tab.tabId, attempt, { observation: 'active', activity: 'idle' })
    tabs.applyObservationIdentity(tab.tabId, attempt, 'session-1')
    await flushPromises()
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'reply-ended', eventId: 'evt-1', receivedAt: 1, runId: tab.runId, generation: tab.generation })
    await vi.advanceTimersByTimeAsync(20_000); await flushPromises()
    io.read.mockResolvedValue({ state: 'ready', items: [sessionItem('Delayed AI title', 'ai')], hasMore: false })
    await vi.advanceTimersByTimeAsync(45_000); await flushPromises()
    expect(useUnifiedSessionsStore().sessions.find(row => row.opened && row.nativeSessionId === 'session-1')?.title).toBe('Delayed AI title')
    const reads = io.read.mock.calls.length
    await vi.advanceTimersByTimeAsync(120_000); await flushPromises()
    expect(io.read).toHaveBeenCalledTimes(reads)
  })

  it.each(['new-prompt', 'unmount', 'budget'] as const)('TitleSync_RetryStops_006 %s', async reason => {
    render(); await flushPromises()
    const tabs = useNativeTabsStore()
    const created = tabs.create({ cli: 'claude', projectId: 'project', projectPath: '/repo', profileId: 'cl', profileRevision: '7', action: { kind: 'new' } })
    const tab = tabs.tab(created.tabId)!
    const attempt = captureNativeAttempt(tab)
    tabs.applyLaunchStatus(tab.tabId, { instanceId: 'i', requestId: tab.requestId, run: { runId: tab.runId, generation: tab.generation }, revision: '1', phase: 'running', failure: null })
    tabs.applyObservation(tab.tabId, attempt, { observation: 'active', activity: 'idle' })
    tabs.applyObservationIdentity(tab.tabId, attempt, 'session-1')
    await flushPromises()
    tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'reply-ended', eventId: 'evt-1', receivedAt: 1, runId: tab.runId, generation: tab.generation })
    await vi.advanceTimersByTimeAsync(300); await flushPromises()
    if (reason === 'unmount') wrappers.pop()!.unmount()
    if (reason === 'new-prompt') {
      tabs.applyObservationNotice(tab.tabId, attempt, { kind: 'prompt-submitted', eventId: 'evt-2', receivedAt: 2, runId: tab.runId, generation: tab.generation })
      await vi.advanceTimersByTimeAsync(300); await flushPromises()
    }
    if (reason === 'budget') { await vi.advanceTimersByTimeAsync(65_000); await flushPromises() }
    const reads = io.read.mock.calls.length
    await vi.advanceTimersByTimeAsync(120_000); await flushPromises()
    expect(io.read).toHaveBeenCalledTimes(reads)
  })
})
