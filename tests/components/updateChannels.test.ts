import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import UpdateSection from '@/components/settings/sections/UpdateSection.vue'
import { useUpdateStore } from '@/stores/update'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import type { UpdateInfo } from '@/types'

type Progress = { payload: { admissionId: string; phase: string; downloaded: number; total: number } }
const io = vi.hoisted(() => ({ settings: vi.fn(), save: vi.fn(), install: vi.fn(), cancel: vi.fn(), progress: null as null | ((event: Progress) => void) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (_event, handler) => { io.progress = handler; return vi.fn() }) }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), getUpdaterSettings: io.settings, saveUpdaterChannel: io.save,
  installDesktopUpdate: io.install, cancelDesktopUpdate: io.cancel }))
let wrapper: VueWrapper | undefined
const official = (): UpdateInfo => ({ version: '0.27.0-rc.1', currentVersion: '0.26.0', hasUpdate: true, releaseNotes: 'Test notes', downloadUrl: '', platformAsset: null,
  selectedChannel: 'test', channel: 'rc', installEligible: true, admissionId: '00000000-0000-4000-8000-000000000001',
  officialRelease: { id: 123, tag: 'v0.27.0-rc.1', sourceSha: 'a'.repeat(40) } })
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); io.progress = null
  io.settings.mockResolvedValue({ proxy: null, channel: 'stable' }); io.save.mockResolvedValue(undefined)
  mockIPC(() => undefined)
})
afterEach(() => { wrapper?.unmount(); wrapper = undefined; clearMocks(); document.body.innerHTML = '' })
async function render() {
  wrapper = mount(UpdateSection, { attachTo: document.body, props: { active: true }, global: {
    plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })], stubs: { HistoricalVersionsPanel: true },
  } })
  await flushPromises()
  return wrapper
}

describe('update channel and lifecycle interaction', () => {
  it('UpdateRecovery_ManualGuideHasNoInstallationEffects_005', async () => {
    const view = await render()
    await view.get('[data-update-recovery-guide]').trigger('click'); await flushPromises()
    const dialog = document.querySelector('[data-update-recovery-dialog]')
    expect(dialog?.textContent).toContain('not rolled back')
    expect(dialog?.textContent).toContain('no one-click return')
    expect(io.install).not.toHaveBeenCalled(); expect(io.save).not.toHaveBeenCalled()
    await view.setProps({ active: false }); await view.setProps({ active: true }); await flushPromises()
    expect(document.querySelector('[data-update-recovery-dialog]')).toBeNull()
  })
  it('UpdateChannel_PersistsExplicitChoiceAndRevokesOldReceipt_001', async () => {
    const view = await render(), updates = useUpdateStore()
    updates.setUpdateInfo({ ...official(), selectedChannel: 'preview' }); await flushPromises()
    await view.get('[data-update-channel-select]').setValue('stable'); await flushPromises()
    expect(io.save).toHaveBeenCalledExactlyOnceWith('stable')
    expect(updates.selectedChannel).toBe('stable'); expect(updates.updateInfo).toBeNull()
    expect(view.get('[data-update-channel-risk]').text()).toContain('never automatically downgrades')
    expect(io.install).not.toHaveBeenCalled()
  })
  it('UpdateChannel_SaveFailurePreservesSelectionWithoutAdmission_002', async () => {
    const view = await render(), updates = useUpdateStore()
    updates.setUpdateInfo({ ...official(), selectedChannel: 'test' })
    io.save.mockRejectedValue({ code: 'UPDATER_SETTINGS_SAVE_FAILED', stage: 'configuration', raw: 'TOKEN' })
    await view.get('[data-update-channel-select]').setValue('preview'); await flushPromises()
    expect(updates.selectedChannel).toBe('test'); expect(updates.updateInfo).toBeNull()
    expect(view.text()).not.toContain('TOKEN'); expect(view.find('[data-update-error]').exists()).toBe(true)
    expect(view.get('[data-update-check]').attributes('disabled')).toBeDefined()
    io.settings.mockResolvedValue({ proxy: null, channel: 'preview' })
    await view.get('[data-update-reload-settings]').trigger('click'); await flushPromises()
    expect(updates.selectedChannel).toBe('preview')
    expect(view.get('[data-update-check]').attributes('disabled')).toBeUndefined()
    expect(io.save).toHaveBeenCalledTimes(1)
  })
  it('UpdateLifecycle_CancelWaitsForHostAndRejectsStaleProgress_003', async () => {
    io.settings.mockResolvedValue({ proxy: null, channel: 'test' })
    const view = await render(), updates = useUpdateStore(), info = official()
    updates.setUpdateInfo(info); await flushPromises()
    let fail!: (cause: unknown) => void
    io.install.mockReturnValue(new Promise((_, reject) => { fail = reject }))
    io.cancel.mockResolvedValue({ status: 'requested' })
    await view.get('[data-update-install]').trigger('click'); await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-update-confirm-install]')!.click()
    document.querySelector<HTMLButtonElement>('[data-update-confirm-install]')!.click(); await flushPromises()
    expect(io.install).toHaveBeenCalledTimes(1)
    expect(document.querySelector('[data-update-confirmation]')).toBeNull()
    io.progress?.({ payload: { admissionId: 'stale', phase: 'installing', downloaded: 100, total: 100 } })
    expect(updates.downloadState).toBe('downloading')
    io.progress?.({ payload: { admissionId: info.admissionId!, phase: 'verifying', downloaded: 100, total: 100 } })
    await flushPromises(); expect(view.get('[data-update-progress]').text()).toContain('signature')
    await view.get('[data-update-cancel]').trigger('click'); await flushPromises()
    expect(io.cancel).toHaveBeenCalledExactlyOnceWith(info.admissionId)
    expect(updates.downloadState).toBe('verifying')
    expect(view.get('[data-update-cancel-status]').text()).toContain('Waiting for confirmation')
    fail({ code: 'UPDATER_CANCELLED', stage: 'download' }); await flushPromises()
    expect(updates.downloadState).toBe('cancelled'); expect(updates.updateInfo?.admissionId).toBeNull()
    expect(view.get('[data-update-progress]').text()).toContain('installation is unchanged')
  })
  it('UpdateLifecycle_InstallBoundaryClosesCancellation_004', async () => {
    const view = await render(), updates = useUpdateStore(), info = official()
    updates.setUpdateInfo(info); updates.setDownloadState('downloading'); await flushPromises()
    io.progress?.({ payload: { admissionId: info.admissionId!, phase: 'installing', downloaded: 100, total: 100 } })
    io.progress?.({ payload: { admissionId: info.admissionId!, phase: 'downloading', downloaded: 1, total: 100 } })
    await flushPromises()
    expect(updates.downloadState).toBe('installing'); expect(view.find('[data-update-cancel]').exists()).toBe(false)
    expect(io.cancel).not.toHaveBeenCalled()
  })
  it('UpdateLifecycle_RemountDuringDownloadReloadsAfterFailure_006', async () => {
    const view = await render(), updates = useUpdateStore()
    updates.setUpdateInfo(official()); updates.setDownloadState('downloading')
    view.unmount(); wrapper = undefined
    const next = await render()
    expect(next.get('[data-update-check]').attributes('disabled')).toBeDefined()
    updates.recordFailure({ code: 'UPDATER_REQUEST_FAILED', stage: 'download', raw: 'TOKEN' })
    updates.setDownloadState('error'); await flushPromises()
    expect(next.get('[data-update-check]').attributes('disabled')).toBeUndefined()
    expect(next.get('[data-update-error]').text()).toContain('UPDATER_REQUEST_FAILED')
    expect(next.text()).not.toContain('TOKEN')
  })
  it('UpdateSettings_UnknownSaveBlocksBackgroundUntilReadback_007', async () => {
    const view = await render(), updates = useUpdateStore()
    let checks = 0
    mockIPC(command => { if (command === 'check_desktop_update') { ++checks; return official() } })
    io.save.mockRejectedValue({ code: 'UPDATER_SETTINGS_SAVE_FAILED', stage: 'configuration' })
    await view.get('[data-update-channel-select]').setValue('test'); await flushPromises()
    expect(updates.canCheck).toBe(false)
    await updates.checkForUpdate()
    expect(checks).toBe(0); expect(updates.updateInfo).toBeNull()
    view.unmount(); wrapper = undefined
    expect(updates.canCheck).toBe(false)
    io.settings.mockResolvedValue({ proxy: null, channel: 'test' })
    await render()
    expect(updates.canCheck).toBe(true); expect(updates.selectedChannel).toBe('test')
    expect(io.save).toHaveBeenCalledTimes(1)
  })
  it('UpdateCancellation_LateOldReceiptCannotHideNewCancel_008', async () => {
    io.settings.mockResolvedValue({ proxy: null, channel: 'test' })
    const view = await render(), updates = useUpdateStore()
    let failOld!: (cause: unknown) => void, failNew!: (cause: unknown) => void
    let finishOldCancel!: (receipt: { status: 'requested' }) => void
    io.install.mockReturnValueOnce(new Promise((_, reject) => { failOld = reject }))
      .mockReturnValueOnce(new Promise((_, reject) => { failNew = reject }))
    io.cancel.mockReturnValueOnce(new Promise(resolve => { finishOldCancel = resolve }))
      .mockResolvedValueOnce({ status: 'requested' })
    updates.setUpdateInfo(official()); await flushPromises()
    await view.get('[data-update-install]').trigger('click'); await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-update-confirm-install]')!.click(); await flushPromises()
    await view.get('[data-update-cancel]').trigger('click'); await flushPromises()
    failOld({ code: 'UPDATER_REQUEST_FAILED', stage: 'download' }); await flushPromises()
    const nextId = '00000000-0000-4000-8000-000000000002'
    updates.setUpdateInfo({ ...official(), admissionId: nextId }); await flushPromises()
    await view.get('[data-update-install]').trigger('click'); await flushPromises()
    document.querySelector<HTMLButtonElement>('[data-update-confirm-install]')!.click(); await flushPromises()
    finishOldCancel({ status: 'requested' }); await flushPromises()
    expect(view.find('[data-update-cancel]').exists()).toBe(true)
    await view.get('[data-update-cancel]').trigger('click'); await flushPromises()
    expect(io.cancel).toHaveBeenLastCalledWith(nextId)
    failNew({ code: 'UPDATER_CANCELLED', stage: 'download' }); await flushPromises()
  })
})
