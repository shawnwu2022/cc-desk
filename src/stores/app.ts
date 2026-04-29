import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getAppConfig,
  updateAppConfig,
  saveLastProject,
  getDefaultClaudeOptions,
  saveDefaultClaudeOptions,
  getHomeData,
  getProjects,
  getCheckResults,
  runChecks
} from '@/api/tauri'

import type { ClaudeOptions, CheckResult, Project, SessionInfo } from '@/types'

const PAGE_SIZE = 12

export interface PendingResume {
  sessionId: string
  sessionName?: string
}

export const useAppStore = defineStore('app', () => {
  const cwd = ref<string>('')
  const theme = ref<string>('light')
  const fontSize = ref<number>(12)

  // 启动控制
  const pendingResume = ref<PendingResume | null>(null)
  const shouldAutoOpenSessions = ref(false)

  // 环境检查
  const checkResults = ref<CheckResult[]>([])
  const checkFailed = ref(false)

  // 缓存：项目列表（分页）和近期会话
  const cachedProjects = ref<Project[]>([])
  const cachedRecentSessions = ref<SessionInfo[]>([])
  const cacheLoaded = ref(false)
  const projectsPage = ref(0)
  const hasMoreProjects = ref(true)
  const isLoadingProjects = ref(false)

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

  const failedChecks = computed(() => checkResults.value.filter(c => !c.passed))

  async function loadAppConfig() {
    try {
      const config = await getAppConfig()
      theme.value = config.theme || 'light'
      fontSize.value = config.fontSize || 12

      claudeOptions.value = {
        resume: '',
        skipPermissions: config.defaultSkipPermissions ?? false,
        customArgs: config.defaultCustomArgs ?? ''
      }
    } catch (err) {
      console.error('Failed to load app config:', err)
    }
  }

  async function doChecks(force = false) {
    try {
      checkResults.value = force ? await runChecks() : await getCheckResults()
      checkFailed.value = checkResults.value.some(c => !c.passed)
    } catch (err) {
      console.error('Failed to run checks:', err)
    }
  }

  async function loadCache() {
    if (cacheLoaded.value) return
    try {
      const data = await getHomeData(PAGE_SIZE, 20)
      cachedProjects.value = data.projects
      cachedRecentSessions.value = data.recentSessions
      projectsPage.value = 1
      hasMoreProjects.value = data.hasMore
      cacheLoaded.value = true
    } catch (err) {
      console.error('Failed to load cache:', err)
    }
  }

  async function loadMoreProjects() {
    if (isLoadingProjects.value || !hasMoreProjects.value) return
    isLoadingProjects.value = true
    try {
      const offset = projectsPage.value * PAGE_SIZE
      const projs = await getProjects(PAGE_SIZE, offset)
      cachedProjects.value.push(...projs)
      projectsPage.value++
      hasMoreProjects.value = projs.length === PAGE_SIZE
    } catch (err) {
      console.error('Failed to load more projects:', err)
    } finally {
      isLoadingProjects.value = false
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

  return {
    cwd,
    theme,
    fontSize,
    claudeOptions,
    currentProject,
    pendingResume,
    shouldAutoOpenSessions,
    checkResults,
    checkFailed,
    failedChecks,
    cachedProjects,
    cachedRecentSessions,
    cacheLoaded,
    hasMoreProjects,
    isLoadingProjects,
    loadAppConfig,
    runChecks: doChecks,
    loadCache,
    loadMoreProjects,
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
