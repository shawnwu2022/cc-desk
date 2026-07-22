import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getProjectConfig } from '@/api/tauri'

// 从统一类型导入
import type { ProjectConfigResult } from '@/types'

export const useConfigStore = defineStore('config', () => {
  const projectConfig = ref<ProjectConfigResult | null>(null)
  const isLoading = ref(false)
  const error = ref<string | null>(null)
  const loadedCwd = ref<string | null>(null)

  // 加载项目配置（带缓存，同项目不重复加载）
  async function loadProjectConfig(projectPath: string) {
    if (!projectPath) return
    if (loadedCwd.value === projectPath && projectConfig.value) return

    isLoading.value = true
    error.value = null

    try {
      const config = await getProjectConfig(projectPath)
      projectConfig.value = config
      loadedCwd.value = projectPath
    } catch (err) {
      error.value = 'Failed to load project config'
      console.error('[ConfigStore] Failed to load project config:', err)
    } finally {
      isLoading.value = false
    }
  }

  // 强制刷新项目配置
  async function refreshProjectConfig(projectPath: string) {
    if (!projectPath) return
    loadedCwd.value = null
    await loadProjectConfig(projectPath)
  }

  // 清空配置
  function clearConfig() {
    projectConfig.value = null
    error.value = null
    loadedCwd.value = null
  }

  return {
    projectConfig,
    isLoading,
    error,
    loadProjectConfig,
    refreshProjectConfig,
    clearConfig
  }
})