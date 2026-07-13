<template>
  <div class="sessions-panel">
    <!-- Header -->
    <PanelHeader :title="t('sessions')" @close="$emit('close')">
      <template #actions>
        <button class="action-btn" @click="handleRefresh" :title="t('refreshSessions')">
          <img src="@/assets/icons/refresh.svg" :alt="t('refreshSessions')" />
        </button>
      </template>
    </PanelHeader>

    <!-- 搜索框 -->
    <div class="search-box">
      <img class="search-icon" src="@/assets/icons/search.svg" alt="Search" />
      <input
        v-model="searchQuery"
        type="text"
        :placeholder="t('searchSessions')"
        class="search-input"
      />
      <button
        v-if="searchQuery"
        class="search-clear-btn"
        @click="searchQuery = ''"
        :title="t('clearSearch')"
      >
        <img src="@/assets/icons/close.svg" :alt="t('clearSearch')" />
      </button>
    </div>

    <!-- 项目树列表 -->
    <div class="panel-content" ref="scrollContainer" @scroll="handleScroll">
      <!-- P1: projects.json 未加载完成不渲染树，避免读空 pin/archive 状态导致置顶闪回/已存档短暂显示 -->
      <div v-if="!sessionStore.projectsStateLoaded && !sessionStore.projectsStateError" class="loading-indicator">
        {{ t('loading') }}
      </div>
      <div v-else-if="sessionStore.projectsStateError" class="state-hint">
        <span>{{ t('projectsLoadFailed') }}</span>
        <button class="state-retry-btn" @click="retryProjectsLoad">{{ t('retry') }}</button>
      </div>
      <template v-else>
      <!-- 搜索结果模式：命中的项目组（带 matchedHistoryIds） -->
      <template v-if="searchQuery.trim()">
        <ProjectNode
          v-for="g in filteredGroups"
          :key="g.projectPath"
          :project="g"
          :expanded="true"
          :is-current="sameProjectPath(g.projectPath, appStore.cwd)"
          :active-tab-id="sessionStore.activeTabId"
          :history="matchedHistoryFor(g)"
          :disable-toggle="true"
          @toggle-expand="(p) => sessionStore.toggleExpand(p)"
          @new-session-in="(p) => $emit('newSessionIn', p)"
          @switch-session="(id) => $emit('switchSession', id)"
          @rename-session="(id, name) => $emit('renameSession', id, name)"
          @restart-session="(id) => $emit('restartSession', id)"
          @close-tab="(id) => $emit('closeTab', id)"
          @resume-session="(id, name) => $emit('resumeSessionInProject', g.projectPath, id, name)"
          @close-all-sessions="(p) => $emit('closeAllSessions', p)"
          @open-in-explorer="(p) => $emit('openInExplorer', p)"
          @pin-project="(p) => $emit('pinProject', p)"
          @unpin-project="(p) => $emit('unpinProject', p)"
          @archive-session="(p, sid) => $emit('archiveSession', p, sid)"
          @restore-session="(p, sid) => $emit('restoreSession', p, sid)"
          @show-archived="(p) => $emit('showArchived', p)"
        />
        <div v-if="filteredGroups.length === 0 && !sessionStore.isLoading" class="empty-hint">
          {{ t('noSessionsFound') }}
        </div>
        <div v-if="sessionStore.isLoading" class="loading-indicator">
          {{ t('loading') }}
        </div>
      </template>

      <!-- 正常模式：全量项目组（置顶 -> 字母序 -> 孤儿置底） -->
      <template v-else>
        <ProjectNode
          v-for="g in displayedGroups"
          :key="g.projectPath"
          :project="g"
          :expanded="sessionStore.isExpanded(g.projectPath)"
          :is-current="sameProjectPath(g.projectPath, appStore.cwd)"
          :active-tab-id="sessionStore.activeTabId"
          :history="sessionStore.getHistoryFor(g.projectPath)"
          :loading="historyLoadStateFor(g.projectPath).loading"
          :error="historyLoadStateFor(g.projectPath).error"
          @toggle-expand="(p) => onToggleExpand(p)"
          @new-session-in="(p) => $emit('newSessionIn', p)"
          @switch-session="(id) => $emit('switchSession', id)"
          @rename-session="(id, name) => $emit('renameSession', id, name)"
          @restart-session="(id) => $emit('restartSession', id)"
          @close-tab="(id) => $emit('closeTab', id)"
          @resume-session="(id, name) => $emit('resumeSessionInProject', g.projectPath, id, name)"
          @close-all-sessions="(p) => $emit('closeAllSessions', p)"
          @open-in-explorer="(p) => $emit('openInExplorer', p)"
          @pin-project="(p) => $emit('pinProject', p)"
          @unpin-project="(p) => $emit('unpinProject', p)"
          @archive-session="(p, sid) => $emit('archiveSession', p, sid)"
          @restore-session="(p, sid) => $emit('restoreSession', p, sid)"
          @show-archived="(p) => $emit('showArchived', p)"
        />
        <div v-if="displayedGroups.length === 0 && !sessionStore.isLoading" class="empty-hint">
          {{ t('noProjectsYet') }}
        </div>
        <div v-if="sessionStore.isLoading" class="loading-indicator">
          {{ t('loading') }}
        </div>
        <div v-if="sessionStore.isLoadingMore" class="loading-indicator">
          {{ t('loadingMore') }}
        </div>
      </template>
      </template>
    </div>

    <!-- 底部操作区 -->
    <footer class="panel-footer">
      <div class="action-buttons">
        <button
          class="action-btn with-hint"
          @click="$emit('newSession')"
          :title="t('newSessionTitle')"
        >
          <div class="btn-content">
            <img src="@/assets/icons/plus.svg" :alt="t('newBtn')" />
            <span class="btn-label">{{ t('newBtn') }}</span>
          </div>
          <span class="btn-hint">{{ alt }}+N</span>
        </button>
        <button
          class="action-btn with-hint"
          @click="$emit('restartSession')"
          :disabled="!sessionStore.activeTab"
          :title="t('restartSessionTitle')"
        >
          <div class="btn-content">
            <img src="@/assets/icons/refresh.svg" :alt="t('restartBtn')" />
            <span class="btn-label">{{ t('restartBtn') }}</span>
          </div>
          <span class="btn-hint">{{ alt }}+R</span>
        </button>
      </div>

      <div class="options-content">
        <label class="option-item">
          <input type="checkbox" v-model="appStore.claudeOptions.skipPermissions" />
          <span class="option-label">{{ t('allow') }}</span>
          <code class="option-flag warning">skip-permissions</code>
        </label>

        <div class="option-item text-option">
          <span class="option-label">{{ t('customArgs') }}</span>
          <input type="text" v-model="appStore.claudeOptions.customArgs" placeholder="--model sonnet" />
        </div>
      </div>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSessionStore, type ProjectGroup } from '@/stores/session'
import { useAppStore } from '@/stores/app'
import { sameProjectPath, normalizePath } from '@/utils/path'

const { t } = useI18n()
import { alt } from '@/utils/platform'
import ProjectNode from './ProjectNode.vue'
import PanelHeader from '../sidebar/PanelHeader.vue'

const emit = defineEmits<{
  close: []
  switchSession: [tabId: string]
  renameSession: [tabId: string, name: string]
  // footer 按钮（重启当前活动 tab）不带 tabId；ProjectNode 转发时带具体 tabId
  restartSession: [tabId?: string]
  newSession: []
  // 兼容旧调用方（当前项目维度）；ProjectNode 的 resume 走 resumeSessionInProject
  resumeSession: [sessionId: string]
  resumeSessionInProject: [projectPath: string, sessionId: string, name?: string]
  closeTab: [tabId: string]
  closeAllTabs: []
  closeOtherTabs: []
  newSessionIn: [projectPath: string]
  toggleExpand: [projectPath: string]
  closeAllSessions: [projectPath: string]
  openInExplorer: [projectPath: string]
  // v3 新增：置顶 / 存档（转发给 TerminalView 调 store）
  pinProject: [projectPath: string]
  unpinProject: [projectPath: string]
  archiveSession: [projectPath: string, sessionId: string]
  restoreSession: [projectPath: string, sessionId: string]
  showArchived: [projectPath: string]
}>()

const sessionStore = useSessionStore()
const appStore = useAppStore()
const scrollContainer = ref<HTMLElement>()
const searchQuery = ref('')

// 全部分组（基于 cachedProjects 构建 + 排序：置顶 → 字母序 → 孤儿置底）
// v5-T4：传 hiddenProjects 过滤隐藏项目（及其孤儿 tab）
const allGroups = computed<ProjectGroup[]>(() => {
  const built = sessionStore.buildProjectGroups(appStore.cachedProjects, appStore.hiddenProjects)
  return sessionStore.sortProjectGroups(built)
})

// 正常模式显示的分组（无搜索时）
const displayedGroups = computed(() => allGroups.value)

// 搜索结果：filterProjectGroups 做项目名/会话名命中，命中组带 matchedHistoryIds
const filteredGroups = computed(() => {
  if (!searchQuery.value.trim()) return allGroups.value
  return sessionStore.filterProjectGroups(allGroups.value, searchQuery.value)
})

// 搜索模式下，每个分组只展示命中的历史会话（无 matchedHistoryIds 时回退到全量）
function matchedHistoryFor(g: { projectPath: string; matchedHistoryIds?: string[] }) {
  const all = sessionStore.getHistoryFor(g.projectPath)
  if (!g.matchedHistoryIds || g.matchedHistoryIds.length === 0) return all
  const ids = new Set(g.matchedHistoryIds)
  return all.filter(s => ids.has(s.sessionId))
}

// per 项目历史加载状态（v6 codex batch2 #8：替代给所有 ProjectNode 传同一 isLoading）
// 读 historyLoadState.get(normalizePath(path))；未登记返 idle（loading=false, error=null）
function historyLoadStateFor(path: string): { loading: boolean; error: string | null } {
  return sessionStore.historyLoadState.get(normalizePath(path)) ?? { loading: false, error: null }
}

// 正常模式展开/折叠：切换 store 状态（v3 纯手动），展开时懒加载该项目的会话历史
function onToggleExpand(path: string) {
  sessionStore.toggleExpand(path)
  if (sessionStore.isExpanded(path)) {
    sessionStore.loadHistorySessions(path)
  }
}

// v3 纯手动展开：初始无默认展开，此处仅在分组重算时为已手动展开的项目补懒载历史
// （loadHistorySessions 内部 inflight 去重 + 缓存命中秒回，重复安全）。
watch(displayedGroups, (groups) => {
  for (const g of groups) {
    if (sessionStore.isExpanded(g.projectPath)) {
      sessionStore.loadHistorySessions(g.projectPath)
    }
  }
}, { immediate: true })

// 滚动到底加载更多项目（仅正常模式；搜索时无分页）
function handleScroll() {
  const el = scrollContainer.value
  if (!el || searchQuery.value) return
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 80) {
    appStore.loadMoreProjects()
  }
}

function handleRefresh() {
  if (appStore.cwd) {
    sessionStore.loadHistorySessions(appStore.cwd, true)
  }
}

// P2: projects.json 加载失败后重试（loadProjectsState 失败时已重置 loadPromise，可重新触发）
function retryProjectsLoad() {
  sessionStore.loadProjectsState()
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

.sessions-panel :deep(.action-btn:not(.with-hint)) {
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

.sessions-panel :deep(.action-btn:not(.with-hint) img) {
  width: 16px;
  height: 16px;
}

.sessions-panel :deep(.action-btn:not(.with-hint):hover) {
  color: var(--text-primary);
}

.search-box {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  margin: 8px 12px;
}

.search-icon {
  width: 14px;
  height: 14px;
  opacity: 0.6;
}

.search-clear-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 0;
  border-radius: 4px;
  flex-shrink: 0;
}

.search-clear-btn img {
  width: 12px;
  height: 12px;
}

.search-clear-btn:hover {
  color: var(--text-primary);
  background: var(--bg-tertiary);
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

.loading-indicator,
.empty-hint {
  text-align: center;
  padding: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.state-hint {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.state-retry-btn {
  padding: 4px 14px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 4px;
  font-size: 12px;
  transition: all 0.15s ease;
}

.state-retry-btn:hover {
  border-color: var(--accent-color);
  color: var(--accent-color);
}

.panel-footer {
  border-top: 1px solid var(--border-color);
  padding: 8px 12px;
}

.action-buttons {
  display: flex;
  gap: 8px;
  margin-bottom: 6px;
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
  padding: 4px 12px;
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
  padding: 6px 8px;
  background: var(--bg-primary);
  border-radius: 6px;
  border: 1px solid var(--border-color);
}

.option-item {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
  cursor: pointer;
}

.option-item:last-child {
  margin-bottom: 0;
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
  color: var(--status-error);
  border-color: rgba(232, 112, 90, 0.3);
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
