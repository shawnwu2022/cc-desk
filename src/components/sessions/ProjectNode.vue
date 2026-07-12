<template>
  <div class="project-node" :class="{ current: isCurrent }">
    <!-- 项目行：整行点击 = 展开/折叠（v3：点项目不切换，切换靠点会话节点） -->
    <div class="project-row">
      <!-- ▸ 展开箭头：独立命中区，与整行点击合并触发 toggleExpand -->
      <button
        class="expand-arrow"
        :class="{ expanded: expanded }"
        @click.stop="$emit('toggleExpand', project.projectPath)"
        :title="expanded ? t('collapse') : t('expand')"
      >
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
          <polyline points="9 6 15 12 9 18" />
        </svg>
      </button>

      <!-- 项目名区：整行点击展开/折叠 -->
      <div class="project-main" @click="$emit('toggleExpand', project.projectPath)">
        <span class="project-name">{{ project.name }}</span>
        <!-- 置顶标记 -->
        <span v-if="project.isPinned" class="pin-mark" :title="t('pinned')">
          <svg width="11" height="11" viewBox="0 0 16 16" fill="currentColor">
            <path d="M9.828.722a.5.5 0 0 1 .354.146l4.95 4.95a.5.5 0 0 1 0 .707c-.48.48-1.072.588-1.503.588-.177 0-.335-.018-.46-.039l-3.134 3.134a5.927 5.927 0 0 1 .16 1.013c.046.702-.032 1.687-.72 2.375a.5.5 0 0 1-.707 0l-2.829-2.828-3.182 3.182c-.195.195-1.219.902-1.414.707-.195-.195.512-1.22.707-1.414l3.182-3.182-2.828-2.829a.5.5 0 0 1 0-.707c.688-.688 1.673-.767 2.375-.72a5.922 5.922 0 0 1 1.013.16l3.134-3.133a2.772 2.772 0 0 1-.04-.461c0-.43.108-1.022.589-1.503a.5.5 0 0 1 .353-.146z"/>
          </svg>
        </span>
        <!-- 状态徽标：●N 运行 / 琥珀点 pending -->
        <span v-if="project.runningCount > 0" class="badge running">●{{ project.runningCount }}</span>
        <span v-else-if="project.pendingCount > 0" class="badge pending" :title="t('pendingHint')">●</span>
        <span v-if="project.isOrphan" class="orphan-tag">{{ t('uncollected') }}</span>
      </div>

      <!-- hover 操作 -->
      <div class="hover-actions">
        <button class="action-btn" @click.stop="$emit('newSessionIn', project.projectPath)" :title="t('newSessionTitle')">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </button>
        <button class="action-btn" @click.stop="menuOpen = !menuOpen" :title="t('more')">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="5" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="12" cy="19" r="1" />
          </svg>
        </button>
      </div>

      <!-- ⋯ 菜单 -->
      <div v-if="menuOpen" class="menu" @click.stop>
        <button v-if="!project.isPinned" @click="onMenu('pin')">{{ t('pin') }}</button>
        <button v-else @click="onMenu('unpin')">{{ t('unpin') }}</button>
        <button @click="onMenu('showArchived')">{{ t('archivedSessions') }}</button>
        <button @click="onMenu('closeAll')">{{ t('closeAllSessions') }}</button>
        <button @click="onMenu('openInExplorer')">{{ t('openFolder') }}</button>
      </div>

      <!-- 已存档会话弹层（v-if，不新建子组件） -->
      <div v-if="archivedOpen" class="archived-panel" @click.stop>
        <div class="archived-header">
          <span class="archived-title">{{ t('archivedSessions') }}</span>
          <button class="archived-close" @click="archivedOpen = false" :title="t('close')">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
        <div v-if="archivedList.length === 0" class="archived-empty">{{ t('noArchivedSessions') }}</div>
        <div v-else class="archived-list">
          <div v-for="item in archivedList" :key="item.sessionId" class="archived-item">
            <div class="archived-info">
              <span class="archived-name" :title="item.name">{{ item.name }}</span>
              <span v-if="item.lastActiveAt > 0" class="archived-time">{{ timeAgo(item.lastActiveAt) }}</span>
            </div>
            <button class="restore-btn" @click="onRestore(item.sessionId)" :title="t('restore')">{{ t('restore') }}</button>
          </div>
        </div>
      </div>
    </div>

    <!-- 会话列表（展开时） -->
    <div v-if="expanded" class="session-sub">
      <SessionList
        :tabs="project.tabs"
        :history="history"
        :active-id="activeTabId"
        :closable="true"
        @switch="onSessionSwitch"
        @rename="(id, name) => emit('renameSession', id, name)"
        @restart="(id) => emit('restartSession', id)"
        @close="(id) => emit('closeTab', id)"
        @archive="(id) => emit('archiveSession', project.projectPath, id)"
      />
      <div v-if="project.tabs.length === 0 && history.length === 0 && !loading" class="empty-hint">
        {{ t('noHistorySessions') }}
      </div>
      <div v-if="loading" class="loading-indicator">{{ t('loading') }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import SessionList from './SessionList.vue'
import { useSessionStore } from '@/stores/session'
import type { ProjectGroup, HistorySession } from '@/stores/session'

const { t } = useI18n()
const sessionStore = useSessionStore()
const props = defineProps<{
  project: ProjectGroup
  expanded: boolean
  isCurrent: boolean
  activeTabId: string | null
  history: HistorySession[]
  loading?: boolean
  matchedHistoryIds?: string[]
}>()

// <script setup> 中 defineEmits 只能调用一次；用 const 取得 emit 函数
const emit = defineEmits<{
  toggleExpand: [projectPath: string]
  newSessionIn: [projectPath: string]
  switchSession: [tabId: string]
  renameSession: [tabId: string, name: string]
  restartSession: [tabId: string]
  closeTab: [tabId: string]
  resumeSession: [sessionId: string, name?: string]
  closeAllSessions: [projectPath: string]
  openInExplorer: [projectPath: string]
  // v3 新增：置顶 / 存档
  pinProject: [projectPath: string]
  unpinProject: [projectPath: string]
  archiveSession: [projectPath: string, sessionId: string]
  restoreSession: [projectPath: string, sessionId: string]
  showArchived: [projectPath: string]
}>()

const menuOpen = ref(false)
const archivedOpen = ref(false)

// 该项目的已存档会话信息列表（响应式：restore 后 store 更新则自动收缩）
// name/lastActiveAt 从 historyCacheMap 查；未加载则 name 回退 ID 截断
const archivedList = computed(() => sessionStore.getArchivedSessionInfos(props.project.projectPath))

// 点击外部关闭菜单与已存档弹层：⋯ 按钮 / 菜单 / 弹层内部已 @click.stop，不会触发此处
function closeOnOutside() {
  menuOpen.value = false
  archivedOpen.value = false
}

onMounted(() => {
  document.addEventListener('click', closeOnOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', closeOnOutside)
})

function onMenu(action: 'closeAll' | 'openInExplorer' | 'pin' | 'unpin' | 'showArchived') {
  menuOpen.value = false
  const path = props.project.projectPath
  if (action === 'closeAll') emit('closeAllSessions', path)
  else if (action === 'openInExplorer') emit('openInExplorer', path)
  else if (action === 'pin') emit('pinProject', path)
  else if (action === 'unpin') emit('unpinProject', path)
  else if (action === 'showArchived') {
    emit('showArchived', path)
    archivedOpen.value = true
  }
}

function onRestore(sessionId: string) {
  emit('restoreSession', props.project.projectPath, sessionId)
}

// 已存档弹层的相对时间（与 SessionItem 的 timeAgo 口径一致）
function timeAgo(ts: number): string {
  const diff = Date.now() - ts
  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(diff / 3600000)
  const days = Math.floor(diff / 86400000)
  if (minutes < 1) return t('justNow')
  if (minutes < 60) return `${minutes}m`
  if (hours < 24) return `${hours}h`
  return `${days}d`
}

function resumeName(sessionId: string): string | undefined {
  return props.history.find(s => s.sessionId === sessionId)?.name
}

/**
 * SessionList 统一 emit switch(id)；区分 id 是 tabId（在 tabs 里）还是 sessionId（历史）：
 * tab -> switchSession；历史 -> resumeSession（带 name）。
 */
function onSessionSwitch(id: string) {
  if (props.project.tabs.some(t => t.tabId === id)) {
    emit('switchSession', id)
  } else {
    emit('resumeSession', id, resumeName(id))
  }
}
</script>

<style scoped>
.project-node { position: relative; }
.project-row {
  display: flex; align-items: center; gap: 4px;
  padding: 4px 4px 4px 0; border-radius: 6px;
}
.project-row:hover { background: var(--hover-bg); }
.project-node.current .project-name { color: var(--accent-color); font-weight: 600; }
.expand-arrow {
  width: 18px; height: 18px; flex-shrink: 0;
  display: flex; align-items: center; justify-content: center;
  border: none; background: transparent; color: var(--text-tertiary);
  cursor: pointer; transition: transform 0.15s ease;
}
.expand-arrow.expanded { transform: rotate(90deg); }
.project-main {
  flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px;
  cursor: pointer; padding: 4px 6px;
}
.project-name {
  font-size: 13px; font-weight: 500; color: var(--text-primary);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.pin-mark {
  display: flex; align-items: center; flex-shrink: 0;
  color: var(--accent-color);
}
.badge { font-size: 11px; flex-shrink: 0; }
.badge.running { color: var(--status-success); }
.badge.pending { color: var(--accent-gold); animation: status-pulse 2s ease-in-out infinite; }
.orphan-tag {
  font-size: 10px; color: var(--text-tertiary);
  border: 1px solid var(--border-color); border-radius: 3px; padding: 0 4px;
}
.hover-actions { display: flex; gap: 2px; opacity: 0; transition: opacity 0.15s ease; }
.project-row:hover .hover-actions { opacity: 1; }
.action-btn {
  width: 20px; height: 20px; border: none; background: transparent;
  color: var(--text-secondary); cursor: pointer; border-radius: 3px;
  display: flex; align-items: center; justify-content: center;
}
.action-btn:hover { background: var(--bg-secondary); color: var(--text-primary); }
.menu {
  position: absolute; right: 8px; top: 28px; z-index: 10;
  background: var(--bg-primary); border: 1px solid var(--border-color);
  border-radius: 6px; box-shadow: var(--shadow-md); padding: 4px; min-width: 140px;
}
.menu button {
  display: block; width: 100%; text-align: left; padding: 6px 8px;
  border: none; background: transparent; color: var(--text-primary);
  cursor: pointer; font-size: 12px; border-radius: 4px;
}
.menu button:hover { background: var(--hover-bg); }
/* 已存档会话弹层 */
.archived-panel {
  position: absolute; right: 8px; top: 28px; z-index: 11;
  background: var(--bg-primary); border: 1px solid var(--border-color);
  border-radius: 6px; box-shadow: var(--shadow-md); padding: 6px; min-width: 200px; max-width: 280px;
}
.archived-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 2px 4px 6px; gap: 8px;
}
.archived-title { font-size: 12px; font-weight: 600; color: var(--text-primary); }
.archived-close {
  width: 20px; height: 20px; border: none; background: transparent;
  color: var(--text-secondary); cursor: pointer; border-radius: 3px;
  display: flex; align-items: center; justify-content: center;
}
.archived-close:hover { background: var(--bg-secondary); color: var(--text-primary); }
.archived-empty { padding: 8px; font-size: 11px; color: var(--text-tertiary); text-align: center; }
.archived-list { display: flex; flex-direction: column; gap: 2px; max-height: 240px; overflow-y: auto; }
.archived-item {
  display: flex; align-items: center; justify-content: space-between; gap: 8px;
  padding: 4px 6px; border-radius: 4px;
}
.archived-item:hover { background: var(--hover-bg); }
.archived-info {
  flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px;
  overflow: hidden;
}
.archived-name {
  font-size: 11px; color: var(--text-secondary);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.archived-time { font-size: 10px; color: var(--text-tertiary); flex-shrink: 0; }
.restore-btn {
  border: 1px solid var(--border-color); background: transparent;
  color: var(--text-primary); cursor: pointer; border-radius: 3px;
  font-size: 11px; padding: 2px 6px; flex-shrink: 0;
}
.restore-btn:hover { border-color: var(--accent-color); color: var(--accent-color); }
.session-sub { padding-left: 22px; }
.empty-hint, .loading-indicator { padding: 8px; font-size: 11px; color: var(--text-tertiary); text-align: center; }
@keyframes status-pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.6; } }
</style>
