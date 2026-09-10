import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const mockGetAllSkills = vi.fn()
const mockGetAllAgents = vi.fn()
const mockGetAllMcpServers = vi.fn()
const mockGetAllPlugins = vi.fn()

vi.mock('@/api/tauri', () => ({
  getAllSkills: (...args: unknown[]) => mockGetAllSkills(...args),
  getAllAgents: (...args: unknown[]) => mockGetAllAgents(...args),
  getAllMcpServers: (...args: unknown[]) => mockGetAllMcpServers(...args),
  getAllPlugins: (...args: unknown[]) => mockGetAllPlugins(...args),
}))

import { useSidebarStore } from '@/stores/sidebar'

describe('sidebar store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mockGetAllSkills.mockReset().mockResolvedValue([])
    mockGetAllAgents.mockReset().mockResolvedValue([])
    mockGetAllMcpServers.mockReset().mockResolvedValue([])
    mockGetAllPlugins.mockReset().mockResolvedValue([])
  })

  describe('togglePanel', () => {
    it('TogglePanel_Open_001', () => {
      const store = useSidebarStore()

      store.togglePanel('sessions')

      expect(store.panelVisible).toBe(true)
      expect(store.activePanel).toBe('sessions')
    })

    it('TogglePanel_Close_001', () => {
      const store = useSidebarStore()
      store.activePanel = 'sessions'
      store.panelVisible = true

      store.togglePanel('sessions')

      expect(store.panelVisible).toBe(false)
    })

    it('TogglePanel_Switch_001', () => {
      const store = useSidebarStore()
      store.activePanel = 'sessions'
      store.panelVisible = true

      store.togglePanel('mcp')

      expect(store.activePanel).toBe('mcp')
      expect(store.panelVisible).toBe(true)
    })

    it('TogglePanel_CloseSettings_001', () => {
      const store = useSidebarStore()
      store.showSettings = true

      store.togglePanel('sessions')

      expect(store.showSettings).toBe(false)
      expect(store.activePanel).toBe('sessions')
      expect(store.panelVisible).toBe(true)
    })
  })

  describe('settings navigation', () => {
    it('Settings_KnownSection_001', () => {
      const store = useSidebarStore()

      store.openSettings('shortcuts')

      expect(store.showSettings).toBe(true)
      expect(store.activeSettingsSection).toBe('shortcuts')
    })

    it('Settings_RemovedSectionFallsBack_001', () => {
      const store = useSidebarStore()

      store.openSettings('providers')

      expect(store.activeSettingsSection).toBe('appearance')
    })
  })

  describe('updateAvailable badge', () => {
    it('Badge_None_001', () => {
      const store = useSidebarStore()
      expect(store.updateAvailable).toBe(false)
    })

    it('Badge_AppUpdate_001', () => {
      const store = useSidebarStore()
      store.setUpdateInfo({
        version: '0.8.0',
        currentVersion: '0.7.0',
        hasUpdate: true,
        releaseNotes: '',
        downloadUrl: '',
        platformAsset: null,
      })

      expect(store.updateAvailable).toBe(true)
    })

    it('Badge_NoUpdate_001', () => {
      const store = useSidebarStore()
      store.setUpdateInfo({
        version: '0.7.0',
        currentVersion: '0.7.0',
        hasUpdate: false,
        releaseNotes: '',
        downloadUrl: '',
        platformAsset: null,
      })

      expect(store.updateAvailable).toBe(false)
    })
  })

  describe('read-only native projections', () => {
    it('LoadAll_QueriesEveryProjectionOnce_001', async () => {
      const store = useSidebarStore()
      mockGetAllSkills.mockResolvedValue([{ name: 'deploy' }])
      mockGetAllAgents.mockResolvedValue([{ name: 'reviewer' }])
      mockGetAllMcpServers.mockResolvedValue([{ name: 'filesystem' }])
      mockGetAllPlugins.mockResolvedValue([{ id: 'tools@example' }])

      await store.loadAllSidebarData('/project')

      expect(mockGetAllSkills).toHaveBeenCalledWith('/project')
      expect(mockGetAllAgents).toHaveBeenCalledWith('/project')
      expect(mockGetAllMcpServers).toHaveBeenCalledWith('/project')
      expect(mockGetAllPlugins).toHaveBeenCalledWith('/project')
      expect(store.loadedCwd).toBe('/project')
    })

    it('LoadAll_SameCwdUsesProjectionCache_001', async () => {
      const store = useSidebarStore()

      await store.loadAllSidebarData('/project')
      await store.loadAllSidebarData('/project')

      expect(mockGetAllSkills).toHaveBeenCalledTimes(1)
      expect(mockGetAllAgents).toHaveBeenCalledTimes(1)
      expect(mockGetAllMcpServers).toHaveBeenCalledTimes(1)
      expect(mockGetAllPlugins).toHaveBeenCalledTimes(1)
    })

    it('LoadFailure_DoesNotLeaveLoadingState_001', async () => {
      const store = useSidebarStore()
      mockGetAllSkills.mockRejectedValue(new Error('read failed'))

      await store.loadSkills('/project')

      expect(store.skillsLoading).toBe(false)
      expect(store.skills).toEqual([])
    })

    it('DoesNotExposeNativeMutationActions_001', () => {
      const store = useSidebarStore()

      expect('toggleSkillEnabled' in store).toBe(false)
      expect('toggleAgentEnabled' in store).toBe(false)
      expect('toggleMcpServerEnabled' in store).toBe(false)
      expect('togglePluginEnabled' in store).toBe(false)
    })
  })
})
