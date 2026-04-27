<template>
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
import { ref, onMounted, onUnmounted } from 'vue'
import { useAppStore } from '@/stores/app'
import { useSidebarStore } from '@/stores/sidebar'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  selectDirectory,
  onMenuSettings,
  onMenuShortcuts,
  onConfigFontSize,
  onTerminalRestart
} from '@/api/tauri'
import { useAppShortcuts } from '@/composables/useAppShortcuts'
import WelcomeView from '@/components/WelcomeView.vue'
import ProjectSelectView from '@/components/ProjectSelectView.vue'
import TerminalView from '@/components/TerminalView.vue'
import SettingsOverlay from '@/components/settings/SettingsOverlay.vue'

type ViewType = 'welcome' | 'projects' | 'terminal'

const appStore = useAppStore()
const sidebarStore = useSidebarStore()
const { handleKeydown } = useAppShortcuts()
const currentView = ref<ViewType>('welcome')

// Unlisten functions for cleanup
let unlistenSettings: (() => void) | null = null
let unlistenShortcuts: (() => void) | null = null
let unlistenFontSize: (() => void) | null = null
let unlistenRestart: (() => void) | null = null

onMounted(async () => {
  // 全局快捷键
  window.addEventListener('keydown', handleKeydown, true)

  if (appStore.cwd) {
    currentView.value = 'terminal'
  } else {
    // 加载缓存（项目列表 + 近期会话）
    await appStore.loadCache()
    if (appStore.cachedProjects.length > 0) {
      currentView.value = 'projects'
    }
  }

  // Listen for menu events — 设置可在任何视图打开
  unlistenSettings = await onMenuSettings(() => {
    sidebarStore.openSettings()
  })

  unlistenShortcuts = await onMenuShortcuts(() => {
    sidebarStore.openSettings('shortcuts')
  })

  unlistenFontSize = await onConfigFontSize((size) => {
    appStore.setFontSize(size)
  })

  unlistenRestart = await onTerminalRestart((data) => {
    if (currentView.value === 'terminal') {
      appStore.setCwd(data.cwd)
    }
  })
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown, true)
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
    appStore.setAutoOpenSessions(true)
    currentView.value = 'terminal'
  }
}

async function handleOpenProject(path: string) {
  appStore.setCwd(path)
  appStore.setAutoOpenSessions(true)
  currentView.value = 'terminal'
}

function handleResumeSession(projectPath: string, sessionId: string, sessionName?: string) {
  appStore.setCwd(projectPath)
  appStore.setClaudeOptions({ resume: sessionId })
  appStore.setPendingResume(sessionId, sessionName)
  currentView.value = 'terminal'
}

function handleBack() {
  currentView.value = 'projects'
  getCurrentWindow().setTitle('CC-Box').catch(() => {})
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
</style>