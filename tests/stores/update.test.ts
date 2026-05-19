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
})
