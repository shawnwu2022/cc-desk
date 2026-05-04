<template>
  <div class="sessions-panel">
    <!-- Header -->
    <PanelHeader title="Sessions" @close="$emit('close')">
      <template #actions>
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
          closable
          @switch="(id) => $emit('switchSession', id)"
          @rename="(id, name) => $emit('renameSession', id, name)"
          @restart="() => $emit('restartSession')"
          @close="(id) => $emit('closeTab', id)"
        />
      </div>

      <!-- History -->
      <div v-if="filteredHistory.length > 0" class="section">
        <div class="section-title">History</div>
        <SessionList
          :history="filteredHistory"
          :active-id="null"
          :snippet-map="snippetMap"
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

    <!-- 底部操作区 -->
    <footer class="panel-footer">
      <div class="action-buttons">
        <button
          class="action-btn with-hint"
          @click="$emit('newSession')"
          title="New session (Alt+N)"
        >
          <div class="btn-content">
            <img src="@/assets/icons/plus.svg" alt="New session" />
            <span class="btn-label">New</span>
          </div>
          <span class="btn-hint">{{ alt }}+N</span>
        </button>
        <button
          class="action-btn with-hint"
          @click="$emit('restartSession')"
          :disabled="!sessionStore.activeTab"
          title="Restart session (Alt+R)"
        >
          <div class="btn-content">
            <img src="@/assets/icons/refresh.svg" alt="Restart" />
            <span class="btn-label">Restart</span>
          </div>
          <span class="btn-hint">{{ alt }}+R</span>
        </button>
      </div>

      <div class="options-content">
        <label class="option-item">
          <input type="checkbox" v-model="appStore.claudeOptions.skipPermissions" />
          <span class="option-label">Allow</span>
          <code class="option-flag warning">skip-permissions</code>
        </label>

        <div class="option-item text-option">
          <span class="option-label">Custom args</span>
          <input type="text" v-model="appStore.claudeOptions.customArgs" placeholder="--model sonnet" />
        </div>
      </div>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useSessionStore } from '@/stores/session'
import { useAppStore } from '@/stores/app'
import { alt } from '@/utils/platform'
import SessionList from './SessionList.vue'
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
const scrollContainer = ref<HTMLElement>()
const searchQuery = ref('')

const projectTabs = computed(() => {
  const cwd = appStore.cwd
  if (!cwd) return []
  return sessionStore.getProjectTabs(cwd)
})

const filteredHistory = computed(() => {
  const query = searchQuery.value.toLowerCase()
  if (!query) return sessionStore.historySessions

  const byName = sessionStore.historySessions.filter(s =>
    s.name.toLowerCase().includes(query)
  )

  const msgResults = sessionStore.messageSearchResults
  const byNameIds = new Set(byName.map(s => s.sessionId))

  return [
    ...byName,
    ...msgResults
      .filter(r => !byNameIds.has(r.sessionId))
      .map(r => ({
        sessionId: r.sessionId,
        name: r.name,
        projectPath: r.projectPath,
        lastActiveAt: r.lastActiveAt,
      }))
  ]
})

const snippetMap = computed(() => {
  const map = new Map<string, string>()
  for (const r of sessionStore.messageSearchResults) {
    map.set(r.sessionId, r.snippet)
  }
  return map
})

watch(() => appStore.cwd, (newCwd) => {
  if (newCwd) {
    sessionStore.loadHistorySessions(newCwd)
  }
}, { immediate: true })

watch(searchQuery, (query) => {
  if (appStore.cwd) {
    sessionStore.debouncedSearchMessages(appStore.cwd, query)
  }
})

function handleRefresh() {
  if (appStore.cwd) {
    sessionStore.loadHistorySessions(appStore.cwd)
  }
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

.action-buttons {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}

.action-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 12px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 6px;
  font-size: 13px;
  transition: all 0.15s ease;
}

.action-btn img {
  width: 14px;
  height: 14px;
}

.action-btn:hover:not(:disabled) {
  border-color: var(--accent-color);
  color: var(--accent-color);
}

.action-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.action-btn.with-hint {
  flex-direction: column;
  padding: 6px 12px;
  gap: 2px;
}

.btn-content {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
}

.btn-label {
  font-size: 13px;
}

.btn-hint {
  font-size: 10px;
  color: var(--text-tertiary);
  font-family: var(--font-mono);
}

.options-content {
  padding: 10px;
  background: var(--bg-primary);
  border-radius: 6px;
  border: 1px solid var(--border-color);
}

.option-item {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  cursor: pointer;
}

.option-item input[type="checkbox"] {
  width: 14px;
  height: 14px;
  accent-color: var(--accent-color);
}

.option-label {
  font-size: 12px;
  color: var(--text-primary);
}

.option-flag {
  font-size: 10px;
  padding: 1px 5px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 3px;
  font-family: 'Consolas', 'Monaco', monospace;
  color: var(--text-secondary);
}

.option-flag.warning {
  color: #e74c3c;
  border-color: rgba(231, 76, 60, 0.3);
}

.text-option {
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}

.text-option input[type="text"] {
  width: 100%;
  padding: 5px 8px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-primary);
}

.text-option input[type="text"]:focus {
  outline: none;
  border-color: var(--accent-color);
}
</style>
