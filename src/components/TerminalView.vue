<template>
  <div
    class="terminal-view"
    data-terminal-view
    :style="terminalSurfaceStyle"
  >
    <!-- 图标栏（常驻） -->
    <IconBar
      :active-panel="sidebarStore.activePanel"
      @toggle="handleTogglePanel"
      @toggle-settings="sidebarStore.toggleSettings()"
      @open-folder="handleOpenFolder"
    />

    <!-- 侧边栏 + 终端 -->
    <SidebarPanel
      :visible="sidebarStore.panelVisible"
      :active-panel="sidebarStore.activePanel"
      @close="sidebarStore.closePanel"
      @switch-session="handleSwitchSession"
      @rename-session="handleRenameSession"
      @restart-session="handleRestartSession"
      @new-session="handleNewSession"
      @resume-session="handleResumeSession"
      @close-tab="handleCloseTab"
      @close-all-tabs="handleCloseAllTabs"
      @close-other-tabs="handleCloseOtherTabs"
      @new-session-in="handleNewSessionIn"
      @close-all-sessions-in="handleCloseAllSessionsIn"
      @open-in-explorer="handleOpenInExplorer"
      @resume-session-in-project="(path, id) => handleSwitchToProjectSession(path, id)"
      @pin-project="handlePin"
      @unpin-project="handleUnpin"
      @archive-session="(path, id) => handleArchive(path, id)"
      @restore-session="(path, id) => handleRestore(path, id)"
    />

    <!-- 主内容区 -->
    <div class="main-content">
      <TerminalHeader
        :project-name="appStore.currentProject || t('claudeCode')"
        @back="handleBack"
      />
      <div class="terminal-container">
        <!-- 空状态提示：没有任何 session -->
        <div v-if="showEmptyState" class="empty-state-overlay">
          <div class="empty-state-content">
            <p class="empty-state-text">{{ t('startNewSession') }}</p>
            <button class="empty-state-btn" @click="handleNewSession">
              <img src="@/assets/icons/plus.svg" alt="New" />
              {{ t('newSession') }}
            </button>
          </div>
        </div>
        <XTermTerminal
          ref="terminalRef"
          :font-size="appStore.fontSize"
          @pty-started="handlePtyStarted"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch, nextTick, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { computeTerminalSurfaceVars, getTerminalTheme } from '@/config/terminalThemes'
import { useSessionStore } from '@/stores/session'
import { useSidebarStore, type SidebarPanelType } from '@/stores/sidebar'
import { useConfigStore } from '@/stores/config'
import { openInFileManager, logMessage } from '@/api/tauri'
import { sendTerminalCommand } from '@/composables/useTerminalCommand'
import { useWindowAttention } from '@/composables/useWindowAttention'
import { useStatusMonitor } from '@/composables/useStatusMonitor'
import { resolveSwitchAction } from '@/composables/useProjectTreeNavigation'
import { getCurrentWindow } from '@tauri-apps/api/window'
import TerminalHeader from './TerminalHeader.vue'
import XTermTerminal from './XTermTerminal.vue'
import IconBar from './IconBar.vue'
import SidebarPanel from './sidebar/SidebarPanel.vue'

const emit = defineEmits<{
  back: []
}>()

const props = defineProps<{
  visible?: boolean
}>()

const appStore = useAppStore()
const sessionStore = useSessionStore()
const sidebarStore = useSidebarStore()
const configStore = useConfigStore()
const terminalRef = ref()

// 终端表面色 CSS 变量：随终端主题变化（与 GUI 浅/暗独立），向下继承给容器/滚动条/空态
const terminalSurfaceStyle = computed(() =>
  computeTerminalSurfaceVars(getTerminalTheme(appStore.terminalTheme))
)

const { t } = useI18n()

const { isFocused } = useWindowAttention()
const isTerminalVisible = computed(() => props.visible ?? false)
useStatusMonitor({ isFocused, isTerminalVisible })

// 空状态：当前项目没有任何 tab
const showEmptyState = computed(() => {
  const cwd = appStore.cwd
  if (!cwd) return false
  const projectTabs = sessionStore.getProjectTabs(cwd)
  return projectTabs.length === 0
})

function updateWindowTitle(cwd: string) {
  const parts = cwd.replace(/\\/g, '/').split('/')
  const folderName = parts[parts.length - 1] || 'CC-Box'
  getCurrentWindow().setTitle(folderName).catch(() => {})
}

async function startResumeSession(projectPath: string, sessionId: string, sessionName?: string) {
  const tabId = sessionStore.createTab(projectPath, { sessionId, name: sessionName })
  sessionStore.setActiveTab(tabId)
  await nextTick()
  if (terminalRef.value?.startTab) {
    terminalRef.value.startTab(tabId)
  }
}

defineExpose({
  startResumeSession
})

// 初始化
onMounted(async () => {
  // 监听快捷键事件（必须在 cwd 检查之前设置）
  window.addEventListener('terminal:newSession', handleNewSession)
  window.addEventListener('terminal:restartSession', handleRestartSession)

  const cwd = appStore.cwd
  if (!cwd) return

  updateWindowTitle(cwd)

  try {
    await sessionStore.loadHistorySessions(cwd)
    configStore.loadProjectConfig(cwd)

    // 检查是否已有该项目的运行中 Tab
    const runningTab = sessionStore.getRunningTabForProject(cwd)

    if (appStore.pendingResume) {
      const { sessionId, sessionName } = appStore.pendingResume
      appStore.clearPendingResume()
      await startResumeSession(cwd, sessionId, sessionName)
    } else if (runningTab) {
      sessionStore.setActiveTab(runningTab.tabId)
    }
  } catch (err) {
    console.error('[TerminalView] onMounted ERROR:', err)
  }
})

onUnmounted(() => {
  window.removeEventListener('terminal:newSession', handleNewSession)
  window.removeEventListener('terminal:restartSession', handleRestartSession)
})

// KeepAlive 激活 → 改为 visible watcher（v-show 常驻 DOM）
watch(() => props.visible, async (isVisible) => {
  if (isVisible) {
    const cwd = appStore.cwd
    if (cwd) {
      updateWindowTitle(cwd)
      await nextTick()
      terminalRef.value?.fitCurrentTerminal?.()
      configStore.loadProjectConfig(cwd)
    }
  }
})

// 监听 cwd 变化
watch(() => appStore.cwd, async (newCwd, oldCwd) => {
  if (newCwd && newCwd !== oldCwd) {
    updateWindowTitle(newCwd)

    try {
      await sessionStore.loadHistorySessions(newCwd)
      configStore.loadProjectConfig(newCwd)
    } catch (err) {
      console.error('[TerminalView] cwd watch ERROR:', err)
    }
  }
})

function handleBack() {
  emit('back')
}

function handleTogglePanel(panel: SidebarPanelType) {
  sidebarStore.togglePanel(panel)
}

function handleOpenFolder() {
  if (appStore.cwd) openInFileManager(appStore.cwd)
}

// 切换到已有 Tab
function handleSwitchSession(tabId: string) {
  sessionStore.setActiveTab(tabId)
}

// 重命名会话
function handleRenameSession(tabId: string, name: string) {
  const tab = sessionStore.tabs.get(tabId)
  if (tab?.status === 'running' && tab.ptyId) {
    sendTerminalCommand(`/rename ${name}\r`)
  }
  sessionStore.updateTabName(tabId, name)
}

// 新建会话
function handleNewSession() {
  const cwd = appStore.cwd
  if (!cwd) {
    logMessage('warn', 'handleNewSession: no cwd')
    return
  }
  if (!terminalRef.value) {
    logMessage('warn', 'handleNewSession: no terminalRef')
    return
  }
  terminalRef.value.startNewSession(cwd)
}

// 重启会话
function handleRestartSession() {
  if (sessionStore.activeTabId && terminalRef.value) {
    terminalRef.value.restartTab(sessionStore.activeTabId)
  }
}

// 恢复历史会话（在新 Tab 中 --resume）
function handleResumeSession(sessionId: string) {
  const cwd = appStore.cwd
  if (!cwd || !terminalRef.value) return

  // 查找历史会话的名称
  const historySession = sessionStore.historySessions.find(s => s.sessionId === sessionId)
  const name = historySession?.name

  const tabId = sessionStore.createTab(cwd, { sessionId, name })
  sessionStore.setActiveTab(tabId)

  terminalRef.value.startTab(tabId)
}

// 关闭 Tab
function handleCloseTab(tabId: string) {
  sessionStore.closeTab(tabId)
}

// 关闭所有 Tab
function handleCloseAllTabs() {
  const cwd = appStore.cwd
  if (cwd) {
    sessionStore.closeAllTabs(cwd)
  }
}

// 关闭其他 Tab
function handleCloseOtherTabs() {
  const activeId = sessionStore.activeTabId
  if (activeId) {
    sessionStore.closeOtherTabs(activeId)
  }
}

/**
 * 点会话节点：解析动作后执行（v3：点项目=展开/折叠，不经此函数；切换只靠点会话节点）。
 * 对抗审查 D/E —— 全程参数直传，不读写全局单值中间态；复用 startResumeSession 天然无竞态。
 */
async function handleSwitchToProjectSession(projectPath: string, sessionId: string) {
  appStore.setCwd(projectPath)
  // 确保该项目历史已加载（缓存命中秒回），供 resolveSwitchAction 判定 activate/resume
  await sessionStore.loadHistorySessions(projectPath)
  await nextTick()
  const action = resolveSwitchAction({
    projectPath,
    sessionId,
    tabs: sessionStore.getProjectTabs(projectPath),
    history: sessionStore.getHistoryFor(projectPath),
  })
  switch (action.type) {
    case 'activate':
      sessionStore.setActiveTab(action.tabId)
      return
    case 'resume':
      await startResumeSession(action.projectPath, action.sessionId, action.name)
      return
  }
}

/** 项目节点 + 新建：切到该项目并新建空会话 */
function handleNewSessionIn(projectPath: string) {
  appStore.setCwd(projectPath)
  if (terminalRef.value) terminalRef.value.startNewSession(projectPath)
}

function handleCloseAllSessionsIn(projectPath: string) {
  sessionStore.closeAllTabs(projectPath)
}

// v3 项目置顶（持久化到 projects.json，跨重启保持）
function handlePin(projectPath: string) {
  sessionStore.pinProject(projectPath)
}

function handleUnpin(projectPath: string) {
  sessionStore.unpinProject(projectPath)
}

// v3 会话存档/恢复（archived 会话从历史树隐藏，项目 ⋯ 菜单可查看与恢复）
function handleArchive(projectPath: string, sessionId: string) {
  sessionStore.archiveSession(projectPath, sessionId)
}

function handleRestore(projectPath: string, sessionId: string) {
  sessionStore.restoreSession(projectPath, sessionId)
}

async function handleOpenInExplorer(projectPath: string) {
  // 复用 @tauri-apps/plugin-shell 的 open：capabilities/default.json 已授权，open(文件夹) 由系统默认文件管理器打开
  const { open } = await import('@tauri-apps/plugin-shell')
  await open(projectPath)
}

// PTY 启动回调
function handlePtyStarted(tabId: string, _ptyId: string) {
  const cwd = appStore.cwd
  if (cwd) {
    appStore.ensureProjectInList(cwd)
    // 恢复历史会话时不需要重载（claimedSessionIds 已自动过滤），仅新建会话时刷新
    const tab = sessionStore.tabs.get(tabId)
    if (!tab?.isResume) {
      sessionStore.loadHistorySessions(cwd, true)
    }
  }
}
</script>

<style scoped>
.terminal-view {
  display: flex;
  flex: 1;
  min-height: 0;
  background: var(--bg-primary);
}

.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.terminal-container {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  position: relative;
  background: var(--terminal-surface-bg);
}

.empty-state-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--terminal-surface-bg);
  z-index: 10;
}

.empty-state-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}

.empty-state-text {
  font-size: 14px;
  color: var(--terminal-surface-fg);
}

.empty-state-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 24px;
  border: 1px solid var(--accent-primary);
  background: var(--accent-primary);
  color: #fff;
  cursor: pointer;
  border-radius: var(--radius-lg);
  font-size: 14px;
  font-weight: 500;
  transition: all 0.15s ease;
  box-shadow: var(--shadow-md);
}

.empty-state-btn img {
  width: 16px;
  height: 16px;
  filter: brightness(0) invert(1);
}

.empty-state-btn:hover {
  background: var(--accent-secondary);
  transform: translateY(-1px);
  box-shadow: var(--shadow-lg);
}
</style>
