import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { UpdateInfo, DownloadProgress, ClaudeCliUpdateInfo, ClaudeVersionEntry } from '@/types'
import { listClaudeVersions } from '@/api/tauri'

export type DownloadState = 'idle' | 'downloading' | 'installing' | 'done' | 'error'

export type HistoryDownloadState =
  | 'idle'
  | 'downloading'
  | 'installing'
  | 'done'
  | 'error'
  | 'cancelled'

export interface HistoryDownloadProgress {
  version: string
  progress: number
  message: string
  state: HistoryDownloadState
  savedPath: string
  error: string
}

export const useUpdateStore = defineStore('update', () => {
  // 软件更新状态
  const updateInfo = ref<UpdateInfo | null>(null)
  const downloadState = ref<DownloadState>('idle')
  const downloadProgress = ref<DownloadProgress>({ downloaded: 0, total: 0, percent: 0 })
  const downloadError = ref('')

  // Claude CLI 更新状态（保留兼容）
  const claudeCliUpdateInfo = ref<ClaudeCliUpdateInfo | null>(null)
  const claudeCliDownloadState = ref<DownloadState>('idle')
  const claudeCliDownloadProgress = ref(0)
  const claudeCliDownloadMessage = ref('')
  const claudeCliDownloadError = ref('')

  // 本地已安装版本（启动时读取，无 HTTP）
  const installedClaudeVersion = ref<string | null>(null)

  // Claude CLI 历史版本列表
  const claudeVersionList = ref<ClaudeVersionEntry[]>([])
  const claudeVersionLatest = ref('')
  const claudeVersionListLoading = ref(false)
  const claudeVersionListError = ref('')
  const claudeVersionListLoadedAt = ref(0)

  // 历史版本下载状态（按版本号区分）
  const historyDownloads = ref<Map<string, HistoryDownloadProgress>>(new Map())

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

  // 本地版本
  function setInstalledClaudeVersion(version: string | null) {
    installedClaudeVersion.value = version
  }

  // 历史版本列表
  async function loadClaudeVersionList(force = false) {
    // 5 分钟内不重复拉取（除非 force）
    const now = Date.now()
    if (!force && claudeVersionListLoadedAt.value && now - claudeVersionListLoadedAt.value < 5 * 60 * 1000 && claudeVersionList.value.length > 0) {
      return
    }
    claudeVersionListLoading.value = true
    claudeVersionListError.value = ''
    try {
      const data = await listClaudeVersions()
      claudeVersionList.value = data.versions || []
      claudeVersionLatest.value = data.latest || ''
      claudeVersionListLoadedAt.value = now
    } catch (err) {
      claudeVersionListError.value = String(err)
      claudeVersionList.value = []
    } finally {
      claudeVersionListLoading.value = false
    }
  }

  function setHistoryDownload(version: string, patch: Partial<HistoryDownloadProgress>) {
    const existing = historyDownloads.value.get(version) || {
      version,
      progress: 0,
      message: '',
      state: 'idle' as HistoryDownloadState,
      savedPath: '',
      error: '',
    }
    historyDownloads.value.set(version, { ...existing, ...patch })
    // 触发响应式（Map 替换）
    historyDownloads.value = new Map(historyDownloads.value)
  }

  function getHistoryDownload(version: string): HistoryDownloadProgress | undefined {
    return historyDownloads.value.get(version)
  }

  function resetHistoryDownload(version: string) {
    historyDownloads.value.delete(version)
    historyDownloads.value = new Map(historyDownloads.value)
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
    installedClaudeVersion,
    setInstalledClaudeVersion,
    claudeVersionList,
    claudeVersionLatest,
    claudeVersionListLoading,
    claudeVersionListError,
    loadClaudeVersionList,
    historyDownloads,
    setHistoryDownload,
    getHistoryDownload,
    resetHistoryDownload,
  }
})
