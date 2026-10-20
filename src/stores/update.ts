import { isOrdinaryUpdateEligible } from '@/utils/updatePolicy'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { DownloadProgress, UpdateInfo } from '@/types'
import { checkForUpdates } from '@/api/tauri'
import { useSidebarStore } from '@/stores/sidebar'

export type DownloadState = 'idle' | 'downloading' | 'installing' | 'error'

export const useUpdateStore = defineStore('update', () => {
  const updateInfo = ref<UpdateInfo | null>(null)
  const confirmedUpdate = ref<UpdateInfo | null>(null)
  const checking = ref(false)
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

  const hasUpdate = computed(() => confirmedUpdate.value !== null)
  const availableVersion = computed(() => confirmedUpdate.value?.version ?? null)
  const canCheck = computed(() => suspendedChecks.value === 0 && !['downloading', 'installing'].includes(downloadState.value))

  function setUpdateInfo(info: UpdateInfo | null) {
    updateInfo.value = info
    confirmedUpdate.value = isOrdinaryUpdateEligible(info) ? info : null
    // 兼容设置页导航的纯展示状态；安装始终只使用当前 updateInfo 的后端准入。
    useSidebarStore().setUpdateInfo(confirmedUpdate.value ?? info)
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
    // 后端每次检查都会退休旧安装准入，版本提醒不因此消失。
    revokeInstallAdmission()
    const promise = Promise.resolve().then(checkForUpdates).then(info => {
      if (revision === checkRevision && [...owners].some(owns => owns())) setUpdateInfo(info)
      return info
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
    downloadState.value = state
  }

  function setDownloadProgress(progress: DownloadProgress) {
    downloadProgress.value = progress
  }

  function setDownloadError(error: string) {
    downloadError.value = error
  }

  function clearError() {
    downloadError.value = ''
  }

  function resetDownload() {
    downloadState.value = 'idle'
    downloadProgress.value = { downloaded: 0, total: 0, percent: 0 }
    downloadError.value = ''
  }

  return {
    updateInfo,
    downloadState,
    downloadProgress,
    downloadError,
    hasUpdate,
    availableVersion,
    checking,
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
