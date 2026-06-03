<template>
  <!-- 无 Teleport，直接作为 flex 子元素 -->
  <aside class="sidebar-drawer" :class="{ visible }">
    <div class="sidebar-inner">
      <!-- 头部 -->
      <header class="sidebar-header">
        <h2>{{ t('sessions') }}</h2>
        <div class="header-actions">
          <button class="new-btn" @click="handleNewSession" :title="t('newSessionTitle')">
            <img src="@/assets/icons/plus.svg" :alt="t('newBtn')" />
          </button>
          <button class="refresh-btn" @click="handleRefresh" :title="t('refreshSessions')">
            <img src="@/assets/icons/refresh.svg" :alt="t('refresh')" />
          </button>
          <button class="close-btn" @click="$emit('close')">
            <img src="@/assets/icons/close.svg" :alt="t('close')" />
          </button>
        </div>
      </header>

      <!-- 搜索框 -->
      <div class="search-box">
        <img class="search-icon" src="@/assets/icons/search.svg" alt="Search" />
        <input
          v-model="sessionStore.searchQuery"
          type="text"
          :placeholder="t('searchSessions')"
          class="search-input"
        />
      </div>

      <!-- 会话列表（滚动加载） -->
      <div
        class="sidebar-content"
        ref="scrollContainer"
        @scroll="handleScroll"
      >
        <SessionList
          :sessions="sessionStore.filteredSessions"
          :active-id="sessionStore.activeSessionId"
          :running-sessions="runningSessions"
          @switch="handleSwitchSession"
          @rename="handleRenameSession"
        />

        <!-- 加载更多指示器 -->
        <div v-if="sessionStore.isLoading" class="loading-indicator">
          {{ t('loading') }}
        </div>
        <div v-else-if="sessionStore.hasMore && !sessionStore.searchQuery" class="load-more-hint">
          {{ t('loadingMore') }}
        </div>
        <div v-else-if="!sessionStore.hasMore && sessionStore.loadedCount > 0" class="no-more-hint">
          {{ sessionStore.loadedCount }} {{ t('sessions') }}
        </div>
        <div v-else-if="sessionStore.loadedCount === 0" class="empty-hint">
          {{ t('noSessionsFound') }}
        </div>
      </div>

      <!-- 当前会话状态 -->
      <SessionStatus
        :session="sessionStore.activeSession"
        :project-path="appStore.cwd"
        :is-running="isCurrentRunning"
        @restart="handleRestartSession"
      />

      <!-- 设置区（折叠） -->
      <footer class="sidebar-footer">
        <button class="settings-toggle" @click="settingsExpanded = !settingsExpanded">
          <img src="@/assets/icons/settings.svg" :alt="t('settings')" />
          <span>{{ t('settings') }}</span>
          <img class="chevron" src="@/assets/icons/chevron.svg" alt="Toggle" :class="{ expanded: settingsExpanded }" />
        </button>

        <div v-if="settingsExpanded" class="settings-content">
          <div class="font-size-control">
            <span class="settings-label">{{ t('fontSize') }}:</span>
            <div class="font-size-buttons">
              <button @click="decreaseFontSize" :disabled="fontSize <= 10">-</button>
              <span class="font-size-value">{{ fontSize }}</span>
              <button @click="increaseFontSize" :disabled="fontSize >= 24">+</button>
            </div>
          </div>
        </div>
      </footer>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSessionStore } from '@/stores/session'
import { useAppStore } from '@/stores/app'
import SessionList from './sidebar/SessionList.vue'
import SessionStatus from './sidebar/SessionStatus.vue'

const { t } = useI18n()

defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  close: []
  switchSession: [sessionId: string]
  renameSession: [sessionId: string, name: string]
  restartSession: []
  newSession: []
}>()

const sessionStore = useSessionStore()
const appStore = useAppStore()
const settingsExpanded = ref(false)
const scrollContainer = ref<HTMLElement>()

// 字体大小
const fontSize = computed(() => appStore.fontSize)

// 正在运行的会话 ID 列表（从 sessionStore 获取）
const runningSessions = computed(() => sessionStore.runningSessionIds)

// 当前会话是否运行中
const isCurrentRunning = computed(() =>
  runningSessions.value.includes(sessionStore.activeSessionId || '')
)

// 监听 cwd 变化，加载对应项目的会话列表
watch(() => appStore.cwd, (newCwd) => {
  if (newCwd) {
    sessionStore.loadSessions(newCwd)
  }
}, { immediate: true })

// 滚动加载更多
function handleScroll() {
  if (!scrollContainer.value || sessionStore.searchQuery) return

  const { scrollTop, scrollHeight, clientHeight } = scrollContainer.value

  // 滚动到底部 50px 内时加载更多
  if (scrollHeight - scrollTop - clientHeight < 50) {
    if (appStore.cwd) {
      sessionStore.loadMore(appStore.cwd)
    }
  }
}

function handleSwitchSession(sessionId: string) {
  emit('switchSession', sessionId)
}

function handleRenameSession(sessionId: string, name: string) {
  emit('renameSession', sessionId, name)
  sessionStore.updateName(sessionId, name)
}

function handleRestartSession() {
  emit('restartSession')
}

function handleNewSession() {
  emit('newSession')
}

function handleRefresh() {
  if (appStore.cwd) {
    sessionStore.loadSessions(appStore.cwd)
  }
}

function decreaseFontSize() {
  appStore.setFontSize(fontSize.value - 1)
}

function increaseFontSize() {
  appStore.setFontSize(fontSize.value + 1)
}

// Escape 关闭
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
.sidebar-drawer {
  width: 0;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  transition: width 0.25s ease;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.sidebar-drawer.visible {
  width: 260px;
}

.sidebar-inner {
  width: 260px;
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;  /* 允许 flex 子元素收缩 */
}

.sidebar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px;
  border-bottom: 1px solid var(--border-color);
}

.sidebar-header h2 {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.header-actions {
  display: flex;
  gap: 8px;
}

.new-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
}

.new-btn img {
  width: 16px;
  height: 16px;
}

.new-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.refresh-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
}

.refresh-btn img {
  width: 16px;
  height: 16px;
}

.refresh-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
}

.close-btn img {
  width: 16px;
  height: 16px;
}

.close-btn:hover {
  background: var(--hover-bg);
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
  width: 14px;
  height: 14px;
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

.sidebar-content {
  flex: 1;
  overflow-y: auto;
  padding: 0 12px;
  min-height: 0;  /* 允许收缩并启用滚动 */
}

.loading-indicator,
.load-more-hint,
.no-more-hint,
.empty-hint {
  text-align: center;
  padding: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.sidebar-footer {
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
  width: 14px;
  height: 14px;
}

.settings-toggle:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.chevron {
  width: 14px;
  height: 14px;
  transition: transform 0.2s ease;
}

.chevron.expanded {
  transform: rotate(90deg);
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
  cursor: not-allowed;
}

.font-size-value {
  font-size: 14px;
  color: var(--text-primary);
  min-width: 24px;
  text-align: center;
}
</style>