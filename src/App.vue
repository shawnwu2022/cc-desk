<template>
<div class="app-root">
  <TitleBar />

  <!-- 环境检查提示 -->
  <div v-if="appStore.checkFailed" class="check-failed-overlay">
    <div class="check-failed-card">
      <h2>{{ t('environmentCheck') }}</h2>
      <div class="check-list">
        <div v-for="check in appStore.checkResults" :key="check.name"
          class="check-item" :class="{ passed: check.passed, failed: !check.passed }">
          <div class="check-status-icon">{{ check.passed ? '✓' : '✗' }}</div>
          <div class="check-detail">
            <div class="check-item-header">
              <span class="check-name">{{ check.name }}</span>
              <span class="check-message">{{ check.message }}</span>
            </div>
            <div v-if="check.passed && check.detectedPath" class="check-detected-path">
              {{ check.detectedPath }}
            </div>
          </div>
        </div>
      </div>

      <!-- 安装进度显示（多任务） -->
      <div v-if="isInstalling" class="install-progress-section">
        <div class="install-tasks">
          <div v-for="task in installTasks" :key="task.name" class="install-task">
            <div class="install-task-header">
              <span class="install-task-name">{{ task.name }}</span>
              <span class="install-task-status">{{ task.status }}</span>
            </div>
            <div class="install-task-progress-bar">
              <div class="install-task-progress-fill" :style="{ width: task.progress + '%' }"></div>
            </div>
          </div>
        </div>
      </div>

      <!-- 按钮：Auto Install 和 Retry -->
      <div class="check-btn-row">
        <button class="check-auto-btn" @click="autoInstall" :disabled="isInstalling">
          {{ isInstalling ? t('installing') : t('autoInstall') }}
        </button>
        <button class="check-retry-btn" @click="retryChecks" :disabled="isInstalling">
          {{ t('retry') }}
        </button>
      </div>
    </div>
  </div>

  <!-- 启动加载/失败门禁 -->
  <div v-if="startupError" class="startup-error-overlay">
    <div class="startup-error-card">
      <h2>{{ t('startupFailed') }}</h2>
      <p class="startup-error-msg">{{ startupError }}</p>
      <button class="startup-retry-btn" @click="initStartup">{{ t('retry') }}</button>
    </div>
  </div>

  <!-- 添加项目 spawn 失败提示（独立于启动门禁）。
       v6 codex batch1 #2：persistFailed=true 时标题/语义切换为「保存失败」（Claude 已跑，重试只重 persist）；
       否则 claudeStartFailed（重试=重 spawn 同目录） -->
  <div v-if="projectSpawnError" class="startup-error-overlay">
    <div class="startup-error-card">
      <h2>{{ projectSpawnError.persistFailed ? t('saveLastOpenedFailed') : t('claudeStartFailed') }}</h2>
      <p class="startup-error-msg">{{ projectSpawnError.msg }}</p>
      <div class="startup-error-actions">
        <button class="startup-cancel-btn" @click="projectSpawnError = null">{{ t('cancel') }}</button>
        <button class="startup-retry-btn" @click="retryProjectSpawn">{{ t('retry') }}</button>
      </div>
    </div>
  </div>

  <!-- 全局设置浮层 -->
  <SettingsOverlay />

  <!-- 终端视图常驻 DOM，保持所有 PTY 和终端实例不销毁 -->
  <TerminalView
    ref="terminalViewRef"
    v-show="currentView === 'terminal'"
    :visible="currentView === 'terminal'"
    @back="handleBack"
    @select-project="handleOpenProject"
  />

  <!-- 覆盖层视图（固定定位叠加在终端之上） -->
  <Transition name="fade" mode="out-in">
    <WelcomeView
      v-if="currentView === 'welcome'"
      class="overlay-view"
      @select-project="handleSelectProject"
    />
    <ProjectSelectView
      v-else-if="currentView === 'projects'"
      class="overlay-view"
      @select-project="handleOpenProject"
      @add-project="handleSelectProject"
      @resume-session="handleResumeSession"
      @back-to-terminal="handleBackToTerminal"
    />
  </Transition>
</div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, defineAsyncComponent, nextTick } from 'vue'
import { useAppStore } from '@/stores/app'
import { applyThemeToDom } from '@/utils/theme'
import { sameProjectPath } from '@/utils/path'
import { useHookStore } from '@/stores/hook'
import { useSessionStore } from '@/stores/session'
import { useSidebarStore } from '@/stores/sidebar'
import { useUpdateStore } from '@/stores/update'
import { useI18n } from 'vue-i18n'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-shell'
import {
  selectDirectory,
  onMenuSettings,
  onMenuShortcuts,
  onConfigFontSize,
  onTerminalRestart,
  checkForUpdates,
  getInstalledClaudeVersion,
  downloadAndInstallClaude,
  downloadAndInstallGit,
  onInstallProgress,
  onOpenDirectory
} from '@/api/tauri'
import { useAppShortcuts } from '@/composables/useAppShortcuts'
import { decideStartupView } from '@/composables/useStartupDecision'
import { isPersistFailedError } from '@/composables/useSessionStartWaiter'
import WelcomeView from '@/components/WelcomeView.vue'
import ProjectSelectView from '@/components/ProjectSelectView.vue'
import TitleBar from '@/components/TitleBar.vue'

const TerminalView = defineAsyncComponent(() => import('@/components/TerminalView.vue'))
const SettingsOverlay = defineAsyncComponent(() => import('@/components/settings/SettingsOverlay.vue'))

type ViewType = 'welcome' | 'projects' | 'terminal'

const appStore = useAppStore()
const sessionStore = useSessionStore()
const sidebarStore = useSidebarStore()
const updateStore = useUpdateStore()
const { t } = useI18n()
const { setupShortcutListeners } = useAppShortcuts()
const currentView = ref<ViewType>('welcome')
const terminalViewRef = ref()

// 启动加载/失败门禁（v5-T7）：initStartup 中三路并行加载或启动摘要门禁失败时置位，
// 弹出错误卡 + 重试按钮；成功后清空。
const startupError = ref<string | null>(null)

// 添加项目 spawn 失败提示（v5-T7 concern 1）：独立于 startupError，避免重试语义错位。
// 用户要重 spawn 同目录，而非重跑 initStartup（重新决策），故单独 overlay + 重试按钮直连 startProjectSession。
// v6 codex batch1 #2：persistFailed 标记区分 sessionStart 成功后 lastOpened 持久化失败（Claude 已跑，
// 不重 spawn，重试只重 persist）。标题/文案据 persistFailed 切换（saveLastOpenedFailed vs claudeStartFailed）。
const projectSpawnError = ref<{ path: string; msg: string; persistFailed: boolean } | null>(null)

// 应用 GUI 主题到 DOM。loadAppConfig 是异步 fire-and-forget：initAfterChecks 先用 store 初始值
// （'light'）设置 DOM，待 loadAppConfig 把 theme.value 更新为持久化值后，由该 watch 同步到 DOM。
// setTheme（设置页实时切换）也会触发它，保证两条路径都生效。
watch(() => appStore.theme, (newTheme) => applyThemeToDom(newTheme))

// 自动安装状态
const isInstalling = ref(false)

// 多任务进度跟踪
interface InstallTask {
  name: string       // "Claude CLI" | "Git"
  status: string     // "waiting" | "fetching" | "downloading" | "extracting" | "done" | "error"
  progress: number   // 0-100
  message: string
}

const installTasks = ref<InstallTask[]>([])

let unlistenInstallProgress: (() => void) | null = null
let unlistenOpenDir: (() => void) | null = null

// Unlisten functions for cleanup
let unlistenSettings: (() => void) | null = null
let unlistenShortcuts: (() => void) | null = null
let unlistenFontSize: (() => void) | null = null
let unlistenRestart: (() => void) | null = null
// 窗口聚焦 reload 监听（多实例/外部修改同步），独立于 useWindowAttention 解耦
let unlistenFocusReload: (() => void) | null = null
const shortcutUnlisteners: (() => void)[] = []

onMounted(async () => {
  await appStore.runChecks()
  if (appStore.checkFailed) {
    return
  }

  initAfterChecks()
  await initStartup()

  // 窗口聚焦时拉取最新 projects.json：多实例并发写后切回本窗口可见最新状态，
  // 亦覆盖外部直接编辑 projects.json 的场景。独立监听，不塞进 useWindowAttention（解耦）。
  // reloadProjectsState 与 action 共用 opLock，串行完整 request + apply；静默失败不打断聚焦。
  const win = getCurrentWindow()
  unlistenFocusReload = await win.onFocusChanged(({ payload: focused }) => {
    if (focused) sessionStore.reloadProjectsState()
  })
})

onUnmounted(() => {
  unlistenSettings?.()
  unlistenShortcuts?.()
  unlistenFontSize?.()
  unlistenRestart?.()
  unlistenInstallProgress?.()
  unlistenOpenDir?.()
  unlistenFocusReload?.()
  shortcutUnlisteners.forEach(fn => fn())
  window.removeEventListener('app:toggleHome', handleToggleHome)
})

async function handleSelectProject() {
  const result = await selectDirectory()
  if (!result) return
  const path = result.path
  // v6 codex batch1 #10：隐藏项目不能成 cwd，目录打开入口兜底（管理页按钮层禁用之外的硬保护）。
  if (appStore.isHidden(path)) {
    projectSpawnError.value = { path, msg: t('showFirst'), persistFailed: false }
    return
  }
  const existing = appStore.cachedProjects.find(p => sameProjectPath(p.path, path))
  if (existing) {
    // 已存在项目走统一切换
    handleOpenProject(path)
    return
  }
  // 新项目：进终端 + startProjectSession 事务（spawn 后切 cwd + 等 sessionStart 持久化）。
  // cancelled/spawnFail/timeout/提前退出 reject 统一 catch -> projectSpawnError 提示
  // （T6 concern：unmount 期间 reject 'cancelled' 亦走此路径，不悬空）。
  // v6 codex batch1 #2：persist_failed（sessionStart 成功但持久化失败）单独区分--Claude 已跑，
  // 不重 spawn，重试只重 persist（retryProjectSpawn 据 persistFailed 标记走 retryPersist）。
  currentView.value = 'terminal'
  await nextTick()
  try {
    appStore.setClaudeOptions({ resume: '' }) // 新项目不恢复会话
    await terminalViewRef.value?.startProjectSession(path)
  } catch (e) {
    if (isPersistFailedError(e)) {
      // persist 失败：Claude 已成功运行，保留 tab，错误提示「保存失败」，重试只重 persist
      projectSpawnError.value = { path, msg: t('saveLastOpenedFailed'), persistFailed: true }
    } else {
      // 失败分离 projectSpawnError（不混 startupError）：重试语义=重 spawn 同 path，非重跑 initStartup
      projectSpawnError.value = { path, msg: t('claudeStartFailed') + ': ' + String(e), persistFailed: false }
    }
  }
}

// 添加项目 spawn 失败重试：
// - spawn 失败（persistFailed=false）：重 spawn 同 path（非重跑 initStartup 决策，lastOpened 未持久化结果不可预测）。
// - persist 失败（persistFailed=true，v6 codex batch1 #2）：Claude 已跑，不重 spawn，仅重 persist lastOpened。
//   失败再次设 projectSpawnError（保持 persistFailed 标记），成功则 overlay 自动消失。
async function retryProjectSpawn() {
  if (!projectSpawnError.value) return
  const { path, persistFailed } = projectSpawnError.value
  projectSpawnError.value = null
  if (persistFailed) {
    // 只重 persist：Claude 已跑，不重 spawn
    try {
      await appStore.setCurrentProject(path, { persist: true })
    } catch (e) {
      projectSpawnError.value = { path, msg: t('saveLastOpenedFailed'), persistFailed: true }
    }
    return
  }
  try {
    await terminalViewRef.value?.startProjectSession(path)
  } catch (e) {
    if (isPersistFailedError(e)) {
      projectSpawnError.value = { path, msg: t('saveLastOpenedFailed'), persistFailed: true }
    } else {
      projectSpawnError.value = { path, msg: t('claudeStartFailed') + ': ' + String(e), persistFailed: false }
    }
  }
}

/** 判定错误是否为 persist_failed：使用 useSessionStartWaiter 的 isPersistFailedError（i18n 无关，code 标记） */

async function handleOpenProject(path: string) {
  // v6 codex batch1 #10：隐藏项目不能成 cwd（domain 层兜底，管理页按钮禁用之外的硬保护）。
  if (appStore.isHidden(path)) {
    projectSpawnError.value = { path, msg: t('showFirst'), persistFailed: false }
    return
  }
  // v6 codex batch1 #3：setCwd 改 setCurrentProject(persist:true) 可等待 + 错误传播。
  //   先 setCwdLocal 同步切 cwd 保 UI 响应，再 setCurrentProject 持久化；persist 失败提示用户但不中断打开。
  appStore.setCwdLocal(path)
  currentView.value = 'terminal'
  // 启动时自动加载 sidebar 数据
  sidebarStore.loadAllSidebarData(path)
  appStore.setCurrentProject(path, { persist: true }).catch(err => {
    console.error('[App] handleOpenProject persist failed:', err)
    projectSpawnError.value = { path, msg: t('saveLastOpenedFailed'), persistFailed: true }
  })

  // 当前激活 tab 属于该项目则保持，否则切换到运行中 Tab
  const activeTab = sessionStore.activeTab
  if (activeTab && sameProjectPath(activeTab.projectPath, path)) return

  await nextTick()
  const runningTab = sessionStore.getRunningTabForProject(path)
  if (runningTab) {
    sessionStore.setActiveTab(runningTab.tabId)
  }
}

async function handleResumeSession(projectPath: string, sessionId: string, sessionName?: string) {
  // v6 codex batch1 #10：隐藏项目不能成 cwd。
  if (appStore.isHidden(projectPath)) {
    projectSpawnError.value = { path: projectPath, msg: t('showFirst'), persistFailed: false }
    return
  }
  // v6 codex batch1 #3：setCwd 改 setCurrentProject(persist:true) 可等待 + 错误传播。
  appStore.setCwdLocal(projectPath)
  appStore.setCurrentProject(projectPath, { persist: true }).catch(err => {
    console.error('[App] handleResumeSession persist failed:', err)
    projectSpawnError.value = { path: projectPath, msg: t('saveLastOpenedFailed'), persistFailed: true }
  })

  // 如果该 session 已在运行，直接切换到对应 tab
  const existingTab = sessionStore.getTabBySessionId(sessionId)
  if (existingTab && existingTab.status === 'running') {
    sessionStore.setActiveTab(existingTab.tabId)
    currentView.value = 'terminal'
    sidebarStore.loadAllSidebarData(projectPath)
    return
  }

  currentView.value = 'terminal'
  sidebarStore.loadAllSidebarData(projectPath)

  // 直接调用 TerminalView 方法恢复会话（不再依赖 watch）
  // v6 codex batch1 #12：await startResumeSession + catch 失败传播（startResumeSession 现在
  // await startTab + 失败抛错），恢复失败提示用户 + 重试（重 spawn）。
  await nextTick()
  if (terminalViewRef.value?.startResumeSession) {
    try {
      await terminalViewRef.value.startResumeSession(projectPath, sessionId, sessionName)
    } catch (e) {
      console.error('[App] handleResumeSession startResumeSession failed:', e)
      projectSpawnError.value = { path: projectPath, msg: t('claudeStartFailed') + ': ' + String(e), persistFailed: false }
    }
  } else {
    // 异步组件尚未加载的 fallback
    appStore.setClaudeOptions({ resume: sessionId })
    appStore.setPendingResume(sessionId, sessionName)
  }
}

function handleBack() {
  currentView.value = 'projects'
}

// 管理页「返回终端」按钮：仅在有 cwd 时可点（按钮自身禁用门禁，此为切换逻辑）。
function handleBackToTerminal() {
  if (!appStore.cwd) return
  currentView.value = 'terminal'
}

function handleToggleHome() {
  if (currentView.value === 'terminal') {
    currentView.value = 'projects'
  } else if (currentView.value === 'projects' && appStore.cwd) {
    currentView.value = 'terminal'
  }
}

function openUrl(url: string) {
  open(url)
}

async function retryChecks() {
  await appStore.runChecks(true)
  if (!appStore.checkFailed) {
    initAfterChecks()
    await initStartup()
  }
}

function initAfterChecks() {
  // 先用 store 初始值（'light'）应用主题到 DOM，避免首屏闪烁；
  // loadAppConfig（在 initStartup 内并行执行）完成后会把 theme.value 更新为持久化值，
  // 由 setup 中的 watch(appStore.theme) 同步到 DOM（见上方）
  applyThemeToDom(appStore.theme)

  useHookStore().init()

  shortcutUnlisteners.push(...setupShortcutListeners())

  onMenuSettings(() => {
    sidebarStore.openSettings()
  }).then(fn => { unlistenSettings = fn })

  onMenuShortcuts(() => {
    sidebarStore.openSettings('shortcuts')
  }).then(fn => { unlistenShortcuts = fn })

  onConfigFontSize((size) => {
    appStore.setFontSize(size)
  }).then(fn => { unlistenFontSize = fn })

  onTerminalRestart((data) => {
    if (currentView.value === 'terminal') {
      appStore.setCwd(data.cwd)
    }
  }).then(fn => { unlistenRestart = fn })

  window.addEventListener('app:toggleHome', handleToggleHome)

  checkForUpdates().then(info => {
    if (info) {
      sidebarStore.setUpdateInfo(info)
      updateStore.setUpdateInfo(info)
    }
  }).catch(() => {})

  // 启动只读本地 Claude CLI 版本号，不发 HTTP 请求对比 OSS
  getInstalledClaudeVersion().then(version => {
    updateStore.setInstalledClaudeVersion(version)
  }).catch(() => {})

  // 监听右键菜单传入的目录
  // v6 codex batch1 #3/#10：setCwd 改 setCurrentProject(persist:true) 可等待；隐藏项目拒绝成 cwd。
  onOpenDirectory((dir) => {
    // 隐藏项目不能成 cwd（domain 层兜底，复用管理页 showFirst 语义）
    if (appStore.isHidden(dir)) {
      projectSpawnError.value = { path: dir, msg: t('showFirst'), persistFailed: false }
      return
    }
    if (appStore.isKnownProject(dir)) {
      handleOpenProject(dir)
    } else {
      appStore.setCwdLocal(dir)
      appStore.setClaudeOptions({ resume: '' })
      currentView.value = 'terminal'
      sidebarStore.loadAllSidebarData(dir)
      appStore.setCurrentProject(dir, { persist: true }).catch(err => {
        console.error('[App] onOpenDirectory persist failed:', err)
        projectSpawnError.value = { path: dir, msg: t('saveLastOpenedFailed'), persistFailed: true }
      })
    }
  }).then(fn => { unlistenOpenDir = fn })
}

/**
 * 启动协调（v5-T7 §4.2，性能 #2 合并双扫）：
 * loadAppConfig 先（拿 lastOpened/hidden，供 loadCache 合并扫描）-> loadCache（合并 getHomeData+startup_state，一次扫描）‖ loadProjectsState
 * -> 门禁（任一失败 -> 错误+重试）-> 用缓存 startupState 决策。
 *
 * 收尾 T3/T4 unhandled：loadAppConfig/loadCache（T3 改抛错）/ loadProjectsState（T4 rethrow）
 * 原本在 initAfterChecks 里 fire-and-forget 调用会产生 unhandled rejection；现统一收进
 * Promise.allSettled，reject 由 results 显式消费，不再悬空。
 *
 * 性能 #2：原 loadCache（getHomeData）与 get_project_startup_state 各扫一次 ~/.claude/projects/；
 * 现合并为 get_home_data 一次扫描返回 HomeData+startup_state（store::assemble_home_data）。
 */
async function initStartup() {
  startupError.value = null
  try {
    // 1. loadAppConfig 先（拿 lastOpened/hidden，供 loadCache 合并扫描；只读 config.json，轻）
    await appStore.loadAppConfig()
    // 2. 并行加载（loadCache 合并首页数据+启动摘要，一次扫描；loadProjectsState 独立）
    const results = await Promise.allSettled([
      appStore.loadCache(true),
      sessionStore.loadProjectsState(),
    ])
    const failed = results.find(r => r.status === 'rejected') as PromiseRejectedResult | undefined
    if (failed) {
      startupError.value = String(failed.reason)
      return
    }
    // 3. 启动摘要（来自 loadCache 合并的数据，无需再扫）
    const state = appStore.startupState
    if (!state) {
      startupError.value = 'startup state unavailable'
      return
    }
    // 4. lastOpenedProjectInfo 注入缓存（确保树里可高亮，含分页 12 外）
    if (state.lastOpenedProjectInfo) {
      appStore.ensureProjectInList(state.lastOpenedProjectInfo.path)
    }
    // 5. 决策
    const decision = decideStartupView(state, appStore.lastOpenedProject, appStore.isHidden)
    currentView.value = decision.view
    if (decision.openSessionsPanel) {
      sidebarStore.togglePanel('sessions')
    }
    if (decision.restoreProject) {
      // 恢复上次项目：persist:false（lastOpened 已持久化，不重复写）
      await appStore.setCurrentProject(decision.restoreProject, { persist: false })
    }
  } catch (e) {
    startupError.value = String(e)
  }
}

// 自动安装（并发执行）
async function autoInstall() {
  isInstalling.value = true

  // 初始化任务列表
  installTasks.value = []

  // 根据检查结果添加任务
  for (const check of appStore.checkResults) {
    if (!check.passed) {
      installTasks.value.push({
        name: check.name,
        status: 'waiting',
        progress: 0,
        message: t('installWaiting')
      })
    }
  }

  // 监听进度事件
  onInstallProgress((progress) => {
    const taskName = progress.item === 'claude' ? 'Claude CLI' : 'Git Bash'
    const task = installTasks.value.find(t => t.name === taskName)
    if (task) {
      task.status = progress.stage
      task.progress = progress.progress
      task.message = progress.message
    }
  }).then(fn => { unlistenInstallProgress = fn })

  try {
    // 并发安装所有缺失项
    const installPromises: Promise<void>[] = []

    const needsClaude = appStore.checkResults.some(c => c.name === 'Claude CLI' && !c.passed)
    if (needsClaude) {
      installPromises.push(
        downloadAndInstallClaude().then(() => {
          const task = installTasks.value.find(item => item.name === 'Claude CLI')
          if (task) {
            task.status = 'done'
            task.progress = 100
            task.message = t('installComplete')
          }
        })
      )
    }

    const needsGit = appStore.checkResults.some(c => c.name === 'Git Bash' && !c.passed)
    if (needsGit) {
      installPromises.push(
        downloadAndInstallGit().then(() => {
          const task = installTasks.value.find(item => item.name === 'Git Bash')
          if (task) {
            task.status = 'done'
            task.progress = 100
            task.message = t('installComplete')
          }
        })
      )
    }

    // 等待所有安装完成
    await Promise.all(installPromises)

    // 延迟一秒后重新检查
    await new Promise(r => setTimeout(r, 1000))

    // 重新运行检查（会自动添加 PATH）
    await appStore.runChecks(true)

    // 检查是否全部通过
    if (!appStore.checkFailed) {
      initAfterChecks()
      await initStartup()
    } else {
      // 如果仍有失败项，更新错误信息
      for (const task of installTasks.value) {
        if (task.status !== 'done') {
          task.status = 'error'
          task.message = t('installVerifyFailed')
        }
      }
    }
  } catch (e) {
    // 更新错误状态
    for (const task of installTasks.value) {
      if (task.status !== 'done') {
        task.status = 'error'
        task.message = t('installFailed', { error: String(e) })
      }
    }
    console.error('Auto install failed:', e)
  } finally {
    isInstalling.value = false
    unlistenInstallProgress?.()
    unlistenInstallProgress = null
  }
}
</script>

<style scoped>
.app-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}

.overlay-view {
  position: fixed;
  top: 32px;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 10;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.check-failed-overlay {
  position: fixed;
  top: 32px;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
}

.check-failed-card {
  background: var(--bg-primary);
  border-radius: var(--radius-lg);
  padding: 24px 32px;
  max-width: 420px;
  width: 90%;
  box-shadow: var(--shadow-lg);
}

.check-failed-card h2 {
  font-size: 16px;
  font-weight: 600;
  color: var(--status-error);
  margin-bottom: 16px;
}

.check-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-bottom: 20px;
}

.check-item {
  display: flex;
  gap: 10px;
  padding: 10px 12px;
  background: var(--bg-secondary);
  border-radius: var(--radius-md);
}

.check-item.passed {
  border-left: 3px solid var(--status-success, #4caf50);
}

.check-item.failed {
  border-left: 3px solid var(--status-error, #ef5350);
}

.check-status-icon {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  font-weight: 700;
}

.check-item.passed .check-status-icon {
  color: var(--status-success, #4caf50);
}

.check-item.failed .check-status-icon {
  color: var(--status-error, #ef5350);
}

.check-detail {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.check-item-header {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.check-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.check-message {
  font-size: 12px;
  color: var(--text-secondary);
}

.check-detected-path {
  font-size: 11px;
  color: var(--text-secondary);
  font-family: var(--font-mono);
  padding: 2px 6px;
  background: var(--bg-primary);
  border-radius: var(--radius-sm);
  word-break: break-all;
}

.check-btn-row {
  display: flex;
  gap: 8px;
  margin-top: 16px;
}

.check-auto-btn {
  flex: 1;
  padding: 10px;
  background: var(--status-success, #4caf50);
  color: var(--text-inverse);
  border: none;
  border-radius: var(--radius-md);
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
}

.check-auto-btn:hover:not(:disabled) {
  opacity: 0.9;
}

.check-auto-btn:disabled {
  opacity: 0.6;
  cursor: wait;
}

.check-retry-btn {
  flex: 1;
  padding: 10px;
  background: transparent;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-secondary);
  cursor: pointer;
}

.check-retry-btn:hover:not(:disabled) {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}

.check-retry-btn:disabled {
  opacity: 0.5;
  cursor: wait;
}

/* 多任务进度 */
.install-progress-section {
  margin-top: 16px;
  padding: 16px;
  background: var(--bg-secondary);
  border-radius: var(--radius-md);
}

.install-tasks {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.install-task {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.install-task-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.install-task-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.install-task-status {
  font-size: 12px;
  color: var(--text-secondary);
}

.install-task-progress-bar {
  height: 6px;
  background: var(--bg-primary);
  border-radius: 3px;
  overflow: hidden;
}

.install-task-progress-fill {
  height: 100%;
  background: var(--accent-primary);
  transition: width 0.3s ease;
}

/* 启动加载失败门禁 */
.startup-error-overlay {
  position: fixed;
  top: 32px;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
}

.startup-error-card {
  background: var(--bg-primary);
  border-radius: var(--radius-lg);
  padding: 24px 32px;
  max-width: 420px;
  width: 90%;
  box-shadow: var(--shadow-lg);
  text-align: center;
}

.startup-error-card h2 {
  font-size: 16px;
  font-weight: 600;
  color: var(--status-error);
  margin-bottom: 16px;
}

.startup-error-msg {
  font-size: 13px;
  color: var(--text-secondary);
  margin-bottom: 20px;
  word-break: break-word;
}

.startup-retry-btn {
  padding: 10px 24px;
  background: var(--accent-primary);
  color: var(--text-inverse);
  border: none;
  border-radius: var(--radius-md);
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
}

.startup-retry-btn:hover {
  opacity: 0.9;
}

.startup-error-actions {
  display: flex;
  gap: 12px;
  justify-content: center;
}

.startup-cancel-btn {
  padding: 10px 24px;
  background: transparent;
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
}

.startup-cancel-btn:hover {
  opacity: 0.8;
}
</style>
