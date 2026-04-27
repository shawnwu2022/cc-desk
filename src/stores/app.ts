import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getAppConfig,
  updateAppConfig,
  saveLastProject,
  getDefaultClaudeOptions,
  saveDefaultClaudeOptions,
  getProjects,
  getAllRecentSessions
} from '@/api/tauri'

import type { ClaudeOptions, Project, SessionInfo } from '@/types'

export interface PendingResume {
  sessionId: string
  sessionName?: string
}

export const useAppStore = defineStore('app', () => {
  const cwd = ref<string>('')
  const theme = ref<string>('light')
  const fontSize = ref<number>(10)

  // 启动控制
  const pendingResume = ref<PendingResume | null>(null)
  const shouldAutoOpenSessions = ref(false)

  // 缓存：项目列表和近期会话
  const cachedProjects = ref<Project[]>([])
  const cachedRecentSessions = ref<SessionInfo[]>([])
  const cacheLoaded = ref(false)

  // Claude 启动选项
  const claudeOptions = ref<ClaudeOptions>({
    resume: '',
    skipPermissions: false,
    customArgs: ''
  })

  const currentProject = computed(() => {
    if (!cwd.value) return null
    const parts = cwd.value.replace(/\\/g, '/').split('/')
    return parts[parts.length - 1] || cwd.value
  })

  async function loadAppConfig() {
    try {
      const config = await getAppConfig()
      theme.value = config.theme || 'light'
      fontSize.value = config.fontSize || 10

      claudeOptions.value = {
        resume: '',
        skipPermissions: config.defaultSkipPermissions ?? false,
        customArgs: config.defaultCustomArgs ?? ''
      }

      if (config.lastOpenedProject) {
        cwd.value = config.lastOpenedProject
      }
    } catch (err) {
      console.error('Failed to load app config:', err)
    }
  }

  async function loadCache() {
    if (cacheLoaded.value) return
    try {
      const [projs, sessions] = await Promise.all([
        getProjects(),
        getAllRecentSessions(20)
      ])
      cachedProjects.value = projs
      cachedRecentSessions.value = sessions
      cacheLoaded.value = true
    } catch (err) {
      console.error('Failed to load cache:', err)
    }
  }

  function refreshRecentSessions(sessions: SessionInfo[]) {
    cachedRecentSessions.value = sessions
  }

  function setCwd(path: string) {
    cwd.value = path
    saveLastProject(path)
  }

  function setTheme(newTheme: string) {
    theme.value = newTheme
    updateAppConfig({ theme: newTheme })
  }

  function setFontSize(size: number) {
    fontSize.value = Math.max(10, Math.min(24, size))
    updateAppConfig({ fontSize: size })
  }

  function setClaudeOptions(options: Partial<ClaudeOptions>) {
    claudeOptions.value = { ...claudeOptions.value, ...options }
  }

  function resetClaudeOptions() {
    getDefaultClaudeOptions().then(defaults => {
      claudeOptions.value = {
        resume: '',
        skipPermissions: defaults.skipPermissions ?? false,
        customArgs: defaults.customArgs ?? ''
      }
    })
  }

  async function saveAsDefault(): Promise<boolean> {
    try {
      await saveDefaultClaudeOptions({
        skipPermissions: claudeOptions.value.skipPermissions,
        customArgs: claudeOptions.value.customArgs
      })
      return true
    } catch (err) {
      console.error('Failed to save default options:', err)
      return false
    }
  }

  function getClaudeArgs(): string[] {
    const opts = claudeOptions.value
    const args: string[] = []

    if (opts.resume) args.push('--resume', opts.resume)
    if (opts.skipPermissions) args.push('--dangerously-skip-permissions')
    if (opts.customArgs) {
      const custom = opts.customArgs.trim().split(/\s+/).filter(Boolean)
      args.push(...custom)
    }

    return args
  }

  function setPendingResume(sessionId: string, sessionName?: string) {
    pendingResume.value = { sessionId, sessionName }
  }

  function clearPendingResume() {
    pendingResume.value = null
  }

  function setAutoOpenSessions(val: boolean) {
    shouldAutoOpenSessions.value = val
  }

  loadAppConfig()

  return {
    cwd,
    theme,
    fontSize,
    claudeOptions,
    currentProject,
    pendingResume,
    shouldAutoOpenSessions,
    cachedProjects,
    cachedRecentSessions,
    cacheLoaded,
    loadAppConfig,
    loadCache,
    refreshRecentSessions,
    setCwd,
    setTheme,
    setFontSize,
    setClaudeOptions,
    resetClaudeOptions,
    saveAsDefault,
    getClaudeArgs,
    setPendingResume,
    clearPendingResume,
    setAutoOpenSessions
  }
})
