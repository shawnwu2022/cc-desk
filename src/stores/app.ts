import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getAppConfig,
  updateAppConfig,
  saveLastProject,
  saveDefaultClaudeOptions,
  getHomeData,
  getProjects,
  getCheckResults,
  runChecks,
  syncClaudeEnv
} from '@/api/tauri'

import type { ClaudeOptions, DefaultClaudeOptions, CheckResult, Project, SessionInfo } from '@/types'

const PAGE_SIZE = 12

/** 默认环境变量（代码中定义，用户可重置） */
const DEFAULT_CLAUDE_ENV_VARS: Record<string, string> = {
  LANG: 'en_US.UTF-8',
  LC_ALL: 'en_US.UTF-8',
  PYTHONUTF8: '1',
  CLAUDE_CODE_SCROLL_SPEED: '5',
  PYTHONIOENCODING: 'utf-8',
  CLAUDE_CODE_NO_FLICKER: '1',
}

export { DEFAULT_CLAUDE_ENV_VARS }

export interface PendingResume {
  sessionId: string
  sessionName?: string
}

export const useAppStore = defineStore('app', () => {
  const cwd = ref<string>('')
  const theme = ref<string>('light')
  const fontSize = ref<number>(12)
  const claudeEnvVars = ref<Record<string, string>>({})

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
  const openedProjectPaths = ref<Set<string>>(new Set())
  const projectsPage = ref(0)
  const hasMoreProjects = ref(true)
  const isLoadingProjects = ref(false)

  // Claude 默认启动参数（持久化，Settings 绑定）
  const defaultClaudeOptions = ref<DefaultClaudeOptions>({
    skipPermissions: false,
    customArgs: ''
  })

  // Claude 当前使用启动参数（SessionsPanel/ProjectSelectView 绑定）
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

      // 加载环境变量（首次使用默认值）
      claudeEnvVars.value = Object.keys(config.claudeEnvVars ?? {}).length > 0
        ? config.claudeEnvVars!
        : { ...DEFAULT_CLAUDE_ENV_VARS }

      // 启动时写入 ~/.claude/settings.json
      await doSyncEnv()

      defaultClaudeOptions.value = {
        skipPermissions: config.defaultSkipPermissions ?? false,
        customArgs: config.defaultCustomArgs ?? ''
      }
      claudeOptions.value = {
        resume: '',
        skipPermissions: defaultClaudeOptions.value.skipPermissions,
        customArgs: defaultClaudeOptions.value.customArgs
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
    openedProjectPaths.value.add(path)
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

  /** 同步当前 claudeEnvVars 到 cc-box config + ~/.claude/settings.json */
  async function doSyncEnv(removedKeys: string[] = []) {
    updateAppConfig({ claudeEnvVars: claudeEnvVars.value })
    await syncClaudeEnv(claudeEnvVars.value, removedKeys)
  }

  /** 更新环境变量并同步 */
  async function setClaudeEnvVars(vars: Record<string, string>, removedKeys: string[] = []) {
    claudeEnvVars.value = vars
    await doSyncEnv(removedKeys)
  }

  /** 将默认变量恢复为代码默认值，保留用户添加的变量 */
  async function resetClaudeEnvVars() {
    const updated = { ...claudeEnvVars.value }
    for (const [key, value] of Object.entries(DEFAULT_CLAUDE_ENV_VARS)) {
      updated[key] = value
    }
    claudeEnvVars.value = updated
    await doSyncEnv()
  }

  function setClaudeOptions(options: Partial<ClaudeOptions>) {
    claudeOptions.value = { ...claudeOptions.value, ...options }
  }

  function resetClaudeOptions() {
    claudeOptions.value = {
      resume: '',
      skipPermissions: defaultClaudeOptions.value.skipPermissions,
      customArgs: defaultClaudeOptions.value.customArgs
    }
  }

  async function setDefaultClaudeOptions(opts: Partial<DefaultClaudeOptions>) {
    defaultClaudeOptions.value = { ...defaultClaudeOptions.value, ...opts }
    claudeOptions.value = {
      resume: claudeOptions.value.resume,
      skipPermissions: defaultClaudeOptions.value.skipPermissions,
      customArgs: defaultClaudeOptions.value.customArgs
    }
    await saveDefaultClaudeOptions(defaultClaudeOptions.value)
  }

  async function saveAsDefault(): Promise<boolean> {
    try {
      const opts = {
        skipPermissions: claudeOptions.value.skipPermissions,
        customArgs: claudeOptions.value.customArgs
      }
      defaultClaudeOptions.value = opts
      await saveDefaultClaudeOptions(opts)
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
    claudeEnvVars,
    defaultClaudeOptions,
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
    openedProjectPaths,
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
    setClaudeEnvVars,
    resetClaudeEnvVars,
    setClaudeOptions,
    setDefaultClaudeOptions,
    resetClaudeOptions,
    saveAsDefault,
    getClaudeArgs,
    setPendingResume,
    clearPendingResume,
    setAutoOpenSessions
  }
})
