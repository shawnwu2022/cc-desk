<template>
  <!-- 环境检查提示 -->
  <div v-if="appStore.checkFailed" class="check-failed-overlay">
    <div class="check-failed-card">
      <h2>Environment Check</h2>
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
            <div v-if="!check.passed" class="check-fail-actions">
              <div class="path-input-row">
                <input type="text" v-model="checkInputs[check.name]" :placeholder="'Set ' + check.name + ' path...'" />
                <button class="path-browse-btn" @click="browseFor(check.name)">Browse</button>
              </div>
              <button v-if="check.url" class="check-action-btn" @click="openUrl(check.url)">
                {{ check.action }}
              </button>
            </div>
          </div>
        </div>
      </div>
      <div class="check-btn-row">
        <button class="check-save-btn" @click="savePathsAndRetry" :disabled="isSavingPaths">
          {{ isSavingPaths ? 'Saving...' : 'Save & Retry' }}
        </button>
        <button class="check-retry-btn" @click="retryChecks">Retry</button>
      </div>
    </div>
  </div>

  <!-- 全局设置浮层 -->
  <SettingsOverlay />

  <!-- 终端视图常驻 DOM，保持所有 PTY 和终端实例不销毁 -->
  <TerminalView
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
      @open-settings="sidebarStore.openSettings()"
    />
  </Transition>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, defineAsyncComponent } from 'vue'
import { useAppStore } from '@/stores/app'
import { useSidebarStore } from '@/stores/sidebar'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-shell'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import {
  selectDirectory,
  onMenuSettings,
  onMenuShortcuts,
  onConfigFontSize,
  onTerminalRestart,
  updateAppConfig,
  checkForUpdates
} from '@/api/tauri'
import { useAppShortcuts } from '@/composables/useAppShortcuts'
import WelcomeView from '@/components/WelcomeView.vue'
import ProjectSelectView from '@/components/ProjectSelectView.vue'

const TerminalView = defineAsyncComponent(() => import('@/components/TerminalView.vue'))
const SettingsOverlay = defineAsyncComponent(() => import('@/components/settings/SettingsOverlay.vue'))

type ViewType = 'welcome' | 'projects' | 'terminal'

const appStore = useAppStore()
const sidebarStore = useSidebarStore()
const { handleKeydown, setupFocusRecovery, cleanup: cleanupShortcuts } = useAppShortcuts()
const currentView = ref<ViewType>('welcome')

// 路径输入（key 为 check name）
const checkInputs = ref<Record<string, string>>({})
const isSavingPaths = ref(false)

// Unlisten functions for cleanup
let unlistenSettings: (() => void) | null = null
let unlistenShortcuts: (() => void) | null = null
let unlistenFontSize: (() => void) | null = null
let unlistenRestart: (() => void) | null = null

onMounted(async () => {
  // 全局快捷键
  window.addEventListener('keydown', handleKeydown, true)
  await setupFocusRecovery()

  // 环境检查（同步拉取 Rust 已缓存的结果）
  await appStore.runChecks()
  if (appStore.checkFailed) {
    // 回填输入框：用检测结果中的 detectedPath
    for (const check of appStore.checkResults) {
      if (check.detectedPath) {
        checkInputs.value[check.name] = check.detectedPath
      }
    }
    return
  }

  initAfterChecks()
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown, true)
  cleanupShortcuts()
  unlistenSettings?.()
  unlistenShortcuts?.()
  unlistenFontSize?.()
  unlistenRestart?.()
})

async function handleSelectProject() {
  const result = await selectDirectory()
  if (result) {
    appStore.setCwd(result.path)
    appStore.setClaudeOptions({
      resume: '',
      skipPermissions: false,
      customArgs: ''
    })
    currentView.value = 'terminal'
    // 启动时自动加载 sidebar 数据
    sidebarStore.loadAllSidebarData(result.path)
  }
}

async function handleOpenProject(path: string) {
  appStore.setCwd(path)
  currentView.value = 'terminal'
  // 启动时自动加载 sidebar 数据
  sidebarStore.loadAllSidebarData(path)
}

function handleResumeSession(projectPath: string, sessionId: string, sessionName?: string) {
  appStore.setCwd(projectPath)
  appStore.setClaudeOptions({ resume: sessionId })
  appStore.setPendingResume(sessionId, sessionName)
  currentView.value = 'terminal'
  // 启动时自动加载 sidebar 数据
  sidebarStore.loadAllSidebarData(projectPath)
}

function handleBack() {
  currentView.value = 'projects'
}

function openUrl(url: string) {
  open(url)
}

async function retryChecks() {
  await appStore.runChecks(true)
  if (!appStore.checkFailed) {
    initAfterChecks()
  } else {
    for (const check of appStore.checkResults) {
      if (check.detectedPath) {
        checkInputs.value[check.name] = check.detectedPath
      }
    }
  }
}

async function browseFor(name: string) {
  const result = await openDialog({
    multiple: false,
    filters: [{ name: 'Executable', extensions: ['exe'] }]
  })
  if (result && typeof result === 'string') {
    checkInputs.value[name] = result
  }
}

function initAfterChecks() {
  appStore.loadAppConfig()

  appStore.loadCache().then(() => {
    if (appStore.cachedProjects.length > 0 && currentView.value === 'welcome') {
      currentView.value = 'projects'
    }
  })

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

  // 后台检查更新
  checkForUpdates().then(info => {
    if (info.hasUpdate) {
      sidebarStore.updateAvailable = true
    }
  }).catch(() => {})
}

async function savePathsAndRetry() {
  isSavingPaths.value = true
  try {
    const updates: Record<string, string | null> = {}
    for (const check of appStore.checkResults) {
      // 只处理未通过的检查项，不覆盖已通过的路径
      if (!check.passed) {
        const val = checkInputs.value[check.name]?.trim()
        if (check.name === 'Claude CLI') updates.claudePath = val || null
        if (check.name === 'Git Bash') updates.gitBashPath = val || null
      }
    }
    // 即使是删除操作（传 null），也需要调用 updateAppConfig
    await updateAppConfig(updates)
    await appStore.runChecks(true)
    if (!appStore.checkFailed) {
      initAfterChecks()
    } else {
      // 重新回填 detectedPath
      for (const check of appStore.checkResults) {
        if (check.detectedPath) {
          checkInputs.value[check.name] = check.detectedPath
        }
      }
    }
  } finally {
    isSavingPaths.value = false
  }
}
</script>

<style scoped>
.overlay-view {
  position: fixed;
  inset: 0;
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
  inset: 0;
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

.check-fail-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 4px;
}

.check-action-btn {
  display: inline-flex;
  align-self: flex-start;
  padding: 4px 10px;
  background: var(--accent-primary);
  color: var(--text-inverse);
  border: none;
  border-radius: var(--radius-sm);
  font-size: 11px;
  cursor: pointer;
}

.check-action-btn:hover {
  opacity: 0.9;
}

.path-input-row {
  display: flex;
  gap: 6px;
}

.path-input-row input {
  flex: 1;
  padding: 6px 10px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: 12px;
  color: var(--text-primary);
  font-family: var(--font-mono);
  min-width: 0;
}

.path-input-row input:focus {
  outline: none;
  border-color: var(--focus-ring);
}

.path-browse-btn {
  padding: 6px 10px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  white-space: nowrap;
}

.path-browse-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}

.check-btn-row {
  display: flex;
  gap: 8px;
  margin-top: 16px;
}

.check-save-btn {
  flex: 1;
  padding: 8px;
  background: var(--accent-primary);
  color: var(--text-inverse);
  border: none;
  border-radius: var(--radius-md);
  font-size: 13px;
  cursor: pointer;
}

.check-save-btn:hover:not(:disabled) {
  opacity: 0.9;
}

.check-save-btn:disabled {
  opacity: 0.5;
  cursor: wait;
}

.check-retry-btn {
  padding: 8px 16px;
  background: transparent;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: 13px;
  color: var(--text-secondary);
  cursor: pointer;
}

.check-retry-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}
</style>