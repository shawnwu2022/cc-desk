import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import App from '@/App.vue'
import { useShellStore } from '@/stores/shell'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
const io = vi.hoisted(() => ({ read: vi.fn(), legacy: vi.fn(), profile: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(),
  getProjectsState: async () => ({ pinnedProjects: [], archivedSessions: {} }), getProjects: async () => [], getSessions: io.legacy,
  updateAppConfig: async () => {}, getAppConfig: async () => ({ theme: 'light', language: 'en' }), onHookEvent: async () => () => {},
  createNativeProjectionClient: () => ({ scope: async () => ({ cli: 'codex', sourceRootKey: 'root' }), read: io.read }) }))
vi.mock('@/api/cli', () => ({ cliListProfiles: io.profile }))
vi.mock('@/api/workspace', () => ({ listRegisteredProjects: async () => ({ revision: '1', projects: [{ projectId: 'project', hostId: 'host', sourcePathKey: 'source', selectedPath: '/repo', canonicalPath: '/repo', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }) }))
vi.mock('@xterm/xterm', () => ({ Terminal: class {} }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isFocused: async () => true, onFocusChanged: async () => () => {}, requestUserAttention: async () => {}, onResized: async () => () => {}, isMaximized: async () => false }) }))
const historyKey = JSON.stringify(['local', 'codex', 'root', 'history-id'])
const NativeHost = defineComponent({ props: ['tabId'], setup(props, { expose }) {
  expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} })
  return () => h('div', { 'data-restored-tab': props.tabId })
} })
const wrappers: VueWrapper[] = []
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); localStorage.clear()
  io.legacy.mockResolvedValue([])
  io.profile.mockResolvedValue({ revision: '7', profiles: [{ id: 'cx', revision: '7', cli: 'codex', name: 'Work config', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'inherit' }, env: {} }] })
  io.read.mockReset().mockResolvedValue({ state: 'ready', items: [{ type: 'session', sessionKey: historyKey, nativeSessionId: 'history-id', title: 'Restore this', cwd: '/repo', updatedAt: '2026-09-30T00:00:00Z' }], hasMore: false })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); document.body.innerHTML = '' })
// 普通 App 的快捷恢复必须打开真实恢复界面，并经确认准入现有宿主。
it('Resume_QuickMenuReachesHost_001', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-restored-tab': props.tabId }) } }), SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'new-session', project: { projectKey: '/repo', projectPath: '/repo', intent: 'restore' } }); await flushPromises()
  expect(document.querySelector('[role=dialog]')).not.toBeNull()
  expect(document.querySelector('[data-resume-query]')).not.toBeNull()
  ;(document.querySelector('[data-resume-result]') as HTMLButtonElement).click(); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(1)
  expect(w.find('[data-restored-tab]').exists()).toBe(true)
  expect(document.querySelector('[role=dialog]')).toBeNull()
})

// 点击历史直接恢复冻结的准确来源，不受选择器筛选或二次确认影响。
it('Resume_TreeKeepsChosenSession_002', async () => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-restored-tab': props.tabId }) } }), SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const row = useUnifiedSessionsStore().sessions[0]
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: row.id }); await flushPromises()
  expect(document.querySelector('[data-confirm-resume]')).toBeNull()
  expect(useUnifiedSessionsStore().resumeDialog).toBeNull()
  expect(useNativeTabsStore().tabs.size).toBe(1)
  expect([...useNativeTabsStore().tabs.values()][0]).toMatchObject({ action: { kind: 'resume-id', nativeSessionId: row.nativeSessionId }, projectPath: row.projectPath, profileId: row.nativeOrigin?.profileId, profileRevision: row.nativeOrigin?.profileRevision })
})

// 较晚的旧搜索响应不能取代新的查询，也不能在隐藏后打开弹窗。
it('Resume_RejectsStaleSearch_003', async () => {
  const { default: Dialog } = await import('@/components/sessions/ResumeSessionDialog.vue')
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const { createNativeCliAdapter } = await import('@/session/adapters/nativeCliAdapter')
  const tabs = useNativeTabsStore()
  tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' }, title: 'Alpha' })
  tabs.create({ cli: 'codex', projectId: 'project', projectPath: '/repo', profileId: 'cx', profileRevision: '7', action: { kind: 'new' }, title: 'Beta' })
  const catalog = useUnifiedSessionsStore()
  catalog.configureAdapters([createNativeCliAdapter({ tabs, history: { all: () => [] }, archive: { getArchivedSessions: () => [], archiveSession: vi.fn(), restoreSession: vi.fn() }, runtime: { createTab: vi.fn(), restartTab: vi.fn(), stopTab: vi.fn() } })])
  const pending: Array<() => void> = []
  catalog.configureHistoryLoader(() => new Promise(resolve => pending.push(() => resolve(false))))
  catalog.openResumeDialog({ project: { projectKey: '/repo', projectPath: '/repo' }, mode: 'history' })
  const w = mount(Dialog, { attachTo: document.body, props: { active: true }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } }); wrappers.push(w)
  const input = document.querySelector('[data-resume-query]') as HTMLInputElement
  input.value = 'Alpha'; input.dispatchEvent(new Event('input')); await flushPromises()
  input.value = 'Beta'; input.dispatchEvent(new Event('input')); await flushPromises()
  pending[pending.length - 1](); await flushPromises()
  expect(document.querySelector('.resume-results')?.textContent).toContain('Beta')
  pending.slice(0, -1).forEach(resolve => resolve()); await flushPromises()
  expect(document.querySelector('.resume-results')?.textContent).not.toContain('Alpha')
  await w.setProps({ active: false }); await flushPromises()
  catalog.openResumeDialog({ project: { projectKey: '/repo', projectPath: '/repo' }, mode: 'history' }); await flushPromises()
  expect(catalog.resumeDialog).toBeNull(); expect(document.querySelector('[role=dialog]')).toBeNull()
})

// 确认前切换项目/配置后，恢复按钮不能沿用旧的直接恢复草稿。
it('Resume_DirectUsesExplicitConfig_004', async () => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const { useCliProfilesStore } = await import('@/stores/cliProfiles')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'restore-session', project: { projectKey: '/repo', projectPath: '/repo' }, cli: 'codex', mode: 'resume-id' }); await flushPromises()
  ;(document.querySelector('[data-prepare-resume]') as HTMLButtonElement).click(); await flushPromises()
  expect(document.body.textContent).toContain(en.resumeChooseRequired)
  const select = document.querySelector('[data-resume-config]') as HTMLSelectElement; select.value = 'cx'; select.dispatchEvent(new Event('change'))
  const id = document.querySelector('[data-resume-id]') as HTMLInputElement; id.value = 'history-id'; id.dispatchEvent(new Event('input')); await flushPromises()
  ;(document.querySelector('[data-prepare-resume]') as HTMLButtonElement).click(); await flushPromises()
  useCliProfilesStore().profiles[0].revision = '8'
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  expect(document.body.textContent).toContain(en.resumeConfigurationChanged)
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(useUnifiedSessionsStore().resumeDialog?.mode).toBe('resume-id')
})

// 关闭验证中的恢复对话框后，迟到的历史读取不能启动会话。
it('Resume_CloseCancelsLateAdmission_005', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'restore-session', project: { projectKey: '/repo', projectPath: '/repo' }, mode: 'history' }); await flushPromises()
  ;(document.querySelector('[data-resume-result]') as HTMLButtonElement).click(); await flushPromises()
  let finish!: (value: unknown) => void
  io.read.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  useShellStore().navigate('settings'); await flushPromises()
  finish({ state: 'ready', items: [{ type: 'session', sessionKey: historyKey, nativeSessionId: 'history-id', title: 'Restore this' }], hasMore: false }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0); expect(document.querySelector('[role=dialog]')).toBeNull()
})

// 缺失历史的提示和显式移除仅清理应用记录，且原始异常文本从不进入界面。
it('Resume_MissingRecordCanBeRemoved_006', async () => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'restore-session', project: { projectKey: '/repo', projectPath: '/repo' }, mode: 'history' }); await flushPromises()
  ;(document.querySelector('[data-resume-result]') as HTMLButtonElement).click(); await flushPromises()
  io.read.mockResolvedValue({ state: 'ready', items: [], hasMore: false })
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  expect(document.body.textContent).toContain('This session can no longer be restored')
  ;(document.querySelector('[data-remove-record]') as HTMLButtonElement).click(); await flushPromises()
  expect(document.body.textContent).toContain(en.resumeRemoveHint)
  ;(document.querySelector('[data-confirm-remove]') as HTMLButtonElement).click(); await flushPromises()
  expect(useUnifiedSessionsStore().sessions).toEqual([]); expect(useNativeTabsStore().tabs.size).toBe(0)
})

// CLI 自带恢复列表使用显式选择的已有项目/配置，保持精确 backend action。
it('Resume_PickerUsesExistingSource_007', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-restored-tab': props.tabId }) } }), SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'restore-session', project: { projectKey: '/repo', projectPath: '/repo' }, cli: 'codex', mode: 'resume-picker' }); await flushPromises()
  const select = document.querySelector('[data-resume-config]') as HTMLSelectElement; select.value = 'cx'; select.dispatchEvent(new Event('change'))
  const scope = document.querySelector('[data-resume-scope]') as HTMLSelectElement; scope.value = 'all'; scope.dispatchEvent(new Event('change')); await flushPromises()
  ;(document.querySelector('[data-prepare-resume]') as HTMLButtonElement).click(); await flushPromises()
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  expect([...useNativeTabsStore().tabs.values()]).toMatchObject([{ cli: 'codex', projectId: 'project', profileId: 'cx', profileRevision: '7', action: { kind: 'resume-picker', scope: 'all' } }])
  expect(w.find('[data-restored-tab]').exists()).toBe(true)
})

// 高级恢复沿用用户明确选择的启动配置，但不生成安全默认配置。
it('Resume_AdvancedKeepsExplicitChoice_008', async () => {
  const { useNewSessionDraftStore } = await import('@/stores/newSessionDraft')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const draft = useNewSessionDraftStore(); draft.open({ projectKey: '/repo', projectPath: '/repo' }, 'codex'); draft.startMode = 'resume-id'; await flushPromises()
  draft.launchConfigId = 'cx'; await flushPromises()
  ;(document.querySelector('[data-create-session]') as HTMLButtonElement).click(); await flushPromises()
  expect((document.querySelector('[data-resume-config]') as HTMLSelectElement).value).toBe('cx')
  expect(document.querySelector('[data-resume-id]')).not.toBeNull(); expect(useNativeTabsStore().tabs.size).toBe(0)
})

// 历史来源配置丢失或鉴权来源修订冲突时，不以当前默认配置恢复，也不泄露错误正文。
it.each(['missing-config', 'source-conflict'])('Resume_OriginFailureGuidance_009_%s', async failure => {
  const { useCliProfilesStore } = await import('@/stores/cliProfiles')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'restore-session', project: { projectKey: '/repo', projectPath: '/repo' }, mode: 'history' }); await flushPromises()
  ;(document.querySelector('[data-resume-result]') as HTMLButtonElement).click(); await flushPromises()
  if (failure === 'missing-config') useCliProfilesStore().profiles = []
  else io.read.mockRejectedValueOnce({ code: 'REVISION_CONFLICT', message: '/private/secret env TOKEN' })
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  expect(document.body.textContent).toContain(en.resumeConfigurationChanged)
  expect(document.body.textContent).not.toContain('/private/secret'); expect(useNativeTabsStore().tabs.size).toBe(0)
})

// 按 ID 恢复真实准入后再次恢复相同来源，只切换现有未知状态尝试。
it('Resume_DirectIdReusesOpenAttempt_010', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-restored-tab': props.tabId }) } }), SettingsView: true } } }); wrappers.push(w); await flushPromises()
  for (let attempt = 0; attempt < 2; attempt++) {
    useShellStore().requestWorkspaceAction({ kind: 'restore-session', project: { projectKey: '/repo', projectPath: '/repo' }, cli: 'codex', mode: 'resume-id' }); await flushPromises()
    const select = document.querySelector('[data-resume-config]') as HTMLSelectElement; select.value = 'cx'; select.dispatchEvent(new Event('change'))
    const id = document.querySelector('[data-resume-id]') as HTMLInputElement; id.value = 'history-id'; id.dispatchEvent(new Event('input')); await flushPromises()
    ;(document.querySelector('[data-prepare-resume]') as HTMLButtonElement).click(); await flushPromises()
    ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
    expect(useNativeTabsStore().tabs.size).toBe(1)
    useNativeTabsStore().markUnknown([...useNativeTabsStore().tabs.keys()][0])
  }
  expect(w.find('[data-restored-tab]').exists()).toBe(true)
  expect([...useNativeTabsStore().tabs.values()][0]).toMatchObject({ generation: 1, status: 'unknown', sourceSessionKey: historyKey, action: { kind: 'resume-id', nativeSessionId: 'history-id' } })
})

// 按 ID 确认后项目被同路径的新注册替换，不得启动到替换项目。
it('Resume_DirectRejectsReplacedProject_011', async () => {
  const { useWorkspaceStore } = await import('@/stores/workspace')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'restore-session', project: { projectKey: '/repo', projectPath: '/repo' }, cli: 'codex', mode: 'resume-id' }); await flushPromises()
  const select = document.querySelector('[data-resume-config]') as HTMLSelectElement; select.value = 'cx'; select.dispatchEvent(new Event('change'))
  const id = document.querySelector('[data-resume-id]') as HTMLInputElement; id.value = 'history-id'; id.dispatchEvent(new Event('input')); await flushPromises()
  ;(document.querySelector('[data-prepare-resume]') as HTMLButtonElement).click(); await flushPromises()
  useWorkspaceStore().projects[0].projectId = 'replacement'
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  expect(document.body.textContent).toContain(en.resumeConfigurationChanged); expect(useNativeTabsStore().tabs.size).toBe(0)
})

// 取消后的新显式确认拥有自己的准入权，不继承旧对话框已失效的取消状态。
it('Resume_ReconfirmAfterClose_012', async () => {
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-restored-tab': props.tabId }) } }), SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const request = { kind: 'restore-session' as const, project: { projectKey: '/repo', projectPath: '/repo' }, mode: 'history' as const }
  useShellStore().requestWorkspaceAction(request); await flushPromises()
  ;(document.querySelector('[data-resume-result]') as HTMLButtonElement).click(); await flushPromises()
  let finish!: (value: unknown) => void
  io.read.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  ;(document.querySelector('.resume-actions button') as HTMLButtonElement).click(); await flushPromises()
  expect(document.querySelector('[role=dialog]')).toBeNull()
  useShellStore().requestWorkspaceAction(request); await flushPromises()
  ;(document.querySelector('[data-resume-result]') as HTMLButtonElement).click(); await flushPromises()
  ;(document.querySelector('[data-confirm-resume]') as HTMLButtonElement).click(); await flushPromises()
  finish({ state: 'ready', items: [{ type: 'session', sessionKey: historyKey, nativeSessionId: 'history-id', title: 'Restore this' }], hasMore: false }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(1); expect(document.querySelector('[role=dialog]')).toBeNull()
  expect(w.find('[data-restored-tab]').exists()).toBe(true)
})

// 树中已选历史在两种语言、激活/恢复入口都只检查当前目标，不再次搜索或展示列表。
it.each(['en', 'zh'] as const)('Resume_TargetOnlyBothRoutes_013_%s', async locale => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })], stubs: { NativeCliTerminal: NativeHost, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(); const row = catalog.sessions[0]
  const readsBefore = io.read.mock.calls.length
  for (const kind of ['activate', 'menu-action'] as const) {
    useShellStore().requestWorkspaceAction(kind === 'activate' ? { kind, sessionId: row.id } : { kind, action: 'resume', sessionId: row.id }); await flushPromises()
    expect(document.querySelector('[role=dialog]')).toBeNull()
    expect(catalog.resumeDialog).toBeNull()
    expect(document.querySelector('[data-resume-query]')).toBeNull()
    expect(document.querySelector('[data-confirm-resume]')).toBeNull()
    expect(useNativeTabsStore().tabs.size).toBe(1)
    expect([...useNativeTabsStore().tabs.values()][0]).toMatchObject({
      title: row.title, cli: row.cli, projectPath: row.projectPath,
      profileId: row.nativeOrigin?.profileId, profileRevision: row.nativeOrigin?.profileRevision,
      sourceSessionKey: row.adapterSessionId, action: { kind: 'resume-id', nativeSessionId: row.nativeSessionId },
    })
    expect(io.read.mock.calls.length).toBe(readsBefore + 1)
    expect(w.find('[data-restored-tab]').exists()).toBe(true)
  }
})

// 当前目标重复检查恢复按钮只创建一个尝试，随后再次点击已打开行直接复用。
it('Resume_TargetConfirmOnlyOnce_014', async () => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: defineComponent({ props: ['tabId'], setup(props, { expose }) { expose({ focus() {}, fitVisible() {}, async stop() {}, async recover() {} }); return () => h('div', { 'data-restored-tab': props.tabId }) } }), SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(); const history = catalog.sessions[0]
  const readsBefore = io.read.mock.calls.length
  let finish!: (value: unknown) => void
  io.read.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: history.id }); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'menu-action', action: 'resume', sessionId: history.id }); await flushPromises()
  expect(document.querySelector('[data-confirm-resume]')).toBeNull()
  expect(io.read.mock.calls.length).toBe(readsBefore + 1)
  expect(useNativeTabsStore().tabs.size).toBe(0)
  finish({ state: 'ready', items: [{ type: 'session', sessionKey: historyKey, nativeSessionId: 'history-id', title: 'Restore this', cwd: '/repo' }], hasMore: false }); await flushPromises()
  const tab = [...useNativeTabsStore().tabs.values()][0]
  useNativeTabsStore().markUnknown(tab.tabId); await flushPromises()
  const readsAfter = io.read.mock.calls.length
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: 'native-tab:' + tab.tabId }); await flushPromises()
  expect(document.querySelector('[role=dialog]')).toBeNull()
  expect(useNativeTabsStore().tabs.size).toBe(1)
  expect(catalog.activeSessionId).toBe('native-tab:' + tab.tabId)
  expect(tab.generation).toBe(1)
  expect(tab.status).toBe('unknown')
  expect(io.read.mock.calls.length).toBe(readsAfter)
})

// 当前目标检查时取消或切换项目，迟到的历史读取不准入，保留新页面。
it.each(['navigation', 'project'] as const)('Resume_TargetCancelsLateRead_015_%s', async change => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: NativeHost, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  let finish!: (value: unknown) => void
  io.read.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: catalog.sessions[0].id }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(document.querySelector('[data-confirm-resume]')).toBeNull()
  if (change === 'navigation') useShellStore().navigate('settings')
  else { catalog.selectProjectContext('/other'); useShellStore().navigate('workspace') }
  await flushPromises()
  finish({ state: 'ready', items: [{ type: 'session', sessionKey: historyKey, nativeSessionId: 'history-id', title: 'Restore this', cwd: '/repo' }], hasMore: false }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(document.querySelector('[role=dialog]')).toBeNull()
  if (change === 'navigation') expect(useShellStore().section).toBe('settings')
  else expect(catalog.activeSessionId).toBeNull()
})

// 当前目标保持原项目和配置身份；检查期间修订或注册替换也不能借用新来源启动。
it.each(['configuration', 'project'] as const)('Resume_TargetRetainsOrigin_016_%s', async change => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const { useCliProfilesStore } = await import('@/stores/cliProfiles')
  const { useWorkspaceStore } = await import('@/stores/workspace')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: NativeHost, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(); const history = catalog.sessions[0]
  let finish!: (value: unknown) => void
  io.read.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: history.id }); await flushPromises()
  if (change === 'configuration') useCliProfilesStore().profiles[0].revision = '8'
  else useWorkspaceStore().projects[0].projectId = 'replacement'
  finish({ state: 'ready', items: [{ type: 'session', sessionKey: historyKey, nativeSessionId: 'history-id', title: 'Restore this', cwd: '/repo' }], hasMore: false }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(document.querySelector('[role=dialog]')).toBeNull()
  expect(catalog.actionFeedback?.messageKey).toBe('resumeConfigurationChanged')
  expect(document.body.textContent).not.toContain('/private/secret')
})

// 新目标替代旧请求后，旧读取完成不得启动旧目标或覆盖新目标的选择。
it('Resume_TargetRejectsSuperseded_017', async () => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const secondKey = JSON.stringify(['local', 'codex', 'root', 'second-id'])
  const items = [
    { type: 'session', sessionKey: historyKey, nativeSessionId: 'history-id', title: 'First target', cwd: '/repo' },
    { type: 'session', sessionKey: secondKey, nativeSessionId: 'second-id', title: 'Second target', cwd: '/repo' },
  ]
  io.read.mockResolvedValue({ state: 'ready', items, hasMore: false })
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: NativeHost, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore()
  const first = catalog.sessions.find(row => row.nativeSessionId === 'history-id')!
  const second = catalog.sessions.find(row => row.nativeSessionId === 'second-id')!
  let finish!: (value: unknown) => void
  io.read.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: first.id }); await flushPromises()
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: second.id }); await flushPromises()
  finish({ state: 'ready', items, hasMore: false }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(1)
  const tab = [...useNativeTabsStore().tabs.values()][0]
  expect(tab).toMatchObject({ title: 'Second target', sourceSessionKey: secondKey, action: { kind: 'resume-id', nativeSessionId: 'second-id' } })
  expect(catalog.activeSessionId).toBe('native-tab:' + tab.tabId)
  expect(document.querySelector('[role=dialog]')).toBeNull()
})

// 当前目标已从目录消失时，只给出无法恢复提示，不回退其他会话或直接恢复表单。
it('Resume_TargetMissingStaysClosed_018', async () => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: true, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  useUnifiedSessionsStore().openResumeDialog({ project: { projectKey: '/repo', projectPath: '/repo' }, cli: 'codex', mode: 'session', sessionId: 'missing-target' }); await flushPromises()
  const dialog = document.querySelector('[role=dialog]')!
  expect(dialog.textContent).toContain(en.resumeUnavailable)
  expect(dialog.querySelector('[data-confirm-resume]')).toBeNull()
  expect(dialog.querySelector('[data-resume-query]')).toBeNull()
  expect(dialog.querySelector('[data-resume-config]')).toBeNull()
  expect(useNativeTabsStore().tabs.size).toBe(0)
})

// 直接恢复失败后的 Retry 重新核实原来源，而不是仅刷新目录；仍只准入一次。
it('Resume_TargetRetryKeepsExactSource_019', async () => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: NativeHost, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(), row = catalog.sessions[0]
  io.read.mockRejectedValueOnce({ code: 'SOURCE_UNAVAILABLE', message: '/private/secret TOKEN' })
  useShellStore().requestWorkspaceAction({ kind: 'activate', sessionId: row.id }); await flushPromises()
  expect(catalog.actionFeedback?.retryable).toBe(true)
  expect(document.body.textContent).not.toContain('/private/secret')
  expect(useNativeTabsStore().tabs.size).toBe(0)
  expect(document.querySelector('[role=dialog]')).toBeNull()
  await catalog.refresh()
  expect(catalog.sessions.some(session => session.id === row.id)).toBe(false)
  const retry = [...document.querySelectorAll<HTMLButtonElement>('[data-action-feedback] button')].find(button => button.textContent === en.retry)!
  expect(retry).toBeDefined()
  retry.click(); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(1)
  expect([...useNativeTabsStore().tabs.values()][0]).toMatchObject({
    projectPath: row.projectPath, profileId: row.nativeOrigin?.profileId, profileRevision: row.nativeOrigin?.profileRevision,
    sourceSessionKey: row.adapterSessionId, action: { kind: 'resume-id', nativeSessionId: row.nativeSessionId },
  })
})

// 消失历史的冻结 Retry 也必须服从当前配置、导航和替代请求，不能迟到准入。
it.each(['configuration', 'navigation', 'replacement'] as const)('Resume_TargetRetryRechecksAdmission_020_%s', async change => {
  const { useUnifiedSessionsStore } = await import('@/stores/unifiedSessions')
  const { useCliProfilesStore } = await import('@/stores/cliProfiles')
  const w = mount(App, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { NativeCliTerminal: NativeHost, SettingsView: true } } }); wrappers.push(w); await flushPromises()
  const catalog = useUnifiedSessionsStore(), shell = useShellStore(), row = catalog.sessions[0]
  io.read.mockRejectedValueOnce({ code: 'SOURCE_UNAVAILABLE' })
  shell.requestWorkspaceAction({ kind: 'activate', sessionId: row.id }); await flushPromises()
  await catalog.refresh()
  expect(catalog.sessions.some(session => session.id === row.id)).toBe(false)
  let finish!: (value: unknown) => void
  io.read.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  const retry = [...document.querySelectorAll<HTMLButtonElement>('[data-action-feedback] button')].find(button => button.textContent === en.retry)!
  retry.click(); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  if (change === 'configuration') useCliProfilesStore().profiles[0].revision = '8'
  else if (change === 'navigation') shell.navigate('settings')
  else shell.requestWorkspaceAction({ kind: 'restore-session', project: row, mode: 'history' })
  await flushPromises()
  finish({ state: 'ready', items: [{ type: 'session', sessionKey: historyKey, nativeSessionId: 'history-id', title: 'Restore this', cwd: '/repo' }], hasMore: false }); await flushPromises()
  expect(useNativeTabsStore().tabs.size).toBe(0)
  if (change === 'configuration') expect(catalog.actionFeedback?.messageKey).toBe('resumeConfigurationChanged')
  else expect(catalog.actionFeedback).toBeNull()
  if (change === 'replacement') expect(catalog.resumeDialog?.mode).toBe('history')
})
