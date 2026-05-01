<template>
  <div class="project-select-view">
    <!-- 左侧：近期会话列表 -->
    <aside class="sessions-panel">
      <header class="panel-header">
        <h2>Recent Sessions</h2>
      </header>

      <div class="session-list">
        <button
          v-for="session in recentSessions"
          :key="session.sessionId"
          class="session-item"
          @click="handleResumeSession(session)"
        >
          <svg class="session-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="4 17 10 11 4 5"/>
            <line x1="12" y1="19" x2="20" y2="19"/>
          </svg>
          <div class="session-info">
            <span class="session-name">{{ session.name }}</span>
            <span class="session-project">{{ getProjectName(session.projectPath) }}</span>
          </div>
          <span class="session-time">{{ formatTimeAgo(session.lastActiveAt) }}</span>
        </button>

        <div v-if="recentSessions.length === 0" class="empty-sessions">
          <span>No recent sessions</span>
        </div>
      </div>

      <!-- 底部：启动参数设置 -->
      <footer class="startup-options">
        <div class="options-header">
          <span class="options-title">Startup Options</span>
        </div>

        <label class="option-item">
          <input type="checkbox" v-model="localOptions.skipPermissions" />
          <span class="option-label">Allow</span>
          <code class="option-flag warning">skip-permissions</code>
        </label>

        <div class="option-item text-option">
          <span class="option-label">Custom args</span>
          <input type="text" v-model="localOptions.customArgs" placeholder="--model sonnet" />
        </div>

        <button
          class="save-default-btn"
          :class="{ saving: isSaving, success: saveSuccess }"
          :disabled="isSaving"
          @click="handleSaveDefault"
        >
          <span v-if="isSaving">Saving...</span>
          <span v-else-if="saveSuccess">Saved!</span>
          <span v-else>Save as Default</span>
        </button>
      </footer>
    </aside>

    <!-- 右侧：项目列表 -->
    <div class="projects-panel">
      <header class="panel-header">
        <div class="header-row">
          <h2>Projects</h2>
          <button class="settings-btn" @click="$emit('openSettings')" title="Settings (Ctrl+,)">
            <img src="@/assets/icons/settings.svg" alt="Settings" />
          </button>
        </div>
        <div class="search-box">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="11" cy="11" r="8"/>
            <line x1="21" y1="21" x2="16.65" y2="16.65"/>
          </svg>
          <input
            type="text"
            v-model="searchQuery"
            placeholder="Search..."
          />
        </div>
      </header>

      <div class="project-list" ref="projectListRef" @scroll="handleProjectScroll">
        <button
          v-for="project in filteredProjects"
          :key="project.path"
          class="project-item"
          @click="$emit('selectProject', project.path)"
        >
          <svg class="folder-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
          </svg>
          <div class="project-info">
            <span class="project-name">{{ project.name }}</span>
            <span class="project-path">{{ project.path }}</span>
          </div>
        </button>

        <div v-if="!searchQuery && appStore.hasMoreProjects && !appStore.isLoadingProjects" class="load-more-section">
          <button class="load-more-btn" @click="handleLoadMore">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="12" y1="5" x2="12" y2="19"/>
              <line x1="5" y1="12" x2="19" y2="12"/>
            </svg>
            <span>Load More Projects</span>
          </button>
        </div>

        <div v-if="appStore.isLoadingProjects" class="loading-more">
          <span>Loading...</span>
        </div>

        <div v-if="filteredProjects.length === 0 && !appStore.isLoadingProjects" class="empty-list">
          <span v-if="searchQuery">No matching projects</span>
          <span v-else>No projects yet</span>
        </div>
      </div>

      <footer class="panel-footer">
        <button class="add-btn" @click="$emit('addProject')">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="12" y1="5" x2="12" y2="19"/>
            <line x1="5" y1="12" x2="19" y2="12"/>
          </svg>
          <span>Add Project</span>
        </button>
      </footer>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAppStore } from '@/stores/app'
import type { SessionInfo } from '@/api/tauri'

const emit = defineEmits<{
  selectProject: [path: string]
  addProject: []
  resumeSession: [projectPath: string, sessionId: string, sessionName?: string]
  openSettings: []
}>()

const appStore = useAppStore()
const searchQuery = ref('')
const projectListRef = ref<HTMLElement | null>(null)

const localOptions = ref({
  skipPermissions: appStore.claudeOptions.skipPermissions,
  customArgs: appStore.claudeOptions.customArgs
})

const isSaving = ref(false)
const saveSuccess = ref(false)

const projects = computed(() => appStore.cachedProjects)
const recentSessions = computed(() => appStore.cachedRecentSessions)

const filteredProjects = computed(() => {
  const query = searchQuery.value.toLowerCase()
  const openedSet = appStore.openedProjectPaths

  let list = projects.value
  if (query) {
    list = list.filter(p =>
      p.name.toLowerCase().includes(query) ||
      p.path.toLowerCase().includes(query)
    )
  }

  return [...list].sort((a, b) => {
    const aOpened = openedSet.has(a.path) ? 1 : 0
    const bOpened = openedSet.has(b.path) ? 1 : 0
    if (aOpened !== bOpened) return bOpened - aOpened
    return (b.lastDuration ?? 0) - (a.lastDuration ?? 0)
  })
})

function handleProjectScroll() {
  const el = projectListRef.value
  if (!el || searchQuery.value) return

  const nearBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 80
  if (nearBottom && appStore.hasMoreProjects && !appStore.isLoadingProjects) {
    appStore.loadMoreProjects()
  }
}

function handleLoadMore() {
  if (!appStore.isLoadingProjects && appStore.hasMoreProjects) {
    appStore.loadMoreProjects()
  }
}

watch(localOptions, (val) => {
  appStore.setClaudeOptions(val)
}, { deep: true })

onMounted(() => {
  appStore.loadCache()
})

function getProjectName(projectPath: string): string {
  const parts = projectPath.replace(/\\/g, '/').split('/')
  return parts[parts.length - 1] || projectPath
}

function formatTimeAgo(timestamp: number): string {
  const diff = Date.now() - timestamp
  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(diff / 3600000)
  const days = Math.floor(diff / 86400000)
  if (minutes < 1) return 'now'
  if (minutes < 60) return `${minutes}m`
  if (hours < 24) return `${hours}h`
  return `${days}d`
}

function handleResumeSession(session: SessionInfo) {
  emit('resumeSession', session.projectPath, session.sessionId, session.name)
}

async function handleSaveDefault() {
  isSaving.value = true
  saveSuccess.value = false
  const success = await appStore.saveAsDefault()
  isSaving.value = false
  if (success) {
    saveSuccess.value = true
    setTimeout(() => { saveSuccess.value = false }, 3000)
  }
}
</script>

<style scoped>
.project-select-view {
  display: flex;
  height: 100vh;
  background: var(--bg-primary);
}

/* 左侧近期会话面板 */
.sessions-panel {
  width: 280px;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
}

.sessions-panel .panel-header {
  padding: 16px;
  border-bottom: 1px solid var(--border-color);
}

.sessions-panel .panel-header h2 {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: -0.3px;
}

.session-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.session-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 10px 12px;
  border: none;
  background: transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  text-align: left;
  transition: all 0.15s ease;
}

.session-item:hover {
  background: var(--hover-bg);
}

.session-icon {
  flex-shrink: 0;
  color: var(--text-tertiary);
  transition: color 0.15s ease;
}

.session-item:hover .session-icon {
  color: var(--accent-gold);
}

.session-info {
  flex: 1;
  min-width: 0;
}

.session-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-project {
  font-size: 11px;
  color: var(--text-tertiary);
  display: block;
}

.session-time {
  font-size: 11px;
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.empty-sessions {
  padding: 32px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 13px;
}

/* 底部启动参数 */
.startup-options {
  border-top: 1px solid var(--border-color);
  padding: 12px;
}

.options-header {
  margin-bottom: 8px;
}

.options-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
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
  accent-color: var(--accent-primary);
}

.option-label {
  font-size: 12px;
  color: var(--text-primary);
}

.option-flag {
  font-size: 10px;
  padding: 1px 5px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  font-family: var(--font-mono);
  color: var(--text-secondary);
}

.option-flag.warning {
  color: var(--status-error);
  border-color: rgba(196, 92, 74, 0.3);
}

.text-option {
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}

.text-option input[type="text"] {
  width: 100%;
  padding: 6px 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: 12px;
  color: var(--text-primary);
  transition: border-color 0.15s ease;
}

.text-option input[type="text"]:focus {
  outline: none;
  border-color: var(--focus-ring);
}

.save-default-btn {
  margin-top: 8px;
  padding: 8px 12px;
  background: transparent;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: 11px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s ease;
  width: 100%;
}

.save-default-btn:hover:not(:disabled):not(.success) {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}

.save-default-btn.saving {
  opacity: 0.6;
  cursor: wait;
}

.save-default-btn.success {
  border-color: var(--status-success);
  color: var(--status-success);
  background: rgba(61, 140, 110, 0.1);
}

/* 右侧项目列表 */
.projects-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 400px;
}

.projects-panel .panel-header {
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color);
}

.header-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.projects-panel .panel-header h2 {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: -0.3px;
}

.settings-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 6px;
  transition: all 0.15s ease;
}

.settings-btn img {
  width: 16px;
  height: 16px;
  opacity: 0.85;
}

.settings-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.settings-btn:hover img {
  opacity: 1;
}

.search-box {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  transition: border-color 0.15s ease;
}

.search-box:focus-within {
  border-color: var(--focus-ring);
}

.search-box svg {
  color: var(--text-secondary);
}

.search-box input {
  flex: 1;
  border: none;
  background: transparent;
  font-size: 13px;
  color: var(--text-primary);
  outline: none;
}

.search-box input::placeholder {
  color: var(--text-secondary);
}

.project-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.project-item {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 12px 16px;
  border: none;
  background: transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  text-align: left;
  transition: all 0.15s ease;
}

.project-item:hover {
  background: var(--bg-secondary);
}

.folder-icon {
  flex-shrink: 0;
  color: var(--text-secondary);
  transition: color 0.15s ease;
}

.project-item:hover .folder-icon {
  color: var(--accent-gold);
}

.project-info {
  flex: 1;
  min-width: 0;
}

.project-name {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
  display: block;
}

.project-path {
  font-size: 12px;
  color: var(--text-secondary);
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty-list {
  padding: 32px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 13px;
}

.load-more-section {
  padding: 16px;
  text-align: center;
}

.load-more-btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 10px 20px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.load-more-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
  background: var(--hover-bg);
}

.load-more-btn svg {
  opacity: 0.7;
}

.load-more-btn:hover svg {
  opacity: 1;
}

.loading-more {
  padding: 16px;
  text-align: center;
  color: var(--text-tertiary);
  font-size: 12px;
}

.panel-footer {
  padding: 12px 16px;
  border-top: 1px solid var(--border-color);
}

.add-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  width: 100%;
  padding: 10px 16px;
  background: transparent;
  border: 1px dashed var(--border-dark);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.add-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
  border-style: solid;
}

/* 滚动条 */
.session-list::-webkit-scrollbar,
.project-list::-webkit-scrollbar {
  width: 6px;
}

.session-list::-webkit-scrollbar-thumb,
.project-list::-webkit-scrollbar-thumb {
  background: var(--border-dark);
  border-radius: 3px;
}

.session-list::-webkit-scrollbar-track,
.project-list::-webkit-scrollbar-track {
  background: transparent;
}
</style>
