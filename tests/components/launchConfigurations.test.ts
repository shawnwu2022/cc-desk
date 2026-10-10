import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import SettingsView from '@/components/settings/SettingsView.vue'
import { useSidebarStore } from '@/stores/sidebar'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import { useAppStore } from '@/stores/app'
import type { CliProfile, ProfilePatch } from '@/types/profile'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isMaximized: async () => false, onResized: async () => () => {} }) }))
const wrappers: VueWrapper[] = []
let rows: CliProfile[], writes: ProfilePatch[], revision: string
let readProfiles: () => unknown, profileReads: number
beforeEach(() => {
  clearMocks(); localStorage.clear(); setActivePinia(createPinia()); revision = '7'; writes = []
  profileReads = 0; readProfiles = () => ({ revision, profiles: rows })
  rows = [{ id: 'cc', cli: 'claude', revision: '7', name: 'Claude work', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'set', value: ['a b', '', '"quoted"'] }, skipPermissions: { mode: 'set', value: false }, observer: { mode: 'set', value: false }, env: { API_TOKEN: { mode: 'set', value: { kind: 'host-ref', name: 'PRIVATE_REFERENCE' } }, FLAG: { mode: 'set', value: { kind: 'literal', value: 'PRIVATE_VALUE', nonSecret: true } }, REMOVED: { mode: 'unset' }, AMBIENT: { mode: 'inherit' } } },
    { id: 'cx', cli: 'codex', revision: '7', name: 'Codex work', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: false }, env: {} }]
  mockIPC((command, payload) => {
    if (command === 'cli_list_profiles') { profileReads++; return readProfiles() }
    if (command === 'cli_patch_profile') {
      const input = payload as { expectedRevision: string; patch: ProfilePatch }; writes.push(input.patch)
      if (input.expectedRevision !== revision) throw { code: 'REVISION_CONFLICT' }
      revision = String(Number(revision) + 1)
      const patch = input.patch
      if (patch.op === 'create') rows = [...rows, { ...patch.profile, revision }]
      if (patch.op === 'update') rows = rows.map(row => row.id === patch.id ? { ...row, ...patch.changes, revision } : row)
      if (patch.op === 'delete') rows = rows.filter(row => row.id !== patch.id)
      return { revision, profiles: rows }
    }
    if (command === 'get_app_config') return { language: 'en', theme: 'light' }
    if (command === 'cli_list_projects') return { revision, projects: [] }
    if (command === 'cli_register_project') {
      revision = String(Number(revision) + 1)
      return { revision, projectId: 'fresh', projects: [{ projectId: 'fresh', hostId: 'host', sourcePathKey: 'fresh', selectedPath: '/fresh', canonicalPath: '/fresh', alias: { mode: 'inherit' }, pinned: { mode: 'inherit' }, hidden: { mode: 'inherit' } }] }
    }
    throw new Error('unexpected')
  })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); document.body.innerHTML = ''; clearMocks(); vi.restoreAllMocks() })
async function render() {
  useSidebarStore().activeSettingsSection = 'launch-configurations'
  const w = mount(SettingsView, { attachTo: document.body, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })], stubs: { UpdateSection: true, AboutSection: true, ShortcutsSection: true } } })
  wrappers.push(w); await flushPromises(); return w
}
function button(selector: string) { const element = document.querySelector<HTMLButtonElement>(selector); expect(element, selector).not.toBeNull(); return element! }
async function edit(w: VueWrapper, id = 'cc') { await w.get(`[data-launch-row="${id}"] [data-launch-edit]`).trigger('click'); await flushPromises() }
async function menu(w: VueWrapper, id: string, action: string) { await w.get(`[data-launch-row="${id}"] [data-launch-menu]`).trigger('click'); await flushPromises(); button(`[data-item-id="${action}"]`).click(); await flushPromises() }
async function input(selector: string, value: string) { const el = document.querySelector<HTMLInputElement>(selector); expect(el, selector).not.toBeNull(); el!.value = value; el!.dispatchEvent(new Event('input', { bubbles: true })); await flushPromises() }
// 真实设置页按工具分组，并使用现有全局默认偏好显示标记。
it('LaunchSettings_GroupDefault_001', async () => {
  const w = await render()
  expect(w.findAll('[data-launch-group]').map(group => group.attributes('data-launch-group'))).toEqual(['claude', 'codex'])
  expect(w.get('[data-launch-row="cc"]').text()).toContain('Default')
  expect(w.get('[data-launch-row="cx"]').text()).toContain('Default')
  expect(w.get('[data-launch-row="cc"]').findAll('[data-launch-edit]')).toHaveLength(1)
  expect(w.text()).not.toMatch(/Profile|PRIVATE_VALUE|PRIVATE_REFERENCE/)
})
// 参数按行精确保存，空参数/空格/引号保留，环境值与引用名不投影到 DOM。
it('LaunchSettings_ExactArgsAndEnv_002', async () => {
  const w = await render(); await edit(w)
  expect(document.body.innerHTML).not.toMatch(/PRIVATE_VALUE|PRIVATE_REFERENCE/)
  expect(document.body.textContent).toContain('API_TOKEN'); expect(document.body.textContent).toContain('Set')
  await input('[data-launch-argv]', ' spaced \n\n"quoted"')
  expect(writes).toEqual([])
  button('[data-launch-save]').click(); await flushPromises()
  expect(writes).toEqual([{ op: 'update', id: 'cc', changes: expect.objectContaining({ defaultArgs: { mode: 'set', value: [' spaced ', '', '"quoted"'] } }) }])
  expect(rows[0].env.FLAG).toEqual({ mode: 'set', value: { kind: 'literal', value: 'PRIVATE_VALUE', nonSecret: true } })
})
// 取消复杂编辑不发写请求；离开设置分类关闭编辑器。
it('LaunchSettings_CancelAndLeave_003', async () => {
  const w = await render(); await edit(w); await input('[data-launch-name]', 'Discard me')
  button('[data-launch-cancel]').click(); await flushPromises(); expect(writes).toEqual([])
  await edit(w); useSidebarStore().activeSettingsSection = 'general'; await flushPromises()
  expect(document.querySelector('[data-launch-save]')).toBeNull(); expect(writes).toEqual([])
})
// 菜单复制使用新身份，重命名保留旧身份，设置默认复用既有新建偏好。
it('LaunchSettings_CopyRenameDefault_004', async () => {
  const w = await render(); await menu(w, 'cc', 'copy'); await input('[data-launch-name]', 'Copy')
  button('[data-launch-save]').click(); await flushPromises()
  const copy = rows.find(row => row.name === 'Copy')!; expect(copy.id).not.toBe('cc'); expect(copy.env).toEqual(rows[0].env)
  await menu(w, copy.id, 'rename'); await input('[data-launch-name]', 'Renamed'); button('[data-launch-save]').click(); await flushPromises()
  expect(rows.find(row => row.id === copy.id)?.name).toBe('Renamed')
  await menu(w, copy.id, 'default'); expect(useNewSessionDraftStore().preferred({ projectPath: '/fresh' }, 'claude')?.id).toBe(copy.id)
})
// 冲突只重读，不用新的工作区修订自动重复提交或显示原始异常。
it('LaunchSettings_ConflictNoReplay_005', async () => {
  const w = await render(); await edit(w); await input('[data-launch-name]', 'Late')
  revision = '8'; rows = rows.map(row => ({ ...row, revision: '8', name: row.id === 'cc' ? 'External' : row.name }))
  button('[data-launch-save]').click(); await flushPromises()
  expect(writes).toHaveLength(1); expect(useCliProfilesStore().profile('cc')?.name).toBe('External')
  expect(document.body.textContent).toContain(en.errorRevisionConflict)
  expect(button('[data-launch-save]').disabled).toBe(true)
  expect((document.querySelector('[data-launch-name]') as HTMLInputElement).value).toBe('Late')
  button('[data-launch-save]').click(); await flushPromises(); expect(writes).toHaveLength(1)
})
// JSON 才能表达含换行的单个参数；打开编辑器不静默拆分它。
it('LaunchSettings_JsonRoundTrip_006', async () => {
  rows[0].defaultArgs = { mode: 'set', value: ['two\nlines', ''] }
  const w = await render(); await edit(w)
  expect((document.querySelector('[data-launch-argv]') as HTMLTextAreaElement).value).toBe('["two\\nlines",""]')
  button('[data-launch-save]').click(); await flushPromises()
  expect(rows[0].defaultArgs).toEqual({ mode: 'set', value: ['two\nlines', ''] })
})

// 保存已经发出后离开，再打开另一配置；迟到成功不关闭新编辑器或发布旧 Toast。
it('LaunchSettings_StaleSaveOwner_007', async () => {
  const { useNotificationsStore } = await import('@/stores/notifications')
  const w = await render(); await edit(w); await input('[data-launch-name]', 'Old save')
  let release!: (value: unknown) => void
  mockIPC(command => command === 'cli_patch_profile' ? new Promise(resolve => { release = resolve }) : { revision: '7', profiles: rows })
  button('[data-launch-save]').click(); await flushPromises(); button('[data-launch-cancel]').click(); await flushPromises()
  await edit(w, 'cx'); const oldToasts = useNotificationsStore().toasts.length
  release({ revision: '8', profiles: rows.map(row => row.id === 'cc' ? { ...row, name: 'Old save', revision: '8' } : row) }); await flushPromises()
  expect((document.querySelector('[data-launch-name]') as HTMLInputElement).value).toBe('Codex work')
  expect(useNotificationsStore().toasts).toHaveLength(oldToasts)
})
// 容器只是隐藏而不卸载时，也须取消编辑器、菜单和删除确认。
it('LaunchSettings_InactiveCloses_008', async () => {
  const w = await render(); await edit(w)
  await w.setProps({ active: false }); await flushPromises()
  expect(document.querySelector('[data-launch-save]')).toBeNull()
  await w.setProps({ active: true }); await flushPromises(); await menu(w, 'cc', 'delete')
  expect(useCliProfilesStore().deleteConfirmation?.profileId).toBe('cc')
  await w.setProps({ active: false }); expect(useCliProfilesStore().deleteConfirmation).toBeNull()
})
// 单个空参数保持 JSON；不能通过格式切换静默变成零参数。
it('LaunchSettings_EmptyArgUsesJson_009', async () => {
  rows[0].defaultArgs = { mode: 'set', value: [''] }
  const w = await render(); await edit(w)
  const select = document.querySelector('[data-launch-args-format]') as HTMLSelectElement
  select.value = 'lines'; select.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises()
  expect(document.body.textContent).toContain(en.newSessionArgvFormatError)
  expect((document.querySelector('[data-launch-argv]') as HTMLTextAreaElement).value).toBe('[""]')
  button('[data-launch-save]').click(); await flushPromises(); expect(rows[0].defaultArgs).toEqual({ mode: 'set', value: [''] })
})

// bootstrap 的配置缓存仍为0，真实项目注册推进共享CAS后首次新建只写一次。
it('LaunchSettings_ProjectRevision_010', async () => {
  revision = '0'; rows = []
  const profiles = useCliProfilesStore(); await profiles.load()
  await useAppStore().addManagedProject('/fresh')
  expect(profiles.revision).toBe('0'); expect(revision).toBe('1')
  const w = await render()
  await w.get('[data-launch-create]').trigger('click'); await flushPromises()
  await input('[data-launch-name]', 'First configuration')
  button('[data-launch-save]').click(); await flushPromises()
  expect(rows.map(row => row.name)).toEqual(['First configuration'])
  expect(writes).toHaveLength(1); expect(revision).toBe('2')
  expect(document.querySelector('[data-launch-save]')).toBeNull()
})

// 后台项目元数据推进共享CAS时，每次打开都重新读取，不依赖只针对注册的失效标记。
it('LaunchSettings_MetadataRevision_011', async () => {
  const w = await render(); revision = '8'
  await edit(w); await input('[data-launch-name]', 'After metadata')
  button('[data-launch-save]').click(); await flushPromises()
  expect(rows[0].name).toBe('After metadata'); expect(writes).toHaveLength(1)
  expect(revision).toBe('9'); expect(document.querySelector('[data-launch-save]')).toBeNull()
})

// 只读准备未完成前没有可保存的旧快照，重复点击不能发起第二次准备。
it('LaunchSettings_WaitsForRead_012', async () => {
  const w = await render(); revision = '8'
  let release!: (value: unknown) => void
  readProfiles = () => new Promise(resolve => { release = resolve })
  const reads = profileReads
  await w.get('[data-launch-create]').trigger('click'); await flushPromises()
  expect(document.querySelector('[data-launch-save]')).toBeNull()
  expect(button('[data-launch-create]').disabled).toBe(true)
  button('[data-launch-create]').click(); await flushPromises(); expect(profileReads).toBe(reads + 1)
  release({ revision, profiles: rows }); await flushPromises()
  button('[data-launch-save]').click(); await flushPromises()
  expect(writes).toHaveLength(1); expect(revision).toBe('9')
})

// 取消或离开设置使迟到只读结果失去编辑所有权，返回原界面也不能复活旧请求。
it.each(['cancel', 'inactive', 'category'] as const)('LaunchSettings_CancelRead_013_%s', async change => {
  const w = await render()
  let release!: (value: unknown) => void
  readProfiles = () => new Promise(resolve => { release = resolve })
  const reads = profileReads
  await w.get('[data-launch-create]').trigger('click'); await flushPromises()
  expect(profileReads).toBe(reads + 1)
  if (change === 'cancel') button('[data-launch-prepare-cancel]').click()
  else if (change === 'inactive') { await w.setProps({ active: false }); await w.setProps({ active: true }) }
  else { useSidebarStore().activeSettingsSection = 'general'; await flushPromises(); useSidebarStore().activeSettingsSection = 'launch-configurations' }
  await flushPromises(); release({ revision, profiles: rows }); await flushPromises()
  expect(document.querySelector('[data-launch-save]')).toBeNull(); expect(writes).toEqual([])
})

// 准备读取失败显示安全错误，不能退回旧缓存挂载可保存表单。
it('LaunchSettings_ReadFailure_014', async () => {
  const w = await render()
  readProfiles = () => Promise.reject({ code: 'SOURCE_BUSY', message: 'PRIVATE_FAILURE' })
  await w.get('[data-launch-create]').trigger('click'); await flushPromises()
  expect(document.querySelector('[data-launch-save]')).toBeNull()
  expect(w.text()).toContain(en.errorResourceUnavailable); expect(w.text()).toContain(en.retry); expect(w.text()).not.toContain('PRIVATE_FAILURE')
  expect(writes).toEqual([])
})

// 已打开的编辑器不因后台只读刷新而丢失草稿或采用新的CAS修订。
it('LaunchSettings_ReloadKeepsDraft_015', async () => {
  const w = await render(); await edit(w); await input('[data-launch-name]', 'Unsaved local name')
  revision = '8'; rows = rows.map(row => ({ ...row, revision: '8', name: row.id === 'cc' ? 'External' : row.name }))
  await useCliProfilesStore().load(); await flushPromises()
  expect((document.querySelector('[data-launch-name]') as HTMLInputElement).value).toBe('Unsaved local name')
  button('[data-launch-save]').click(); await flushPromises()
  expect(document.body.textContent).toContain(en.errorRevisionConflict)
  expect(writes).toEqual([]); expect(button('[data-launch-save]').disabled).toBe(true)
  expect((document.querySelector('[data-launch-name]') as HTMLInputElement).value).toBe('Unsaved local name')
})

// 加载时触发按钮失焦或菜单项移除后，原对话框仍能将取消焦点还给可见触发器。
it.each(['create', 'rename', 'contextmenu'] as const)('LaunchSettings_AsyncFocus_016_%s', async entry => {
  const w = await render()
  let release!: (value: unknown) => void
  readProfiles = () => new Promise(resolve => { release = resolve })
  const opener = button(entry === 'create' ? '[data-launch-create]' : '[data-launch-row="cc"] [data-launch-menu]')
  opener.focus()
  if (entry === 'contextmenu') await w.get('[data-launch-row="cc"]').trigger('contextmenu')
  else opener.click()
  await flushPromises()
  if (entry !== 'create') { const item = button('[data-item-id="rename"]'); item.focus(); item.click(); await flushPromises() }
  expect(document.querySelector('[data-launch-save]')).toBeNull()
  opener.blur()
  release({ revision, profiles: rows }); await flushPromises()
  button('[data-launch-cancel]').click(); await flushPromises()
  expect(document.activeElement).toBe(opener)
})

// 已有的bootstrap读取仍在途时，不能创建冻结旧revision的编辑器。
it('LaunchSettings_BootstrapLoading_017', async () => {
  const reads: ((value: unknown) => void)[] = []
  readProfiles = () => new Promise(resolve => { reads.push(resolve) })
  const pending = useCliProfilesStore().load()
  await render()
  button('[data-launch-create]').click(); await flushPromises()
  expect(button('[data-launch-create]').disabled).toBe(true)
  expect(document.querySelector('[data-launch-save]')).toBeNull()
  expect(reads).toHaveLength(2)
  reads[0]({ revision: '6', profiles: rows.map(row => ({ ...row, revision: '6' })) }); await pending; await flushPromises()
  expect(document.querySelector('[data-launch-save]')).toBeNull()
  reads[1]({ revision, profiles: rows }); await flushPromises()
  expect(document.querySelector('[data-launch-save]')).not.toBeNull()
})

// 取消后明确新建另一个工具，迟到的旧读取不能关闭或替代新的准备与草稿。
it('LaunchSettings_NewReadOwner_018', async () => {
  const w = await render()
  const reads: ((value: unknown) => void)[] = []
  readProfiles = () => new Promise(resolve => { reads.push(resolve) })
  await w.get('[data-launch-create]').trigger('click'); await flushPromises()
  button('[data-launch-prepare-cancel]').click(); await flushPromises()
  await w.get('[data-launch-group="codex"] [data-launch-create]').trigger('click'); await flushPromises()
  expect(reads).toHaveLength(2)
  reads[1]({ revision: '8', profiles: rows }); await flushPromises()
  await input('[data-launch-name]', 'New Codex draft')
  reads[0]({ revision: '7', profiles: rows }); await flushPromises()
  expect((document.querySelector('[data-launch-name]') as HTMLInputElement).value).toBe('New Codex draft')
  expect(document.body.textContent).toContain('Codex CLI'); expect(writes).toEqual([])
})
