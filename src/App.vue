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
  getProjects,
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
  // Check if there's a current working directory
  if (appStore.cwd) {
    currentView.value = 'terminal'
  } else {
    // Check if there are any projects from Claude Code native config
    const projects = await getProjects()
    if (projects.length > 0) {
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
    // Add project 时，启动命令为空（不包含额外启动命令）
    appStore.setClaudeOptions({
      continue: false,
      resume: '',
      skipPermissions: false,
      customArgs: ''
    })
    currentView.value = 'terminal'
  }
}

async function handleOpenProject(path: string) {
  appStore.setCwd(path)
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