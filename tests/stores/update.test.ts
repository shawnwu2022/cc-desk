import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, test, vi } from 'vitest'
import { useUpdateStore } from '@/stores/update'
import { useSidebarStore } from '@/stores/sidebar'
import type { UpdateInfo } from '@/types'

const io = vi.hoisted(() => ({ check: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), checkForUpdates: io.check }))
const official = (): UpdateInfo => ({ version: '0.99.0', currentVersion: '0.23.4', hasUpdate: true, releaseNotes: 'Release notes', downloadUrl: '', platformAsset: null,
  channel: 'stable', installEligible: true, admissionId: '00000000-0000-4000-8000-000000000001', officialRelease: { id: 1, tag: 'v0.99.0', sourceSha: 'a'.repeat(40) } })

describe('update store', () => {
  beforeEach(() => { setActivePinia(createPinia()); io.check.mockReset() })

  // 手动和后台共享一次真实检查，只有仍存活的调用方能够发布结果。
  test('Update_CheckSingleFlight_001', async () => {
    let finish!: (info: UpdateInfo) => void, backgroundAlive = true
    io.check.mockImplementation(() => new Promise<UpdateInfo>(resolve => { finish = resolve }))
    const store = useUpdateStore()
    const background = store.checkForUpdate(() => backgroundAlive)
    const manual = store.checkForUpdate(() => true)
    await Promise.resolve()
    expect(io.check).toHaveBeenCalledTimes(1)
    expect(store.checking).toBe(true)
    backgroundAlive = false; finish(official()); await Promise.all([background, manual])
    expect(store.availableVersion).toBe('0.99.0')
    expect(store.hasUpdate).toBe(true)
    expect(store.checking).toBe(false)
  })

  // 网络失败不抹掉已经验证的版本提醒，但旧安装准入已由后端清除，不能继续展示为可安装。
  test('Update_FailedCheckRetainsNotice_002', async () => {
    const store = useUpdateStore(); store.setUpdateInfo(official())
    io.check.mockRejectedValue({ code: 'UPDATER_TIMEOUT', stage: 'check' })
    await expect(store.checkForUpdate()).rejects.toMatchObject({ code: 'UPDATER_TIMEOUT' })
    expect(store.hasUpdate).toBe(true)
    expect(store.availableVersion).toBe('0.99.0')
    expect(store.updateInfo).toMatchObject({ version: '0.99.0', installEligible: false, admissionId: null })
    io.check.mockResolvedValue({ ...official(), hasUpdate: false, installEligible: false, admissionId: null })
    await store.checkForUpdate()
    expect(store.hasUpdate).toBe(false)
  })

  // 成功返回的负面分类是新证据；同一版本号不能继承先前的官方确认。
  test.each(['candidate', 'test-only', 'unverified'] as const)('Update_NegativeReceiptRevokesNotice_005 %s', async channel => {
    const store = useUpdateStore(); store.setUpdateInfo(official())
    io.check.mockResolvedValue({ ...official(), channel, installEligible: false, admissionId: null, eligibilityReason: 'UPDATER_SIGNATURE_INVALID' })
    await store.checkForUpdate()
    expect(store.updateInfo?.channel).toBe(channel)
    expect(store.hasUpdate).toBe(false)
    expect(store.availableVersion).toBeNull()
    expect(useSidebarStore().updateAvailable).toBe(false)
  })

  test('Update_DisposedCheckCannotPublish_003', async () => {
    let finish!: (info: UpdateInfo) => void, alive = true
    io.check.mockImplementation(() => new Promise<UpdateInfo>(resolve => { finish = resolve }))
    const store = useUpdateStore(), pending = store.checkForUpdate(() => alive)
    await Promise.resolve(); alive = false; finish(official()); await pending
    expect(store.updateInfo).toBeNull()
    expect(store.hasUpdate).toBe(false)
  })

  test('Update_ProxyChangeFencesCheck_004', async () => {
    let finish!: (info: UpdateInfo) => void
    io.check.mockImplementation(() => new Promise<UpdateInfo>(resolve => { finish = resolve }))
    const store = useUpdateStore(), pending = store.checkForUpdate()
    await Promise.resolve()
    const resume = store.suspendChecks(); store.setUpdateInfo(null)
    finish(official()); await pending; resume()
    expect(store.updateInfo).toBeNull()
    store.setDownloadState('downloading')
    await store.checkForUpdate()
    expect(io.check).toHaveBeenCalledTimes(1)
  })

  test('tracks only CC Desk update download state', () => {
    const store = useUpdateStore()
    store.setDownloadState('downloading')
    store.setDownloadProgress({ downloaded: 25, total: 100, percent: 25 })

    expect(store.downloadState).toBe('downloading')
    expect(store.downloadProgress.percent).toBe(25)
    expect('claudeVersionList' in store).toBe(false)
  })

  test('resets transient update state', () => {
    const store = useUpdateStore()
    store.setDownloadState('error')
    store.setDownloadError('failed')

    store.resetDownload()

    expect(store.downloadState).toBe('idle')
    expect(store.downloadError).toBe('')
    expect(store.downloadProgress).toEqual({ downloaded: 0, total: 0, percent: 0 })
  })
})
