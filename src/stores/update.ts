import { isOrdinaryUpdateEligible, isReleaseChannel, type ReleaseChannel } from '@/utils/updatePolicy'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { DownloadProgress, UpdateInfo } from '@/types'
import { checkForUpdates } from '@/api/tauri'
import { useSidebarStore } from '@/stores/sidebar'
import { updateFailure } from '@/utils/updateErrors'
import { readUpdatePreferences, writeUpdatePreferences, safeHistoryEntry, type IgnoredCandidate, type UpdateHistoryEntry } from '@/utils/updatePreferences'

export type DownloadState = 'idle' | 'downloading' | 'verifying' | 'installing' | 'restarting' | 'cancelled' | 'error'

export const useUpdateStore = defineStore('update', () => {
  const preferences = readUpdatePreferences()
  const ignoredCandidate = ref<IgnoredCandidate | null>(preferences.value.ignored)
  const history = ref<UpdateHistoryEntry[]>(preferences.value.history)
  const preferencesPersistent = ref(preferences.persistent)
  const updateInfo = ref<UpdateInfo | null>(null)
  const confirmedUpdate = ref<UpdateInfo | null>(null)
  const checking = ref(false)
  const selectedChannel = ref<ReleaseChannel>('stable')
  const settingsUncertain = ref(false)
  const lastFailure = ref<ReturnType<typeof updateFailure> | null>(null)
  const suspendedChecks = ref(0)
  let checkRevision = 0
  let pendingCheck: { promise: Promise<UpdateInfo | null>; owners: Set<() => boolean> } | undefined
  const downloadState = ref<DownloadState>('idle')
  const downloadProgress = ref<DownloadProgress>({
    downloaded: 0,
    total: 0,
    percent: 0,
  })
  const downloadError = ref('')

  const isCandidateIgnored = computed(() => !!ignoredCandidate.value && !!updateInfo.value
    && ignoredCandidate.value.version === updateInfo.value.version && ignoredCandidate.value.releaseId === updateInfo.value.officialRelease?.id
    && ignoredCandidate.value.sourceSha === updateInfo.value.officialRelease?.sourceSha)
  const hasUpdate = computed(() => confirmedUpdate.value !== null && !isCandidateIgnored.value)
  const availableVersion = computed(() => confirmedUpdate.value?.version ?? null)
  const busy = computed(() => ['downloading', 'verifying', 'installing', 'restarting'].includes(downloadState.value))
  const canCheck = computed(() => suspendedChecks.value === 0 && !busy.value && !settingsUncertain.value)
  function persist() {
    const saved = writeUpdatePreferences({ ignored: ignoredCandidate.value, history: history.value })
    // 读入损坏已丢弃内容；后台写入成功不能悄悄抹去本窗口的丢失提示。
    preferencesPersistent.value = saved && preferences.persistent
  }
  function addHistory(event: UpdateHistoryEntry['event'], failure: unknown = null) {
    const entry = safeHistoryEntry({ at: Date.now(), event, version: updateInfo.value?.version ?? null, channel: selectedChannel.value, failure })
    if (!entry) return
    history.value = [entry, ...history.value].slice(0, 20); persist()
  }
  function syncIndicator() { useSidebarStore().setUpdateInfo(isCandidateIgnored.value ? null : confirmedUpdate.value ?? updateInfo.value) }
  function ignoreCurrentCandidate(): boolean {
    const info = updateInfo.value
    if (busy.value || checking.value || !isOrdinaryUpdateEligible(info) || !info?.officialRelease) return false
    ignoredCandidate.value = { version: info.version, releaseId: info.officialRelease.id, sourceSha: info.officialRelease.sourceSha }
    revokeInstallAdmission(); syncIndicator(); addHistory('ignored')
    return true
  }
  function clearIgnoredCandidate(): boolean {
    if (busy.value || checking.value || !ignoredCandidate.value) return false
    ignoredCandidate.value = null; revokeInstallAdmission(); syncIndicator(); addHistory('ignore-cleared')
    return true
  }
  function clearHistory() { history.value = []; persist() }

  function setSettingsUncertain(value: boolean) {
    if (settingsUncertain.value === value) return
    settingsUncertain.value = value
    if (value) { ++checkRevision; setUpdateInfo(null) }
  }

  function setSelectedChannel(channel: ReleaseChannel) {
    if (selectedChannel.value === channel) return
    ++checkRevision
    selectedChannel.value = channel
    setUpdateInfo(null)
  }

  function setUpdateInfo(info: UpdateInfo | null) {
    if (settingsUncertain.value && info) return
    if (info && isReleaseChannel(info.selectedChannel)) selectedChannel.value = info.selectedChannel
    updateInfo.value = info
    confirmedUpdate.value = isOrdinaryUpdateEligible(info) ? info : null
    // 兼容设置页导航的纯展示状态；安装始终只使用当前 updateInfo 的后端准入。
    syncIndicator()
  }

  function revokeInstallAdmission() {
    if (updateInfo.value) updateInfo.value = { ...updateInfo.value, admissionId: null, installEligible: false }
  }

  function checkForUpdate(current: () => boolean = () => true): Promise<UpdateInfo | null> {
    if (!canCheck.value) return Promise.resolve(null)
    if (pendingCheck) {
      pendingCheck.owners.add(current)
      return pendingCheck.promise
    }
    const revision = checkRevision, owners = new Set([current])
    checking.value = true
    lastFailure.value = null
    // 后端每次检查都会退休旧安装准入，版本提醒不因此消失。
    revokeInstallAdmission()
    const promise = Promise.resolve().then(checkForUpdates).then(info => {
      if (revision === checkRevision && [...owners].some(owns => owns())) {
        setUpdateInfo(info); addHistory(info.hasUpdate ? 'available' : 'no-update')
      }
      return info
    }).catch(cause => {
      if (revision === checkRevision && [...owners].some(owns => owns())) recordFailure(cause)
      throw cause
    }).finally(() => {
      if (pendingCheck?.promise === promise) { pendingCheck = undefined; checking.value = false }
    })
    pendingCheck = { promise, owners }
    return promise
  }

  function suspendChecks(): () => void {
    ++checkRevision; ++suspendedChecks.value
    let resumed = false
    return () => {
      if (resumed) return
      resumed = true; --suspendedChecks.value
    }
  }

  function setDownloadState(state: DownloadState) {
    if (downloadState.value !== state && ['downloading', 'verifying', 'installing', 'restarting', 'cancelled'].includes(state)) {
      addHistory(state as 'downloading' | 'verifying' | 'installing' | 'restarting' | 'cancelled')
    }
    downloadState.value = state
  }

  function setDownloadProgress(progress: DownloadProgress) {
    downloadProgress.value = progress
  }

  function setDownloadError(error: string) {
    downloadError.value = error
  }

  function recordFailure(cause: unknown) { lastFailure.value = updateFailure(cause); if (lastFailure.value.code !== 'UPDATER_CANCELLED') addHistory('failed', cause) }

  function clearError() {
    downloadError.value = ''
  }

  function resetDownload() {
    downloadState.value = 'idle'
    downloadProgress.value = { downloaded: 0, total: 0, percent: 0 }
    downloadError.value = ''
  }

  return {
    ignoredCandidate, isCandidateIgnored, ignoreCurrentCandidate, clearIgnoredCandidate, history, clearHistory, preferencesPersistent,
    updateInfo,
    downloadState,
    downloadProgress,
    downloadError,
    hasUpdate,
    availableVersion,
    checking,
    selectedChannel,
    settingsUncertain,
    setSettingsUncertain,
    lastFailure,
    recordFailure,
    busy,
    setSelectedChannel,
    canCheck,
    checkForUpdate,
    suspendChecks,
    setUpdateInfo,
    revokeInstallAdmission,
    setDownloadState,
    setDownloadProgress,
    setDownloadError,
    clearError,
    resetDownload,
  }
})
