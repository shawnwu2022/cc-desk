import { describe, it, expect, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useUpdateStore } from '@/stores/update'
import type { UpdateInfo, ClaudeCliUpdateInfo } from '@/types'

// 构造一个标准 UpdateInfo 对象
function makeUpdateInfo(overrides: Partial<UpdateInfo> = {}): UpdateInfo {
  return {
    version: '0.8.0',
    currentVersion: '0.7.0',
    hasUpdate: true,
    releaseNotes: '### Features\n- New feature',
    downloadUrl: '',
    platformAsset: null,
    ...overrides,
  }
}

describe('update store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  describe('setUpdateInfo', () => {
    // 初始状态 updateInfo 为 null
    it('UpdateInfo_Initial_001', () => {
      const store = useUpdateStore()
      expect(store.updateInfo).toBeNull()
    })

    // 设置 UpdateInfo 后可读取
    it('UpdateInfo_Set_001', () => {
      const store = useUpdateStore()
      const info = makeUpdateInfo()
      store.setUpdateInfo(info)
      expect(store.updateInfo).toEqual(info)
    })

    // 设置 null 可清除更新信息
    it('UpdateInfo_Clear_001', () => {
      const store = useUpdateStore()
      store.setUpdateInfo(makeUpdateInfo())
      store.setUpdateInfo(null)
      expect(store.updateInfo).toBeNull()
    })
  })

  describe('downloadState transitions', () => {
    // 初始状态为 idle
    it('DownloadState_Initial_001', () => {
      const store = useUpdateStore()
      expect(store.downloadState).toBe('idle')
    })

    // idle → downloading → installing 状态转换
    it('DownloadState_Transition_001', () => {
      const store = useUpdateStore()
      store.setDownloadState('downloading')
      expect(store.downloadState).toBe('downloading')
      store.setDownloadState('installing')
      expect(store.downloadState).toBe('installing')
    })

    // 任意状态 → error 设置错误信息
    it('DownloadState_Error_001', () => {
      const store = useUpdateStore()
      store.setDownloadState('downloading')
      store.setDownloadError('Network timeout')
      expect(store.downloadState).toBe('downloading')
      expect(store.downloadError).toBe('Network timeout')
    })
  })

  describe('downloadProgress', () => {
    // 初始进度为 0
    it('Progress_Initial_001', () => {
      const store = useUpdateStore()
      expect(store.downloadProgress).toEqual({ downloaded: 0, total: 0, percent: 0 })
    })

    // 设置进度后可读取
    it('Progress_Set_001', () => {
      const store = useUpdateStore()
      store.setDownloadProgress({ downloaded: 500000, total: 1000000, percent: 50 })
      expect(store.downloadProgress.percent).toBe(50)
      expect(store.downloadProgress.downloaded).toBe(500000)
    })
  })

  describe('resetDownload', () => {
    // resetDownload 将所有下载状态恢复到初始值
    it('Reset_FullRestore_001', () => {
      const store = useUpdateStore()
      store.setDownloadState('downloading')
      store.setDownloadProgress({ downloaded: 800000, total: 1000000, percent: 80 })
      store.setDownloadError('some error')

      store.resetDownload()

      expect(store.downloadState).toBe('idle')
      expect(store.downloadProgress).toEqual({ downloaded: 0, total: 0, percent: 0 })
      expect(store.downloadError).toBe('')
    })
  })

  describe('clearError', () => {
    // clearError 只清除错误信息，不影响其他状态
    it('ClearError_OnlyError_001', () => {
      const store = useUpdateStore()
      store.setDownloadState('downloading')
      store.setDownloadProgress({ downloaded: 500, total: 1000, percent: 50 })
      store.setDownloadError('Network timeout')

      store.clearError()

      expect(store.downloadError).toBe('')
      expect(store.downloadState).toBe('downloading')
      expect(store.downloadProgress.percent).toBe(50)
    })
  })

  describe('Claude CLI update', () => {
    // 初始状态 claudeCliUpdateInfo 为 null
    it('ClaudeCli_Initial_001', () => {
      const store = useUpdateStore()
      expect(store.claudeCliUpdateInfo).toBeNull()
      expect(store.claudeCliDownloadState).toBe('idle')
      expect(store.claudeCliDownloadProgress).toBe(0)
      expect(store.claudeCliDownloadMessage).toBe('')
      expect(store.claudeCliDownloadError).toBe('')
    })

    // 设置 Claude CLI 更新信息
    it('ClaudeCli_SetUpdateInfo_001', () => {
      const store = useUpdateStore()
      const info: ClaudeCliUpdateInfo = {
        installedVersion: '1.0.30',
        latestVersion: '1.0.33',
        hasUpdate: true,
        notInstalled: false,
      }
      store.setClaudeCliUpdateInfo(info)
      expect(store.claudeCliUpdateInfo).toEqual(info)
    })

    // 设置 null 清除
    it('ClaudeCli_ClearUpdateInfo_001', () => {
      const store = useUpdateStore()
      store.setClaudeCliUpdateInfo({ installedVersion: '1.0.30', latestVersion: '1.0.33', hasUpdate: true, notInstalled: false })
      store.setClaudeCliUpdateInfo(null)
      expect(store.claudeCliUpdateInfo).toBeNull()
    })

    // 下载状态转换
    it('ClaudeCli_DownloadState_001', () => {
      const store = useUpdateStore()
      store.setClaudeCliDownloadState('downloading')
      expect(store.claudeCliDownloadState).toBe('downloading')
      store.setClaudeCliDownloadState('done')
      expect(store.claudeCliDownloadState).toBe('done')
    })

    // 下载进度更新
    it('ClaudeCli_DownloadProgress_001', () => {
      const store = useUpdateStore()
      store.setClaudeCliDownloadProgress(50, '下载中 50%')
      expect(store.claudeCliDownloadProgress).toBe(50)
      expect(store.claudeCliDownloadMessage).toBe('下载中 50%')
    })

    // resetClaudeCliDownload 恢复初始状态
    it('ClaudeCli_Reset_001', () => {
      const store = useUpdateStore()
      store.setClaudeCliDownloadState('downloading')
      store.setClaudeCliDownloadProgress(80, '下载中')
      store.setClaudeCliDownloadError('some error')

      store.resetClaudeCliDownload()

      expect(store.claudeCliDownloadState).toBe('idle')
      expect(store.claudeCliDownloadProgress).toBe(0)
      expect(store.claudeCliDownloadMessage).toBe('')
      expect(store.claudeCliDownloadError).toBe('')
    })

    // 下载错误
    it('ClaudeCli_DownloadError_001', () => {
      const store = useUpdateStore()
      store.setClaudeCliDownloadError('Network error')
      expect(store.claudeCliDownloadError).toBe('Network error')
    })
  })

  // ============================================
  // 本地 Claude 版本（启动检查）
  // ============================================
  describe('installedClaudeVersion', () => {
    // 初始状态为 null
    it('InstalledClaude_Initial_001', () => {
      const store = useUpdateStore()
      expect(store.installedClaudeVersion).toBeNull()
    })

    // 设置已安装版本
    it('InstalledClaude_Set_001', () => {
      const store = useUpdateStore()
      store.setInstalledClaudeVersion('1.0.16')
      expect(store.installedClaudeVersion).toBe('1.0.16')
    })

    // 设置 null（未安装）
    it('InstalledClaude_SetNull_001', () => {
      const store = useUpdateStore()
      store.setInstalledClaudeVersion('1.0.16')
      store.setInstalledClaudeVersion(null)
      expect(store.installedClaudeVersion).toBeNull()
    })
  })

  // ============================================
  // 历史版本下载状态
  // ============================================
  describe('historyDownloads', () => {
    // 初始状态空 Map
    it('HistoryDownload_Initial_001', () => {
      const store = useUpdateStore()
      expect(store.historyDownloads.size).toBe(0)
      expect(store.getHistoryDownload('1.0.17')).toBeUndefined()
    })

    // setHistoryDownload 创建新条目
    it('HistoryDownload_Set_001', () => {
      const store = useUpdateStore()
      store.setHistoryDownload('1.0.17', { state: 'downloading', progress: 50, message: '下载中', savedPath: '', error: '' })
      const d = store.getHistoryDownload('1.0.17')
      expect(d).toBeDefined()
      expect(d?.state).toBe('downloading')
      expect(d?.progress).toBe(50)
    })

    // setHistoryDownload 增量更新
    it('HistoryDownload_Patch_001', () => {
      const store = useUpdateStore()
      store.setHistoryDownload('1.0.17', { state: 'downloading', progress: 0, message: '', savedPath: '', error: '' })
      store.setHistoryDownload('1.0.17', { progress: 80 })
      const d = store.getHistoryDownload('1.0.17')
      expect(d?.progress).toBe(80)
      expect(d?.state).toBe('downloading')
    })

    // 完成状态保留 savedPath
    it('HistoryDownload_Done_001', () => {
      const store = useUpdateStore()
      store.setHistoryDownload('1.0.17', { state: 'done', progress: 100, savedPath: 'C:\\Downloads\\claude-1.0.17.exe', message: '', error: '' })
      const d = store.getHistoryDownload('1.0.17')
      expect(d?.state).toBe('done')
      expect(d?.savedPath).toContain('claude-1.0.17.exe')
    })

    // resetHistoryDownload 删除条目
    it('HistoryDownload_Reset_001', () => {
      const store = useUpdateStore()
      store.setHistoryDownload('1.0.17', { state: 'done', progress: 100, savedPath: 'p', message: '', error: '' })
      store.resetHistoryDownload('1.0.17')
      expect(store.getHistoryDownload('1.0.17')).toBeUndefined()
    })

    // 取消状态：从 downloading 切换到 cancelled
    it('HistoryDownload_Cancelled_001', () => {
      const store = useUpdateStore()
      store.setHistoryDownload('1.0.17', { state: 'downloading', progress: 60, message: '', savedPath: '', error: '' })
      store.setHistoryDownload('1.0.17', { state: 'cancelled', progress: 0, message: '已取消' })
      const d = store.getHistoryDownload('1.0.17')
      expect(d?.state).toBe('cancelled')
      expect(d?.progress).toBe(0)
      expect(d?.message).toBe('已取消')
    })

    // 取消后可重新发起：cancelled → downloading
    it('HistoryDownload_Cancelled_To_Downloading_001', () => {
      const store = useUpdateStore()
      store.setHistoryDownload('1.0.17', { state: 'cancelled', progress: 0, message: '', savedPath: '', error: '' })
      store.setHistoryDownload('1.0.17', { state: 'downloading', progress: 0, message: '重新下载', savedPath: '', error: '' })
      const d = store.getHistoryDownload('1.0.17')
      expect(d?.state).toBe('downloading')
    })

    // 安装中状态：downloading → installing
    it('HistoryDownload_Installing_001', () => {
      const store = useUpdateStore()
      store.setHistoryDownload('1.0.17', { state: 'downloading', progress: 100, savedPath: 'C:\\Downloads\\c.exe', message: '', error: '' })
      store.setHistoryDownload('1.0.17', { state: 'installing', progress: 0, message: '正在安装' })
      const d = store.getHistoryDownload('1.0.17')
      expect(d?.state).toBe('installing')
      expect(d?.savedPath).toBe('C:\\Downloads\\c.exe')
    })

    // 安装完成：installing → done，savedPath 保留
    it('HistoryDownload_Installing_To_Done_001', () => {
      const store = useUpdateStore()
      store.setHistoryDownload('1.0.17', { state: 'installing', progress: 0, savedPath: 'p', message: '', error: '' })
      store.setHistoryDownload('1.0.17', { state: 'done', progress: 100, message: '安装完成' })
      const d = store.getHistoryDownload('1.0.17')
      expect(d?.state).toBe('done')
      expect(d?.savedPath).toBe('p')
    })
  })
})
