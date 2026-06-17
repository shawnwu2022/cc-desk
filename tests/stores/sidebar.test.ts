import { describe, it, expect, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useSidebarStore } from '@/stores/sidebar'

describe('sidebar store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  describe('togglePanel', () => {
    // panelVisible=false 时调用 togglePanel("sessions") 后 panelVisible 变为 true
    it('TogglePanel_Open_001', () => {
      const store = useSidebarStore()

      expect(store.panelVisible).toBe(false)
      expect(store.activePanel).toBeNull()

      store.togglePanel('sessions')

      expect(store.panelVisible).toBe(true)
      expect(store.activePanel).toBe('sessions')
    })

    // 面板已打开且 activePanel="sessions" 时调用 togglePanel("sessions") 后 panelVisible 变为 false
    it('TogglePanel_Close_001', () => {
      const store = useSidebarStore()

      store.activePanel = 'sessions'
      store.panelVisible = true

      store.togglePanel('sessions')

      expect(store.panelVisible).toBe(false)
    })

    // activePanel="sessions" 时调用 togglePanel("mcp") 后 activePanel 变为 "mcp"
    it('TogglePanel_Switch_001', () => {
      const store = useSidebarStore()

      store.activePanel = 'sessions'
      store.panelVisible = true

      store.togglePanel('mcp')

      expect(store.activePanel).toBe('mcp')
      expect(store.panelVisible).toBe(true)
    })

    // showSettings=true 时调用 togglePanel("sessions") 后 showSettings 变为 false
    it('TogglePanel_CloseSettings_001', () => {
      const store = useSidebarStore()

      store.showSettings = true

      store.togglePanel('sessions')

      expect(store.showSettings).toBe(false)
      expect(store.activePanel).toBe('sessions')
      expect(store.panelVisible).toBe(true)
    })
  })

  describe('updateAvailable badge', () => {
    // 无任何更新时 badge 不显示
    it('Badge_None_001', () => {
      const store = useSidebarStore()
      expect(store.updateAvailable).toBe(false)
    })

    // 仅软件更新时 badge 显示
    it('Badge_AppUpdate_001', () => {
      const store = useSidebarStore()
      store.setUpdateInfo({ version: '0.8.0', currentVersion: '0.7.0', hasUpdate: true, releaseNotes: '', downloadUrl: '', platformAsset: null })
      expect(store.updateAvailable).toBe(true)
    })

    // Claude CLI 更新不再驱动 badge（启动检查已改为只读本地版本，不对比 OSS）
    it('Badge_CliUpdate_Ignored_001', () => {
      const store = useSidebarStore()
      store.setClaudeCliUpdateInfo({ installedVersion: '1.0.30', latestVersion: '1.0.33', hasUpdate: true, notInstalled: false })
      expect(store.updateAvailable).toBe(false)
    })

    // 两者都有更新时 badge 显示
    it('Badge_Both_001', () => {
      const store = useSidebarStore()
      store.setUpdateInfo({ version: '0.8.0', currentVersion: '0.7.0', hasUpdate: true, releaseNotes: '', downloadUrl: '', platformAsset: null })
      store.setClaudeCliUpdateInfo({ installedVersion: '1.0.30', latestVersion: '1.0.33', hasUpdate: true, notInstalled: false })
      expect(store.updateAvailable).toBe(true)
    })

    // 软件无更新但 Claude CLI 无更新时 badge 不显示
    it('Badge_NoUpdate_001', () => {
      const store = useSidebarStore()
      store.setUpdateInfo({ version: '0.7.0', currentVersion: '0.7.0', hasUpdate: false, releaseNotes: '', downloadUrl: '', platformAsset: null })
      store.setClaudeCliUpdateInfo({ installedVersion: '1.0.33', latestVersion: '1.0.33', hasUpdate: false, notInstalled: false })
      expect(store.updateAvailable).toBe(false)
    })
  })
})
