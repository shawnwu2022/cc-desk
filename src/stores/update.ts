import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { DownloadProgress, UpdateInfo } from '@/types'

export type DownloadState = 'idle' | 'downloading' | 'installing' | 'error'

export const useUpdateStore = defineStore('update', () => {
  const updateInfo = ref<UpdateInfo | null>(null)
  const downloadState = ref<DownloadState>('idle')
  const downloadProgress = ref<DownloadProgress>({
    downloaded: 0,
    total: 0,
    percent: 0,
  })
  const downloadError = ref('')

  const hasUpdate = computed(() => updateInfo.value?.hasUpdate ?? false)

  function setUpdateInfo(info: UpdateInfo | null) {
    updateInfo.value = info
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
    setUpdateInfo,
    setDownloadState,
    setDownloadProgress,
    setDownloadError,
    clearError,
    resetDownload,
  }
})
