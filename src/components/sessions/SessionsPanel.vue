<template>
  <div class="sessions-panel">
    <!-- Header -->
    <PanelHeader title="Sessions" @close="$emit('close')">
      <template #actions>
        <button class="action-btn" @click="$emit('newSession')" title="New session">
          <img src="@/assets/icons/plus.svg" alt="New session" />
        </button>
        <button class="action-btn" @click="handleRefresh" title="Refresh sessions">
          <img src="@/assets/icons/refresh.svg" alt="Refresh" />
        </button>
      </template>
    </PanelHeader>

    <!-- 搜索框 -->
    <div class="search-box">
      <img class="search-icon" src="@/assets/icons/search.svg" alt="Search" />
      <input
        v-model="searchQuery"
        type="text"
        placeholder="Search sessions..."
        class="search-input"
      />
    </div>

    <!-- 会话列表 -->
    <div class="panel-content" ref="scrollContainer">
      <!-- Open Tabs -->
      <div v-if="projectTabs.length > 0" class="section">
        <div class="section-title">Open Tabs</div>
        <SessionList
          :tabs="projectTabs"
          :active-id="sessionStore.activeTabId"
          @switch="(id) => $emit('switchSession', id)"
          @rename="(id, name) => $emit('renameSession', id, name)"
          @restart="() => $emit('restartSession')"
        />
      </div>

      <!-- History -->
      <div v-if="filteredHistory.length > 0" class="section">
        <div class="section-title">History</div>
        <SessionList
          :history="filteredHistory"
          :active-id="null"
          @switch="(id) => $emit('resumeSession', id)"
        />
      </div>

      <!-- 空状态 -->
      <div v-if="projectTabs.length === 0 && filteredHistory.length === 0" class="empty-hint">
        No sessions found
      </div>

      <div v-if="sessionStore.isLoading" class="loading-indicator">
        Loading...
      </div>
    </div>

    <!-- 当前会话状态 -->
    <SessionStatus
      :tab="sessionStore.activeTab"
      :project-path="appStore.cwd"
      @restart="$emit('restartSession')"
    />

    <!-- 设置区 -->
    <footer class="panel-footer">
      <button class="settings-toggle" @click="settingsExpanded = !settingsExpanded">
        <img src="@/assets/icons/settings.svg" alt="Settings" />
        <span>Settings</span>
        <img class="chevron" src="@/assets/icons/chevron.svg" alt="Toggle" :class="{ expanded: settingsExpanded }" />
      </button>

      <div v-if="settingsExpanded" class="settings-content">
        <div class="font-size-control">
          <span class="settings-label">Font Size:</span>
          <div class="font-size-buttons">
            <button @click="decreaseFontSize" :disabled="fontSize <= 10">-</button>
            <span class="font-size-value">{{ fontSize }}</span>
            <button @click="increaseFontSize" :disabled="fontSize >= 24">+</button>
          </div>
        </div>
      </div>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useSessionStore } from '@/stores/session'
import { useAppStore } from '@/stores/app'
import SessionList from './SessionList.vue'
import SessionStatus from './SessionStatus.vue'
import PanelHeader from '../sidebar/PanelHeader.vue'

const emit = defineEmits<{
  close: []
  switchSession: [tabId: string]
  renameSession: [tabId: string, name: string]
  restartSession: []
  newSession: []
  resumeSession: [sessionId: string]
  closeTab: [tabId: string]
}>()

const sessionStore = useSessionStore()
const appStore = useAppStore()
const settingsExpanded = ref(false)
const scrollContainer = ref<HTMLElement>()
const searchQuery = ref('')

// 字体大小
const fontSize = computed(() => appStore.fontSize)

// 当前项目的 Tab 列表
const projectTabs = computed(() => {
  const cwd = appStore.cwd
  if (!cwd) return []
  return sessionStore.getProjectTabs(cwd)
})

// 过滤后的历史会话
const filteredHistory = computed(() => {
  if (!searchQuery.value) return sessionStore.historySessions
  return sessionStore.historySessions.filter(s =>
    s.name.toLowerCase().includes(searchQuery.value.toLowerCase())
  )
})

// 监听 cwd 变化
watch(() => appStore.cwd, (newCwd) => {
  if (newCwd) {
    sessionStore.loadHistorySessions(newCwd)
  }
}, { immediate: true })

function handleRefresh() {
  if (appStore.cwd) {
    sessionStore.loadHistorySessions(appStore.cwd)
  }
}

function decreaseFontSize() {
  appStore.setFontSize(fontSize.value - 1)
}

function increaseFontSize() {
  appStore.setFontSize(fontSize.value + 1)
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<style scoped>
.sessions-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.sessions-panel :deep(.action-btn) {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  padding: 0;
}

.sessions-panel :deep(.action-btn img) {
  width: 16px;
  height: 16px;
}

.sessions-panel :deep(.action-btn:hover) {
  color: var(--text-primary);
}

.search-box {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  margin: 12px;
}

.search-icon {
  width: 16px;
  height: 16px;
}

.search-input {
  flex: 1;
  border: none;
  background: transparent;
  font-size: 13px;
  color: var(--text-primary);
  outline: none;
}

.search-input::placeholder {
  color: var(--text-tertiary);
}

.panel-content {
  flex: 1;
  overflow-y: auto;
  padding: 0 12px;
  min-height: 0;
}

.section {
  margin-bottom: 12px;
}

.section-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  margin-bottom: 6px;
  padding: 0 4px;
}

.loading-indicator,
.empty-hint {
  text-align: center;
  padding: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.panel-footer {
  border-top: 1px solid var(--border-color);
  padding: 12px;
}

.settings-toggle {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 12px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 6px;
  font-size: 13px;
  transition: all 0.15s ease;
}

.settings-toggle img:first-child {
  width: 16px;
  height: 16px;
}

.settings-toggle:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.chevron {
  width: 16px;
  height: 16px;
  transition: transform 0.2s ease;
}

.chevron.expanded {
  transform: rotate(180deg);
}

.settings-content {
  padding: 12px;
  background: var(--bg-primary);
  border-radius: 6px;
  margin-top: 8px;
}

.font-size-control {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.settings-label {
  font-size: 13px;
  color: var(--text-primary);
}

.font-size-buttons {
  display: flex;
  align-items: center;
  gap: 8px;
}

.font-size-buttons button {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-color);
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 4px;
  font-size: 14px;
}

.font-size-buttons button:hover:not(:disabled) {
  border-color: var(--accent-color);
  color: var(--accent-color);
}

.font-size-buttons button:disabled {
  opacity: 0.5;
  cursor: not-not-allowed;
}

.font-size-value {
  font-size: 14px;
  color: var(--text-primary);
  min-width: 24px;
  text-align: center;
}
</style>
