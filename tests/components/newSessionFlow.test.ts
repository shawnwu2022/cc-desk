import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import ProjectNode from '@/components/sessions/ProjectNode.vue'
import NewSessionMenu from '@/components/sessions/NewSessionMenu.vue'
import NewSessionDialog from '@/components/sessions/NewSessionDialog.vue'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import en from '@/i18n/locales/en'
import type { SessionAdapter, UnifiedProjectGroup } from '@/types/unifiedSession'
const project: UnifiedProjectGroup = { projectKey: '/repo', projectPath: '/repo', name: 'Repo', sessions: [], pinned: false, hidden: false, runningCount: 0, needsUserCount: 0, lastActivityAt: 0 }
const wrappers: VueWrapper[] = []
const global = () => ({ plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] })
beforeEach(() => { localStorage.clear(); setActivePinia(createPinia()) })
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); document.body.innerHTML = '' })
describe('New session flow', () => {
  it('NewSession_TwoClickMenu_001', async () => {
    const w = mount(ProjectNode, { props: { project, expanded: true }, global: global(), attachTo: document.body }); wrappers.push(w)
    await w.get('[data-project-quick-action]').trigger('click'); await flushPromises()
    expect(w.emitted('new-session-request')).toBeUndefined()
    expect(document.querySelectorAll('[role=menuitem]')).toHaveLength(4)
    ;(document.querySelector('[data-item-id=codex]') as HTMLButtonElement).click(); await flushPromises()
    expect(w.emitted('new-session-request')?.[0]).toEqual([{ projectKey: '/repo', projectPath: '/repo', intent: 'codex' }])
    expect(document.body.textContent).not.toMatch(/revision|profile|argv/i)
  })
  it('NewSession_DialogUsesExactArguments_002', async () => {
    const draft = useNewSessionDraftStore(); draft.open(project, 'codex')
    const w = mount(NewSessionDialog, { props: { active: true }, global: global(), attachTo: document.body }); wrappers.push(w)
    draft.rawEnabled = true; draft.argvText = 'two words\n\n--literal= x'; await flushPromises()
    ;(document.querySelector('[data-create-session]') as HTMLButtonElement).click(); await flushPromises()
    expect(w.emitted('create')?.[0]?.[0]).toMatchObject({ cli: 'codex', action: { kind: 'raw', argv: ['two words', '', '--literal= x'] } })
    expect(document.body.textContent).not.toMatch(/revision|generation|run id/i)
  })
  it('NewSession_HiddenSurfaceCloses_003', async () => {
    const w = mount(ProjectNode, { props: { project, expanded: true }, global: global(), attachTo: document.body }); wrappers.push(w)
    await w.get('[data-project-quick-action]').trigger('click'); await flushPromises(); await w.setProps({ surfaceActive: false }); await flushPromises()
    expect(document.querySelector('[role=menu]')).toBeNull()
    const draft = useNewSessionDraftStore(); draft.open(project)
    const dialog = mount(NewSessionDialog, { props: { active: true }, global: global(), attachTo: document.body }); wrappers.push(dialog)
    await dialog.setProps({ active: false }); await flushPromises(); expect(document.querySelector('[role=dialog]')).toBeNull()
  })
  it('NewSession_PlaceholderSurvivesPreparationFailure_004', async () => {
    const store = useUnifiedSessionsStore(); let reject!: (reason: unknown) => void
    store.configureCreationPreparer(() => new Promise((_res, fail) => { reject = fail }))
    store.configureAdapters([{ runtime: 'native-cli', listSessions: async () => [] } as unknown as SessionAdapter])
    const creating = store.createSession({ ...project, cli: 'codex' }); const failed = expect(creating).rejects.toThrow('NEW_SESSION_PREPARATION_FAILED')
    expect(store.sessions).toHaveLength(1); expect(store.sessions[0].processState).toBe('starting')
    const id = store.sessions[0].id; await store.refresh(); expect(store.sessions[0].id).toBe(id)
    reject(new Error('secret path')); await failed
    expect(store.sessions[0]).toMatchObject({ id, processState: 'failed', safeErrorCode: 'NEW_SESSION_PREPARATION_FAILED' })
    expect(JSON.stringify(store.sessions)).not.toContain('secret')
  })
  it('NewSession_CancelPreventsLateAdmission_005', async () => {
    const store = useUnifiedSessionsStore(); let finish!: (value: any) => void
    store.configureCreationPreparer(() => new Promise(resolve => { finish = resolve }))
    const createSession = vi.fn(); store.configureAdapters([{ runtime: 'native-cli', createSession, listSessions: async () => [] } as unknown as SessionAdapter])
    const input = { ...project, cli: 'codex' as const }; const creating = store.createSession(input); const failed = expect(creating).rejects.toThrow('NEW_SESSION_CANCELLED')
    await store.stopSession(store.sessions[0].id); finish(input); await failed
    expect(createSession).not.toHaveBeenCalled(); expect(store.sessions[0].processState).toBe('failed')
  })
  it('NewSession_InactiveDialogCannotCaptureFocus_006', async () => {
    useNewSessionDraftStore().open(project)
    const w = mount(NewSessionDialog, { props: { active: false }, global: global(), attachTo: document.body }); wrappers.push(w); await flushPromises()
    expect(document.querySelector('[role=dialog]')).toBeNull()
  })

  it('NewSession_CloseCannotForgetAdmission_007', async () => {
    const store = useUnifiedSessionsStore(); let finish!: (value: any) => void
    store.configureAdapters([{ runtime: 'native-cli', createSession: () => new Promise(resolve => { finish = resolve }), listSessions: async () => [] } as unknown as SessionAdapter])
    const creating = store.createSession({ ...project, cli: 'codex' }); await flushPromises()
    const id = store.sessions[0].id
    await expect(store.closeSession(id)).rejects.toThrow('NEW_SESSION_ADMISSION_IN_PROGRESS')
    finish({ ...store.sessions[0], id: 'admitted' }); await creating
    expect(store.activeSessionId).not.toBe(id)
  })

  it('NewSession_OneUnavailableToolStillAllowsOther_008', async () => {
    const w = mount(NewSessionMenu, { props: { open: true, anchor: { x: 0, y: 0 }, availability: { codex: 'unavailable', claude: 'unknown' } }, global: global(), attachTo: document.body }); wrappers.push(w); await flushPromises()
    expect((document.querySelector('[data-item-id=codex]') as HTMLButtonElement).disabled).toBe(true)
    const claude = document.querySelector('[data-item-id=claude]') as HTMLButtonElement
    expect(claude.disabled).toBe(false); claude.click(); await flushPromises()
    expect(w.emitted('select')).toEqual([['claude']])
  })
  it('NewSession_RawPermissionsDescribeActualSemantics_009', async () => {
    const draft = useNewSessionDraftStore(); draft.open(project); draft.rawEnabled = true
    const w = mount(NewSessionDialog, { global: global(), attachTo: document.body }); wrappers.push(w); await flushPromises()
    expect(document.body.textContent).toContain('Raw arguments control permissions')
    expect(document.body.textContent).not.toContain('Desk’s configured skip-permission flag: disabled')
  })

  it('NewSession_LateInactiveRequestCannotOpenModal_010', async () => {
    const w = mount(NewSessionDialog, { props: { active: false }, global: global(), attachTo: document.body }); wrappers.push(w)
    useNewSessionDraftStore().open(project); await flushPromises()
    expect(document.querySelector('[role=dialog]')).toBeNull()
    await w.setProps({ active: true }); await flushPromises()
    expect(document.querySelector('[role=dialog]')).toBeNull()
  })

  it('NewSession_PermissionSummaryDoesNotPromiseEffectiveChecks_011', async () => {
    const profiles = useCliProfilesStore()
    profiles.profiles = [{ id: 'custom', revision: '1', cli: 'claude', name: 'Custom', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'set', value: ['--dangerously-skip-permissions'] }, skipPermissions: { mode: 'set', value: false }, observer: { mode: 'inherit' }, env: {} }]
    const draft = useNewSessionDraftStore(); draft.open(project); draft.launchConfigId = 'custom'
    const w = mount(NewSessionDialog, { global: global(), attachTo: document.body }); wrappers.push(w); await flushPromises()
    expect(document.body.textContent).not.toContain('Keeps Claude’s standard permission checks')
    expect(document.body.textContent).toContain('Desk’s configured skip-permission flag: disabled')
    expect(document.body.textContent).toContain('Saved arguments and CLI settings can change effective permissions')
    expect(profiles.profiles[0].defaultArgs).toEqual({ mode: 'set', value: ['--dangerously-skip-permissions'] })
    expect(profiles.profiles[0].skipPermissions).toEqual({ mode: 'set', value: false })
  })

  // 创建动作不随表单滚走，仍属于原生表单并且一次点击只提交一次。
  it('NewSession_FooterOwnsForm_012', async () => {
    const draft = useNewSessionDraftStore(); draft.open(project, 'codex')
    const w = mount(NewSessionDialog, { global: global(), attachTo: document.body }); wrappers.push(w); await flushPromises()
    const form = document.querySelector<HTMLFormElement>('.new-session-fields')!
    const button = document.querySelector<HTMLButtonElement>('[data-create-session]')!
    expect(button.closest('.ui-dialog-footer'), 'Create stays outside the scrolling dialog body').not.toBeNull()
    expect(button.form).toBe(form)
    expect(form.contains(button)).toBe(false)
    button.click(); await flushPromises()
    expect(w.emitted('create')).toHaveLength(1)
    expect(draft.visible).toBe(false)
  })

  // 页脚原生 submit 保留参数校验，修正后连续点击不能创建两个会话。
  it('NewSession_InvalidFooterRetry_013', async () => {
    const draft = useNewSessionDraftStore(); draft.open(project, 'codex')
    draft.rawEnabled = true; draft.argvFormat = 'json'; draft.argvText = '['
    const w = mount(NewSessionDialog, { global: global(), attachTo: document.body }); wrappers.push(w); await flushPromises()
    const button = document.querySelector<HTMLButtonElement>('[data-create-session]')!
    button.click(); await flushPromises()
    expect(w.emitted('create')).toBeUndefined()
    expect(draft.visible).toBe(true)
    expect(document.body.textContent).toContain(en.newSessionArgvError)
    draft.argvText = '["two words"]'; await flushPromises()
    button.click(); button.click(); await flushPromises()
    expect(w.emitted('create')).toHaveLength(1)
    expect(w.emitted('create')![0][0]).toMatchObject({ cli: 'codex', action: { kind: 'raw', argv: ['two words'] } })
  })

  // 键盘提交使用相同 form 事件；同步重复提交在关闭前也只发出一次请求。
  it('NewSession_FormSubmitOnce_014', async () => {
    useNewSessionDraftStore().open(project, 'codex')
    const w = mount(NewSessionDialog, { global: global(), attachTo: document.body }); wrappers.push(w); await flushPromises()
    const form = document.querySelector<HTMLFormElement>('.new-session-fields')!
    form.requestSubmit(); form.requestSubmit(); await flushPromises()
    expect(w.emitted('create')).toHaveLength(1)
  })

})
