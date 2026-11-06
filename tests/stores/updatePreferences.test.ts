import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useUpdateStore } from '@/stores/update'
import { useSidebarStore } from '@/stores/sidebar'
import type { UpdateInfo } from '@/types'
const io = vi.hoisted(() => ({ check: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), checkForUpdates: io.check }))
const official = (version = '0.99.0'): UpdateInfo => ({ version, currentVersion: '0.26.0', hasUpdate: true, releaseNotes: 'SECRET note', downloadUrl: '', platformAsset: null,
  selectedChannel: 'stable', channel: 'stable', installEligible: true, admissionId: '00000000-0000-4000-8000-000000000001',
  officialRelease: { id: version === '0.99.0' ? 1 : 2, tag: `v${version}`, sourceSha: 'a'.repeat(40) } })
beforeEach(() => { localStorage.clear(); setActivePinia(createPinia()); vi.clearAllMocks() })
afterEach(() => { vi.restoreAllMocks(); localStorage.clear() })
describe('local update preferences', () => {
  it('UpdateIgnore_OnlyExactCandidateAndFutureVersionsStayVisible_001', () => {
    const store = useUpdateStore(); store.setUpdateInfo(official())
    expect(store.ignoreCurrentCandidate()).toBe(true)
    expect(store.isCandidateIgnored).toBe(true); expect(store.hasUpdate).toBe(false); expect(useSidebarStore().updateAvailable).toBe(false)
    store.setUpdateInfo(official('0.100.0'))
    expect(store.hasUpdate).toBe(true); expect(store.isCandidateIgnored).toBe(false); expect(useSidebarStore().updateAvailable).toBe(true)
    setActivePinia(createPinia()); const reloaded = useUpdateStore(); reloaded.setUpdateInfo(official())
    expect(reloaded.isCandidateIgnored).toBe(true)
    expect(reloaded.clearIgnoredCandidate()).toBe(true)
    expect(reloaded.updateInfo?.admissionId).toBeNull(); expect(reloaded.hasUpdate).toBe(true)
  })
  it('UpdateIgnore_UnverifiedAndBusyCannotBeIgnored_002', () => {
    const store = useUpdateStore(); store.setUpdateInfo({ ...official(), installEligible: false, admissionId: null })
    expect(store.ignoreCurrentCandidate()).toBe(false)
    store.setUpdateInfo(official()); store.setDownloadState('downloading')
    expect(store.ignoreCurrentCandidate()).toBe(false)
  })
  it('UpdateHistory_PersistsOnlyBoundedSafeEvents_003', async () => {
    const store = useUpdateStore(); io.check.mockResolvedValue(official()); await store.checkForUpdate()
    store.recordFailure({ code: 'UPDATER_SIGNATURE_INVALID', stage: 'verify', message: 'TOKEN /PRIVATE_PATH', raw: 'SECRET' })
    for (let index = 0; index < 25; index++) store.setDownloadState(index % 2 ? 'downloading' : 'verifying')
    expect(store.history.length).toBe(20)
    const text = localStorage.getItem('cc-desk-update-preferences-v1')!
    expect(text).not.toMatch(/TOKEN|PRIVATE_PATH|SECRET|admissionId|sourceSha|downloadUrl|releaseNotes/)
    setActivePinia(createPinia()); expect(useUpdateStore().history).toHaveLength(20)
    useUpdateStore().clearHistory(); expect(useUpdateStore().history).toHaveLength(0)
  })
  it('UpdateHistory_CorruptStorageDoesNotBlockChecking_004', async () => {
    localStorage.setItem('cc-desk-update-preferences-v1', JSON.stringify({ schema: 1, ignored: { version: 'TOKEN' }, history: [{ event: 'SECRET', version: '/PRIVATE_PATH' }] }))
    const store = useUpdateStore(); expect(store.history).toEqual([]); expect(store.isCandidateIgnored).toBe(false)
    expect(store.preferencesPersistent).toBe(false)
    io.check.mockResolvedValue(official()); await store.checkForUpdate(); expect(store.hasUpdate).toBe(true)
    expect(store.preferencesPersistent).toBe(false)
  })
  it('UpdatePreferences_StorageFailureIsVisibleAndMemoryRemainsUsable_005', () => {
    const store = useUpdateStore(); store.setUpdateInfo(official())
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('SECRET /private') })
    expect(store.ignoreCurrentCandidate()).toBe(true)
    expect(store.preferencesPersistent).toBe(false); expect(store.isCandidateIgnored).toBe(true)
    store.setUpdateInfo(official('0.100.0')); expect(store.hasUpdate).toBe(true)
  })
})
