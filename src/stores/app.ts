import { defineStore } from 'pinia'
import { ref, computed, onScopeDispose } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  getAppConfig,
  updateAppConfig,
  saveLastProject,
  saveDefaultClaudeOptions,
  getHomeData,
  getProjects,
  getCheckResults,
  runChecks,
} from '@/api/tauri'
import { normalizeTerminalThemeId } from '@/config/terminalThemes'
import { normalizePath } from '@/utils/path'
import { useShellStore } from '@/stores/shell'
import type { AppConfig, GuiThemeMode, GuiDensity, StartupDestination } from '@/types/app'
import type { UnifiedCliKind } from '@/types/unifiedSession'
import { useWorkspaceStore } from '@/stores/workspace'
import { useCliWorkspaceStore } from '@/stores/cliWorkspace'
import type { UnifiedProjectIdentity } from '@/types/unifiedSession'
import { applyThemeToDom } from '@/utils/theme'
import i18n from '@/i18n'

import type { ClaudeOptions, DefaultClaudeOptions, CheckResult, Project, ProjectStartupState, SessionInfo } from '@/types'

interface SimpleSettings {
  guiThemeMode: GuiThemeMode
  guiDensity: GuiDensity
  sidebarWidth: number
  language: 'en' | 'zh'
  startupDestination: StartupDestination
  defaultNewCli: UnifiedCliKind
}
interface AppConfigRead {
  config: AppConfig
  sequence: number
  intents: Record<keyof SimpleSettings, number>
  commits: Record<keyof SimpleSettings, number>
  visibilityVersion: number
}
const simpleSettingKeys = ['guiThemeMode', 'guiDensity', 'sidebarWidth', 'language', 'startupDestination', 'defaultNewCli'] as const
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
  const shell = useShellStore()
  const guiThemeMode = ref<GuiThemeMode>('light')
  const guiDensity = ref<GuiDensity>('standard')
  const sidebarWidth = computed(() => shell.sidebarWidth)
  const startupDestination = ref<StartupDestination>('workspace')
  const defaultNewCli = ref<UnifiedCliKind>('claude')
  const cwd = ref<string>('')
  const theme = ref<string>('light')
  const terminalTheme = ref<string>('cc-box-light')
  const fontSize = ref<number>(12)
  const webglRenderer = ref<boolean>(false)
  const language = ref<'en' | 'zh'>('en')
  const alwaysOnTop = ref<boolean>(false)
  const claudeEnvVars = ref<Record<string, string>>({})

  const systemTheme = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null
  const systemDark = ref(systemTheme?.matches ?? false)
  function publishGuiTheme() {
    theme.value = guiThemeMode.value === 'system' ? systemDark.value ? 'dark' : 'light' : guiThemeMode.value
    applyThemeToDom(theme.value)
  }
  function onSystemTheme(event: MediaQueryListEvent) {
    systemDark.value = event.matches
    if (guiThemeMode.value === 'system') publishGuiTheme()
  }
  if (systemTheme?.addEventListener) systemTheme.addEventListener('change', onSystemTheme)
  else systemTheme?.addListener?.(onSystemTheme)
  onScopeDispose(() => {
    if (systemTheme?.removeEventListener) systemTheme.removeEventListener('change', onSystemTheme)
    else systemTheme?.removeListener?.(onSystemTheme)
  })

  function simpleSettings(): SimpleSettings {
    return { guiThemeMode: guiThemeMode.value, guiDensity: guiDensity.value, sidebarWidth: shell.sidebarWidth,
      language: language.value, startupDestination: startupDestination.value, defaultNewCli: defaultNewCli.value }
  }
  function applySimpleSettings(next: SimpleSettings) {
    guiThemeMode.value = next.guiThemeMode
    guiDensity.value = next.guiDensity
    shell.setSidebarWidth(next.sidebarWidth)
    language.value = next.language
    startupDestination.value = next.startupDestination
    defaultNewCli.value = next.defaultNewCli
    i18n.global.locale.value = next.language
    document.documentElement.dataset.density = next.guiDensity
    publishGuiTheme()
  }
  let confirmedSettings = simpleSettings()
  const settingIntents: Record<keyof SimpleSettings, number> = { guiThemeMode: 0, guiDensity: 0, sidebarWidth: 0, language: 0, startupDestination: 0, defaultNewCli: 0 }
  const settingCommits = { ...settingIntents }
  const pendingSettings = { ...settingIntents }
  const settingsErrors = ref<Partial<Record<keyof SimpleSettings, string>>>({})
  const settingsSaveError = computed(() => Object.values(settingsErrors.value).find(Boolean) ?? null)
  let settingsLoaded = false
  let settingsUnconfirmed = false
  let settingsPublicationSequence = 0
  let settingsLoad: Promise<void> | null = null
  let settingsMutationTail: Promise<void> = Promise.resolve()
  function widthValue(value: unknown): number {
    return typeof value === 'number' && Number.isFinite(value) ? Math.round(Math.max(240, Math.min(360, value))) : 288
  }
  function hydrateSimpleSettings({ config, intents, commits, sequence }: AppConfigRead) {
    // Publication follows underlying read order, not the order in which callers joined it.
    if (sequence < settingsPublicationSequence) return
    const values: SimpleSettings = {
      guiThemeMode: config.guiThemeMode === 'system' || config.guiThemeMode === 'dark' || config.guiThemeMode === 'light'
        ? config.guiThemeMode : config.theme === 'dark' ? 'dark' : 'light',
      guiDensity: config.guiDensity === 'compact' ? 'compact' : 'standard', sidebarWidth: widthValue(config.sidebarWidth),
      language: config.language === 'zh' || config.language === 'en' ? config.language : detectSystemLocale(),
      startupDestination: config.startupDestination === 'projects' ? 'projects' : 'workspace',
      defaultNewCli: config.defaultNewCli === 'codex' ? 'codex' : 'claude',
    }
    const next = simpleSettings()
    for (const key of simpleSettingKeys) {
      // A pending intention may need the read's confirmed baseline for rollback,
      // but an older read can never overwrite a newer acknowledged write or UI choice.
      if (settingCommits[key] !== commits[key]) continue
      Object.assign(confirmedSettings, { [key]: values[key] })
      if (settingIntents[key] === intents[key] && !pendingSettings[key]) Object.assign(next, { [key]: values[key] })
    }
    applySimpleSettings(next)
    settingsPublicationSequence = sequence
    settingsLoaded = true
  }
  function loadSettingsPreferences(force = false): Promise<void> {
    if (settingsLoaded && !force) return Promise.resolve()
    if (settingsLoad) return force ? settingsLoad.catch(() => undefined).then(() => loadSettingsPreferences(true)) : settingsLoad
    settingsLoad = readAppConfig(force).then(read => {
      const { config } = read
      if (!config || typeof config !== 'object' || Array.isArray(config)) throw new Error('APP_CONFIG_UNAVAILABLE')
      hydrateSimpleSettings(read)
    }).finally(() => { settingsLoad = null })
    return settingsLoad
  }
  function isUnknownSettingsCommit(failure: unknown): boolean {
    return !!failure && typeof failure === 'object'
      && Object.prototype.hasOwnProperty.call(failure, 'code')
      && (failure as { code?: unknown }).code === 'COMMIT_STATE_UNKNOWN'
  }
  async function ensureSettingsWriteReady() {
    // Every participant in the lane, including startup migration, honors uncertainty.
    await loadSettingsPreferences(settingsUnconfirmed)
    settingsUnconfirmed = false
  }
  async function reconcileUnconfirmedSettings() {
    settingsUnconfirmed = true
    try {
      await loadSettingsPreferences(true)
      settingsUnconfirmed = false
    } catch { /* Keep the barrier until a later explicit operation obtains a fresh read. */ }
  }
  function saveSimpleSetting<K extends keyof SimpleSettings>(key: K, value: SimpleSettings[K]): Promise<boolean> {
    const intent = ++settingIntents[key]
    ++pendingSettings[key]
    settingsErrors.value = { ...settingsErrors.value, [key]: undefined }
    applySimpleSettings({ ...simpleSettings(), [key]: value })
    const operation = settingsMutationTail.then(async () => {
      let submitted = false
      try {
        await ensureSettingsWriteReady()
        // Exactly one submitted delta per explicit choice. No full config replacement.
        const updates: Record<string, unknown> = { [key]: value }
        if (key === 'guiThemeMode' && value !== 'system') updates.theme = value
        submitted = true
        await updateAppConfig(updates)
        Object.assign(confirmedSettings, { [key]: value })
        ++settingCommits[key]
        return true
      } catch (failure) {
        const unknown = submitted && isUnknownSettingsCommit(failure)
        if (unknown) {
          // Reconcile inside this writer lane; never automatically resubmit the delta.
          await reconcileUnconfirmedSettings()
          if (!settingsUnconfirmed) ++settingCommits[key]
        }
        if (settingIntents[key] === intent) {
          if (!settingsUnconfirmed) applySimpleSettings({ ...simpleSettings(), [key]: confirmedSettings[key] })
          settingsErrors.value = { ...settingsErrors.value, [key]: settingsUnconfirmed ? 'settingsSaveReloadFailed'
            : unknown ? 'settingsSaveUnconfirmed' : submitted ? 'settingsSaveFailed' : 'settingsSaveReadFailed' }
        }
        return false
      } finally { --pendingSettings[key] }
    })
    settingsMutationTail = operation.then(() => undefined)
    return operation
  }

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
  // 启动摘要（合并自 get_home_data，供 initStartup 决策；不再单独调 get_project_startup_state）
  const startupState = ref<ProjectStartupState | null>(null)
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

  // ---- 启动状态源（v5-T3）----
  // 最近打开的项目路径（启动决定视图用）+ 隐藏项目集合（项目列表过滤用）
  const lastOpenedProject = ref<string>('')
  const hiddenProjects = ref<Set<string>>(new Set())
  // app config 加载状态：idle/loading/loaded/error（v5 P1：加载失败感知）
  const loadStatus = ref<'idle' | 'loading' | 'loaded' | 'error'>('idle')
  // setHidden 操作锁：串行化 读->算 next->persist->改本地，防并发丢更新
  let hiddenOpLock: Promise<void> = Promise.resolve()
  // lastOpened 操作锁：串行化 saveLastProject + setCwdLocal，防快速 A->B 各自基于旧内存
  // 持久化、后 persist 覆盖前导致 lastOpened 乱序（终态为 B 但磁盘先写 A 后写 B 顺序虽对，
  // 然 setCwdLocal 若与 persist 交错可能短暂错乱；串行化保证读-写原子）。
  let lastOpenedOpLock: Promise<void> = Promise.resolve()

  const managedProjectsStatus = ref<'idle' | 'loading' | 'ready' | 'error'>('idle')
  const removingProjectPaths = ref(new Set<string>())
  const visibilityChangingPaths = ref(new Set<string>())
  const projectAdmissionVersions = new Map<string, number>()
  let visibilityVersion = 0
  let configRead: Promise<AppConfigRead> | null = null
  let configReadSequence = 0
  function readAppConfig(fresh = false): Promise<AppConfigRead> {
    if (configRead && fresh) return configRead.catch(() => undefined).then(() => readAppConfig())
    if (configRead) return configRead
    configRead = Promise.resolve().then(async () => {
      // Shared waiters inherit the actual request's ownership fence, never their later join time.
      const origin = { sequence: ++configReadSequence, intents: { ...settingIntents }, commits: { ...settingCommits }, visibilityVersion }
      return { config: await getAppConfig(), ...origin }
    }).finally(() => { configRead = null })
    return configRead
  }
  let visibilityFailed = false
  let visibilityLoaded = false
  let visibilityLoad: Promise<void> | null = null
  let managedLoad: Promise<void> | null = null
  const addTails = new Map<string, Promise<UnifiedProjectIdentity>>()

  /** Visibility-only read: does not migrate settings, create profiles or launch a CLI. */
  function loadProjectVisibility(force = false): Promise<void> {
    if (!force && (visibilityLoaded || loadStatus.value === 'loaded' && !visibilityFailed)) return Promise.resolve()
    if (visibilityLoad) return visibilityLoad
    const version = visibilityVersion
    visibilityLoad = readAppConfig().then(({ config, visibilityVersion: originVersion }) => {
      if (originVersion !== visibilityVersion) return
      hiddenProjects.value = new Set(config.hiddenProjects ?? [])
      visibilityLoaded = true
      visibilityFailed = false
      ++visibilityVersion
    }).catch(failure => { if (version === visibilityVersion) { visibilityFailed = true; visibilityLoaded = false }; throw failure }).finally(() => { visibilityLoad = null })
    return visibilityLoad
  }
  function loadManagedProjects(): Promise<void> {
    if (managedLoad) return managedLoad
    managedProjectsStatus.value = 'loading'
    managedLoad = getProjects().then(rows => {
      // Preserve explicitly added empty projects while history discovery is in flight.
      const existing = [...cachedProjects.value]
      cachedProjects.value = rows
      for (const row of existing) if (!cachedProjects.value.some(project => normalizePath(project.path) === normalizePath(row.path))) cachedProjects.value.push(row)
      managedProjectsStatus.value = 'ready'
    }).catch(failure => { managedProjectsStatus.value = 'error'; throw failure })
      .finally(() => { managedLoad = null })
    return managedLoad
  }
  function addManagedProject(path: string): Promise<UnifiedProjectIdentity> {
    const key = normalizePath(path)
    if (isProjectAdmissionBlocked(path)) return Promise.reject(new Error('PROJECT_REMOVAL_IN_PROGRESS'))
    const pending = addTails.get(key)
    if (pending) return pending
    const operation = (async () => {
      await loadProjectVisibility()
      const registry = useWorkspaceStore()
      const id = await useCliWorkspaceStore().ensureNativeProjectRegistration({ path })
      const registered = registry.projects.find(project => project.projectId === id)
      if (!registered) throw new Error('PROJECT_REGISTRATION_FAILED')
      await setManagedHidden(registered.selectedPath, false)
      ensureProjectInList(registered.selectedPath)
      return { projectKey: normalizePath(registered.selectedPath), projectPath: registered.selectedPath }
    })()
    addTails.set(key, operation)
    void operation.finally(() => { if (addTails.get(key) === operation) addTails.delete(key) }).catch(() => undefined)
    return operation
  }
  async function setManagedHidden(path: string, hidden: boolean, admit: () => void = () => {}): Promise<void> {
    await loadProjectVisibility()
    try { await setHidden(path, hidden, admit) }
    catch (failure) {
      if (failure instanceof Error && failure.message === 'PROJECT_HAS_OPEN_SESSIONS') throw failure
      // Unknown acknowledgements may already have committed. Reconcile, never replay.
      await loadProjectVisibility(true).catch(() => { visibilityLoaded = false })
      throw failure
    }
  }
  function isProjectRemoving(path: string): boolean { return removingProjectPaths.value.has(normalizePath(path)) }
  function isProjectAdmissionBlocked(path: string): boolean {
    const key = normalizePath(path)
    return removingProjectPaths.value.has(key) || visibilityChangingPaths.value.has(key)
  }
  /** Every caller freezes its own barrier version, preserving restore cancellation. */
  function captureProjectAdmission(path: string): () => boolean {
    const key = normalizePath(path)
    const version = projectAdmissionVersions.get(key) ?? 0
    const allowed = !isProjectAdmissionBlocked(path)
    return () => allowed && version === (projectAdmissionVersions.get(key) ?? 0) && !isProjectAdmissionBlocked(path)
  }
  function markProjectVisibilityChanging(path: string, changing: boolean) {
    const key = normalizePath(path)
    const next = new Set(visibilityChangingPaths.value)
    if (changing) { next.add(key); projectAdmissionVersions.set(key, (projectAdmissionVersions.get(key) ?? 0) + 1) }
    else next.delete(key)
    visibilityChangingPaths.value = next
  }
  function markProjectRemoving(path: string, removing: boolean) {
    const next = new Set(removingProjectPaths.value)
    if (removing) {
      const key = normalizePath(path)
      next.add(key); projectAdmissionVersions.set(key, (projectAdmissionVersions.get(key) ?? 0) + 1)
    } else next.delete(normalizePath(path))
    removingProjectPaths.value = next
  }

  const currentProject = computed(() => {
    if (!cwd.value) return null
    const parts = cwd.value.replace(/\\/g, '/').split('/')
    return parts[parts.length - 1] || cwd.value
  })

  const failedChecks = computed(() => checkResults.value.filter(c => !c.passed))

  async function loadAppConfig() {
    loadStatus.value = 'loading'
    try {
      const read = await readAppConfig()
      const { config, visibilityVersion: version } = read
      hydrateSimpleSettings(read)
      fontSize.value = config.fontSize || 12
      webglRenderer.value = config.webglRenderer ?? false

      // 终端主题：归一化 + 迁移推断（缺失时按 GUI 映射）
      const inferredTerminalTheme = config.terminalTheme
        ? normalizeTerminalThemeId(config.terminalTheme)
        : (config.theme === 'dark' ? 'cc-box-dark' : 'cc-box-light')
      terminalTheme.value = inferredTerminalTheme

      // 加载环境变量（首次使用默认值）
      claudeEnvVars.value = Object.keys(config.claudeEnvVars ?? {}).length > 0
        ? config.claudeEnvVars!
        : { ...DEFAULT_CLAUDE_ENV_VARS }

      // 启动持久化：env + terminalTheme（仅当需修正/迁移时写 terminalTheme）合并为一次调用，
      // 避免多次读-改-写加剧既有竞态（见 spec「已知限制」）
      const needWriteTheme = inferredTerminalTheme !== config.terminalTheme
      const migrationUpdates = {
        claudeEnvVars: claudeEnvVars.value,
        ...(needWriteTheme ? { terminalTheme: inferredTerminalTheme } : {}),
      }
      // Startup migration and simple GUI changes share submission ordering.
      // Hydration is already available, so this cannot wait on its own migration.
      const migration = settingsMutationTail.then(async () => {
        await ensureSettingsWriteReady()
        try { await updateAppConfig(migrationUpdates) }
        catch (failure) {
          if (isUnknownSettingsCommit(failure)) await reconcileUnconfirmedSettings()
          throw failure
        }
      })
      settingsMutationTail = migration.then(() => undefined, () => undefined)
      await migration

      defaultClaudeOptions.value = {
        skipPermissions: config.defaultSkipPermissions ?? false,
        customArgs: config.defaultCustomArgs ?? ''
      }
      claudeOptions.value = {
        resume: '',
        skipPermissions: defaultClaudeOptions.value.skipPermissions,
        customArgs: defaultClaudeOptions.value.customArgs
      }

      // 启动状态源：读 lastOpenedProject + hiddenProjects（v5-T3）
      lastOpenedProject.value = config.lastOpenedProject ?? ''
      // A newer visibility read/write owns publication, even while startup migration awaits.
      if (version === visibilityVersion) {
        hiddenProjects.value = new Set(config.hiddenProjects ?? [])
        visibilityLoaded = true
        visibilityFailed = false
        ++visibilityVersion
      }

      loadStatus.value = 'loaded'
    } catch (err) {
      loadStatus.value = 'error'
      console.error('Failed to load app config:', err)
      throw err // v5 P1：加载失败不吞，向上传播让 UI 感知
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

  async function loadCache(force = false) {
    if (cacheLoaded.value && !force) return
    try {
      // 传 lastOpened/hidden：后端一次扫描同时返回首页数据 + 启动摘要（合并原 get_project_startup_state）
      const data = await getHomeData(PAGE_SIZE, 20, lastOpenedProject.value, [...hiddenProjects.value])
      cachedProjects.value = data.projects
      cachedRecentSessions.value = data.recentSessions
      projectsPage.value = 1
      hasMoreProjects.value = data.hasMore
      startupState.value = data.startupState
      cacheLoaded.value = true
    } catch (err) {
      console.error('Failed to load cache:', err)
      throw err // v5 P1：加载失败不吞，向上传播让 UI 感知
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

  async function refreshCache() {
    cacheLoaded.value = false
    await loadCache()
  }

  function ensureProjectInList(projectPath: string) {
    const normalized = normalizePath(projectPath)
    if (cachedProjects.value.some(p => normalizePath(p.path) === normalized)) return

    const parts = projectPath.replace(/\\/g, '/').split('/')
    cachedProjects.value.unshift({
      path: projectPath,
      name: parts[parts.length - 1] || projectPath,
      lastDuration: Date.now(),
    })
  }

  /** 检查路径是否为已知项目（归一化后匹配 cachedProjects） */
  function isKnownProject(projectPath: string): boolean {
    const normalized = normalizePath(projectPath)
    return cachedProjects.value.some(p => normalizePath(p.path) === normalized)
  }

  function refreshRecentSessions(sessions: SessionInfo[]) {
    cachedRecentSessions.value = sessions
  }

  /** 该项目是否已隐藏（normalized 比较，兼容 Windows 路径大小写/斜杠差异） */
  function isHidden(path: string): boolean {
    const n = normalizePath(path)
    for (const h of hiddenProjects.value) {
      if (normalizePath(h) === n) return true
    }
    return false
  }

  /**
   * setHidden 操作锁（复用 session.ts withLock 范式）：串行化
   * 读集合 -> 算 next -> persist -> 改本地 全流程，防同窗口快速隐藏 A+B 各自基于旧内存
   * 算 next、后 persist 覆盖前导致磁盘丢一项。
   */
  function withHiddenLock<T>(fn: () => Promise<T>): Promise<T> {
    const prev = hiddenOpLock
    let release!: () => void
    hiddenOpLock = new Promise<void>(r => { release = r })
    return (async () => {
      await prev
      try {
        return await fn()
      } finally {
        release()
      }
    })()
  }

  /**
   * lastOpened 操作锁（复用 withLock 范式）：串行化 saveLastProject + setCwdLocal，
   * 防快速 A->B 切换时并发 persist 乱序覆盖 lastOpened。
   */
  function withLastOpenedLock<T>(fn: () => Promise<T>): Promise<T> {
    const prev = lastOpenedOpLock
    let release!: () => void
    lastOpenedOpLock = new Promise<void>(r => { release = r })
    return (async () => {
      await prev
      try {
        return await fn()
      } finally {
        release()
      }
    })()
  }

  /**
   * 设置/取消隐藏项目（opLock 串行 + persist-first 回滚 + 规范化比较 + cwd 保护）。
   * - cwd 保护（v6 codex batch1 #10）：拒绝隐藏当前 cwd（隐藏项目不能成 cwd，store/domain 层保证不变量）。
   *   管理页按钮层禁用是 UI 兜底，此为 domain 层硬保护，防 App.vue 打开入口绕过。
   * - 算 next：规范化比较移除旧条目，hidden=true 时加回（用原始 path 保真）。
   * - 幂等：状态未变则不持久化。
   * - persist-first：成功后才改本地；失败抛错，hiddenProjects 不变。
   */
  async function setHidden(path: string, hidden: boolean, admit: () => void = () => {}): Promise<void> {
    // cwd 保护：隐藏当前 cwd 破坏「隐藏项目不能成 cwd」不变量，直接拒绝（不抛错以兼容 UI 幂等调用，
    // 管理页按钮已 disabled，此处为 domain 层兜底防绕过）。隐藏=false（取消隐藏）不受限。
    if (hidden && cwd.value && normalizePath(path) === normalizePath(cwd.value)) {
      return
    }
    return withHiddenLock(async () => {
      if (hidden && cwd.value && normalizePath(path) === normalizePath(cwd.value)) return
      const n = normalizePath(path)
      const next = new Set<string>()
      let existed = false
      for (const h of hiddenProjects.value) {
        if (normalizePath(h) === n) { existed = true; continue }
        next.add(h)
      }
      if (hidden) next.add(path)
      // 幂等：状态未变则跳过持久化
      if (hidden === existed) return
      // Admit at the actual serialized write boundary, after every earlier await.
      admit()
      // persist-first：失败抛错，本地不变
      await updateAppConfig({ hiddenProjects: [...next] })
      hiddenProjects.value = next
      ++visibilityVersion
    })
  }

  /**
   * 只更新内存 cwd + openedProjectPaths，不持久化。
   * 用途：sessionStart 事务中 spawn 前切 cwd（终端立即用新 cwd 启动），
   * 持久化由后续 setCurrentProject(persist:true) 或 setCwd 完成。
   */
  function setCwdLocal(path: string) {
    cwd.value = path
    openedProjectPaths.value.add(path)
  }

  /**
   * setCurrentProject（persist-first + lastOpenedOpLock 串行）。
   * - persist=true：先 await saveLastProject 成功，再 setCwdLocal；失败抛错且 cwd 不变。
   *   注：sessionStart 事务中 spawn 前已 setCwdLocal（cwd 已切），此处 persist 失败时 cwd 保持
   *   已切状态（终端已跑），仅 lastOpened 未持久化--错误传播让 UI 提示，终端不中断（spec v6 §4.4）。
   * - persist=false / undefined：只 setCwdLocal（恢复启动用，不重复持久化）。
   * - lastOpenedOpLock（v6 codex batch1 #3）：串行化 persist 路径，防快速 A->B 切换并发
   *   saveLastProject 乱序覆盖 lastOpened（A 后切到 B，但 B 的 persist 先完成、A 后完成则磁盘留 A）。
   *   persist=false 不涉及磁盘写，无需串行（直接 setCwdLocal，避免无谓排队）。
   */
  async function setCurrentProject(path: string, opts: { persist?: boolean } = {}): Promise<void> {
    if (opts.persist === true) {
      return withLastOpenedLock(async () => {
        await saveLastProject(path) // 失败抛错，setCwdLocal 不执行
        setCwdLocal(path)
      })
    }
    setCwdLocal(path)
  }

  /** 兼容旧调用方：setCwdLocal + fire-and-forget saveLastProject */
  function setCwd(path: string) {
    setCwdLocal(path)
    saveLastProject(path)
  }

  function setTheme(newTheme: string): Promise<boolean> {
    return saveSimpleSetting('guiThemeMode', newTheme === 'system' || newTheme === 'dark' ? newTheme : 'light')
  }
  function setGuiDensity(value: string): Promise<boolean> { return saveSimpleSetting('guiDensity', value === 'compact' ? 'compact' : 'standard') }
  function setSidebarWidth(value: number): Promise<boolean> { return saveSimpleSetting('sidebarWidth', widthValue(value)) }
  function setStartupDestination(value: string): Promise<boolean> { return saveSimpleSetting('startupDestination', value === 'projects' ? 'projects' : 'workspace') }
  function setDefaultNewCli(value: string): Promise<boolean> { return saveSimpleSetting('defaultNewCli', value === 'codex' ? 'codex' : 'claude') }

  function setTerminalTheme(id: string) {
    const normalized = normalizeTerminalThemeId(id)
    terminalTheme.value = normalized
    updateAppConfig({ terminalTheme: normalized })
  }

  function setFontSize(size: number) {
    fontSize.value = Math.max(10, Math.min(24, size))
    updateAppConfig({ fontSize: size })
  }

  // 渲染后端开关：true=WebGL（高频滚动流畅，但 CJK glyph atlas 可能留白/错位），
  // false=DOM（默认，稳定）。仅对新开终端生效（renderer 在 term.open 时设定）。
  function setWebglRenderer(enabled: boolean) {
    webglRenderer.value = enabled
    updateAppConfig({ webglRenderer: enabled })
  }

  function detectSystemLocale(): 'en' | 'zh' {
    const browserLang = navigator.language || 'en'
    return browserLang.toLowerCase().startsWith('zh') ? 'zh' : 'en'
  }

  function setLanguage(lang: string): Promise<boolean> {
    return saveSimpleSetting('language', lang === 'zh' ? 'zh' : 'en')
  }

  /** 同步当前 claudeEnvVars 到 CC Desk config */
  async function doSyncEnv() {
    await updateAppConfig({ claudeEnvVars: claudeEnvVars.value })
  }

  /** 更新环境变量并同步 */
  async function setClaudeEnvVars(vars: Record<string, string>) {
    claudeEnvVars.value = vars
    await doSyncEnv()
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

  async function toggleAlwaysOnTop() {
    try {
      const win = getCurrentWindow()
      const newState = !alwaysOnTop.value
      await win.setAlwaysOnTop(newState)
      alwaysOnTop.value = newState
    } catch (err) {
      console.error('Failed to toggle always on top:', err)
    }
  }

  return {
    cwd,
    guiThemeMode, guiDensity, sidebarWidth, startupDestination, defaultNewCli, settingsSaveError, loadSettingsPreferences,
    setGuiDensity, setSidebarWidth, setStartupDestination, setDefaultNewCli,
    theme,
    terminalTheme,
    fontSize,
    webglRenderer,
    language,
    claudeEnvVars,
    defaultClaudeOptions,
    claudeOptions,
    currentProject,
    pendingResume,
    shouldAutoOpenSessions,
    checkResults,
    checkFailed,
    failedChecks,
    managedProjectsStatus, loadManagedProjects, loadProjectVisibility, addManagedProject, setManagedHidden, isProjectRemoving, markProjectRemoving, isProjectAdmissionBlocked, captureProjectAdmission, markProjectVisibilityChanging,
    cachedProjects,
    cachedRecentSessions,
    cacheLoaded,
    startupState,
    openedProjectPaths,
    hasMoreProjects,
    isLoadingProjects,
    lastOpenedProject,
    hiddenProjects,
    loadStatus,
    loadAppConfig,
    runChecks: doChecks,
    loadCache,
    loadMoreProjects,
    refreshCache,
    ensureProjectInList,
    isKnownProject,
    refreshRecentSessions,
    isHidden,
    setHidden,
    setCwdLocal,
    setCurrentProject,
    setCwd,
    setTheme,
    setTerminalTheme,
    setFontSize,
    setWebglRenderer,
    setLanguage,
    setClaudeEnvVars,
    resetClaudeEnvVars,
    setClaudeOptions,
    setDefaultClaudeOptions,
    resetClaudeOptions,
    saveAsDefault,
    getClaudeArgs,
    setPendingResume,
    clearPendingResume,
    setAutoOpenSessions,
    alwaysOnTop,
    toggleAlwaysOnTop
  }
})
