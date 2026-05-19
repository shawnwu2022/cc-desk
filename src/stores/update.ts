import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { UpdateInfo, DownloadProgress, ClaudeCliUpdateInfo } from '@/types'

export type DownloadState = 'idle' | 'downloading' | 'installing' | 'error'

export const useUpdateStore = defineStore('update', () => {
  // 软件更新状态
  const updateInfo = ref<UpdateInfo | null>(null)
  const downloadState = ref<DownloadState>('idle')
  const downloadProgress = ref<DownloadProgress>({ downloaded: 0, total: 0, percent: 0 })
  const downloadError = ref('')

  // Claude CLI 更新状态
  const claudeCliUpdateInfo = ref<ClaudeCliUpdateInfo | null>(null)
  const claudeCliDownloadState = ref<DownloadState>('idle')
  const claudeCliDownloadProgress = ref(0)
  const claudeCliDownloadMessage = ref('')
  const claudeCliDownloadError = ref('')

  // 软件更新函数
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

  function resetDownload() {
    downloadState.value = 'idle'
    downloadProgress.value = { downloaded: 0, total: 0, percent: 0 }
    downloadError.value = ''
  }

  function clearError() {
    downloadError.value = ''
  }

  // Claude CLI 更新函数
  function setClaudeCliUpdateInfo(info: ClaudeCliUpdateInfo | null) {
    claudeCliUpdateInfo.value = info
  }

  function setClaudeCliDownloadState(state: DownloadState) {
    claudeCliDownloadState.value = state
  }

  function setClaudeCliDownloadProgress(progress: number, message: string) {
    claudeCliDownloadProgress.value = progress
    claudeCliDownloadMessage.value = message
  }

  function setClaudeCliDownloadError(error: string) {
    claudeCliDownloadError.value = error
  }

  function resetClaudeCliDownload() {
    claudeCliDownloadState.value = 'idle'
    claudeCliDownloadProgress.value = 0
    claudeCliDownloadMessage.value = ''
    claudeCliDownloadError.value = ''
  }

  return {
    updateInfo,
    downloadState,
    downloadProgress,
    downloadError,
    setUpdateInfo,
    setDownloadState,
    setDownloadProgress,
    setDownloadError,
    resetDownload,
    clearError,
    claudeCliUpdateInfo,
    claudeCliDownloadState,
    claudeCliDownloadProgress,
    claudeCliDownloadMessage,
    claudeCliDownloadError,
    setClaudeCliUpdateInfo,
    setClaudeCliDownloadState,
    setClaudeCliDownloadProgress,
    setClaudeCliDownloadError,
    resetClaudeCliDownload,
  }
})
