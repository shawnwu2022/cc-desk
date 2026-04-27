<template>
  <div class="terminal-view">
    <!-- 图标栏（常驻） -->
    <IconBar
      :active-panel="sidebarStore.activePanel"
      @toggle="handleTogglePanel"
      @open-folder="handleOpenFolder"
    />

    <!-- 侧边栏面板 -->
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
import { ref, onMounted, watch, onActivated, nextTick } from 'vue'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useSidebarStore, type SidebarPanelType } from '@/stores/sidebar'
import { useConfigStore } from '@/stores/config'
import { openInFileManager } from '@/api/tauri'
import { sendTerminalCommand } from '@/composables/useTerminalCommand'
import TerminalHeader from './TerminalHeader.vue'
import XTermTerminal from './XTermTerminal.vue'
import IconBar from './IconBar.vue'
import SidebarPanel from './sidebar/SidebarPanel.vue'

const emit = defineEmits<{
  back: []
}>()

const appStore = useAppStore()
const sessionStore = useSessionStore()
const sidebarStore = useSidebarStore()
const configStore = useConfigStore()
const terminalRef = ref()

// 标记是否已启动 PTY
let hasStartedPty = false

// 初始化
onMounted(async () => {
  const cwd = appStore.cwd
  if (!cwd) return

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
    } else if (appStore.shouldAutoOpenSessions) {
      // 从 ProjectSelectView 点击项目进入，不自动启动，打开 Sessions 面板
      appStore.setAutoOpenSessions(false)
      sidebarStore.togglePanel('sessions')
    } else if (!hasStartedPty) {
      hasStartedPty = true
      await nextTick()
      if (terminalRef.value?.startWithOptions) {
        terminalRef.value.startWithOptions(cwd, appStore.claudeOptions)
      }
    }
  } catch (err) {
    console.error('[TerminalView] onMounted ERROR:', err)
  }
})

// KeepAlive 激活
onActivated(async () => {
  const cwd = appStore.cwd
  if (cwd) {
    await sessionStore.loadHistorySessions(cwd)
    configStore.loadProjectConfig(cwd)

    const runningTab = sessionStore.getRunningTabForProject(cwd)
    if (runningTab && sessionStore.activeTabId !== runningTab.tabId) {
      sessionStore.setActiveTab(runningTab.tabId)
    }
  }
})

// 监听 cwd 变化
watch(() => appStore.cwd, async (newCwd, oldCwd) => {
  if (newCwd && newCwd !== oldCwd) {
    hasStartedPty = false

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
      } else if (appStore.shouldAutoOpenSessions) {
        appStore.setAutoOpenSessions(false)
        sidebarStore.togglePanel('sessions')
      } else if (!hasStartedPty) {
        hasStartedPty = true
        await nextTick()
        if (terminalRef.value?.startWithOptions) {
          terminalRef.value.startWithOptions(newCwd, appStore.claudeOptions)
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
  if (cwd && terminalRef.value) {
    terminalRef.value.startNewSession(cwd)
  }
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
  height: 100vh;
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
}
</style>
