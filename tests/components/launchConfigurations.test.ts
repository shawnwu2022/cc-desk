import { beforeEach, afterEach, it, expect, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import SettingsView from '@/components/settings/SettingsView.vue'
import { useSidebarStore } from '@/stores/sidebar'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import type { CliProfile, ProfilePatch } from '@/types/profile'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isMaximized: async () => false, onResized: async () => () => {} }) }))
const wrappers: VueWrapper[] = []
let rows: CliProfile[], writes: ProfilePatch[], revision: string
beforeEach(() => {
  clearMocks(); localStorage.clear(); setActivePinia(createPinia()); revision = '7'; writes = []
  rows = [{ id: 'cc', cli: 'claude', revision: '7', name: 'Claude work', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'set', value: ['a b', '', '"quoted"'] }, skipPermissions: { mode: 'set', value: false }, observer: { mode: 'set', value: false }, env: { API_TOKEN: { mode: 'set', value: { kind: 'host-ref', name: 'PRIVATE_REFERENCE' } }, FLAG: { mode: 'set', value: { kind: 'literal', value: 'PRIVATE_VALUE', nonSecret: true } }, REMOVED: { mode: 'unset' }, AMBIENT: { mode: 'inherit' } } },
    { id: 'cx', cli: 'codex', revision: '7', name: 'Codex work', launcher: { kind: 'native' }, programPath: { mode: 'inherit' }, defaultArgs: { mode: 'inherit' }, skipPermissions: { mode: 'inherit' }, observer: { mode: 'set', value: false }, env: {} }]
  mockIPC((command, payload) => {
    if (command === 'cli_list_profiles') return { revision, profiles: rows }
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
