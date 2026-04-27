<template>
  <Transition name="fade" mode="out-in">
    <WelcomeView
      v-if="currentView === 'welcome'"
      @select-project="handleSelectProject"
    />
    <ProjectSelectView
      v-else-if="currentView === 'projects'"
      @select-project="handleOpenProject"
      @add-project="handleSelectProject"
      @resume-session="handleResumeSession"
    />
    <KeepAlive v-else>
      <TerminalView
        @back="handleBack"
        @select-project="handleOpenProject"
      />
    </KeepAlive>
  </Transition>

  <!-- Settings Modal -->
  <SettingsModal
    :visible="settingsVisible"
    @close="settingsVisible = false"
  />

  <!-- Shortcuts Modal -->
  <ShortcutsModal
    :visible="shortcutsVisible"
    @close="shortcutsVisible = false"
  />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useAppStore } from '@/stores/app'
import {
  selectDirectory,
  onMenuSettings,
  onMenuShortcuts,
  onConfigFontSize,
  onTerminalRestart
} from '@/api/tauri'
import WelcomeView from '@/components/WelcomeView.vue'
import ProjectSelectView from '@/components/ProjectSelectView.vue'
import TerminalView from '@/components/TerminalView.vue'
import SettingsModal from '@/components/SettingsModal.vue'
import ShortcutsModal from '@/components/ShortcutsModal.vue'

type ViewType = 'welcome' | 'projects' | 'terminal'

const appStore = useAppStore()
const currentView = ref<ViewType>('welcome')
const settingsVisible = ref(false)
const shortcutsVisible = ref(false)

// Unlisten functions for cleanup
let unlistenSettings: (() => void) | null = null
let unlistenShortcuts: (() => void) | null = null
let unlistenFontSize: (() => void) | null = null
let unlistenRestart: (() => void) | null = null

onMounted(async () => {
  if (appStore.cwd) {
    currentView.value = 'terminal'
  } else {
    // 加载缓存（项目列表 + 近期会话）
    await appStore.loadCache()
    if (appStore.cachedProjects.length > 0) {
      currentView.value = 'projects'
    }
  }

  // Listen for menu events
  unlistenSettings = await onMenuSettings(() => {
    settingsVisible.value = true
  })

  unlistenShortcuts = await onMenuShortcuts(() => {
    shortcutsVisible.value = true
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
}
</script>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>