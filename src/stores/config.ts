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
  // Each selection owns its result, error and loading state, including cache hits.
  let requestOwner: object = {}

  // 加载项目配置（带缓存，同项目不重复加载）
  async function loadProjectConfig(projectPath: string) {
    if (!projectPath) {
      clearConfig()
      return
    }
    const owner = requestOwner = {}
    if (loadedCwd.value === projectPath && projectConfig.value) {
      isLoading.value = false
      error.value = null
      return
    }

    projectConfig.value = null
    loadedCwd.value = null
    isLoading.value = true
    error.value = null

    try {
      const config = await getProjectConfig(projectPath)
      if (requestOwner !== owner) return
      projectConfig.value = config
      loadedCwd.value = projectPath
    } catch {
      if (requestOwner !== owner) return
      error.value = 'Failed to load project config'
    } finally {
      if (requestOwner === owner) isLoading.value = false
    }
  }

  // 强制刷新项目配置
  async function refreshProjectConfig(projectPath: string) {
    if (!projectPath) {
      clearConfig()
      return
    }
    loadedCwd.value = null
    await loadProjectConfig(projectPath)
  }

  // 清空配置
  function clearConfig() {
    requestOwner = {}
    isLoading.value = false
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