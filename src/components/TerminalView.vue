<template>
  <div class="terminal-view" data-terminal-view>
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
    />

    <!-- 主内容区 -->
    <div class="main-content">
      <TerminalHeader
        :project-name="appStore.currentProject || 'Claude Code'"
        @back="handleBack"
      />
      <div class="terminal-container">
        <!-- 空状态提示：没有任何 session -->
        <div v-if="showEmptyState" class="empty-state-overlay">
          <div class="empty-state-content">
            <p class="empty-state-text">Start a new Claude session</p>
            <button class="empty-state-btn" @click="handleNewSession">
              <img src="@/assets/icons/plus.svg" alt="New" />
              New Session
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
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useSidebarStore, type SidebarPanelType } from '@/stores/sidebar'
import { useConfigStore } from '@/stores/config'
import { openInFileManager, logMessage } from '@/api/tauri'
import { sendTerminalCommand } from '@/composables/useTerminalCommand'
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

// 标记是否已启动 PTY
let hasStartedPty = false

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

// 初始化
onMounted(async () => {
  // 监听快捷键事件（必须在 cwd 检查之前设置）
  window.addEventListener('terminal:newSession', handleNewSession)
  window.addEventListener('terminal:restartSession', handleRestartSession)
  window.addEventListener('terminal:backToProjects', handleBack)

  const cwd = appStore.cwd
  if (!cwd) return

  updateWindowTitle(cwd)

  try {
    await sessionStore.loadHistorySessions(cwd)
    configStore.loadProjectConfig(cwd)

    // 检查是否已有该项目的运行中 Tab
    const runningTab = sessionStore.getRunningTabForProject(cwd)

    if (runningTab) {
      sessionStore.setActiveTab(runningTab.tabId)
    } else if (appStore.pendingResume) {
      // 从 ProjectSelectView 点击会话恢复 — 直接创建带 sessionId 和 name 的 tab
      hasStartedPty = true
      const { sessionId, sessionName } = appStore.pendingResume
      appStore.clearPendingResume()
      await nextTick()

      const tabId = sessionStore.createTab(cwd, { sessionId, name: sessionName })
      sessionStore.setActiveTab(tabId)
      if (terminalRef.value?.startTab) {
        terminalRef.value.startTab(tabId)
      }
    }
  } catch (err) {
    console.error('[TerminalView] onMounted ERROR:', err)
  }
})

onUnmounted(() => {
  window.removeEventListener('terminal:newSession', handleNewSession)
  window.removeEventListener('terminal:restartSession', handleRestartSession)
  window.removeEventListener('terminal:backToProjects', handleBack)
})

// KeepAlive 激活 → 改为 visible watcher（v-show 常驻 DOM）
watch(() => props.visible, async (isVisible) => {
  if (isVisible) {
    const cwd = appStore.cwd
    if (cwd) {
      updateWindowTitle(cwd)
      await nextTick()
      terminalRef.value?.fitCurrentTerminal?.()
      sessionStore.loadHistorySessions(cwd)
      configStore.loadProjectConfig(cwd)

      const runningTab = sessionStore.getRunningTabForProject(cwd)
      if (runningTab && sessionStore.activeTabId !== runningTab.tabId) {
        sessionStore.setActiveTab(runningTab.tabId)
      }
    }
  }
})

// 监听 cwd 变化
watch(() => appStore.cwd, async (newCwd, oldCwd) => {
  if (newCwd && newCwd !== oldCwd) {
    hasStartedPty = false
    updateWindowTitle(newCwd)

    try {
      await sessionStore.loadHistorySessions(newCwd)
      configStore.loadProjectConfig(newCwd)

      const runningTab = sessionStore.getRunningTabForProject(newCwd)
      if (runningTab) {
        sessionStore.setActiveTab(runningTab.tabId)
      } else if (appStore.pendingResume) {
        hasStartedPty = true
        const { sessionId, sessionName } = appStore.pendingResume
        appStore.clearPendingResume()
        await nextTick()

        const tabId = sessionStore.createTab(newCwd, { sessionId, name: sessionName })
        sessionStore.setActiveTab(tabId)
        if (terminalRef.value?.startTab) {
          terminalRef.value.startTab(tabId)
        }
      }
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

// PTY 启动回调
function handlePtyStarted(_tabId: string, _ptyId: string) {
  // 刷新历史会话（匹配后可能需要更新）
  const cwd = appStore.cwd
  if (cwd) {
    sessionStore.loadHistorySessions(cwd)
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
  background: var(--terminal-bg);
}

.empty-state-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--terminal-bg);
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
  color: var(--text-secondary);
}

.empty-state-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 24px;
  border: 1px solid var(--accent-primary);
  background: var(--accent-primary);
  color: var(--text-inverse);
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
